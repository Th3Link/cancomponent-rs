use crate::can;
use cancomponents_core::button_message::ButtonMessage;
use cancomponents_core::button_message::ButtonState;
use cancomponents_core::can_message_type::CanMessageType;
use core::cell::RefCell;
use critical_section::Mutex;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Timer};
use esp_hal::gpio::Event;
use esp_hal::gpio::Input;
use esp_hal::gpio::InputConfig;
use esp_hal::gpio::Io;
use esp_hal::gpio::Pull;
use esp_hal::handler;
use esp_hal::peripherals::IO_MUX;
use esp_hal::ram;
use esp_println::println;

use crate::can::send_can_message;

static BUTTONS: [(
    Mutex<RefCell<Option<Input>>>,
    Channel<CriticalSectionRawMutex, bool, 4>,
); 4] = [
    (Mutex::new(RefCell::new(None)), Channel::new()),
    (Mutex::new(RefCell::new(None)), Channel::new()),
    (Mutex::new(RefCell::new(None)), Channel::new()),
    (Mutex::new(RefCell::new(None)), Channel::new()),
];

const DEBOUNCE_TIME: Duration = Duration::from_millis(10); // Entprellzeit
const MULTI_CLICK_MAX: Duration = Duration::from_millis(200); // Zeitfenster für Double/Triple/Quad
const HOLD_THRESHOLD: Duration = Duration::from_millis(800); // Ab wann "Hold"
const HOLD_REPEAT: Duration = Duration::from_millis(1000); // Ab wann "Hold"

pub struct Button {
    clicks: u16,
    hold_repeat: u16,
    state: ButtonState,
}

impl Button {
    pub fn init(
        button0: impl esp_hal::gpio::InputPin + 'static,
        button1: impl esp_hal::gpio::InputPin + 'static,
        button2: impl esp_hal::gpio::InputPin + 'static,
        button3: impl esp_hal::gpio::InputPin + 'static,
        io_mux: IO_MUX<'static>,
        spawner: &Spawner,
    ) {
        let mut io = Io::new(io_mux);
        io.set_interrupt_handler(handler);

        let config = InputConfig::default().with_pull(Pull::Up);
        let mut button0 = Input::new(button0, config);
        let mut button1 = Input::new(button1, config);
        let mut button2 = Input::new(button2, config);
        let mut button3 = Input::new(button3, config);

        critical_section::with(|cs| {
            button0.listen(Event::AnyEdge);
            BUTTONS[0].0.borrow_ref_mut(cs).replace(button0);

            button1.listen(Event::AnyEdge);
            BUTTONS[1].0.borrow_ref_mut(cs).replace(button1);

            button2.listen(Event::AnyEdge);
            BUTTONS[2].0.borrow_ref_mut(cs).replace(button2);

            button3.listen(Event::AnyEdge);
            BUTTONS[3].0.borrow_ref_mut(cs).replace(button3);
        });

        spawner.spawn(run(0)).unwrap();
        spawner.spawn(run(1)).unwrap();
        spawner.spawn(run(2)).unwrap();
        spawner.spawn(run(3)).unwrap();
    }

    pub async fn iterate(&mut self, index: usize) {
        let debounce_time = Timer::after(DEBOUNCE_TIME);
        let next_state = BUTTONS[index].1.receive();
        match select(next_state, debounce_time).await {
            Either::First(_) => {
                //bounces
                return;
            }
            Either::Second(_) => {
                // debounced go on
            }
        }

        match self.state {
            ButtonState::Released => {
                let next_state = BUTTONS[index].1.receive().await;
                if next_state {
                    self.state = ButtonState::Pressed;
                    let bm = ButtonMessage::new(index, ButtonState::Pressed, 0);
                    send_can_message(CanMessageType::ButtonEvent, &bm.to_bytes(), false).await;
                }
            }
            ButtonState::Pressed => {
                let hold_threshold = Timer::after(HOLD_THRESHOLD);
                let next_state = BUTTONS[index].1.receive();
                match select(next_state, hold_threshold).await {
                    Either::First(_) => {
                        self.clicks += 1;
                        self.state = ButtonState::Multi;
                    }
                    Either::Second(_) => {
                        self.state = ButtonState::Hold;
                        let bm = ButtonMessage::new(index, ButtonState::Hold, 0);
                        send_can_message(CanMessageType::ButtonEvent, &bm.to_bytes(), false).await;
                        println!("Button {index}: hold");
                    }
                }
            }
            ButtonState::Hold => {
                let hold_repeat = Timer::after(HOLD_REPEAT);
                let next_state = BUTTONS[index].1.receive();
                match select(next_state, hold_repeat).await {
                    Either::First(_) => {
                        self.state = ButtonState::Released;
                        let bm = ButtonMessage::new(index, ButtonState::Released, 0);
                        send_can_message(CanMessageType::ButtonEvent, &bm.to_bytes(), false).await;
                        println!("Button {index}: hold released");
                        self.hold_repeat = 0;
                    }
                    Either::Second(_) => {
                        self.hold_repeat += 1;
                        let bm = ButtonMessage::new(index, ButtonState::Hold, self.hold_repeat);
                        send_can_message(CanMessageType::ButtonEvent, &bm.to_bytes(), false).await;
                        println!("Button {index}: hold_repeat {}", self.hold_repeat);
                    }
                }
            }
            ButtonState::Multi => {
                let hold_repeat = Timer::after(MULTI_CLICK_MAX);
                let next_state = BUTTONS[index].1.receive();
                match select(next_state, hold_repeat).await {
                    Either::First(s) => {
                        if s {
                            self.clicks += 1;
                            let bm = ButtonMessage::new(index, ButtonState::Pressed, 0);
                            send_can_message(CanMessageType::ButtonEvent, &bm.to_bytes(), false)
                                .await;
                        } else {
                            let bm = ButtonMessage::new(index, ButtonState::Released, 0);
                            send_can_message(CanMessageType::ButtonEvent, &bm.to_bytes(), false)
                                .await;
                        }
                    }
                    Either::Second(_) => {
                        self.state = ButtonState::Released;
                        println!("Button {index}: multi press {}", self.clicks);
                        let bm = ButtonMessage::new(index, ButtonState::Multi, self.clicks);
                        send_can_message(CanMessageType::ButtonEvent, &bm.to_bytes(), false).await;
                        self.clicks = 0;
                    }
                }
            }
            _ => {}
        }
    }
}

#[embassy_executor::task(pool_size = 4)]
pub async fn run(index: usize) {
    let mut button = Button {
        clicks: 0,
        hold_repeat: 0,
        state: ButtonState::Released,
    };
    loop {
        button.iterate(index).await;
    }
}

#[handler]
#[ram]
fn handler() {
    critical_section::with(|cs| {
        for (_i, cell) in BUTTONS.iter().enumerate() {
            if let Some(btn) = cell.0.borrow_ref_mut(cs).as_mut() {
                if btn.is_interrupt_set() {
                    let pressed = btn.is_low(); // Pull-Up: Low = gedrückt
                    cell.1.try_send(pressed).ok();
                    btn.clear_interrupt();
                }
            }
        }
    });
}
