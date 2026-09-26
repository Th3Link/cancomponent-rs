//! Drives the relay outputs (and, in rollershutter modes, relay *pairs*)
//! through two I2C GPIO expanders, and runs the timed on/off scheduling for
//! `Relais`/`Rollershutter` CAN commands.

use crate::can::send_can_message;
use crate::config::{self, config};
use crate::console_log;
use crate::error::{report_error, Component, ErrorCode, Severity};
use cancomponents_core::can_id::CanId;
use cancomponents_core::can_message_type::CanMessageType;
use cancomponents_core::relais::{Message, Mode, State};
use cancomponents_core::relais_manager::RelayManager;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use esp_hal::gpio::interconnect::{PeripheralInput, PeripheralOutput};
use esp_hal::i2c::master::{Config, I2c};
use esp_hal::Async;

const MAX_RELAIS: usize = 16;

/// Queue of pending `Relais`/`Rollershutter` commands, consumed by
/// `relais_task`. Also used by the commissioning CLI (`cli::cmd_relais`) to
/// drive relays during bring-up without going through the CAN bus.
static RELAIS_CHANNEL: Channel<CriticalSectionRawMutex, Message, MAX_RELAIS> = Channel::new();

pub async fn relais_handler(_id: CanId, data: &[u8], _remote_request: bool) {
    match Message::from_bytes(data) {
        Ok(msg) => RELAIS_CHANNEL.send(msg).await,
        Err(_) => {
            report_error(
                Component::Relais,
                ErrorCode::InvalidData,
                Severity::Warning,
                0,
                &[data.len() as u8],
            )
            .await
        }
    }
}

/// Queues a relay/rollershutter command for `relais_task`, the same way a
/// CAN `Relais`/`Rollershutter` frame would via [`relais_handler`]. Used by
/// the commissioning CLI for live bring-up testing.
pub async fn send_relais_command(msg: Message) {
    RELAIS_CHANNEL.send(msg).await;
}

pub struct Relais {
    i2c: I2c<'static, Async>,
    expanders: [u8; 2],
    bank_addr: [u8; 2],
    relais_mode: Mode,
}

impl Relais {
    /// Sets up the two I2C GPIO-expander banks (register `0x3` = direction,
    /// all bits output; register `0x1` = output latch, all initially low
    /// i.e. all relays off) and spawns the task that drives them.
    pub async fn init(
        i2c0: esp_hal::peripherals::I2C0<'static>,
        sda: impl PeripheralInput<'static> + PeripheralOutput<'static>,
        scl: impl PeripheralInput<'static> + PeripheralOutput<'static>,
        bank_addr: [u8; 2],
        spawner: &Spawner,
    ) {
        let relais_mode = config()
            .await
            .get_u8(config::Key::RelaisMode)
            .await
            .map(Mode::from)
            .unwrap_or(Mode::Off);
        console_log!("relais init: {relais_mode:?}");
        let mut i2c = I2c::new(i2c0, Config::default())
            .unwrap()
            .with_sda(sda)
            .with_scl(scl)
            .into_async();

        i2c.write_async(bank_addr[0], &[0x3, 0x0]).await.ok();
        i2c.write_async(bank_addr[1], &[0x3, 0x0]).await.ok();
        i2c.write_async(bank_addr[0], &[0x1, 0x0]).await.ok();
        i2c.write_async(bank_addr[1], &[0x1, 0x0]).await.ok();

        let relais = Relais {
            i2c,
            expanders: [0, 0],
            bank_addr,
            relais_mode,
        };

        spawner.spawn(relais_task(relais).unwrap());
    }
    /// Logical relay number -> (expander index 0/1, output bit 0-7).
    /// Indices 12-15 are unused on current hardware and map to a harmless
    /// placeholder (expander 0, bit 0) rather than being left out, so
    /// `sethw` can stay a simple array lookup instead of a fallible one.
    const MAPPING: [(usize, u8); MAX_RELAIS] = [
        (0, 3),
        (0, 2),
        (0, 1),
        (0, 7),
        (0, 6),
        (0, 5),
        (0, 4),
        (1, 11 - 8),
        (1, 10 - 8),
        (1, 9 - 8),
        (1, 15 - 8),
        (1, 14 - 8),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ];

    /// Applies a commanded state to logical relay `num`, interpreting it
    /// according to `relais_mode`. In the rollershutter modes, `num` is a
    /// *shutter* index and actually drives the relay pair `(2*num, 2*num+1)`.
    pub async fn set(&mut self, num: usize, state: State) {
        console_log!("relais set: {state:?}");
        match self.relais_mode {
            Mode::Relais => {
                console_log!("relais set: {state:?}");
                self.sethw(num, state).await;
            }
            // Wiring variant where even relays switch power to the motor
            // and odd relays select direction, so Up and Down intentionally
            // produce the same pair here (unlike HardwareRollershutter,
            // whose relay pair encodes direction directly).
            Mode::SoftwareRollershutter => match state {
                State::Up => {
                    self.sethw(num * 2, State::On).await;
                    self.sethw(num * 2 + 1, State::Off).await;
                }
                State::Down => {
                    self.sethw(num * 2, State::On).await;
                    self.sethw(num * 2 + 1, State::Off).await;
                }
                _ => {
                    self.sethw(num * 2, State::Off).await;
                    self.sethw(num * 2 + 1, State::Off).await;
                }
            },
            Mode::HardwareRollershutter => match state {
                State::Up => {
                    self.sethw(num * 2, State::On).await;
                    self.sethw(num * 2 + 1, State::Off).await;
                }
                State::Down => {
                    self.sethw(num * 2, State::On).await;
                    self.sethw(num * 2 + 1, State::On).await;
                }
                _ => {
                    self.sethw(num * 2, State::Off).await;
                    self.sethw(num * 2 + 1, State::Off).await;
                }
            },
            _ => {}
        }
    }

    /// Sets or clears one physical relay bit and writes the whole expander
    /// output latch (register `0x1`) back out.
    async fn sethw(&mut self, num: usize, state: State) {
        if let Some(&(expander, bit)) = Self::MAPPING.get(num) {
            let mask = 1 << bit;
            if state == State::On {
                self.expanders[expander] |= mask;
                console_log!("switch on {expander} {bit}");
            } else {
                self.expanders[expander] &= !mask;
            }
            console_log!(
                "write {} to {}",
                self.expanders[expander],
                self.bank_addr[expander]
            );
            self.i2c
                .write_async(self.bank_addr[expander], &[0x3, 0x0])
                .await
                .ok();
            self.i2c
                .write_async(self.bank_addr[expander], &[0x1, self.expanders[expander]])
                .await
                .inspect_err(|e| console_log!("{e}"))
                .ok();
        }
    }
}

/// Drives `relais` from CAN/CLI commands, and applies auto-off schedules
/// tracked by `RelayManager` — e.g. a "Relais on for 500ms" command that
/// must revert without a follow-up message.
#[embassy_executor::task]
async fn relais_task(mut relais: Relais) {
    let mut manager: RelayManager<MAX_RELAIS> = RelayManager::new();

    loop {
        let now = Instant::now();

        // 1. Apply any schedules that expired since the last iteration.
        for (num, state) in manager.poll_expired(now).into_iter() {
            relais.set(num, state).await;
            let data = Message {
                num,
                state,
                duration: Duration::from_millis(0),
                bank: 0,
            }
            .to_bytes();
            send_can_message(CanMessageType::RelaisState, &data, false).await;
        }

        // 2. Wait for the next command or the next schedule to expire.
        let recv = RELAIS_CHANNEL.receive();
        let delay = Timer::after(manager.next_timeout(now));

        match select(recv, delay).await {
            Either::First(msg) => {
                let changed =
                    manager.apply_command(msg.num, &msg.state, msg.duration, Instant::now());
                if changed {
                    relais.set(msg.num, msg.state).await;
                    let data = Message {
                        num: msg.num,
                        state: msg.state,
                        duration: Duration::from_millis(0),
                        bank: msg.bank,
                    }
                    .to_bytes();
                    send_can_message(CanMessageType::RelaisState, &data, false).await;
                }
            }
            Either::Second(_) => {}
        }
    }
}
