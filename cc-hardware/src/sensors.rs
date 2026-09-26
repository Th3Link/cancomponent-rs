//! `ExtensionMode::Sensors`/`LegacySensors` handler. Not yet implemented —
//! `init` currently just claims its peripherals and does nothing with them.

use embassy_executor::Spawner;
use esp_hal::gpio::AnyPin;
use esp_hal::i2c::master::I2c;
use esp_hal::Async;

pub struct Sensors {
    _i2c: I2c<'static, Async>,
}

impl Sensors {
    pub fn init(
        _i2c: I2c<'static, Async>,
        _onewire: AnyPin<'static>,
        _pir: AnyPin<'static>,
        _spawner: &Spawner,
    ) {
    }
}
