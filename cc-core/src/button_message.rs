#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ButtonState {
    Released = 0,
    Pressed = 1,
    Hold = 2,
    Single = 3,
    Double = 4,
    Tripple = 5,
    Quadruple = 6,
    Multi = 128,
}

impl core::convert::TryFrom<u8> for ButtonState {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use ButtonState::*;
        let result = match value {
            0 => Released,
            1 => Pressed,
            2 => Hold,
            3 => Single,
            4 => Double,
            5 => Tripple,
            6 => Quadruple,
            127 => Multi,
            _ => return Err(()),
        };
        Ok(result)
    }
}
#[derive(Debug, Clone, Copy)]
pub struct ButtonMessage {
    pub num: usize,
    pub state: ButtonState,
    pub count: u16,
}

impl ButtonMessage {
    pub fn new(num: usize, state: ButtonState, count: u16) -> ButtonMessage {
        let state = if state == ButtonState::Multi {
            ButtonState::try_from(count as u8 + 2).unwrap_or(ButtonState::Multi)
        } else {
            state
        };
        ButtonMessage { num, state, count }
    }

    pub async fn from_bytes(data: &[u8]) -> Result<Self, ()> {
        if data.len() < 2 {
            return Err(());
        }

        let num = data[0] as usize;
        let state: ButtonState = ButtonState::try_from(data[1])?;
        let count = u16::from_be_bytes([data[2], data[3]]);
        Ok(ButtonMessage { num, state, count })
    }
    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];

        bytes[0] = self.num as u8;
        bytes[1] = self.state as u8;
        let count_bytes = self.count.to_be_bytes(); // Big-Endian Konvertierung
        bytes[2] = count_bytes[0];
        bytes[3] = count_bytes[1];
        bytes
    }
}
