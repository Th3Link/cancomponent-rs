//! Selects and starts whatever is wired to the second I2C header / GPIO
//! extension connector (buttons, sensors, PWM, or nothing), based on the
//! persisted `ExtensionMode`.

use crate::button::Button;
use crate::config::{self, config};
use crate::pwm::Pwm;
use crate::sensors::Sensors;
use cancomponents_core::extension::Mode;
use embassy_executor::Spawner;
use esp_hal::gpio::{AnyPin, Level, Output, OutputConfig};
use esp_hal::i2c::master::{Config, I2c};

pub struct Extension {}

impl Extension {
    /// Reads `ExtensionMode` from flash (defaulting to `Off`) and wires up
    /// the corresponding peripheral. `pin0`/`pin1` are the I2C SDA/SCL pins
    /// in I2C-based modes; all four pins are individual GPIOs in `Button`
    /// mode. `vcc_gnd`, where present, powers the extension board itself.
    pub async fn init(
        i2c1: esp_hal::peripherals::I2C1<'static>,
        pin0: AnyPin<'static>,
        pin1: AnyPin<'static>,
        pin2: AnyPin<'static>,
        pin3: AnyPin<'static>,
        vcc_gnd: Option<(AnyPin<'static>, AnyPin<'static>)>,
        spawner: &Spawner,
    ) {
        let extension_mode = config()
            .await
            .get_u8(config::Key::ExtensionMode)
            .await
            .map(Mode::from)
            .unwrap_or(Mode::Off);

        match extension_mode {
            Mode::Button => {
                if let Some((vcc, gnd)) = vcc_gnd {
                    Output::new(vcc, Level::High, OutputConfig::default());
                    Output::new(gnd, Level::Low, OutputConfig::default());
                }
                Button::init(pin0, pin1, pin2, pin3, spawner);
            }
            Mode::Sensors => {
                if let Some((vcc, gnd)) = vcc_gnd {
                    Output::new(vcc, Level::High, OutputConfig::default());
                    Output::new(gnd, Level::Low, OutputConfig::default());
                }
                let i2c = I2c::new(i2c1, Config::default())
                    .unwrap()
                    .with_sda(pin0)
                    .with_scl(pin1)
                    .into_async();
                Sensors::init(i2c, pin2, pin3, spawner);
            }
            Mode::LegacySensors => {
                // NOTE: unlike the Button/Sensors branches above, this binds
                // vcc_gnd's tuple elements as `(pin3, vcc)` rather than
                // `(vcc, gnd)` — the opposite order/naming used everywhere
                // else in this function. Left as-is (not touching GPIO
                // output-level wiring logic without hardware to verify
                // against), but this looks like it was meant to say
                // `(vcc, gnd)` here too. Currently harmless in practice only
                // because `Sensors::init` is a no-op stub that ignores its
                // pin arguments.
                let gnd = pin3;
                if let Some((pin3, vcc)) = vcc_gnd {
                    Output::new(vcc, Level::High, OutputConfig::default());
                    Output::new(gnd, Level::Low, OutputConfig::default());
                    let i2c = I2c::new(i2c1, Config::default())
                        .unwrap()
                        .with_sda(pin0)
                        .with_scl(pin1)
                        .into_async();
                    Sensors::init(i2c, pin2, pin3, spawner);
                } else {
                    // Legacy Sensors are only compatible with hwrev 1
                    // From hwrev 2 on vcc and gnd are hard wired
                }
            }
            Mode::Pwm => {
                let i2c = I2c::new(i2c1, Config::default())
                    .unwrap()
                    .with_sda(pin0)
                    .with_scl(pin1)
                    .into_async();
                Pwm::init(i2c, spawner);
            }
            _ => {}
        }

        //let _extension = Extension { i2c };
    }
}
