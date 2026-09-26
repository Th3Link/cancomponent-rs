use crate::can_message_type::CanMessageType;
use embedded_can::ExtendedId;

/// A decoded 29-bit extended CAN ID used by the cancomponents protocol.
///
/// Bit layout, MSB first (29 bits total, matching an extended CAN ID):
/// `is_ng`(1) | `group`(6) | `device_type`(6) | `device_id`(8) | `msg_type`(8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanId {
    /// "Next generation" protocol marker; always `true` for anything this
    /// crate sends. 1 bit.
    pub is_ng: bool,
    /// Reserved for grouping/broadcast domains; always `0` when sending. 6 bit.
    pub group: u8,
    /// Device type, see [`crate::device_type::DeviceType`]. 6 bit.
    pub device_type: u8,
    /// Target/source device id; `0` is the broadcast address. 8 bit.
    pub device_id: u8,
    /// Message type, see [`CanMessageType`]. 8 bit.
    pub msg_type: CanMessageType,
}

impl CanId {
    pub fn new(device_type: u8, device_id: u8, msg_type: CanMessageType) -> Self {
        Self {
            is_ng: true,
            group: 0,
            device_type: device_type & 0x3F, // nur 6 Bit
            device_id,
            msg_type,
        }
    }
}

/// Computes the ESP32 TWAI dual-extended-filter `(code, mask)` register
/// pairs that accept frames addressed to `device_type`/`device_id` (plus
/// broadcast frames, via the second code/mask pair matching any "is_ng"
/// frame). Only the top 3 bits of `device_id` actually participate in the
/// hardware match (the filter only compares the upper 16 bits of the ID
/// register pair), so this is a coarse pre-filter — [`crate::can_id`]
/// callers still re-check the full `device_id` themselves after receiving a
/// frame.
///
/// Returns `(code1, mask1, code2, mask2)` for
/// `DualExtendedFilter::new_from_code_mask([code1, code2], [mask1, mask2])`.
///
/// This is pure bit arithmetic with no hardware dependency, but the exact
/// formula reflects the ESP32 TWAI peripheral's specific filter register
/// layout — treat the numbers this produces as a fixed contract, not
/// something to "simplify".
pub fn filter_code_mask(device_type: u8, device_id: u8) -> (u16, u16, u16, u16) {
    const IS_NG_BIT: u32 = 1 << 28;
    const FULL_MASK: u32 = 0x103FFF00;

    let full_id =
        IS_NG_BIT | ((device_type as u32 & 0x3F) << 16) | ((device_id as u32 & 0xFF) << 8);
    let code1 = ((full_id >> 13) & 0xFFFF) as u16;
    let mask1 = ((FULL_MASK >> 13) & 0xFFFF) as u16;

    // Second code/mask pair: accept any "is_ng" frame regardless of type/id,
    // so broadcast (device_id == 0) frames still reach the dispatcher.
    let code2 = ((IS_NG_BIT >> 13) & 0xFFFF) as u16;

    (code1, mask1, code2, mask1)
}

impl From<CanId> for u32 {
    fn from(id: CanId) -> Self {
        ((id.is_ng as u32) << 28)
            | ((id.group as u32 & 0x3F) << 22)
            | ((id.device_type as u32 & 0x3F) << 16)
            | ((id.device_id as u32) << 8)
            | id.msg_type as u32 & 0xFF
    }
}

impl From<CanId> for ExtendedId {
    fn from(id: CanId) -> Self {
        ExtendedId::new(id.into()).expect("can id cannot be converted")
    }
}

impl From<u32> for CanId {
    fn from(raw: u32) -> Self {
        let msg_type = CanMessageType::from((raw & 0xFF) as u8);
        Self {
            is_ng: ((raw >> 28) & 0x1) != 0,
            group: ((raw >> 22) & 0x3F) as u8,
            device_type: ((raw >> 16) & 0x3F) as u8,
            device_id: ((raw >> 8) & 0xFF) as u8,
            msg_type,
        }
    }
}

impl From<ExtendedId> for CanId {
    fn from(id: ExtendedId) -> Self {
        Self::from(id.as_raw())
    }
}

impl core::fmt::Display for CanId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "NG:{} Group:{} Type:{} ID:{} Msg:{:?}",
            self.is_ng, self.group, self.device_type, self.device_id, self.msg_type
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_id_roundtrip() {
        let can_id = CanId::new(0x01, 0x01, CanMessageType::Nightlight);
        let can_id_u32: u32 = can_id.into();
        let can_id_back = CanId::try_from(can_id_u32);
        assert!(can_id_back.is_ok());

        let can_id_ext = TryInto::<ExtendedId>::try_into(can_id);
        assert!(can_id_ext.is_ok());

        let can_id_ext_back = TryInto::<CanId>::try_into(can_id_ext.unwrap());
        assert!(can_id_ext_back.is_ok())
    }

    /// Regression test pinning the filter register values for a known
    /// input, hand-derived from the formula in `filter_code_mask`. Guards
    /// the shift/mask arithmetic against accidental changes during
    /// refactors — it is not a claim that these values are "correct" in
    /// some independently-verifiable sense (that would need the real TWAI
    /// peripheral).
    #[test]
    fn filter_code_mask_matches_hand_derived_values() {
        let (code1, mask1, code2, mask2) = filter_code_mask(0x01, 0x01);
        assert_eq!(code1, 0x8008);
        assert_eq!(mask1, 0x81FF);
        assert_eq!(code2, 0x8000);
        assert_eq!(mask2, 0x81FF);
    }

    #[test]
    fn filter_code_mask_second_pair_is_device_independent() {
        // The second (code, mask) pair only asserts the "is_ng" bit, so it
        // must be identical regardless of device_type/device_id.
        let (_, _, code2_a, mask2_a) = filter_code_mask(0x01, 0x01);
        let (_, _, code2_b, mask2_b) = filter_code_mask(0x3F, 0xFF);
        assert_eq!(code2_a, code2_b);
        assert_eq!(mask2_a, mask2_b);
    }
}
