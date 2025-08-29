use crate::can::send_can_message;
use crate::gpio_interrupt::register_gpio_handler;
use crate::gpio_interrupt::GpioChannel;
use cancomponents_core::button_message::ButtonMessage;
use cancomponents_core::button_message::ButtonState;
use cancomponents_core::can_message_type::CanMessageType;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};
use esp_hal::gpio::Event;
use esp_hal::gpio::Input;
use esp_hal::gpio::InputConfig;
use esp_hal::gpio::Pull;
use esp_println::println;

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
        spawner: &Spawner,
    ) {
        let config = InputConfig::default().with_pull(Pull::Up);
        let mut button0 = Input::new(button0, config);
        let mut button1 = Input::new(button1, config);
        let mut button2 = Input::new(button2, config);
        let mut button3 = Input::new(button3, config);

        button0.listen(Event::AnyEdge);
        button1.listen(Event::AnyEdge);
        button2.listen(Event::AnyEdge);
        button3.listen(Event::AnyEdge);

        let ch0 = register_gpio_handler(button0).unwrap();
        let ch1 = register_gpio_handler(button1).unwrap();
        let ch2 = register_gpio_handler(button2).unwrap();
        let ch3 = register_gpio_handler(button3).unwrap();

        spawner.spawn(run(0, ch0)).unwrap();
        spawner.spawn(run(1, ch1)).unwrap();
        spawner.spawn(run(2, ch2)).unwrap();
        spawner.spawn(run(3, ch3)).unwrap();
    }

    pub async fn iterate(&mut self, index: usize, channel: &GpioChannel) {
        let debounce_time = Timer::after(DEBOUNCE_TIME);
        let next_state = channel.receive();
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
                let next_state = channel.receive().await;
                if next_state {
                    self.state = ButtonState::Pressed;
                    let bm = ButtonMessage::new(index, ButtonState::Pressed, 0);
                    send_can_message(CanMessageType::ButtonEvent, &bm.to_bytes(), false).await;
                }
            }
            ButtonState::Pressed => {
                let hold_threshold = Timer::after(HOLD_THRESHOLD);
                let next_state = channel.receive();
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
                let next_state = channel.receive();
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
                let next_state = channel.receive();
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
pub async fn run(index: usize, channel: &'static GpioChannel) {
    let mut button = Button {
        clicks: 0,
        hold_repeat: 0,
        state: ButtonState::Released,
    };
    loop {
        button.iterate(index, channel).await;
    }
}
