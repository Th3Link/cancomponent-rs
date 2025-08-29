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

const MAX_HANDLERS: usize = 10;

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

pub fn init(io_mux: IO_MUX<'static>) {
    let mut io = Io::new(io_mux);
    io.set_interrupt_handler(gpio_isr_handler);
}

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
