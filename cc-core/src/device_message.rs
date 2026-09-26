//! Wire format for the `DeviceIdType` CAN message, which assigns a device
//! its `device_id`/`device_type`.

/// Parses a `DeviceIdType` payload into `(device_id, device_type)`.
///
/// Returns `None` if `data` isn't exactly 2 bytes.
pub fn parse_id_type(data: &[u8]) -> Option<(u8, u8)> {
    if data.len() != 2 {
        return None;
    }

    let id = data[0];
    let dtype = data[1];
    Some((id, dtype))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_byte_payload() {
        assert_eq!(parse_id_type(&[5, 7]), Some((5, 7)));
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(parse_id_type(&[]), None);
        assert_eq!(parse_id_type(&[5]), None);
        assert_eq!(parse_id_type(&[5, 7, 9]), None);
    }
}
