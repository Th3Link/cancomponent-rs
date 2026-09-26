//! Drives up to 4 physical buttons: debounces GPIO edges, races them against
//! state-specific timeouts, and reports the result over CAN.
//!
//! The press/hold/multi-click decision logic itself lives in
//! `cancomponents_core::button_fsm::ButtonFsm` (pure, unit-tested); this
//! module only owns the timing/hardware side — which timeout to race a
//! given state against, and turning the FSM's decisions into GPIO setup and
//! CAN sends.

use crate::can::send_can_message;
use crate::console_log;
use crate::gpio_interrupt::register_gpio_handler;
use crate::gpio_interrupt::GpioChannel;
use cancomponents_core::button_fsm::{ButtonEdge, ButtonFsm};
use cancomponents_core::button_message::ButtonState;
use cancomponents_core::can_message_type::CanMessageType;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};
use esp_hal::gpio::Event;
use esp_hal::gpio::Input;
use esp_hal::gpio::InputConfig;
use esp_hal::gpio::Pull;

const DEBOUNCE_TIME: Duration = Duration::from_millis(10);
const MULTI_CLICK_MAX: Duration = Duration::from_millis(200); // window to catch a follow-up click
const HOLD_THRESHOLD: Duration = Duration::from_millis(800); // press duration that counts as "hold"
const HOLD_REPEAT: Duration = Duration::from_millis(1000); // repeat interval while held

pub struct Button {
    fsm: ButtonFsm,
}

impl Button {
    /// Configures 4 GPIOs as pulled-up, edge-interrupt inputs and spawns one
    /// `run` task per button.
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

        spawner.spawn(run(0, ch0).unwrap());
        spawner.spawn(run(1, ch1).unwrap());
        spawner.spawn(run(2, ch2).unwrap());
        spawner.spawn(run(3, ch3).unwrap());
    }

    /// Waits for (and debounces) one GPIO edge, races it against the
    /// current FSM state's timeout (if any), and sends whatever
    /// [`ButtonFsm`] decides to emit.
    pub async fn iterate(&mut self, index: usize, channel: &GpioChannel) {
        let debounce_time = Timer::after(DEBOUNCE_TIME);
        let next_state = channel.receive();
        match select(next_state, debounce_time).await {
            Either::First(_) => {
                // bounced, discard and retry
                return;
            }
            Either::Second(_) => {
                // debounced, go on
            }
        }

        let prev_state = self.fsm.state();
        let message = if prev_state == ButtonState::Released {
            // No timeout to race: a released button only ever reacts to an
            // edge.
            let pressed = channel.receive().await;
            self.fsm.on_edge(edge(pressed))
        } else {
            let timeout = match prev_state {
                ButtonState::Pressed => HOLD_THRESHOLD,
                ButtonState::Hold => HOLD_REPEAT,
                _ => MULTI_CLICK_MAX,
            };
            match select(channel.receive(), Timer::after(timeout)).await {
                Either::First(pressed) => self.fsm.on_edge(edge(pressed)),
                Either::Second(_) => self.fsm.on_timeout(),
            }
        };

        if let Some(msg) = message {
            match (prev_state, msg.state) {
                (ButtonState::Pressed, ButtonState::Hold) => console_log!("Button {index}: hold"),
                (ButtonState::Hold, ButtonState::Hold) => {
                    console_log!("Button {index}: hold_repeat {}", msg.count)
                }
                (ButtonState::Hold, ButtonState::Released) => {
                    console_log!("Button {index}: hold released")
                }
                (ButtonState::Multi, ButtonState::Multi) => {
                    console_log!("Button {index}: multi press {}", msg.count)
                }
                _ => {}
            }
            send_can_message(CanMessageType::ButtonEvent, &msg.to_bytes(), false).await;
        }
    }
}

fn edge(pressed: bool) -> ButtonEdge {
    if pressed {
        ButtonEdge::Pressed
    } else {
        ButtonEdge::Released
    }
}

#[embassy_executor::task(pool_size = 4)]
pub async fn run(index: usize, channel: &'static GpioChannel) {
    let mut button = Button {
        fsm: ButtonFsm::new(index),
    };
    loop {
        button.iterate(index, channel).await;
    }
}
