//! Pure press/hold/multi-click decision logic for a physical button.
//!
//! This is deliberately hardware-free: it only decides *what state to move
//! to* and *what [`ButtonMessage`] to emit* in response to an edge or a
//! timeout. The caller owns all timing (debounce, which timeout to race
//! against a GPIO edge) and is expected to drive the FSM like this:
//!
//! - In [`ButtonState::Released`], wait for an edge only (no timeout race)
//!   and call [`ButtonFsm::on_edge`].
//! - In every other state, race a GPIO edge against a state-specific
//!   timeout (hold threshold / hold repeat / multi-click window) and call
//!   [`ButtonFsm::on_edge`] or [`ButtonFsm::on_timeout`] accordingly.

use crate::button_message::{ButtonMessage, ButtonState};

/// A debounced GPIO transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEdge {
    Pressed,
    Released,
}

pub struct ButtonFsm {
    index: usize,
    state: ButtonState,
    clicks: u16,
    hold_repeat: u16,
}

impl ButtonFsm {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            state: ButtonState::Released,
            clicks: 0,
            hold_repeat: 0,
        }
    }

    pub fn state(&self) -> ButtonState {
        self.state
    }

    /// Feed a debounced GPIO edge to the FSM. Returns a message to send, if
    /// this edge causes one.
    pub fn on_edge(&mut self, edge: ButtonEdge) -> Option<ButtonMessage> {
        match self.state {
            ButtonState::Released => {
                if edge == ButtonEdge::Pressed {
                    self.state = ButtonState::Pressed;
                    Some(ButtonMessage::new(self.index, ButtonState::Pressed, 0))
                } else {
                    None
                }
            }
            ButtonState::Pressed => {
                // Debounce already guarantees the only edge reachable here
                // is the release that ends the first click, so the edge
                // direction itself doesn't need checking.
                self.clicks += 1;
                self.state = ButtonState::Multi;
                None
            }
            ButtonState::Hold => {
                self.state = ButtonState::Released;
                self.hold_repeat = 0;
                Some(ButtonMessage::new(self.index, ButtonState::Released, 0))
            }
            ButtonState::Multi => match edge {
                ButtonEdge::Pressed => {
                    self.clicks += 1;
                    Some(ButtonMessage::new(self.index, ButtonState::Pressed, 0))
                }
                ButtonEdge::Released => {
                    Some(ButtonMessage::new(self.index, ButtonState::Released, 0))
                }
            },
            _ => None,
        }
    }

    /// Feed a state-specific timeout (hold threshold / hold repeat /
    /// multi-click window) to the FSM. Returns a message to send, if this
    /// timeout causes one.
    pub fn on_timeout(&mut self) -> Option<ButtonMessage> {
        match self.state {
            ButtonState::Pressed => {
                self.state = ButtonState::Hold;
                Some(ButtonMessage::new(self.index, ButtonState::Hold, 0))
            }
            ButtonState::Hold => {
                self.hold_repeat += 1;
                Some(ButtonMessage::new(
                    self.index,
                    ButtonState::Hold,
                    self.hold_repeat,
                ))
            }
            ButtonState::Multi => {
                self.state = ButtonState::Released;
                let clicks = self.clicks;
                self.clicks = 0;
                Some(ButtonMessage::new(self.index, ButtonState::Multi, clicks))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_click() {
        let mut fsm = ButtonFsm::new(0);
        assert_eq!(
            fsm.on_edge(ButtonEdge::Pressed).unwrap().state,
            ButtonState::Pressed
        );
        assert!(fsm.on_edge(ButtonEdge::Released).is_none());
        assert_eq!(fsm.state(), ButtonState::Multi);
        // Multi-click window expires with exactly one click recorded.
        let msg = fsm.on_timeout().unwrap();
        assert_eq!(msg.state, ButtonState::Single);
        assert_eq!(fsm.state(), ButtonState::Released);
    }

    #[test]
    fn double_click() {
        let mut fsm = ButtonFsm::new(0);
        fsm.on_edge(ButtonEdge::Pressed);
        fsm.on_edge(ButtonEdge::Released); // -> Multi, clicks = 1
        fsm.on_edge(ButtonEdge::Pressed); // second press within window
        fsm.on_edge(ButtonEdge::Released); // second release within window
        let msg = fsm.on_timeout().unwrap();
        assert_eq!(msg.state, ButtonState::Double);
    }

    #[test]
    fn hold_then_repeat_then_release() {
        let mut fsm = ButtonFsm::new(2);
        fsm.on_edge(ButtonEdge::Pressed);
        let hold_msg = fsm.on_timeout().unwrap();
        assert_eq!(hold_msg.state, ButtonState::Hold);
        assert_eq!(hold_msg.count, 0);
        assert_eq!(fsm.state(), ButtonState::Hold);

        let repeat1 = fsm.on_timeout().unwrap();
        assert_eq!(repeat1.state, ButtonState::Hold);
        assert_eq!(repeat1.count, 1);

        let repeat2 = fsm.on_timeout().unwrap();
        assert_eq!(repeat2.count, 2);

        let released = fsm.on_edge(ButtonEdge::Released).unwrap();
        assert_eq!(released.state, ButtonState::Released);
        assert_eq!(fsm.state(), ButtonState::Released);
    }

    #[test]
    fn five_clicks_falls_back_to_multi_not_a_crash() {
        let mut fsm = ButtonFsm::new(0);
        fsm.on_edge(ButtonEdge::Pressed);
        fsm.on_edge(ButtonEdge::Released); // clicks = 1
        for _ in 0..4 {
            fsm.on_edge(ButtonEdge::Pressed); // clicks += 1
            fsm.on_edge(ButtonEdge::Released);
        }
        // clicks == 5 now
        let msg = fsm.on_timeout().unwrap();
        assert_eq!(msg.state, ButtonState::Multi);
        assert_eq!(msg.count, 5);
    }

    #[test]
    fn released_ignores_release_edge() {
        let mut fsm = ButtonFsm::new(0);
        assert!(fsm.on_edge(ButtonEdge::Released).is_none());
        assert_eq!(fsm.state(), ButtonState::Released);
    }
}
