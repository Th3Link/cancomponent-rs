//! Wire format for the `DeviceError` CAN message.
//!
//! This only covers the pure encode/decode side. Sending a report (with
//! deduplication/rate-limiting against the CAN bus) needs the hardware CAN
//! channel and lives in `cc-hardware::error::report_error`.

use num_enum::{FromPrimitive, IntoPrimitive};

/// An error/status report, encoded as an 8-byte `DeviceError` CAN payload:
/// `[component, code, severity, local_code, details[0..4]]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorReport {
    pub component: Component,
    pub code: ErrorCode,
    pub severity: Severity,
    /// Component-specific sub-code (e.g. an `UpdateErrorCode`), opaque to
    /// this type.
    pub local_code: u8,
    pub details: [u8; 4],
}

impl ErrorReport {
    /// Builds a report, truncating `details` to 4 bytes if longer.
    pub fn new(
        component: Component,
        code: ErrorCode,
        severity: Severity,
        local_code: u8,
        details: &[u8],
    ) -> Self {
        let mut d = [0u8; 4];
        let n = details.len().min(4);
        d[..n].copy_from_slice(&details[..n]);
        Self {
            component,
            code,
            severity,
            local_code,
            details: d,
        }
    }

    pub fn to_bytes(&self) -> [u8; 8] {
        [
            self.component as u8,
            self.code as u8,
            self.severity as u8,
            self.local_code,
            self.details[0],
            self.details[1],
            self.details[2],
            self.details[3],
        ]
    }
}

impl TryFrom<&[u8]> for ErrorReport {
    type Error = ();

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != 8 {
            return Err(());
        }

        Ok(Self {
            component: Component::from(value[0]),
            code: ErrorCode::from(value[1]),
            severity: Severity::from(value[2]),
            local_code: value[3],
            details: [value[4], value[5], value[6], value[7]],
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum ErrorCode {
    #[num_enum(default)]
    Unknown = 0,
    InvalidData = 1,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum Severity {
    #[num_enum(default)]
    Unknown = 0,
    Warning = 1,
    RecoverableError = 2,
    RepeatingError = 3,
    Error = 4,
    CriticalError = 5,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum Component {
    #[num_enum(default)]
    Unknown = 0,
    Can = 1,
    Device = 2,
    Update = 3,
    Storage = 4,
    Ota = 5,
    Relais = 6,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_bytes_from_bytes_roundtrip() {
        let report = ErrorReport::new(
            Component::Relais,
            ErrorCode::InvalidData,
            Severity::Warning,
            42,
            &[1, 2, 3, 4],
        );
        let bytes = report.to_bytes();
        let decoded = ErrorReport::try_from(&bytes[..]).unwrap();
        assert_eq!(decoded, report);
    }

    #[test]
    fn try_from_rejects_wrong_length() {
        assert!(ErrorReport::try_from(&[0u8; 7][..]).is_err());
        assert!(ErrorReport::try_from(&[0u8; 9][..]).is_err());
    }

    #[test]
    fn severity_byte_values_match_declared_discriminants() {
        // Regression test for a mismatched hand-rolled From<u8> impl that
        // previously swapped RecoverableError (2) and RepeatingError (3).
        assert_eq!(Severity::from(1), Severity::Warning);
        assert_eq!(Severity::from(2), Severity::RecoverableError);
        assert_eq!(Severity::from(3), Severity::RepeatingError);
        assert_eq!(Severity::from(4), Severity::Error);
        assert_eq!(Severity::from(5), Severity::CriticalError);
        assert_eq!(Severity::from(200), Severity::Unknown);
    }

    #[test]
    fn component_and_error_code_roundtrip() {
        for c in [
            Component::Can,
            Component::Device,
            Component::Update,
            Component::Storage,
            Component::Ota,
            Component::Relais,
        ] {
            let byte: u8 = c.into();
            assert_eq!(Component::from(byte), c);
        }
        assert_eq!(ErrorCode::from(1), ErrorCode::InvalidData);
        assert_eq!(ErrorCode::from(99), ErrorCode::Unknown);
    }
}
