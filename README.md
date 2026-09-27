# cancomponent-rs

ESP32 firmware for CAN-bus home-automation nodes — relays, rollershutters,
button panels — written in Rust (`#![no_std]`, [embassy](https://embassy.dev/)
async), plus the shared, host-testable wire-protocol crate they're built on.

## Repository layout

- [`cc-core/`](cc-core) — `#![no_std]` CAN wire format (message layouts, CAN
  ID/filter bit-math, error-report protocol) and pure control logic (relay
  scheduling, button click state machine). No hardware dependencies, so it
  builds and tests on any machine with a normal stable Rust toolchain:
  ```bash
  cargo test -p cancomponents-core
  ```
- [`cc-hardware/`](cc-hardware) — the actual ESP32 firmware binary. Requires
  the `esp` Rust toolchain channel (see below).

## Hardware

Each node is an ESP32 on a shared CAN (TWAI) bus, configured with a
`device_id` / `device_type` / `hwrev` triple that determines its pin wiring
and behaviour — relay driver, rollershutter, button panel, and so on. The
supported combinations are wired up explicitly in
[`cc-hardware/src/bin/main.rs`](cc-hardware/src/bin/main.rs).

## Building & flashing

`cc-hardware` targets Xtensa and needs Espressif's Rust toolchain fork,
installed via [`espup`](https://github.com/esp-rs/espup):

```bash
cargo install espup
espup install
source "$HOME/export-esp.sh"   # needed once per new shell
```

Then, from `cc-hardware/`:

```bash
cargo run --release
```

`cargo run` flashes over USB serial and opens a monitor, via `espflash`
(configured as the custom runner in `cc-hardware/.cargo/config.toml`).

## Commissioning console

Connect a serial terminal to UART0 (GPIO3 = RX, GPIO1 = TX) at 115200 baud
and press Enter to open the console. `help` lists all commands; the
important ones are `show` (dump current config), `device-id`,
`device-type`, `hwrev`, `custom-string`, `relais-mode`, `extension-mode`,
and `relais --num <n> --state <on|off|up|down>` for driving relays directly
during bring-up.

## Firmware updates

A device can be updated two ways: the same USB/`espflash` route used for
initial flashing above, or in-place over the CAN bus itself via the
`FlashStart` / `FlashWrite` / `FlashComplete` OTA protocol implemented in
[`cc-hardware/src/update.rs`](cc-hardware/src/update.rs) — so an
already-installed unit never has to come back off the wall for a firmware
update.

## CI

- **CI** — `cargo fmt` / `cargo clippy` for both crates, `cargo nextest`
  with a coverage report and an MSRV check for `cc-core`.
- **Security audit** — [`cargo-audit`](https://github.com/rustsec/rustsec)
  against the RustSec advisory database, on every dependency change and
  once daily.
- **Renovate** — automated dependency-update PRs, once daily.

## License

GNU General Public License v3.0 (or later) — see [LICENSE.md](LICENSE.md).

This is meant to be freely buildable, flashable, and modifiable hardware
and firmware. GPLv3 was chosen deliberately: its copyleft means anything
built on this stays open, and its Section 6 "Installation Information"
requirement means anyone who conveys a modified version as part of a
physical product also has to provide what's needed to install your own
modified firmware on it — nobody gets to lock you out of hardware you own.

```
cancomponent-rs — CAN-bus home-automation firmware
Copyright (C) 2026  Marc-Hendric Luehr

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU General Public License for more details.
```
