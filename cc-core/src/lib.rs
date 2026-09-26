#![no_std]
//! Hardware-independent CAN wire format and pure control logic shared by
//! the cancomponents firmware. Everything here is `#![no_std]` and, aside
//! from what needs `embassy-time`'s `Instant`/`Duration` types (which don't
//! require a running time driver unless you call `Instant::now()`), fully
//! host-testable with `cargo test`.
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
