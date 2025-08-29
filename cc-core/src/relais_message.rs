use embassy_time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum RelaisState {
    Off = 0,
    Up = 1,
    Down = 2,
    On = 3,
    Unknown = 255,
}

impl core::convert::TryFrom<u8> for RelaisState {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use RelaisState::*;
        let result = match value {
            0 => Off,
            1 => Up,
            2 => Down,
            3 => On,
            _ => return Err(()),
        };
        Ok(result)
    }
}
#[derive(Debug, Clone)]
pub struct RelaisMessage {
    pub num: usize,
    pub state: RelaisState,
    pub duration: Duration, // reicht, da 24 Bit = max. ~16.7 Mio ms = ~4.5h
    pub bank: u8,
}

impl RelaisMessage {
    pub async fn from_bytes(data: &[u8]) -> Result<Self, ()> {
        if data.len() < 2 {
            return Err(());
        }

        let num = data[0] as usize;
        let state: RelaisState = RelaisState::try_from(data[1])?;
        let duration = {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(&data[2..6]);
            Duration::from_millis(u32::from_le_bytes(buf) as u64)
        };

        let bank = data[5];

        Ok(RelaisMessage {
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
