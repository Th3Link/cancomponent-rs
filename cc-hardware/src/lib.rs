#![no_std]
//! ESP32 firmware for a CAN-bus-controlled relay/rollershutter/button
//! device ("cancomponents"). Wire format and hardware-independent logic
//! live in the `cancomponents-core` crate; everything in here talks to
//! actual peripherals (CAN/TWAI, I2C GPIO expanders, GPIO interrupts,
//! flash-backed config storage, OTA).
pub mod button;
pub mod can;
pub mod cli;
pub mod config;
pub mod device;
pub mod echo_guard;
pub mod error;
pub mod extension;
pub mod flash;
pub mod gpio_interrupt;
pub mod logging;
pub mod pwm;
pub mod relais;
pub mod sensors;
pub mod update;
