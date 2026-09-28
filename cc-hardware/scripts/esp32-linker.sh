#!/bin/sh
set -eu

REAL_LINKER="xtensa-esp32-elf-gcc"

OUTPUT=
PREV=

for ARG do
    if [ "$PREV" = "-o" ]; then
        OUTPUT=$ARG
        break
    fi

    PREV=$ARG
done

# Perform the actual link.
"$REAL_LINKER" "$@"

if [ -z "$OUTPUT" ]; then
    exit 0
fi

case "$(basename "$OUTPUT")" in
    cancomponents-*)
        PROFILE_DIR=$(dirname "$(dirname "$OUTPUT")")
        BIN="$PROFILE_DIR/cancomponents.bin"

        # rustc reports any linker stdout/stderr as linker warnings.
        # Keep successful builds quiet, but preserve espflash output on failure.
        LOG=$(mktemp)
        trap 'rm -f "$LOG"' EXIT HUP INT TERM

        if espflash \
            --skip-update-check \
            save-image \
            --chip esp32 \
            "$OUTPUT" \
            "$BIN" \
            >"$LOG" 2>&1
        then
            :
        else
            STATUS=$?
            cat "$LOG" >&2
            exit "$STATUS"
        fi

        rm -f "$LOG"
        trap - EXIT HUP INT TERM
        ;;
esac
