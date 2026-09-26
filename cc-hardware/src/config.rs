//! Persistent key/value config storage, backed by `sequential-storage` over
//! raw flash (the `CONFIG_PARTITION` range, separate from the OTA app
//! partitions).

use crate::flash::SharedFlash;
use core::ops::Range;
use core::result::Result;
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use heapless::String;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use sequential_storage::cache::{Cache, Uncached};
use sequential_storage::map::{MapConfig, MapStorage};

pub const CONFIG_PARTITION: Range<u32> = 0x9000..0xFC000;

pub static CONFIG: Mutex<CriticalSectionRawMutex, Option<Config>> = Mutex::new(None);

/// Keys for the single-byte/short-string values persisted in flash. The
/// discriminant is the on-flash key, not just a Rust implementation detail
/// — do not renumber existing entries.
#[derive(Copy, Clone, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum Key {
    RelaisMode = 1,
    ExtensionMode = 2,
    DeviceId = 3,
    DeviceType = 4,
    CustomString = 5,
    /// Stored but currently unused — see the note in `can::init`.
    Baudrate = 6,
    HardwareRevision = 7,
}

/// Initializes the `CONFIG` singleton. Idempotent; safe to call more than
/// once (e.g. from multiple init paths).
pub async fn init() {
    let mut config_guard = CONFIG.lock().await;

    if config_guard.is_none() {
        let config = Config::new();
        *config_guard = Some(config);
    }
}

/// Locks and returns the `CONFIG` singleton. Panics if [`init`] hasn't run
/// yet.
pub async fn config(
) -> embassy_sync::mutex::MappedMutexGuard<'static, CriticalSectionRawMutex, Config> {
    let guard = CONFIG.lock().await;
    embassy_sync::mutex::MutexGuard::map(guard, |opt| opt.as_mut().expect("Config not initialized"))
}

type ConfigCache = Cache<Uncached, Uncached, Uncached, u8>;

pub struct Config {
    map: MapStorage<u8, BlockingAsync<SharedFlash>, ConfigCache>,
    buffer: [u8; 256],
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            map: MapStorage::new(
                BlockingAsync::new(SharedFlash),
                MapConfig::new(CONFIG_PARTITION),
                Cache::new_uncached(),
            ),
            buffer: [0; 256],
        }
    }

    pub async fn get_str<const N: usize>(&mut self, key: Key) -> Option<String<N>> {
        let raw: &[u8] = self
            .map
            .fetch_item(&mut self.buffer, &(key as u8))
            .await
            .ok()
            .flatten()?;
        let mut string = String::<N>::new();
        let s = core::str::from_utf8(raw).ok()?;
        string.push_str(s).ok()?;
        Some(string)
    }

    pub async fn set_str<const N: usize>(&mut self, key: Key, value: &String<N>) -> Result<(), ()> {
        self.map
            .store_item(&mut self.buffer, &(key as u8), &value.as_bytes())
            .await
            .map_err(|_| ())
    }

    /// Hole z.B. eine u32 (z. B. Counter etc.)
    pub async fn get_u32(&mut self, key: Key) -> Option<u32> {
        self.map
            .fetch_item(&mut self.buffer, &(key as u8))
            .await
            .ok()
            .flatten()
    }

    pub async fn set_u32(&mut self, key: Key, value: u32) -> Result<(), ()> {
        self.map
            .store_item(&mut self.buffer, &(key as u8), &value)
            .await
            .map_err(|_| ())
    }

    pub async fn get_u8(&mut self, key: Key) -> Option<u8> {
        self.map
            .fetch_item(&mut self.buffer, &(key as u8))
            .await
            .ok()
            .flatten()
    }

    pub async fn set_u8(&mut self, key: Key, value: u8) -> Result<(), ()> {
        self.map
            .store_item(&mut self.buffer, &(key as u8), &value)
            .await
            .map_err(|_| ())
    }
}
