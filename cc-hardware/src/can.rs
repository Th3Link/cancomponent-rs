use crate::config;
use crate::device::device;
use crate::relais::{relais_handler, rollershutter_handler};
use crate::update::update;
use cancomponents_core::can_id::CanId;
use cancomponents_core::can_message_type::CanMessageType;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embedded_can::Frame;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::twai::filter::DualExtendedFilter;
use esp_hal::twai::{self, EspTwaiFrame, TimingConfig, TwaiMode, TwaiRx, TwaiTx};
use esp_hal::Async;
use esp_println::println;
use static_cell::StaticCell;

pub static CAN_CHANNEL: Channel<CriticalSectionRawMutex, EspTwaiFrame, 32> = Channel::new();
static TWAI_RX: StaticCell<TwaiRx<'_, Async>> = StaticCell::new();
static TWAI_TX: StaticCell<TwaiTx<'_, Async>> = StaticCell::new();

pub static DEVICE_ID: Mutex<CriticalSectionRawMutex, u8> = Mutex::new(255);
pub static DEVICE_TYPE: Mutex<CriticalSectionRawMutex, u8> = Mutex::new(255);

pub fn make_filter(device_type: u8, device_id: u8) -> DualExtendedFilter {
    let is_ng = true;

    let full_id = ((is_ng as u32) << 28)
        | ((device_type as u32 & 0x3F) << 16)
        | ((device_id as u32 & 0xFF) << 8); // nur oberste 3 Bit von device_id

    let full_mask = 0x103FFF00;

    let code1 = ((full_id >> 13) & 0xFFFF) as u16;
    let mask1 = ((full_mask >> 13) & 0xFFFF) as u16;

    let full_id2 = (is_ng as u32) << 28;

    let code2 = ((full_id2 >> 13) & 0xFFFF) as u16;
    println!("{code1:#x} {code2:#x} {mask1:#x} ");
    DualExtendedFilter::new_from_code_mask([code1, code2], [mask1, mask1])
}

pub async fn init(
    twai: esp_hal::peripherals::TWAI0<'static>,
    rx: impl InputPin + 'static,
    tx: impl OutputPin + 'static,
    spawner: &Spawner,
) {
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
        println!("waiting for bus_off");
        embassy_time::Timer::after_millis(100).await;
    }
    let (rx, tx) = twai.split();

    let rx = TWAI_RX.init(rx);
    let tx = TWAI_TX.init(tx);

    spawner.spawn(can_send_task(tx)).unwrap();
    spawner.spawn(can_recieve_task(rx)).unwrap();
}

// Typ für die CAN-Handler-Funktion
pub async fn dispatch(frame: &EspTwaiFrame) {
    let id = match frame.id() {
        embedded_can::Id::Extended(id) => CanId::from(id), // Nur das letzte Byte relevant
        embedded_can::Id::Standard(id) => {
            println!("WARN: Ignoring standard ID: {:?}", id);
            return;
        }
    };
    // type can be filtered, id is incomplete. also allow broadcast (== 0)
    if id.device_id != *DEVICE_ID.lock().await && id.device_id != 0 {
        return;
    }

    println!("recv: {frame:?}");
    match id.msg_type {
        CanMessageType::Relais => relais_handler(id, frame.data(), frame.is_remote_frame()).await,
        CanMessageType::Rollershutter => {
            rollershutter_handler(id, frame.data(), frame.is_remote_frame()).await
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
        _ => unknown_handler(frame).await,
    }
}

async fn silence(_frame: &EspTwaiFrame) {}

async fn ping(id: CanId) {
    send_can_message(id.msg_type, &[], false).await;
}

async fn unknown_handler(frame: &EspTwaiFrame) {
    let id = match frame.id() {
        embedded_can::Id::Extended(id) => id.as_raw(), // Nur das letzte Byte relevant
        embedded_can::Id::Standard(id) => {
            println!("WARN: Ignoring standard ID: {:?}", id);
            return;
        }
    };
    //println!("Unknown msg ID {:#x}, payload: {:?}", id, frame.data());
}

pub async fn send_can_message(msg_id: CanMessageType, data: &[u8], rtr: bool) {
    let device_type = *DEVICE_TYPE.lock().await;
    let device_id = *DEVICE_ID.lock().await;
    let id: embedded_can::ExtendedId = CanId::new(device_type, device_id, msg_id).into();
    let id: esp_hal::twai::ExtendedId = id.into();
    let frame = if rtr {
        EspTwaiFrame::new_remote(id, data.len() as usize).unwrap()
    } else {
        EspTwaiFrame::new(id, data).unwrap()
    };

    CAN_CHANNEL.send(frame).await
}

// === CAN Task ===

#[embassy_executor::task]
pub async fn can_recieve_task(rx: &'static mut TwaiRx<'static, Async>) {
    println!("can_recieve_task started");
    loop {
        if let Ok(frame) = rx.receive_async().await {
            dispatch(&frame).await;
        } else {
            println!("some error..");
        }
    }
}

#[embassy_executor::task]
pub async fn can_send_task(tx: &'static mut TwaiTx<'static, Async>) {
    println!("can_send_task started");
    loop {
        let frame = CAN_CHANNEL.receive().await;
        println!("send: {frame:?}");
        //while (tx.transmit(&frame).inspect_err(|e| println!("{e:?}")).ok();
        tx.transmit_async(&frame)
            .await
            .inspect_err(|e| println!("{e:?}"))
            .unwrap();
        println!("sent: {frame:?}");
    }
}
