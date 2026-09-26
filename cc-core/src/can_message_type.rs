use num_enum::{FromPrimitive, IntoPrimitive};

/// The `msg_type` byte of a [`crate::can_id::CanId`] — identifies what a
/// CAN frame's payload means.
///
/// This is a fixed wire-protocol enum: the discriminants are the actual
/// bytes sent on the bus and must never be renumbered. New message types
/// may be appended with unused values; existing ones must not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum CanMessageType {
    Available = 0,
    DeviceError = 1,
    Restart = 2,
    DeviceUid0 = 3,
    DeviceUid1 = 4,
    DeviceIdType = 5,
    DeviceGroup = 6,
    ApplicationVersion = 7,
    Baudrate = 8,
    Uptime = 9,
    CustomString = 10,
    PwmFrequency = 11,
    RequestParameter = 12,
    ApplicationVersionString = 13,
    UpdateSilence = 14,
    FlashStart = 15,
    FlashSelect = 16,
    FlashErase = 17,
    FlashRead = 18,
    FlashWrite = 19,
    FlashVerify = 20,
    FlashProgress = 21,
    FlashComplete = 22,
    ButtonEvent = 30,
    TemperatureSensor = 31,
    HwRev = 41,
    ExtensionMode = 42,
    LampGroup = 90,
    PirSensor = 128,
    HumiditySensor = 129,
    Relais = 130,
    RelaisState = 131,
    Rollershutter = 132,
    RollershutterState = 133,
    RelaisMode = 134,
    AmbientLightSensor = 140,
    AmbientLightSensorWhite = 141,
    Nightlight = 150,
    PressureSensor = 151,
    Co2Equivalent = 152,
    VocBreath = 153,
    AirQuality = 154,
    LogDownload = 155,
    Ping = 156,
    PingDisable = 157,
    Echo = 158,
    #[num_enum(default)]
    InvalidMessage = 255,
}
