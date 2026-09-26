//! Interactive commissioning console, sharing UART0 with `esp_println`'s
//! debug output (see `crate::logging`).
//!
//! The console stays passive until a bare Enter keypress activates it —
//! until then, incoming bytes are ignored and debug logging via
//! [`console_log!`](crate::console_log) prints normally. Once active,
//! `console_log!` output is muted (through [`crate::logging::CLI_ACTIVE`])
//! so it can't corrupt the line editor's prompt, and the session runs until
//! `exit` is typed or Ctrl+D/Ctrl+C is pressed, after which logging resumes.
//!
//! Line editing (cursor movement, backspace, history via the up/down
//! arrows, tab-completion) and command parsing/validation/help are provided
//! by the [`bareminal_cli`] crate rather than hand-rolled here.

use crate::can::{send_can_message, DEVICE_ID, DEVICE_TYPE};
use crate::config::{self, config};
use crate::console_log;
use crate::device::device;
use crate::logging::CLI_ACTIVE;
use crate::relais::send_relais_command;
use bareminal_cli::cli::{Bareminal, CommandWriter};
use bareminal_macros::Command;
use cancomponents_core::can_message_type::CanMessageType;
use cancomponents_core::extension::Mode as ExtensionMode;
use cancomponents_core::relais::{Message as RelaisMessage, Mode as RelaisMode, State};
use core::fmt::Write as _;
use core::str::FromStr;
use core::sync::atomic::Ordering;
use embassy_executor::Spawner;
use embassy_time::Duration;
use embedded_io_async::Read;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::uart;
use esp_hal::Async;
use heapless::String;

/// Max length of a single formatted response line.
const LINE_CAP: usize = 128;
/// Max length of a typed command line (bareminal_cli's own line buffer).
const MAX_CMD_BUFFER: usize = 160;
/// Number of previous lines kept for up/down-arrow recall.
const HISTORY_SIZE: usize = 8;

type Uart = uart::Uart<'static, Async>;
type UartTx = uart::UartTx<'static, Async>;
type UartRx = uart::UartRx<'static, Async>;
type Cli = Bareminal<Commands<'static>, UartTx, MAX_CMD_BUFFER, HISTORY_SIZE>;

/// Starts the console task.
pub async fn init(
    uart: esp_hal::peripherals::UART0<'static>,
    rx: impl InputPin + 'static,
    tx: impl OutputPin + 'static,
    spawner: &Spawner,
) {
    let uart: Uart = uart::Uart::new(uart, uart::Config::default())
        .unwrap()
        .with_rx(rx)
        .with_tx(tx)
        .into_async();

    spawner.spawn(console_task(uart).unwrap());
}

/// Config-setting and bring-up-testing commands. Doc comments become the
/// text shown by the built-in `help`/`help <command>`.
#[derive(Debug, Command)]
enum Commands<'a> {
    /// Show all current configuration values.
    Show,
    /// Set this device's CAN device id (a restart is needed for the CAN
    /// acceptance filter to actually follow it).
    DeviceId(u8),
    /// Set this device's CAN device type (a restart is needed for the CAN
    /// acceptance filter to actually follow it).
    DeviceType(u8),
    /// Set the hardware revision (takes effect after restart).
    Hwrev(u8),
    /// Set an 8-byte identification string, e.g. "kitchen" (single word,
    /// truncated to 8 bytes).
    CustomString(&'a str),
    /// Set the relay output mode (takes effect after restart).
    #[set(one_of = ["off", "relais", "software_rollershutter", "hardware_rollershutter"])]
    RelaisMode(RelaisModeArg),
    /// Set the extension header mode (takes effect after restart).
    #[set(one_of = [
        "off", "button", "sensors", "pwm", "relais", "legacy_sensors",
        "software_rollershutter", "hardware_rollershutter",
    ])]
    ExtensionMode(ExtensionModeArg),
    /// Directly drive one relay/rollershutter channel, for bring-up testing
    /// without a CAN bus master.
    Relais {
        #[set(short)]
        num: usize,
        #[set(short, one_of = ["on", "off", "up", "down"])]
        state: RelaisStateArg,
    },
    /// Broadcast a CAN Ping, for bus-wiring sanity checks.
    Ping,
    /// Restart the device.
    Restart,
    /// Leave the console (same as Ctrl+C / Ctrl+D).
    Exit,
}

/// CLI-facing spelling of [`RelaisMode`] (kept separate from the core type
/// since `FromStr` can't be implemented on a foreign type here).
#[derive(Debug, Clone, Copy)]
enum RelaisModeArg {
    Off,
    Relais,
    SoftwareRollershutter,
    HardwareRollershutter,
}

impl RelaisModeArg {
    fn as_u8(self) -> u8 {
        match self {
            Self::Off => RelaisMode::Off as u8,
            Self::Relais => RelaisMode::Relais as u8,
            Self::SoftwareRollershutter => RelaisMode::SoftwareRollershutter as u8,
            Self::HardwareRollershutter => RelaisMode::HardwareRollershutter as u8,
        }
    }
}

impl FromStr for RelaisModeArg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "off" => Ok(Self::Off),
            "relais" => Ok(Self::Relais),
            "software_rollershutter" => Ok(Self::SoftwareRollershutter),
            "hardware_rollershutter" => Ok(Self::HardwareRollershutter),
            _ => Err(()),
        }
    }
}

/// CLI-facing spelling of [`ExtensionMode`].
#[derive(Debug, Clone, Copy)]
enum ExtensionModeArg {
    Off,
    Button,
    Sensors,
    Pwm,
    Relais,
    LegacySensors,
    SoftwareRollershutter,
    HardwareRollershutter,
}

impl ExtensionModeArg {
    fn as_u8(self) -> u8 {
        match self {
            Self::Off => ExtensionMode::Off as u8,
            Self::Button => ExtensionMode::Button as u8,
            Self::Sensors => ExtensionMode::Sensors as u8,
            Self::Pwm => ExtensionMode::Pwm as u8,
            Self::Relais => ExtensionMode::Relais as u8,
            Self::LegacySensors => ExtensionMode::LegacySensors as u8,
            Self::SoftwareRollershutter => ExtensionMode::SoftwareRollershutter as u8,
            Self::HardwareRollershutter => ExtensionMode::HardwareRollershutter as u8,
        }
    }
}

impl FromStr for ExtensionModeArg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "off" => Ok(Self::Off),
            "button" => Ok(Self::Button),
            "sensors" => Ok(Self::Sensors),
            "pwm" => Ok(Self::Pwm),
            "relais" => Ok(Self::Relais),
            "legacy_sensors" => Ok(Self::LegacySensors),
            "software_rollershutter" => Ok(Self::SoftwareRollershutter),
            "hardware_rollershutter" => Ok(Self::HardwareRollershutter),
            _ => Err(()),
        }
    }
}

/// CLI-facing spelling of [`State`], for the `relais` bring-up command.
#[derive(Debug, Clone, Copy)]
enum RelaisStateArg {
    On,
    Off,
    Up,
    Down,
}

impl From<RelaisStateArg> for State {
    fn from(v: RelaisStateArg) -> Self {
        match v {
            RelaisStateArg::On => State::On,
            RelaisStateArg::Off => State::Off,
            RelaisStateArg::Up => State::Up,
            RelaisStateArg::Down => State::Down,
        }
    }
}

impl FromStr for RelaisStateArg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            _ => Err(()),
        }
    }
}

#[embassy_executor::task]
async fn console_task(uart: Uart) {
    console_log!("console: press enter for the commissioning console");

    let (mut rx, tx) = uart.split();
    let mut cli: Cli = match Bareminal::new(tx).await {
        Ok(cli) => cli,
        Err(_) => return,
    };

    loop {
        wait_for_enter(&mut rx).await;

        CLI_ACTIVE.store(true, Ordering::Relaxed);
        cli.redraw_prompt().await.ok();
        run_session(&mut cli, &mut rx).await;
        CLI_ACTIVE.store(false, Ordering::Relaxed);
        console_log!("console: closed");
    }
}

/// Blocks (yielding to other tasks) until a bare CR or LF byte arrives,
/// discarding everything else. This is the "log mode" idle state.
async fn wait_for_enter(rx: &mut UartRx) {
    let mut byte = [0u8; 1];
    loop {
        match rx.read_exact(&mut byte).await {
            Ok(()) if matches!(byte[0], b'\r' | b'\n') => return,
            Ok(()) => {}
            Err(_) => embassy_time::Timer::after(Duration::from_millis(50)).await,
        }
    }
}

/// Feeds bytes to `cli` until the user exits (Ctrl+C, Ctrl+D, `exit`, or an
/// unrecoverable I/O error).
async fn run_session(cli: &mut Cli, rx: &mut UartRx) {
    let mut byte = [0u8; 1];
    loop {
        if rx.read_exact(&mut byte).await.is_err() {
            return;
        }
        // Ctrl+C / Ctrl+D: bareminal_cli recognizes Ctrl+C as an input event
        // but (by design) leaves acting on it to the caller; Ctrl+D isn't a
        // control code it knows at all. Handle both the same way here.
        if matches!(byte[0], 0x03 | 0x04) {
            return;
        }

        let ready = match cli.add_byte(byte[0]).await {
            Ok(ready) => ready,
            Err(_) => return,
        };
        if !ready {
            continue;
        }

        loop {
            match cli.next_command().await {
                Ok(None) => {
                    if cli.finalize().await.is_err() {
                        return;
                    }
                    break;
                }
                Ok(Some((command, writer))) => {
                    let is_exit = matches!(command, Commands::Exit);
                    if handle_command(command, writer).await.is_err() || is_exit {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }
}

async fn respond(writer: &mut CommandWriter<UartTx>, text: &str) -> Result<(), ()> {
    writer.write_line(text.as_bytes()).await.map_err(|_| ())
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

async fn handle_command(
    command: Commands<'_>,
    writer: &mut CommandWriter<UartTx>,
) -> Result<(), ()> {
    match command {
        Commands::Show => cmd_show(writer).await,
        Commands::DeviceId(id) => respond(writer, &cmd_device_id(id).await).await,
        Commands::DeviceType(dtype) => respond(writer, &cmd_device_type(dtype).await).await,
        Commands::Hwrev(rev) => respond(writer, &cmd_hwrev(rev).await).await,
        Commands::CustomString(text) => respond(writer, &cmd_custom_string(text).await).await,
        Commands::RelaisMode(mode) => respond(writer, &cmd_relais_mode(mode).await).await,
        Commands::ExtensionMode(mode) => respond(writer, &cmd_extension_mode(mode).await).await,
        Commands::Relais { num, state } => respond(writer, &cmd_relais(num, state).await).await,
        Commands::Ping => {
            send_can_message(CanMessageType::Ping, &[], false).await;
            respond(writer, "ping sent").await
        }
        Commands::Restart => {
            respond(writer, "restarting...").await.ok();
            // Give the UART a moment to actually flush the line above.
            embassy_time::Timer::after(Duration::from_millis(50)).await;
            esp_hal::system::software_reset();
        }
        Commands::Exit => Ok(()),
    }
}

async fn cmd_show(writer: &mut CommandWriter<UartTx>) -> Result<(), ()> {
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

    respond(writer, &fmt_msg(format_args!("device_id: {device_id}"))).await?;
    respond(writer, &fmt_msg(format_args!("device_type: {device_type}"))).await?;
    respond(writer, &fmt_msg(format_args!("hwrev: {hwrev:?}"))).await?;
    respond(
        writer,
        &fmt_msg(format_args!("relais_mode: {relais_mode:?}")),
    )
    .await?;
    respond(
        writer,
        &fmt_msg(format_args!("extension_mode: {extension_mode:?}")),
    )
    .await?;
    match custom_string {
        Some(s) => respond(writer, &fmt_msg(format_args!("custom_string: {s}"))).await?,
        None => respond(writer, "custom_string: (unset)").await?,
    }
    respond(
        writer,
        &fmt_msg(format_args!("uptime: {uptime_minutes} min")),
    )
    .await
}

async fn cmd_device_id(id: u8) -> String<LINE_CAP> {
    if config()
        .await
        .set_u8(config::Key::DeviceId, id)
        .await
        .is_err()
    {
        return str_msg("failed to persist device_id");
    }
    *DEVICE_ID.lock().await = id;
    fmt_msg(format_args!(
        "device_id set to {id} (CAN filter needs a restart to follow it)"
    ))
}

async fn cmd_device_type(dtype: u8) -> String<LINE_CAP> {
    if config()
        .await
        .set_u8(config::Key::DeviceType, dtype)
        .await
        .is_err()
    {
        return str_msg("failed to persist device_type");
    }
    *DEVICE_TYPE.lock().await = dtype;
    fmt_msg(format_args!(
        "device_type set to {dtype} (CAN filter needs a restart to follow it)"
    ))
}

async fn cmd_hwrev(rev: u8) -> String<LINE_CAP> {
    if config()
        .await
        .set_u8(config::Key::HardwareRevision, rev)
        .await
        .is_err()
    {
        return str_msg("failed to persist hwrev");
    }
    fmt_msg(format_args!(
        "hwrev set to {rev} (takes effect after restart)"
    ))
}

async fn cmd_custom_string(text: &str) -> String<LINE_CAP> {
    let mut value: String<8> = String::new();
    let mut truncated = false;
    for ch in text.chars() {
        if value.push(ch).is_err() {
            truncated = true;
            break;
        }
    }

    if config()
        .await
        .set_str(config::Key::CustomString, &value)
        .await
        .is_err()
    {
        return str_msg("failed to persist custom_string");
    }
    if truncated {
        fmt_msg(format_args!(
            "custom_string set to \"{value}\" (truncated to 8 bytes)"
        ))
    } else {
        fmt_msg(format_args!("custom_string set to \"{value}\""))
    }
}

async fn cmd_relais_mode(mode: RelaisModeArg) -> String<LINE_CAP> {
    let value = mode.as_u8();
    if config()
        .await
        .set_u8(config::Key::RelaisMode, value)
        .await
        .is_err()
    {
        return str_msg("failed to persist relais_mode");
    }
    fmt_msg(format_args!(
        "relais_mode set to {:?} (takes effect after restart)",
        RelaisMode::from(value)
    ))
}

async fn cmd_extension_mode(mode: ExtensionModeArg) -> String<LINE_CAP> {
    let value = mode.as_u8();
    if config()
        .await
        .set_u8(config::Key::ExtensionMode, value)
        .await
        .is_err()
    {
        return str_msg("failed to persist extension_mode");
    }
    fmt_msg(format_args!(
        "extension_mode set to {:?} (takes effect after restart)",
        ExtensionMode::from(value)
    ))
}

async fn cmd_relais(num: usize, state: RelaisStateArg) -> String<LINE_CAP> {
    let state: State = state.into();
    send_relais_command(RelaisMessage {
        num,
        state,
        duration: Duration::from_millis(0),
        bank: 0,
    })
    .await;
    fmt_msg(format_args!("relais {num} -> {state:?}"))
}
