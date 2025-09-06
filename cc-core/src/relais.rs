use embassy_time::Duration;
use num_enum::{FromPrimitive, IntoPrimitive};

#[derive(Copy, Clone, Debug, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum Mode {
    #[num_enum(default)]
    Off = 0,
    Relais = 1,
    SoftwareRollershutter = 2,
    HardwareRollershutter = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, FromPrimitive)]
#[repr(u8)]
pub enum State {
    Off = 0,
    Up = 1,
    Down = 2,
    On = 3,
    #[num_enum(default)]
    Unknown = 255,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub num: usize,
    pub state: State,
    pub duration: Duration, // reicht, da 24 Bit = max. ~16.7 Mio ms = ~4.5h
    pub bank: u8,
}

impl Message {
    pub async fn from_bytes(data: &[u8]) -> Result<Self, ()> {
        if data.len() < 2 {
            return Err(());
        }

        let num = data[0] as usize;
        let state: State = State::from(data[1]);
        let duration = {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(&data[2..6]);
            Duration::from_millis(u32::from_le_bytes(buf) as u64)
        };

        let bank = data[5];

        Ok(Message {
            num,
            state,
            duration,
            bank,
        })
    }
    pub fn to_bytes(&self) -> [u8; 6] {
        let mut bytes = [0u8; 6];

        bytes[0] = self.num as u8;
        bytes[1] = self.state.clone() as u8;

        let ms = self.duration.as_millis() as u32;
        let dur_bytes = ms.to_le_bytes();
        bytes[2..6].copy_from_slice(&dur_bytes);

        bytes[5] = self.bank;
        bytes
    }
}
