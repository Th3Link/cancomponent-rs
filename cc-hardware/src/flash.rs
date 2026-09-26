//! Shared access to the on-chip SPI flash.
//!
//! `esp-storage`'s `FlashStorage` now wraps the `FLASH` peripheral itself
//! (an exclusively-owned singleton, obtained once from `esp_hal::init`), so
//! only one `FlashStorage` may ever exist. `config` (persisted settings, via
//! `sequential-storage`) and `update` (OTA writes, via `esp-hal-ota`) both
//! need flash access at arbitrary, independent times, so this module owns
//! the single real `FlashStorage` behind a critical-section-guarded cell and
//! hands out cheap [`SharedFlash`] handles that delegate to it.

use core::cell::RefCell;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use embedded_storage::{ReadStorage, Storage};
use esp_storage::{FlashStorage, FlashStorageError};

static FLASH: Mutex<CriticalSectionRawMutex, RefCell<Option<FlashStorage<'static>>>> =
    Mutex::new(RefCell::new(None));

/// Takes ownership of the `FLASH` peripheral. Must be called exactly once,
/// before any [`SharedFlash`] is used.
pub fn init(flash: esp_hal::peripherals::FLASH<'static>) {
    FLASH.lock(|cell| {
        *cell.borrow_mut() = Some(FlashStorage::new(flash));
    });
}

/// A cheap, `Copy` handle to the single shared [`FlashStorage`]. Every
/// operation locks the underlying critical-section mutex for its duration —
/// fine here since flash access is inherently exclusive/blocking anyway.
#[derive(Clone, Copy)]
pub struct SharedFlash;

impl SharedFlash {
    fn with<R>(&self, f: impl FnOnce(&mut FlashStorage<'static>) -> R) -> R {
        FLASH.lock(|cell| {
            let mut guard = cell.borrow_mut();
            let flash = guard.as_mut().expect("flash::init not called");
            f(flash)
        })
    }
}

impl embedded_storage::nor_flash::ErrorType for SharedFlash {
    type Error = FlashStorageError;
}

impl ReadStorage for SharedFlash {
    type Error = FlashStorageError;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.with(|flash| ReadStorage::read(flash, offset, bytes))
    }

    fn capacity(&self) -> usize {
        self.with(|flash| ReadStorage::capacity(flash))
    }
}

impl Storage for SharedFlash {
    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.with(|flash| Storage::write(flash, offset, bytes))
    }
}

impl ReadNorFlash for SharedFlash {
    const READ_SIZE: usize = FlashStorage::READ_SIZE;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.with(|flash| ReadNorFlash::read(flash, offset, bytes))
    }

    fn capacity(&self) -> usize {
        self.with(|flash| ReadNorFlash::capacity(flash))
    }
}

impl NorFlash for SharedFlash {
    const WRITE_SIZE: usize = FlashStorage::WRITE_SIZE;
    const ERASE_SIZE: usize = FlashStorage::ERASE_SIZE;

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.with(|flash| NorFlash::write(flash, offset, bytes))
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.with(|flash| NorFlash::erase(flash, from, to))
    }
}

impl embedded_storage::nor_flash::MultiwriteNorFlash for SharedFlash {}
