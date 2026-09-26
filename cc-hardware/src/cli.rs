//! Interactive commissioning console, sharing UART0 with `esp_println`'s
//! debug output (see `crate::logging`).
//!
//! The console stays passive until a bare Enter keypress activates it —
//! until then, incoming bytes are ignored and debug logging via
//! [`console_log!`](crate::console_log) prints normally. Once active,
//! `console_log!` output is muted (through [`crate::logging::CLI_ACTIVE`])
//! so it can't corrupt the line editor's prompt, and the session runs until
//! `exit` is typed or Ctrl+D/Ctrl+C is pressed, after which logging resumes:
//!
//! ```text
//! [boot] can initialized
//! [boot] device started
//! <Enter>
//!
//! cancomponents commissioning console — type 'help', 'exit' or Ctrl+D to leave
//! > show
//! device_id: 42
//! ...
//! > exit
//!
//! console: closed
//! ```
//!
//! Line editing (cursor movement, backspace, history via the up/down
//! arrows) is provided by the [`noline`] crate rather than hand-rolled here.

use crate::can::{send_can_message, DEVICE_ID, DEVICE_TYPE};
use crate::config::{self, config};
use crate::console_log;
use crate::device::device;
use crate::logging::CLI_ACTIVE;
use crate::relais::send_relais_command;
use cancomponents_core::can_message_type::CanMessageType;
use cancomponents_core::extension::Mode as ExtensionMode;
use cancomponents_core::relais::{Message as RelaisMessage, Mode as RelaisMode, State};
use core::fmt::Write as _;
use core::str::FromStr;
use core::sync::atomic::Ordering;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read, Write};
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::uart;
use esp_hal::Async;
use heapless::{String, Vec};
use noline::builder::EditorBuilder;

/// Max length of a single line of output (a `show`/`help` line, or a
/// command response).
const LINE_CAP: usize = 128;
/// Max length of a typed command line.
const INPUT_BUF_CAP: usize = 100;
/// Total bytes available for scroll-back history entries.
const HISTORY_BUF_CAP: usize = 512;

/// Starts the console task.
pub async fn init(
    uart: esp_hal::peripherals::UART0<'static>,
    rx: impl InputPin + 'static,
    tx: impl OutputPin + 'static,
    spawner: &Spawner,
) {
    let uart = uart::Uart::new(uart, uart::Config::default())
        .unwrap()
        .with_rx(rx)
        .with_tx(tx)
        .into_async();

    spawner.spawn(console_task(uart).unwrap());
}

#[embassy_executor::task]
async fn console_task(mut uart: uart::Uart<'static, Async>) {
    console_log!("console: press enter for the commissioning console");

    let mut input_buf = [0u8; INPUT_BUF_CAP];
    let mut history_buf = [0u8; HISTORY_BUF_CAP];
    let mut editor = match EditorBuilder::from_slice(&mut input_buf)
        .with_slice_history(&mut history_buf)
        .build_async(&mut uart)
        .await
    {
        Ok(editor) => editor,
        Err(_) => return,
    };

    loop {
        wait_for_enter(&mut uart).await;

        CLI_ACTIVE.store(true, Ordering::Relaxed);
        uart.write_all(
            b"\r\ncancomponents commissioning console -- type 'help', 'exit' or Ctrl+D to leave\r\n",
        )
        .await
        .ok();

        loop {
            match editor.readline("> ", &mut uart).await {
                Ok(line) if line.eq_ignore_ascii_case("exit") => break,
                Ok(line) => handle_line(line, &mut uart).await,
                // Ctrl+C, Ctrl+D on an empty line, or an IO/parser error: leave the console.
                Err(_) => break,
            }
        }

        CLI_ACTIVE.store(false, Ordering::Relaxed);
        console_log!("console: closed");
    }
}

/// Blocks (yielding to other tasks) until a bare CR or LF byte arrives,
/// discarding everything else. This is the "log mode" idle state.
async fn wait_for_enter(uart: &mut uart::Uart<'static, Async>) {
    let mut byte = [0u8; 1];
    loop {
        match uart.read_exact(&mut byte).await {
            Ok(()) if matches!(byte[0], b'\r' | b'\n') => return,
            Ok(()) => {}
            Err(_) => Timer::after(Duration::from_millis(50)).await,
        }
    }
}

fn str_msg(s: &str) -> String<LINE_CAP> {
    let mut out = String::new();
    let _ = out.push_str(s);
    out
}

fn fmt_msg(args: core::fmt::Arguments) -> String<LINE_CAP> {
    let mut out = String::new();
    let _ = out.write_fmt(args);
    out
}

async fn respond(uart: &mut uart::Uart<'static, Async>, msg: &str) {
    uart.write_all(msg.as_bytes()).await.ok();
}

/// Runs one command line, writing its response(s) directly to `uart`.
async fn handle_line(line: &str, uart: &mut uart::Uart<'static, Async>) {
    let parts: Vec<&str, 8> = line.split_whitespace().collect();
    let Some(&cmd) = parts.first() else {
        return;
    };
    let args = &parts[1..];

    match cmd {
        "help" => cmd_help(uart).await,
        "show" => cmd_show(uart).await,
        "device_id" => respond(uart, &cmd_device_id(args).await).await,
        "device_type" => respond(uart, &cmd_device_type(args).await).await,
        "hwrev" => respond(uart, &cmd_hwrev(args).await).await,
        "custom_string" => respond(uart, &cmd_custom_string(args).await).await,
        "relais_mode" => respond(uart, &cmd_relais_mode(args).await).await,
        "extension_mode" => respond(uart, &cmd_extension_mode(args).await).await,
        "relais" => respond(uart, &cmd_relais(args).await).await,
        "ping" => {
            send_can_message(CanMessageType::Ping, &[], false).await;
            respond(uart, "ping sent\r\n").await;
        }
        "restart" => {
            respond(uart, "restarting...\r\n").await;
            // Give the UART a moment to actually flush the line above.
            Timer::after(Duration::from_millis(50)).await;
            esp_hal::system::software_reset();
        }
        _ => respond(uart, "unknown command, try 'help'\r\n").await,
    }
}

async fn cmd_help(uart: &mut uart::Uart<'static, Async>) {
    for line in [
        "commands:",
        "  help",
        "  show",
        "  device_id <u8>",
        "  device_type <u8>",
        "  hwrev <u8>",
        "  custom_string <text>",
        "  relais_mode <off|relais|software_rollershutter|hardware_rollershutter|u8>",
        "  extension_mode <off|button|sensors|pwm|relais|legacy_sensors|u8>",
        "    ...or <software_rollershutter|hardware_rollershutter|u8>",
        "  relais <num> <on|off|up|down>",
        "  ping",
        "  restart",
        "  exit (or Ctrl+D / Ctrl+C)",
    ] {
        respond(uart, &fmt_msg(format_args!("{line}\r\n"))).await;
    }
}

async fn cmd_show(uart: &mut uart::Uart<'static, Async>) {
    let device_id = *DEVICE_ID.lock().await;
    let device_type = *DEVICE_TYPE.lock().await;
    let uptime_minutes = device().await.uptime_minutes();

    let mut cfg = config().await;
    let hwrev = cfg.get_u8(config::Key::HardwareRevision).await;
    let relais_mode = cfg
        .get_u8(config::Key::RelaisMode)
        .await
        .map(RelaisMode::from);
    let extension_mode = cfg
        .get_u8(config::Key::ExtensionMode)
        .await
        .map(ExtensionMode::from);
    let custom_string = cfg.get_str::<8>(config::Key::CustomString).await;
    drop(cfg);

    respond(uart, &fmt_msg(format_args!("device_id: {device_id}\r\n"))).await;
    respond(
        uart,
        &fmt_msg(format_args!("device_type: {device_type}\r\n")),
    )
    .await;
    respond(uart, &fmt_msg(format_args!("hwrev: {hwrev:?}\r\n"))).await;
    respond(
        uart,
        &fmt_msg(format_args!("relais_mode: {relais_mode:?}\r\n")),
    )
    .await;
    respond(
        uart,
        &fmt_msg(format_args!("extension_mode: {extension_mode:?}\r\n")),
    )
    .await;
    match custom_string {
        Some(s) => respond(uart, &fmt_msg(format_args!("custom_string: {s}\r\n"))).await,
        None => respond(uart, "custom_string: (unset)\r\n").await,
    }
    respond(
        uart,
        &fmt_msg(format_args!("uptime: {uptime_minutes} min\r\n")),
    )
    .await;
}

async fn cmd_device_id(args: &[&str]) -> String<LINE_CAP> {
    let Some(id) = args.first().and_then(|t| u8::from_str(t).ok()) else {
        return str_msg("usage: device_id <u8>\r\n");
    };
    if config()
        .await
        .set_u8(config::Key::DeviceId, id)
        .await
        .is_err()
    {
        return str_msg("failed to persist device_id\r\n");
    }
    *DEVICE_ID.lock().await = id;
    fmt_msg(format_args!(
        "device_id set to {id} (CAN filter needs a restart to follow it)\r\n"
    ))
}

async fn cmd_device_type(args: &[&str]) -> String<LINE_CAP> {
    let Some(dtype) = args.first().and_then(|t| u8::from_str(t).ok()) else {
        return str_msg("usage: device_type <u8>\r\n");
    };
    if config()
        .await
        .set_u8(config::Key::DeviceType, dtype)
        .await
        .is_err()
    {
        return str_msg("failed to persist device_type\r\n");
    }
    *DEVICE_TYPE.lock().await = dtype;
    fmt_msg(format_args!(
        "device_type set to {dtype} (CAN filter needs a restart to follow it)\r\n"
    ))
}

async fn cmd_hwrev(args: &[&str]) -> String<LINE_CAP> {
    let Some(rev) = args.first().and_then(|t| u8::from_str(t).ok()) else {
        return str_msg("usage: hwrev <u8>\r\n");
    };
    if config()
        .await
        .set_u8(config::Key::HardwareRevision, rev)
        .await
        .is_err()
    {
        return str_msg("failed to persist hwrev\r\n");
    }
    fmt_msg(format_args!(
        "hwrev set to {rev} (takes effect after restart)\r\n"
    ))
}

async fn cmd_custom_string(args: &[&str]) -> String<LINE_CAP> {
    if args.is_empty() {
        return str_msg("usage: custom_string <text>\r\n");
    }

    let mut value: String<8> = String::new();
    let mut truncated = false;
    'words: for (i, word) in args.iter().enumerate() {
        if i > 0 && value.push(' ').is_err() {
            truncated = true;
            break;
        }
        for ch in word.chars() {
            if value.push(ch).is_err() {
                truncated = true;
                break 'words;
            }
        }
    }

    if config()
        .await
        .set_str(config::Key::CustomString, &value)
        .await
        .is_err()
    {
        return str_msg("failed to persist custom_string\r\n");
    }
    if truncated {
        fmt_msg(format_args!(
            "custom_string set to \"{value}\" (truncated to 8 bytes)\r\n"
        ))
    } else {
        fmt_msg(format_args!("custom_string set to \"{value}\"\r\n"))
    }
}

async fn cmd_relais_mode(args: &[&str]) -> String<LINE_CAP> {
    const NAMES: &[(&str, u8)] = &[
        ("off", RelaisMode::Off as u8),
        ("relais", RelaisMode::Relais as u8),
        (
            "software_rollershutter",
            RelaisMode::SoftwareRollershutter as u8,
        ),
        (
            "hardware_rollershutter",
            RelaisMode::HardwareRollershutter as u8,
        ),
    ];
    let Some(&token) = args.first() else {
        return str_msg(
            "usage: relais_mode <off|relais|software_rollershutter|hardware_rollershutter|u8>\r\n",
        );
    };
    let Some(value) = parse_named_u8(token, NAMES) else {
        return str_msg("unknown relais_mode\r\n");
    };
    if config()
        .await
        .set_u8(config::Key::RelaisMode, value)
        .await
        .is_err()
    {
        return str_msg("failed to persist relais_mode\r\n");
    }
    fmt_msg(format_args!(
        "relais_mode set to {:?} (takes effect after restart)\r\n",
        RelaisMode::from(value)
    ))
}

async fn cmd_extension_mode(args: &[&str]) -> String<LINE_CAP> {
    const NAMES: &[(&str, u8)] = &[
        ("off", ExtensionMode::Off as u8),
        ("button", ExtensionMode::Button as u8),
        ("sensors", ExtensionMode::Sensors as u8),
        ("pwm", ExtensionMode::Pwm as u8),
        ("relais", ExtensionMode::Relais as u8),
        ("legacy_sensors", ExtensionMode::LegacySensors as u8),
        (
            "software_rollershutter",
            ExtensionMode::SoftwareRollershutter as u8,
        ),
        (
            "hardware_rollershutter",
            ExtensionMode::HardwareRollershutter as u8,
        ),
    ];
    let Some(&token) = args.first() else {
        return str_msg("usage: extension_mode <name|u8> -- see 'help'\r\n");
    };
    let Some(value) = parse_named_u8(token, NAMES) else {
        return str_msg("unknown extension_mode\r\n");
    };
    if config()
        .await
        .set_u8(config::Key::ExtensionMode, value)
        .await
        .is_err()
    {
        return str_msg("failed to persist extension_mode\r\n");
    }
    fmt_msg(format_args!(
        "extension_mode set to {:?} (takes effect after restart)\r\n",
        ExtensionMode::from(value)
    ))
}

async fn cmd_relais(args: &[&str]) -> String<LINE_CAP> {
    let (Some(&num_tok), Some(&state_tok)) = (args.first(), args.get(1)) else {
        return str_msg("usage: relais <num> <on|off|up|down>\r\n");
    };
    let Ok(num) = usize::from_str(num_tok) else {
        return str_msg("invalid relais number\r\n");
    };
    let state = if state_tok.eq_ignore_ascii_case("on") {
        State::On
    } else if state_tok.eq_ignore_ascii_case("off") {
        State::Off
    } else if state_tok.eq_ignore_ascii_case("up") {
        State::Up
    } else if state_tok.eq_ignore_ascii_case("down") {
        State::Down
    } else {
        return str_msg("state must be on|off|up|down\r\n");
    };

    send_relais_command(RelaisMessage {
        num,
        state,
        duration: Duration::from_millis(0),
        bank: 0,
    })
    .await;
    fmt_msg(format_args!("relais {num} -> {state:?}\r\n"))
}

/// Matches `token` against a name table (case-insensitively), falling back
/// to parsing it as a raw number.
fn parse_named_u8(token: &str, names: &[(&str, u8)]) -> Option<u8> {
    for &(name, value) in names {
        if token.eq_ignore_ascii_case(name) {
            return Some(value);
        }
    }
    u8::from_str(token).ok()
}
