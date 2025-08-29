use crate::error::{Component, ErrorCode, ErrorReport, Severity};
use cancomponents_core::can_id::CanId;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use esp_hal_ota::Ota;
use esp_println::println;
use esp_storage::FlashStorage;
use heapless::Vec;
use num_enum::FromPrimitive;

// Buffer-Größe als Konstanto
const OTA_BUFFER_SIZE: usize = 4096;
static CURRENT_BUFFER: Mutex<CriticalSectionRawMutex, Vec<u8, OTA_BUFFER_SIZE>> =
    Mutex::new(Vec::new());
static UPDATE: Mutex<CriticalSectionRawMutex, Option<Update>> = Mutex::new(None);
static OTA: Mutex<CriticalSectionRawMutex, Option<Ota<FlashStorage>>> = Mutex::new(None);

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

pub async fn init(_spawner: &Spawner) {
    let mut update_guard = UPDATE.lock().await;

    if update_guard.is_none() {
        let update = Update {};
        *update_guard = Some(update);
    }
}

pub async fn update(
) -> embassy_sync::mutex::MappedMutexGuard<'static, CriticalSectionRawMutex, Update> {
    let guard = UPDATE.lock().await;
    embassy_sync::mutex::MutexGuard::map(guard, |opt| opt.as_mut().expect("Update not initialized"))
}

pub struct Update {}

impl Update {
    pub async fn start(&mut self, id: CanId, data: &[u8], _remote_request: bool) {
        if data.len() < 8 {
            ErrorReport::send(
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
        println!("start update: crc {crc} size {size}");

        match Ota::new(FlashStorage::new()) {
            Ok(mut ota) => {
                if ota.ota_begin(size, crc).is_ok() {
                    let next_ota = ota.get_next_ota_partition();
                    println!("next ota part: {next_ota:?}");
                    *OTA.lock().await = Some(ota);
                } else {
                    // ota_begin fehlgeschlagen
                    ErrorReport::send(
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
                ErrorReport::send(
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
            println!("write chunk");
            let mut ota_guard = OTA.lock().await;
            if let Some(ref mut ota) = *ota_guard {
                match ota.ota_write_chunk(&buffer) {
                    Ok(true) => {
                        println!("last chunk");
                        if ota
                            .ota_flush(true, true)
                            .inspect_err(|e| {
                                println!("{e:?}");
                            })
                            .is_err()
                        {
                            ErrorReport::send(
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
                        println!("Write failed: {:?}", e);
                        ErrorReport::send(
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
