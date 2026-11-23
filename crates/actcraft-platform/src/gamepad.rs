//! Game controllers: which are connected, and the buttons pressed on any of them since the last
//! look, by name ([`actcraft_engine::gamepad::GamepadAction::BUTTONS`]). The window polls it every
//! frame and hands the presses to the engine.

use gilrs::{Button, EventType, Gilrs};

/// The controllers, while it lives.
pub struct Gamepads {
    gilrs: Gilrs,
}

impl Gamepads {
    /// Starts watching the controllers.
    pub fn open() -> Result<Self, String> {
        match Gilrs::new() {
            Ok(gilrs) => Ok(Self { gilrs }),
            // No backend on this system: it still works, with no controllers.
            Err(gilrs::Error::NotImplemented(gilrs)) => Ok(Self { gilrs }),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The buttons pressed since the last call, in order, by name.
    pub fn pressed(&mut self) -> Vec<&'static str> {
        let mut out = Vec::new();
        // Bounded, so a flood of events can't hold up a frame.
        for _ in 0..256 {
            let Some(event) = self.gilrs.next_event() else { break };
            if let EventType::ButtonPressed(button, _) = event.event
                && let Some(name) = button_name(button)
            {
                out.push(name);
            }
        }
        out
    }

    /// The names of the controllers connected now.
    pub fn connected(&self) -> Vec<String> {
        self.gilrs.gamepads().map(|(_, pad)| pad.name().to_owned()).collect()
    }
}

/// A button's name by where it sits on a standard controller; `None` for ones without a place.
pub fn button_name(button: Button) -> Option<&'static str> {
    Some(match button {
        Button::South => "south",
        Button::East => "east",
        Button::North => "north",
        Button::West => "west",
        Button::LeftTrigger => "left_bumper",
        Button::LeftTrigger2 => "left_trigger",
        Button::RightTrigger => "right_bumper",
        Button::RightTrigger2 => "right_trigger",
        Button::Select => "select",
        Button::Start => "start",
        Button::Mode => "mode",
        Button::LeftThumb => "left_stick",
        Button::RightThumb => "right_stick",
        Button::DPadUp => "dpad_up",
        Button::DPadDown => "dpad_down",
        Button::DPadLeft => "dpad_left",
        Button::DPadRight => "dpad_right",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use actcraft_engine::gamepad::GamepadAction;

    #[test]
    fn every_named_button_is_one_the_show_knows() {
        let all = [
            Button::South,
            Button::East,
            Button::North,
            Button::West,
            Button::LeftTrigger,
            Button::LeftTrigger2,
            Button::RightTrigger,
            Button::RightTrigger2,
            Button::Select,
            Button::Start,
            Button::Mode,
            Button::LeftThumb,
            Button::RightThumb,
            Button::DPadUp,
            Button::DPadDown,
            Button::DPadLeft,
            Button::DPadRight,
        ];
        let names: Vec<&str> = all.iter().filter_map(|b| button_name(*b)).collect();
        assert_eq!(names, GamepadAction::BUTTONS, "the same buttons, in the same order");
        assert_eq!(button_name(Button::C), None);
    }
}
