//! Wire format for the `Relais`/`Rollershutter`/`RelaisState`/
//! `RollershutterState` CAN messages.

use embassy_time::Duration;
use num_enum::{FromPrimitive, IntoPrimitive};

/// How a relay output pair is driven. Persisted under `config::Key::RelaisMode`.
#[derive(Copy, Clone, Debug, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum Mode {
    #[num_enum(default)]
    Off = 0,
    Relais = 1,
    SoftwareRollershutter = 2,
    HardwareRollershutter = 3,
}

/// Commanded/reported state of a single relay or rollershutter channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive)]
#[repr(u8)]
pub enum State {
    Off = 0,
    Up = 1,
    Down = 2,
    On = 3,
    #[num_enum(default)]
    Unknown = 255,
}

/// A `Relais`/`Rollershutter` command or `RelaisState`/`RollershutterState`
/// report.
///
/// Wire layout (6 bytes): `[num, state, duration_lo, duration_mid,
/// duration_hi, bank]`. `duration` is a 24-bit little-endian millisecond
/// count (~4.6h max) sharing its 4-byte slot with `bank`, whose byte
/// overwrites what would be the most significant byte of a full `u32`.
#[derive(Debug, Clone)]
pub struct Message {
    pub num: usize,
    pub state: State,
    pub duration: Duration,
    pub bank: u8,
}

impl Message {
    pub fn from_bytes(data: &[u8]) -> Result<Self, crate::DecodeError> {
        if data.len() < 6 {
            return Err(crate::DecodeError);
        }

        let num = data[0] as usize;
        let state: State = State::from(data[1]);
        // Only the low 3 bytes belong to `duration` — the 4th byte of this
        // slot is `bank` (see `to_bytes`), so it must not be folded in here.
        let duration =
            Duration::from_millis(u32::from_le_bytes([data[2], data[3], data[4], 0]) as u64);
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
        bytes[1] = self.state as u8;

        let ms = self.duration.as_millis() as u32;
        let dur_bytes = ms.to_le_bytes();
        bytes[2..5].copy_from_slice(&dur_bytes[0..3]);

        bytes[5] = self.bank;
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_bytes_from_bytes_roundtrip() {
        let msg = Message {
            num: 3,
            state: State::On,
            duration: Duration::from_millis(12_345),
            bank: 7,
        };
        let bytes = msg.to_bytes();
        let decoded = Message::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.num, msg.num);
        assert_eq!(decoded.state, msg.state);
        assert_eq!(decoded.duration, msg.duration);
        assert_eq!(decoded.bank, msg.bank);
    }

    #[test]
    fn bank_byte_does_not_corrupt_duration() {
        // Regression test: byte 5 (bank) used to be folded into the
        // duration as its most significant byte.
        let msg = Message {
            num: 0,
            state: State::Off,
            duration: Duration::from_millis(1_000),
            bank: 0xFF,
        };
        let decoded = Message::from_bytes(&msg.to_bytes()).unwrap();
        assert_eq!(decoded.duration, Duration::from_millis(1_000));
        assert_eq!(decoded.bank, 0xFF);
    }

    #[test]
    fn duration_truncates_to_24_bits() {
        // 24-bit ms range is documented as ~4.6h; anything beyond wraps.
        let max_24bit_ms = (1u32 << 24) - 1;
        let msg = Message {
            num: 0,
            state: State::On,
            duration: Duration::from_millis(max_24bit_ms as u64),
            bank: 0,
        };
        let decoded = Message::from_bytes(&msg.to_bytes()).unwrap();
        assert_eq!(decoded.duration, Duration::from_millis(max_24bit_ms as u64));
    }

    #[test]
    fn from_bytes_rejects_short_input_instead_of_panicking() {
        let zeros = [0u8; 6];
        for len in 0..6 {
            assert!(
                Message::from_bytes(&zeros[..len]).is_err(),
                "len {len} should be rejected"
            );
        }
    }
}
