use crate::can::{send_can_message, DEVICE_ID, DEVICE_TYPE};
use crate::config::{self, config};
use crate::console_log;
use crate::error::{report_error, Component, ErrorCode, Severity};
use cancomponents_core::can_id::CanId;
use cancomponents_core::can_message_type::CanMessageType;
use cancomponents_core::device_message::parse_id_type;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Instant;
use esp_hal::efuse;
use heapless::String;

/// Global device identity/config-mirroring state (custom string, MAC-derived
/// UIDs, boot time for uptime reporting). Handles the CAN messages that
/// read/write generic per-device parameters (as opposed to relay-specific
/// or update-specific ones).
static DEVICE: Mutex<CriticalSectionRawMutex, Option<Device>> = Mutex::new(None);

/// Loads persisted identity from flash and seeds the live
/// `can::DEVICE_ID`/`can::DEVICE_TYPE` mirrors used for CAN addressing.
/// Must run before `can::init`, which reads those mirrors to program the
/// hardware acceptance filter.
pub async fn init() {
    let mut device_guard = DEVICE.lock().await;

    if device_guard.is_none() {
        let mac = efuse::base_mac_address();
        let mac = mac.as_bytes();
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
    /// Minutes since boot. Used both by the CAN `Uptime` reply and the CLI's
    /// `show` command.
    pub fn uptime_minutes(&self) -> u64 {
        Instant::now().duration_since(self.boot_time).as_secs() / 60
    }

    /// Replies with uptime in minutes when polled via RTR frame; ignored
    /// otherwise (uptime isn't settable).
    pub async fn uptime(&mut self, _id: CanId, _data: &[u8], remote_request: bool) {
        if remote_request {
            let bytes = (self.uptime_minutes() as u32).to_le_bytes();
            send_can_message(CanMessageType::Uptime, &bytes, false).await;
        }
    }

    /// Handles `RequestParameter`: replies with every readable parameter in
    /// one shot (uptime, UIDs, custom string, hw revision, relais/extension
    /// mode, application version) — used by a bus master to snapshot a
    /// device's full state.
    pub async fn request_parameter(&mut self, id: CanId, data: &[u8], _remote_request: bool) {
        self.uptime(id, data, true).await;
        self.uid0(id, data, true).await;
        self.uid1(id, data, true).await;
        self.custom_string(id, data, true).await;
        let mut return_id = id;
        return_id.msg_type = CanMessageType::HwRev;
        self.u8_val(return_id, data, true, config::Key::HardwareRevision)
            .await;
        return_id.msg_type = CanMessageType::RelaisMode;
        self.u8_val(return_id, data, true, config::Key::RelaisMode)
            .await;
        return_id.msg_type = CanMessageType::ExtensionMode;
        self.u8_val(return_id, data, true, config::Key::ExtensionMode)
            .await;
        self.application_version(id, data, true).await;
    }
    /// Assigns this device's `device_id`/`device_type` (the `DeviceIdType`
    /// message), persists them, and updates the live CAN addressing
    /// mirrors. Note: the hardware acceptance filter itself is only
    /// programmed once at boot (`can::init`), so a changed id/type only
    /// takes full effect after a restart.
    pub async fn id_type(&mut self, _id: CanId, data: &[u8], _remote_request: bool) -> Option<()> {
        //temporary disabled because gateway issues
        //if self.uid0 == self.mac && self.uid1 == self.mac {
        console_log!("set id and type");
        let (id, dtype) = parse_id_type(data)?;
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

    /// Replies with the MAC-derived UID when polled; otherwise stores the
    /// UID the caller wants this device to compare against (see the
    /// disabled gateway-matching check in `id_type` — currently unused, but
    /// the value is still tracked for when that's re-enabled).
    pub async fn uid0(&mut self, _id: CanId, data: &[u8], remote_request: bool) {
        if remote_request {
            let txdata = self.mac.to_le_bytes();
            send_can_message(CanMessageType::DeviceUid0, &txdata, false).await;
        } else if let Ok(buf) = data.try_into() {
            self.uid0 = u64::from_le_bytes(buf);
            console_log!("set uid0 to {}", self.uid0);
        } else {
            report_error(
                Component::Device,
                ErrorCode::InvalidData,
                Severity::Warning,
                0,
                &[CanMessageType::DeviceUid0 as u8, data.len() as u8],
            )
            .await;
        }
    }

    /// See [`Device::uid0`].
    pub async fn uid1(&mut self, _id: CanId, data: &[u8], remote_request: bool) {
        if remote_request {
            let txdata = self.mac.to_le_bytes();
            send_can_message(CanMessageType::DeviceUid1, &txdata, false).await;
        } else if let Ok(buf) = data.try_into() {
            self.uid1 = u64::from_le_bytes(buf);
            console_log!("set uid1 to {}", self.uid1);
        } else {
            report_error(
                Component::Device,
                ErrorCode::InvalidData,
                Severity::Warning,
                0,
                &[CanMessageType::DeviceUid1 as u8, data.len() as u8],
            )
            .await;
        }
    }

    /// Generic get/set handler for single-byte config values (baudrate,
    /// hw revision, relais/extension mode, ...): replies with the current
    /// value on an RTR frame, stores a new one on a 1-byte payload, or
    /// reports `InvalidData` for anything else.
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
        } else if data.len() == 1 {
            let mut config = config().await;
            config.set_u8(key, data[0]).await.ok()?;
        } else {
            report_error(
                Component::Device,
                ErrorCode::InvalidData,
                Severity::Warning,
                0,
                &[id.msg_type as u8, data.len() as u8, 0u8],
            )
            .await;
        }
        Some(())
    }

    /// Get/set an arbitrary 8-byte identification string (e.g. "kitchen").
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
            console_log!("set custom string");
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

    /// Replies with the build's `git describe` string, truncated to 8 bytes.
    pub async fn application_version(&mut self, _id: CanId, _data: &[u8], _remote_request: bool) {
        let version = env!("VERGEN_GIT_DESCRIBE");
        let version_bytes = version.as_bytes();

        let mut buf = [0u8; 8];
        buf[..version_bytes.len().min(8)]
            .copy_from_slice(&version_bytes[..version_bytes.len().min(8)]);

        send_can_message(CanMessageType::ApplicationVersionString, &buf, false).await;
    }
    /// Software-resets the device if commanded with payload `[1]`. Requires
    /// an explicit payload (not an RTR frame) as a minimal guard against an
    /// accidental broadcast restart.
    pub async fn restart(&mut self, _id: CanId, data: &[u8], remote_request: bool) {
        if !remote_request && data.len() == 1 && data[0] == 1 {
            esp_hal::system::software_reset();
        }
    }
}
