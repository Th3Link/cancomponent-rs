//! Sends `DeviceError` CAN reports, deduplicated and rate-limited so a
//! persistent fault doesn't flood the bus.
//!
//! The wire format itself (`Component`/`ErrorCode`/`Severity`/`ErrorReport`)
//! lives in `cancomponents_core::error` and is re-exported here so callers
//! don't need to depend on both crates for one concept.

use crate::can::send_can_message;
use cancomponents_core::can_message_type::CanMessageType;
pub use cancomponents_core::error::{Component, ErrorCode, ErrorReport, Severity};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Instant};
use heapless::index_map::FnvIndexMap;

type ErrorKey = (Component, ErrorCode, u8);

const MAX_TRACKED_ERRORS: usize = 16;
/// Reports with the same (component, code, local_code) key are suppressed
/// for this long after the first one is sent.
const DEDUP_WINDOW: Duration = Duration::from_secs(1);

static ERROR_TIMESTAMPS: Mutex<
    CriticalSectionRawMutex,
    FnvIndexMap<ErrorKey, Instant, MAX_TRACKED_ERRORS>,
> = Mutex::new(FnvIndexMap::new());

/// Sends a `DeviceError` report over CAN, unless an identical
/// (component, code, local_code) report was already sent within the last
/// second.
pub async fn report_error(
    component: Component,
    code: ErrorCode,
    severity: Severity,
    local_code: u8,
    details: &[u8],
) {
    let key = (component, code, local_code);
    let now = Instant::now();
    {
        let mut map = ERROR_TIMESTAMPS.lock().await;
        match map.get(&key) {
            Some(&last) if now.duration_since(last) < DEDUP_WINDOW => return,
            _ => {
                let _ = map.insert(key, now);
            }
        }
    }

    let report = ErrorReport::new(component, code, severity, local_code, details);
    send_can_message(CanMessageType::DeviceError, &report.to_bytes(), false).await;
}
