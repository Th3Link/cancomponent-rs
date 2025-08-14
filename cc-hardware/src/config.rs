use core::ops::Range;
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use esp_storage::FlashStorage;
use heapless::String;
use sequential_storage::cache::NoCache;
use sequential_storage::map::{fetch_item, store_item};

pub const CONFIG_PARTITION: Range<u32> = 0x9000..0xFC000;

pub static CONFIG: Mutex<CriticalSectionRawMutex, Option<Config>> = Mutex::new(None);

#[repr(u8)]
#[derive(Copy, Clone)]
pub enum Key {
    RelaisMode = 1,
    ExtensionMode = 2,
    DeviceId = 3,
    DeviceType = 4,
    CustomString = 5,
    Baudrate = 6,
    HardwareRevision = 7,
}

#[repr(u8)]
#[derive(Copy, Clone)]
pub enum DeviceType {
    Button = 4,
    Relais = 5,
    Unknown = 255,
}

impl From<u8> for DeviceType {
    fn from(value: u8) -> Self {
        use DeviceType::*;
        match value {
            4 => Button,
            5 => Relais,
            _ => Unknown,
        }
    }
}

pub async fn init() {
    let mut config_guard = CONFIG.lock().await;

    if config_guard.is_none() {
        let config = Config::new();
        *config_guard = Some(config);
    }
}

pub async fn config(
) -> embassy_sync::mutex::MappedMutexGuard<'static, CriticalSectionRawMutex, Config> {
    let guard = CONFIG.lock().await;
    embassy_sync::mutex::MutexGuard::map(guard, |opt| opt.as_mut().expect("Config not initialized"))
}

pub struct Config {
    flash: BlockingAsync<FlashStorage>,
    buffer: [u8; 256],
    cache: NoCache,
}

impl Config {
    pub fn new() -> Self {
        Self {
            flash: BlockingAsync::new(FlashStorage::new()),
            buffer: [0; 256],
            cache: NoCache::new(),
        }
    }

    pub async fn get_str<const N: usize>(&mut self, key: Key) -> Option<String<N>> {
        let raw = fetch_item::<u8, &[u8], _>(
            &mut self.flash,
            CONFIG_PARTITION.clone(),
            &mut self.cache,
            &mut self.buffer,
            &(key as u8),
        )
        .await
        .ok()
        .flatten()?;
        let mut string = String::<N>::new();
        let s = core::str::from_utf8(raw).ok()?;
        string.push_str(s).ok()?;
        Some(string)
    }
    pub async fn set_str<const N: usize>(&mut self, key: Key, value: &String<N>) -> Result<(), ()> {
        store_item(
            &mut self.flash,
            CONFIG_PARTITION.clone(),
            &mut self.cache,
            &mut self.buffer,
            &(key as u8),
            &value.clone().into_bytes().as_slice(),
        )
        .await
        .map_err(|_| ())
    }

    /// Hole z.B. eine u32 (z. B. Counter etc.)
    pub async fn get_u32(&mut self, key: Key) -> Option<u32> {
        fetch_item::<u8, u32, _>(
            &mut self.flash,
            CONFIG_PARTITION.clone(),
            &mut self.cache,
            &mut self.buffer,
            &(key as u8),
        )
        .await
        .ok()
        .flatten()
    }

    pub async fn set_u32(&mut self, key: Key, value: u32) -> Result<(), ()> {
        store_item(
            &mut self.flash,
            CONFIG_PARTITION.clone(),
            &mut self.cache,
            &mut self.buffer,
            &(key as u8),
            &value,
        )
        .await
        .map_err(|_| ())
    }

    pub async fn get_u8(&mut self, key: Key) -> Option<u8> {
        fetch_item::<u8, u8, _>(
            &mut self.flash,
            CONFIG_PARTITION.clone(),
            &mut self.cache,
            &mut self.buffer,
            &(key as u8),
        )
        .await
        .ok()
        .flatten()
    }

    pub async fn set_u8(&mut self, key: Key, value: u8) -> Result<(), ()> {
        store_item(
            &mut self.flash,
            CONFIG_PARTITION.clone(),
            &mut self.cache,
            &mut self.buffer,
            &(key as u8),
            &value,
        )
        .await
        .map_err(|_| ())
    }
}
