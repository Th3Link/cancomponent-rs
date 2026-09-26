//! `ExtensionMode::Pwm` handler. Not yet implemented — `init` currently
//! just claims the I2C peripheral and does nothing with it.

use embassy_executor::Spawner;
use esp_hal::i2c::master::I2c;
use esp_hal::Async;

pub struct Pwm {
    _i2c: I2c<'static, Async>,
}

impl Pwm {
    pub fn init(_i2c: I2c<'static, Async>, _spawner: &Spawner) {}
}
