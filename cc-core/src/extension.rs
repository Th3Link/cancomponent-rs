use num_enum::{FromPrimitive, IntoPrimitive};

/// What the second I2C header / GPIO extension connector is wired up to.
/// Persisted under `config::Key::ExtensionMode` and only read once at boot
/// (see `Extension::init` in `cc-hardware`) — changing it takes effect
/// after a restart.
#[derive(Copy, Clone, Debug, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum Mode {
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
