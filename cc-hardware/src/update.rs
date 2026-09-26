//! OTA firmware update over CAN (`FlashStart`/`FlashWrite`/`FlashComplete`/...).
//!
//! Wire protocol is fixed and mirrors what a bus master (e.g. a flashing
//! tool) expects: `start` carries a CRC32 + size, `write` streams raw
//! firmware bytes buffered until a chunk boundary or `FlashComplete`, at
//! which point the image is verified and the device reboots into it.
use crate::console_log;
use crate::error::{report_error, Component, ErrorCode, Severity};
use crate::flash::SharedFlash;
use cancomponents_core::can_id::CanId;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use esp_hal_ota::Ota;
use heapless::Vec;
use num_enum::FromPrimitive;

const OTA_BUFFER_SIZE: usize = 4096;
/// Bytes received via `FlashWrite` accumulate here until a full chunk (or
/// `FlashComplete`) triggers `ota_write_chunk`.
static CURRENT_BUFFER: Mutex<CriticalSectionRawMutex, Vec<u8, OTA_BUFFER_SIZE>> =
    Mutex::new(Vec::new());
static UPDATE: Mutex<CriticalSectionRawMutex, Option<Update>> = Mutex::new(None);
static OTA: Mutex<CriticalSectionRawMutex, Option<Ota<SharedFlash>>> = Mutex::new(None);

/// `local_code` values reported alongside `Component::Ota`/`Component::Update`
/// `DeviceError`s, distinguishing where in the OTA flow something failed.
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
pub enum UpdateErrorCode {
    #[num_enum(default)]
    Unknown = 0,
    InvalidData = 1,
    Begin = 2,
    Init = 3,
    Write = 4,
    NotStarted = 5,
    VerifyFailed = 6,
}

/// Marks the currently-running app partition valid (confirming a prior OTA
/// update didn't need a rollback) and prepares the `Update` singleton.
pub async fn init(_spawner: &Spawner) {
    let mut update_guard = UPDATE.lock().await;

    if update_guard.is_none() {
        let update = Update {};
        *update_guard = Some(update);
    }

    match Ota::new(SharedFlash) {
        Ok(mut ota) => {
            ota.ota_mark_app_valid().ok();
        }
        Err(_) => {
            report_error(
                Component::Ota,
                ErrorCode::Unknown,
                Severity::RecoverableError,
                UpdateErrorCode::Init as u8,
                &[0u8, 0u8, 0u8],
            )
            .await;
        }
    }
}

pub async fn update(
) -> embassy_sync::mutex::MappedMutexGuard<'static, CriticalSectionRawMutex, Update> {
    let guard = UPDATE.lock().await;
    embassy_sync::mutex::MutexGuard::map(guard, |opt| opt.as_mut().expect("Update not initialized"))
}

pub struct Update {}

impl Update {
    /// Handles `FlashStart`: payload is `[crc32(be), size(be)]` (8 bytes),
    /// begins an OTA write to the inactive partition.
    pub async fn start(&mut self, id: CanId, data: &[u8], _remote_request: bool) {
        if data.len() < 8 {
            report_error(
                Component::Update,
                ErrorCode::InvalidData,
                Severity::Warning,
                UpdateErrorCode::InvalidData as u8,
                &[id.msg_type as u8, data.len() as u8, 0u8],
            )
            .await;
            return;
        }

        let size = u32::from_be_bytes(data[4..8].try_into().unwrap());
        let crc = u32::from_be_bytes(data[0..4].try_into().unwrap());
        console_log!("start update: crc {crc} size {size}");

        match Ota::new(SharedFlash) {
            Ok(mut ota) => {
                if ota.ota_begin(size, crc).is_ok() {
                    let next_ota = ota.get_next_ota_partition();
                    console_log!("next ota part: {next_ota:?}");
                    *OTA.lock().await = Some(ota);
                } else {
                    // ota_begin fehlgeschlagen
                    report_error(
                        Component::Ota,
                        ErrorCode::Unknown,
                        Severity::RecoverableError,
                        UpdateErrorCode::Begin as u8,
                        &[0u8, 0u8, 0u8],
                    )
                    .await;
                    *OTA.lock().await = None;
                }
            }
            Err(_) => {
                report_error(
                    Component::Ota,
                    ErrorCode::Unknown,
                    Severity::RecoverableError,
                    UpdateErrorCode::Init as u8,
                    &[0u8, 0u8, 0u8],
                )
                .await;
                // OTA-Initialisierung fehlgeschlagen
                *OTA.lock().await = None;
            }
        }
    }
    /// Handles `FlashWrite`/`FlashComplete`: buffers raw firmware bytes and
    /// flushes to flash once `OTA_BUFFER_SIZE` is reached or `force_flush`
    /// (i.e. this is the final `FlashComplete` chunk) is set — at which
    /// point the image is verified and, on success, the device reboots
    /// into it.
    pub async fn write(
        &mut self,
        _id: CanId,
        data: &[u8],
        _remote_request: bool,
        force_flush: bool,
    ) {
        let mut buffer = CURRENT_BUFFER.lock().await;
        let should_flush = {
            if buffer.extend_from_slice(data).is_err() {
                true // Buffer voll -> sofort flushen
            } else {
                force_flush || buffer.len() == OTA_BUFFER_SIZE
            }
        };

        if should_flush {
            console_log!("write chunk");
            let mut ota_guard = OTA.lock().await;
            if let Some(ref mut ota) = *ota_guard {
                match ota.ota_write_chunk(&buffer) {
                    Ok(true) => {
                        console_log!("last chunk");
                        if ota
                            .ota_flush(true, true)
                            .inspect_err(|e| {
                                console_log!("{e:?}");
                            })
                            .is_err()
                        {
                            report_error(
                                Component::Update,
                                ErrorCode::InvalidData,
                                Severity::Warning,
                                UpdateErrorCode::VerifyFailed as u8,
                                &[0u8, 0u8, 0u8],
                            )
                            .await;
                        } else {
                            esp_hal::system::software_reset();
                        }
                    }
                    Ok(false) => {
                        // continue writing
                    }
                    Err(e) => {
                        console_log!("Write failed: {:?}", e);
                        report_error(
                            Component::Ota,
                            ErrorCode::Unknown,
                            Severity::RecoverableError,
                            UpdateErrorCode::Write as u8,
                            &[0u8, 0u8, 0u8],
                        )
                        .await;
                    }
                }
            }

            buffer.clear();
        }
    }

    pub async fn progress(&mut self, _id: CanId, _data: &[u8], _remote_request: bool) {}
    pub async fn select(&mut self, _id: CanId, _data: &[u8], _remote_request: bool) {}
    pub async fn erase(&mut self, _id: CanId, _data: &[u8], _remote_request: bool) {}
    pub async fn read(&mut self, _id: CanId, _data: &[u8], _remote_request: bool) {}
    pub async fn verify(&mut self, _id: CanId, _data: &[u8], _remote_request: bool) {}
}
