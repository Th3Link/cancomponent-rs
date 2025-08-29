use num_enum::{FromPrimitive, IntoPrimitive};

#[derive(Copy, Clone, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum Extension {
    Off = 0,
    Button = 1,
    Sensors = 2,
    Pwm = 3,
    Relais = 4,
    LegacySensors = 5,
    SoftwareRollershutter = 6,
    HardwareRollershutter = 7,
    #[num_enum(default)]
    Unknown = 255,
}
