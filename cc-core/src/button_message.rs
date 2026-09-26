//! Wire format for the `ButtonEvent` CAN message (`CanMessageType::ButtonEvent`).
//!
//! Byte layout (4 bytes, big-endian count):
//! `[button_num, state, count_hi, count_lo]`

use num_enum::{IntoPrimitive, TryFromPrimitive};

/// State of a physical button, as reported over CAN.
///
/// `Single`..`Quadruple` and `Multi` are only ever produced by
/// [`ButtonMessage::new`] when finalizing a click sequence — a sender never
/// transitions through them as "live" states the way it does for `Released`/
/// `Pressed`/`Hold`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum ButtonState {
    Released = 0,
    Pressed = 1,
    Hold = 2,
    Single = 3,
    Double = 4,
    Tripple = 5,
    Quadruple = 6,
    /// More than four clicks in one sequence; `count` carries the exact
    /// number since there's no named variant for it.
    Multi = 128,
}

#[derive(Debug, Clone, Copy)]
pub struct ButtonMessage {
    pub num: usize,
    pub state: ButtonState,
    pub count: u16,
}

impl ButtonMessage {
    /// Builds a button message. If `state` is `Multi`, `count` (the number
    /// of clicks in the sequence) is folded into a specific named state
    /// (`Single`..`Quadruple`) when one exists, matching `Released=0`
    /// through `Hold=2` being reserved for the live states and `Single=3`
    /// meaning "one click" — i.e. `count + 2`. Sequences of five or more
    /// clicks have no named variant and stay `Multi`.
    pub fn new(num: usize, state: ButtonState, count: u16) -> ButtonMessage {
        let state = if state == ButtonState::Multi {
            ButtonState::try_from(count as u8 + 2).unwrap_or(ButtonState::Multi)
        } else {
            state
        };
        ButtonMessage { num, state, count }
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, crate::DecodeError> {
        if data.len() < 4 {
            return Err(crate::DecodeError);
        }

        let num = data[0] as usize;
        let state: ButtonState = ButtonState::try_from(data[1]).map_err(|_| crate::DecodeError)?;
        let count = u16::from_be_bytes([data[2], data[3]]);
        Ok(ButtonMessage { num, state, count })
    }

    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];

        bytes[0] = self.num as u8;
        bytes[1] = self.state as u8;
        let count_bytes = self.count.to_be_bytes(); // Big-Endian
        bytes[2] = count_bytes[0];
        bytes[3] = count_bytes[1];
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_state_roundtrips_through_its_own_discriminant() {
        for state in [
            ButtonState::Released,
            ButtonState::Pressed,
            ButtonState::Hold,
            ButtonState::Single,
            ButtonState::Double,
            ButtonState::Tripple,
            ButtonState::Quadruple,
            ButtonState::Multi,
        ] {
            let byte: u8 = state.into();
            assert_eq!(ButtonState::try_from(byte), Ok(state));
        }
    }

    #[test]
    fn button_state_rejects_unknown_byte() {
        assert!(ButtonState::try_from(200u8).is_err());
    }

    #[test]
    fn multi_click_count_maps_to_named_states() {
        assert_eq!(
            ButtonMessage::new(0, ButtonState::Multi, 1).state,
            ButtonState::Single
        );
        assert_eq!(
            ButtonMessage::new(0, ButtonState::Multi, 2).state,
            ButtonState::Double
        );
        assert_eq!(
            ButtonMessage::new(0, ButtonState::Multi, 3).state,
            ButtonState::Tripple
        );
        assert_eq!(
            ButtonMessage::new(0, ButtonState::Multi, 4).state,
            ButtonState::Quadruple
        );
        // Five+ clicks: no named variant, stays Multi.
        assert_eq!(
            ButtonMessage::new(0, ButtonState::Multi, 5).state,
            ButtonState::Multi
        );
    }

    #[test]
    fn to_bytes_from_bytes_roundtrip() {
        let msg = ButtonMessage::new(3, ButtonState::Hold, 7);
        let bytes = msg.to_bytes();
        let decoded = ButtonMessage::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.num, msg.num);
        assert_eq!(decoded.state, msg.state);
        assert_eq!(decoded.count, msg.count);
    }

    #[test]
    fn from_bytes_rejects_short_input() {
        assert!(ButtonMessage::from_bytes(&[0, 1]).is_err());
    }

    #[test]
    fn multi_state_byte_128_decodes() {
        // Regression test for the Multi=128 vs. try_from(127) mismatch.
        let bytes = [0u8, 128u8, 0, 5];
        let decoded = ButtonMessage::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.state, ButtonState::Multi);
    }
}
