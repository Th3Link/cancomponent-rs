use esp_println::println;

use crate::can::{send_can_message, DEVICE_ID, DEVICE_TYPE};
use crate::config::{self, config};
use crate::error::{Component, ErrorCode, ErrorReport, Severity};
use cancomponents_core::can_id::CanId;
use cancomponents_core::can_message_type::CanMessageType;
use cancomponents_core::device_message::IdTypeMsg;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Instant;
use esp_hal::efuse::Efuse;
use heapless::String;

static DEVICE: Mutex<CriticalSectionRawMutex, Option<Device>> = Mutex::new(None);

pub async fn init() {
    let mut device_guard = DEVICE.lock().await;

    if device_guard.is_none() {
        let mac = Efuse::read_base_mac_address();
        let mac = u64::from_be_bytes([0, 0, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]);
        let mut config = config().await;
        let device = Device {
            custom_string: config
                .get_str::<8>(config::Key::CustomString)
                .await
                .unwrap_or_default(),
            id: config.get_u8(config::Key::DeviceId).await.unwrap_or(255),
            dtype: config.get_u8(config::Key::DeviceType).await.unwrap_or(255),
            uid0: 0,
            uid1: 0,
            mac,
            boot_time: Instant::now(),
        };
        *DEVICE_ID.lock().await = device.id;
        *DEVICE_TYPE.lock().await = device.dtype;
        *device_guard = Some(device);
    }
}

pub async fn device(
) -> embassy_sync::mutex::MappedMutexGuard<'static, CriticalSectionRawMutex, Device> {
    let guard = DEVICE.lock().await;
    embassy_sync::mutex::MutexGuard::map(guard, |opt| opt.as_mut().expect("Device not initialized"))
}
pub struct Device {
    custom_string: String<8>,
    id: u8,
    dtype: u8,
    uid0: u64,
    uid1: u64,
    mac: u64,
    boot_time: Instant,
}

impl Device {
    pub async fn uptime(&mut self, _id: CanId, _data: &[u8], remote_request: bool) {
        if remote_request {
            let now = Instant::now();
            let uptime = now.duration_since(self.boot_time);
            let uptime_minutes = uptime.as_secs() / 60;

            let bytes = (uptime_minutes as u32).to_le_bytes(); // 4 bytes

            send_can_message(CanMessageType::Uptime, &bytes, false).await;
        }
    }

    pub async fn request_parameter(&mut self, id: CanId, data: &[u8], _remote_request: bool) {
        self.uptime(id, data, true).await;
        self.uid0(id, data, true).await;
        self.uid1(id, data, true).await;
        self.custom_string(id, data, true).await;
        let mut hwrev_id = id;
        hwrev_id.msg_type = CanMessageType::HwRev;
        self.u8_val(hwrev_id, data, true, config::Key::HardwareRevision)
            .await;
        self.application_version(id, data, true).await;
    }
    pub async fn id_type(&mut self, _id: CanId, data: &[u8], _remote_request: bool) -> Option<()> {
        //temporary disabled because gateway issues
        //if self.uid0 == self.mac && self.uid1 == self.mac {
        println!("set id and type");
        let (id, dtype) = IdTypeMsg::parse(data)?;
        self.id = id;
        self.dtype = dtype;

        let mut config = config().await;
        config.set_u8(config::Key::DeviceId, id).await.ok()?;
        config.set_u8(config::Key::DeviceType, dtype).await.ok()?;
        *DEVICE_ID.lock().await = id;
        *DEVICE_TYPE.lock().await = dtype;
        Some(())
        //}
    }

    pub async fn uid0(&mut self, _id: CanId, data: &[u8], remote_request: bool) {
        if remote_request {
            let txdata = self.mac.to_le_bytes();
            send_can_message(CanMessageType::DeviceUid0, &txdata, false).await;
        } else {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(data);
            self.uid0 = u64::from_le_bytes(buf);
            println!("set uid0 to {}", self.uid0);
        }
    }

    pub async fn uid1(&mut self, _id: CanId, data: &[u8], remote_request: bool) {
        if remote_request {
            let txdata = self.mac.to_le_bytes();
            send_can_message(CanMessageType::DeviceUid1, &txdata, false).await;
        } else {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(data);
            self.uid1 = u64::from_le_bytes(buf);
            println!("set uid1 to {}", self.uid1);
        }
    }

    pub async fn u8_val(
        &mut self,
        id: CanId,
        data: &[u8],
        remote_request: bool,
        key: config::Key,
    ) -> Option<()> {
        if remote_request {
            let mut txdata = [0u8; 1];
            let mut config = config().await;
            txdata[0] = config.get_u8(key).await?;
            send_can_message(id.msg_type, &txdata, false).await;
        } else {
            if data.len() == 1 {
                let mut config = config().await;
                config.set_u8(key, data[0]).await.ok()?;
            } else {
                ErrorReport::send(
                    Component::Device,
                    ErrorCode::InvalidData,
                    Severity::Warning,
                    0,
                    &[id.msg_type as u8, data.len() as u8, 0u8],
                )
                .await;
            }
        }
        Some(())
    }

    pub async fn custom_string(
        &mut self,
        _id: CanId,
        data: &[u8],
        remote_request: bool,
    ) -> Option<()> {
        if remote_request {
            // RTR-Frame: Aktuellen String senden
            let string = self.custom_string.as_bytes();

            let len = self.custom_string.len();
            let mut data = [0u8; 8];
            data[..len].copy_from_slice(&string[..len]);
            send_can_message(CanMessageType::CustomString, &data, false).await;
        } else {
            println!("set custom string");
            let s = core::str::from_utf8(data).ok()?;
            self.custom_string.clear();
            self.custom_string.push_str(s).ok()?;
            let mut config = config().await;
            config
                .set_str(config::Key::CustomString, &self.custom_string)
                .await
                .ok()?;
        }
        Some(())
    }

    pub async fn application_version(&mut self, _id: CanId, _data: &[u8], _remote_request: bool) {
        let version = env!("VERGEN_GIT_DESCRIBE");
        let version_bytes = version.as_bytes();

        let mut buf = [0u8; 8];
        buf[..version_bytes.len().min(8)]
            .copy_from_slice(&version_bytes[..version_bytes.len().min(8)]);

        send_can_message(CanMessageType::ApplicationVersionString, &buf, false).await;
    }
    pub async fn restart(&mut self, _id: CanId, data: &[u8], remote_request: bool) {
        if !remote_request && data.len() == 1 && data[0] == 1 {
            //esp_hal::system::software_reset();
        }
    }
}
