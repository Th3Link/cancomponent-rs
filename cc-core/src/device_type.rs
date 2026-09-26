use num_enum::{FromPrimitive, IntoPrimitive};

/// The kind of hardware a device is, persisted under
/// `config::Key::DeviceType` and reported in every [`crate::can_id::CanId`].
/// Fixed wire-protocol enum — do not renumber existing variants.
#[derive(Copy, Clone, Debug, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum DeviceType {
    #[num_enum(default)]
    Unknown = 0,
    LegacyRelais = 2,
    LegacyLamps = 3,
    Button = 4,
    Relais = 5,
    Gateway = 6,
    Rollershutter = 7,
    SSR = 8,
}
