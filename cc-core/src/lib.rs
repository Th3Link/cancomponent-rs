#![no_std]
//! Hardware-independent CAN wire format and pure control logic shared by
//! the cancomponents firmware. Everything here is `#![no_std]` and, aside
//! from what needs `embassy-time`'s `Instant`/`Duration` types (which don't
//! require a running time driver unless you call `Instant::now()`), fully
//! host-testable with `cargo test`.
/// A wire-format decode failed: the input was too short or contained an
/// invalid discriminant. Carries no further detail — callers only need to
/// know decoding didn't succeed, matching how these bytes arrive off a CAN
/// bus with no way to ask the sender to retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("malformed CAN message data")
    }
}

impl core::error::Error for DecodeError {}

pub mod button_fsm;
pub mod button_message;
pub mod can_id;
pub mod can_message_type;
pub mod device_message;
pub mod device_type;
pub mod error;
pub mod extension;
pub mod relais;
pub mod relais_manager;
