use crate::can::send_can_message;
use crate::config::{self, config};
use crate::relais_manager::RelayManager;
use cancomponents_core::can_id::CanId;
use cancomponents_core::can_message_type::CanMessageType;
use cancomponents_core::relais::{Message, Mode, State};
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use esp_hal::gpio::interconnect::PeripheralOutput;
use esp_hal::i2c::master::{Config, I2c};
use esp_hal::Async;
use esp_println::println;

const MAX_RELAIS: usize = 16;

static RELAIS_CHANNEL: Channel<CriticalSectionRawMutex, Message, MAX_RELAIS> = Channel::new();

pub async fn relais_handler(_id: CanId, data: &[u8], _remote_request: bool) {
    if let Ok(msg) = Message::from_bytes(data).await {
        RELAIS_CHANNEL.send(msg).await;
    }
    // silent error, already reportet is relais_message
}

pub struct Relais {
    i2c: I2c<'static, Async>,
    expanders: [u8; 2],
    bank_addr: [u8; 2],
    relais_mode: Mode,
}

impl Relais {
    pub async fn init(
        i2c0: esp_hal::peripherals::I2C0<'static>,
        sda: impl PeripheralOutput<'static>,
        scl: impl PeripheralOutput<'static>,
        bank_addr: [u8; 2],
        spawner: &Spawner,
    ) {
        let relais_mode = config()
            .await
            .get_u8(config::Key::RelaisMode)
            .await
            .and_then(|v| Some(Mode::from(v)))
            .unwrap_or(Mode::Off);
        println!("relais init: {relais_mode:?}");
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

        spawner.spawn(relais_task(relais)).unwrap();
    }
    /// Each entry: (expander index, bit position)
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

    pub async fn set(&mut self, num: usize, state: State) {
        println!("relais set: {state:?}");
        match self.relais_mode {
            Mode::Relais => {
                println!("relais set: {state:?}");
                self.sethw(num, state).await;
            }
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

    async fn sethw(&mut self, num: usize, state: State) {
        if let Some(&(expander, bit)) = Self::MAPPING.get(num) {
            let mask = 1 << bit;
            if state == State::On {
                self.expanders[expander] |= mask;
                println!("switch on {expander} {bit}");
            } else {
                self.expanders[expander] &= !mask;
            }
            println!(
                "write {} to {}",
                self.expanders[expander], self.bank_addr[expander]
            );
            self.i2c
                .write_async(self.bank_addr[expander], &[0x1, self.expanders[expander]])
                .await
                .inspect_err(|e| println!("{e}"))
                .ok();
        }
    }
}

#[embassy_executor::task]
async fn relais_task(mut relais: Relais) {
    let mut manager: RelayManager<MAX_RELAIS> = RelayManager::new();

    loop {
        let now = Instant::now();

        // 1. Abgelaufene Zeitsteuerungen
        for (num, state) in manager.poll_expired(now).into_iter() {
            relais.set(num, state.clone()).await;
            let data = Message {
                num,
                state,
                duration: Duration::from_millis(0),
                bank: 0,
            }
            .to_bytes();
            send_can_message(CanMessageType::RelaisState, &data, false).await;
        }

        // 2. Warte auf nächsten Befehl oder nächstes Timeout
        let recv = RELAIS_CHANNEL.receive();
        let delay = Timer::after(manager.next_timeout(now));

        match select(recv, delay).await {
            Either::First(msg) => {
                let changed =
                    manager.apply_command(msg.num, &msg.state, msg.duration, Instant::now());
                if changed {
                    relais.set(msg.num, msg.state.clone()).await;
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
