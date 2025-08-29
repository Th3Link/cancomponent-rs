use num_enum::{FromPrimitive, IntoPrimitive};

#[derive(Copy, Clone, IntoPrimitive, FromPrimitive)]
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
