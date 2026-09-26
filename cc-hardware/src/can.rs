//! CAN/TWAI transport: bus init, the acceptance filter, frame dispatch to
//! the per-message-type handlers, and the send queue.

use crate::config;
use crate::console_log;
use crate::device::device;
use crate::echo_guard::{disable_echo, dispatch_echo};
use crate::relais::relais_handler;
use crate::update::update;
use cancomponents_core::can_id::{filter_code_mask, CanId};
use cancomponents_core::can_message_type::CanMessageType;
use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embedded_can::Frame;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::twai::filter::DualExtendedFilter;
use esp_hal::twai::{self, EspTwaiFrame, TimingConfig, TwaiMode};
use esp_hal::Async;
use heapless::String;

/// Outgoing frames queued by [`send_can_message`], drained by [`can_send_task`].
pub static CAN_CHANNEL: Channel<CriticalSectionRawMutex, EspTwaiFrame, 32> = Channel::new();
/// Live mirror of `config::Key::DeviceId`, read by every outgoing/incoming
/// frame. Seeded by `device::init` at boot; updated live by
/// `Device::id_type` and the CLI's `device_id` command (note: the hardware
/// acceptance filter itself is only reprogrammed at boot, see [`make_filter`]).
pub static DEVICE_ID: Mutex<CriticalSectionRawMutex, u8> = Mutex::new(255);
/// See [`DEVICE_ID`]; same caveats, for `config::Key::DeviceType`.
pub static DEVICE_TYPE: Mutex<CriticalSectionRawMutex, u8> = Mutex::new(255);
/// When set, [`send_can_message`] drops everything instead of transmitting
/// (see the `UpdateSilence` message / `silence` handler below).
pub static SILENCE: Mutex<CriticalSectionRawMutex, bool> = Mutex::new(false);

/// Builds the ESP32 TWAI hardware acceptance filter for `device_type`/
/// `device_id`, programmed once at [`init`] time. See
/// [`cancomponents_core::can_id::filter_code_mask`] for the bit-level detail.
pub fn make_filter(device_type: u8, device_id: u8) -> DualExtendedFilter {
    let (code1, mask1, code2, mask2) = filter_code_mask(device_type, device_id);
    console_log!("{code1:#x} {code2:#x} {mask1:#x} ");
    DualExtendedFilter::new_from_code_mask([code1, code2], [mask1, mask2])
}

/// Starts the TWAI peripheral with the acceptance filter for the
/// currently-configured `device_type`/`device_id` (read from
/// [`DEVICE_TYPE`]/[`DEVICE_ID`], so `device::init` must run first) and
/// spawns the send/receive tasks. Blocks while the bus reports "bus off"
/// (e.g. no other node acking, or wiring issue).
pub async fn init(
    twai: esp_hal::peripherals::TWAI0<'static>,
    rx: impl InputPin + 'static,
    tx: impl OutputPin + 'static,
    spawner: &Spawner,
) {
    // NOTE: this timing is fixed regardless of `config::Key::Baudrate` — that
    // config value is stored (settable via the `Baudrate` CAN message) but
    // never read here. Dynamic baud-rate switching isn't implemented, which
    // is why the CLI doesn't expose a `baudrate` command either.
    const TC: TimingConfig = TimingConfig {
        baud_rate_prescaler: 80,
        sync_jump_width: 3,
        tseg_1: 15,
        tseg_2: 4,
        triple_sample: false,
    };

    const TWAI_BAUDRATE: twai::BaudRate = twai::BaudRate::Custom(TC);

    let device_type = *DEVICE_TYPE.lock().await;
    let device_id = *DEVICE_ID.lock().await;

    let mut twai_config =
        twai::TwaiConfiguration::new(twai, rx, tx, TWAI_BAUDRATE, TwaiMode::Normal);
    let filter = make_filter(device_type, device_id);
    twai_config.set_filter(filter);
    let twai = twai_config.into_async().start();
    while twai.is_bus_off() {
        console_log!("waiting for bus_off");
        embassy_time::Timer::after_millis(100).await;
    }
    let (rx, tx) = twai.split();

    spawner.spawn(can_send_task(tx).unwrap());
    spawner.spawn(can_recieve_task(rx).unwrap());
}

/// Routes one received frame to the handler for its `msg_type`, after
/// checking it's addressed to this device (or broadcast, `device_id == 0`).
/// The hardware filter (see [`make_filter`]) is coarser than this check —
/// it only compares the top 3 bits of `device_id` — so this software check
/// is still required for correctness, not just defense in depth.
pub async fn dispatch(frame: &EspTwaiFrame) {
    let id = match frame.id() {
        embedded_can::Id::Extended(id) => CanId::from(id), // Nur das letzte Byte relevant
        embedded_can::Id::Standard(id) => {
            console_log!("WARN: Ignoring standard ID: {:?}", id);
            return;
        }
    };
    // type can be filtered, id is incomplete. also allow broadcast (== 0)
    if id.device_id != *DEVICE_ID.lock().await && id.device_id != 0 {
        return;
    }

    // this adds quite a bit of delay. careful with that...
    // console_log!("recv: {frame:?}");

    match id.msg_type {
        CanMessageType::Relais => relais_handler(id, frame.data(), frame.is_remote_frame()).await,
        CanMessageType::Rollershutter => {
            relais_handler(id, frame.data(), frame.is_remote_frame()).await
        }
        CanMessageType::RelaisMode => {
            let _ = device()
                .await
                .u8_val(
                    id,
                    frame.data(),
                    frame.is_remote_frame(),
                    config::Key::RelaisMode,
                )
                .await;
        }
        CanMessageType::ExtensionMode => {
            let _ = device()
                .await
                .u8_val(
                    id,
                    frame.data(),
                    frame.is_remote_frame(),
                    config::Key::ExtensionMode,
                )
                .await;
        }
        CanMessageType::Uptime => {
            device()
                .await
                .uptime(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::RequestParameter => {
            device()
                .await
                .request_parameter(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::DeviceUid0 => {
            device()
                .await
                .uid0(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::DeviceUid1 => {
            device()
                .await
                .uid1(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::CustomString => {
            let _ = device()
                .await
                .custom_string(id, frame.data(), frame.is_remote_frame())
                .await;
        }
        CanMessageType::DeviceIdType => {
            let _ = device()
                .await
                .id_type(id, frame.data(), frame.is_remote_frame())
                .await;
        }
        CanMessageType::Baudrate => {
            let _ = device()
                .await
                .u8_val(
                    id,
                    frame.data(),
                    frame.is_remote_frame(),
                    config::Key::Baudrate,
                )
                .await;
        }
        CanMessageType::HwRev => {
            let _ = device()
                .await
                .u8_val(
                    id,
                    frame.data(),
                    frame.is_remote_frame(),
                    config::Key::HardwareRevision,
                )
                .await;
        }
        CanMessageType::Restart => {
            device()
                .await
                .restart(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::FlashStart => {
            update()
                .await
                .start(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::FlashProgress => {
            update()
                .await
                .progress(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::FlashSelect => {
            update()
                .await
                .select(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::FlashRead => {
            update()
                .await
                .read(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::FlashWrite => {
            update()
                .await
                .write(id, frame.data(), frame.is_remote_frame(), false)
                .await
        }
        CanMessageType::FlashComplete => {
            update()
                .await
                .write(id, frame.data(), frame.is_remote_frame(), true)
                .await
        }
        CanMessageType::FlashVerify => {
            update()
                .await
                .verify(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::FlashErase => {
            update()
                .await
                .erase(id, frame.data(), frame.is_remote_frame())
                .await
        }
        CanMessageType::UpdateSilence => silence(frame).await,
        CanMessageType::Ping => ping(id).await,
        CanMessageType::Available => ping(id).await,
        CanMessageType::Echo => dispatch_echo().await,
        _ => unknown_handler(frame).await,
    }
}

/// Handles `UpdateSilence`: mutes/unmutes this device's `Echo` keepalive and
/// (while muted) suppresses outgoing CAN traffic entirely via [`SILENCE`].
/// Payload `[0]` = unmute, anything else = mute.
async fn silence(frame: &EspTwaiFrame) {
    let data = frame.data();
    if data.len() == 1 {
        if data[0] == 0 {
            // silence off
            *SILENCE.lock().await = false;
            dispatch_echo().await;
        } else {
            // silence on
            *SILENCE.lock().await = true;
            disable_echo().await;
        }
    }
}

/// Handles `Ping`/`Available`: echoes the same message type back so a bus
/// master can confirm this device is present.
async fn ping(id: CanId) {
    send_can_message(id.msg_type, &[], false).await;
}

async fn unknown_handler(frame: &EspTwaiFrame) {
    let _id = match frame.id() {
        embedded_can::Id::Extended(id) => id.as_raw(), // Nur das letzte Byte relevant
        embedded_can::Id::Standard(id) => {
            console_log!("WARN: Ignoring standard ID: {:?}", id);
            return;
        }
    };
    //console_log!("Unknown msg ID {:#x}, payload: {:?}", id, frame.data());
}

/// Queues an outgoing frame addressed from this device's own id/type. A
/// no-op while [`SILENCE`] is set. `data` must be 8 bytes or fewer (CAN
/// frame limit) — this is not validated here and will panic otherwise.
pub async fn send_can_message(msg_id: CanMessageType, data: &[u8], rtr: bool) {
    if *SILENCE.lock().await {
        return;
    }
    let device_type = *DEVICE_TYPE.lock().await;
    let device_id = *DEVICE_ID.lock().await;
    let id: embedded_can::ExtendedId = CanId::new(device_type, device_id, msg_id).into();
    let id: esp_hal::twai::ExtendedId = id.into();
    let frame = if rtr {
        EspTwaiFrame::new_remote(id, data.len()).unwrap()
    } else {
        EspTwaiFrame::new(id, data).unwrap()
    };

    CAN_CHANNEL.send(frame).await
}

fn log_frame(frame: &EspTwaiFrame) {
    let mut out: String<128> = String::new();

    // ID
    match frame.id() {
        embedded_can::Id::Standard(id) => {
            let _ = write!(out, "id: {:03X}", id.as_raw());
        }
        embedded_can::Id::Extended(id) => {
            let _ = write!(out, "id: {:08X}", id.as_raw());
        }
    }

    match frame.id() {
        embedded_can::Id::Extended(id) => {
            let id = CanId::from(id);
            let _ = write!(out, ", msg: {:?}", id.msg_type);
        }
        embedded_can::Id::Standard(id) => {
            console_log!("WARN: Ignoring standard ID: {:?}", id);
            return;
        }
    };

    // DLC
    let _ = write!(out, ", dlc: {}", frame.dlc());

    // Data
    let _ = write!(out, ", data: [");
    for (i, b) in frame.data().iter().enumerate() {
        if i > 0 {
            let _ = write!(out, " ");
        }
        let _ = write!(out, "{b:02X}");
    }
    let _ = write!(out, "]");

    // Remote
    let _ = write!(out, ", is_remote: {}", frame.is_remote_frame());

    console_log!("{}", out.as_str());
}

/// Receives frames off the bus and dispatches each one.
#[embassy_executor::task]
pub async fn can_recieve_task(mut rx: twai::TwaiRx<'static, Async>) {
    console_log!("can_recieve_task started");
    loop {
        if let Ok(frame) = rx.receive_async().await {
            dispatch(&frame).await;
        } else {
            console_log!("recv error");
        }
    }
}

/// Drains [`CAN_CHANNEL`] and transmits each frame. A transmission that
/// fails (e.g. `TransmissionAborted` when nothing on the bus acknowledges
/// it) is logged and dropped rather than crashing the device — a
/// momentarily quiet/disconnected bus shouldn't take a relay controller
/// down.
#[embassy_executor::task]
pub async fn can_send_task(mut tx: twai::TwaiTx<'static, Async>) {
    console_log!("can_send_task started");
    loop {
        let frame = CAN_CHANNEL.receive().await;
        match tx.transmit_async(&frame).await {
            Ok(()) => log_frame(&frame),
            Err(e) => console_log!("send error: {e:?}"),
        }
    }
}
