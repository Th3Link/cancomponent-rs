//! A shared GPIO interrupt handler fanning out edge events to per-pin
//! channels, since `esp-hal` only allows one ISR to be registered for all
//! of `IO_MUX`. [`register_gpio_handler`] claims a slot for an already
//! interrupt-configured [`Input`]; the ISR then just checks which slots'
//! pins triggered and forwards `true`/`false` (pin level, active-low) into
//! that slot's channel.

use core::cell::RefCell;
use critical_section::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use esp_hal::gpio::Input;
use esp_hal::gpio::Io;
use esp_hal::handler;
use esp_hal::peripherals::IO_MUX;
use esp_hal::ram;
use thiserror::Error;

/// Upper bound on concurrently-registered GPIO interrupt pins (4 buttons +
/// headroom).
const MAX_HANDLERS: usize = 10;

/// Carries pin level on each edge: `true` = low (active, given the
/// pull-up/active-low wiring used throughout this crate), `false` = high.
pub type GpioChannel = Channel<CriticalSectionRawMutex, bool, 4>;

#[derive(Debug, Error)]
pub enum GpioInterruptError {
    #[error("No free slot. increase MAX_HANDLERS")]
    Full,
}
struct GpioHandlerSlot {
    input: Mutex<RefCell<Option<Input<'static>>>>,
    channel: GpioChannel,
}

impl GpioHandlerSlot {
    const fn new() -> Self {
        Self {
            input: Mutex::new(RefCell::new(None)),
            channel: Channel::new(),
        }
    }
}
static GPIO_HANDLERS: [GpioHandlerSlot; MAX_HANDLERS] = [
    GpioHandlerSlot::new(),
    GpioHandlerSlot::new(),
    GpioHandlerSlot::new(),
    GpioHandlerSlot::new(),
    GpioHandlerSlot::new(),
    GpioHandlerSlot::new(),
    GpioHandlerSlot::new(),
    GpioHandlerSlot::new(),
    GpioHandlerSlot::new(),
    GpioHandlerSlot::new(),
];

/// Installs the shared ISR. Call once at boot, before any
/// [`register_gpio_handler`] calls.
pub fn init(io_mux: IO_MUX<'static>) {
    let mut io = Io::new(io_mux);
    io.set_interrupt_handler(gpio_isr_handler);
}

/// Claims a free slot for `input` (which must already have its interrupt
/// configured, e.g. via `Input::listen`) and returns its event channel.
/// Errors with [`GpioInterruptError::Full`] if all `MAX_HANDLERS` slots are
/// taken.
pub fn register_gpio_handler(
    input: Input<'static>,
) -> Result<&'static GpioChannel, GpioInterruptError> {
    critical_section::with(|cs| {
        for slot in GPIO_HANDLERS.iter() {
            let mut input_opt = slot.input.borrow_ref_mut(cs);
            if input_opt.is_none() {
                input_opt.replace(input);
                return Ok(&slot.channel);
            }
        }
        Err(GpioInterruptError::Full)
    })
}

#[handler]
#[ram]
fn gpio_isr_handler() {
    critical_section::with(|cs| {
        for slot in GPIO_HANDLERS.iter() {
            if let Some(input) = slot.input.borrow_ref_mut(cs).as_mut() {
                if input.is_interrupt_set() {
                    let active = input.is_low(); // Active Low
                    slot.channel.try_send(active).ok();
                    input.clear_interrupt();
                }
            }
        }
    });
}
