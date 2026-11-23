//! Game controllers: which are connected, and the buttons pressed on any of them since the last
//! look, by name ([`actcraft_engine::gamepad::GamepadAction::BUTTONS`]). The window polls it every
//! frame and hands the presses to the engine.

use gilrs::{Axis, Button, EventType, Gilrs};

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

    /// What happened since the last call: the buttons pressed, in order, and where each stick or
    /// trigger that moved stands now (0-1; a stick's middle is 0.5, up and right are 1).
    pub fn poll(&mut self) -> GamepadInput {
        let mut input = GamepadInput::default();
        // Bounded, so a flood of events can't hold up a frame.
        for _ in 0..256 {
            let Some(event) = self.gilrs.next_event() else { break };
            let moved = match event.event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(name) = button_name(button) {
                        input.pressed.push(name);
                    }
                    None
                }
                EventType::ButtonChanged(Button::LeftTrigger2, v, _) => Some(("left_trigger", f64::from(v))),
                EventType::ButtonChanged(Button::RightTrigger2, v, _) => Some(("right_trigger", f64::from(v))),
                EventType::AxisChanged(axis, v, _) => axis_name(axis).map(|name| (name, (f64::from(v) + 1.0) / 2.0)),
                _ => None,
            };
            if let Some((name, value)) = moved {
                // Only where it ended up counts.
                input.moved.retain(|(n, _)| *n != name);
                input.moved.push((name, value.clamp(0.0, 1.0)));
            }
        }
        input
    }

    /// The names of the controllers connected now.
    pub fn connected(&self) -> Vec<String> {
        self.gilrs.gamepads().map(|(_, pad)| pad.name().to_owned()).collect()
    }
}

/// What the controllers did since the last look.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GamepadInput {
    pub pressed: Vec<&'static str>,
    pub moved: Vec<(&'static str, f64)>,
}

/// A stick's direction by name; `None` for others (the triggers come as buttons).
pub fn axis_name(axis: Axis) -> Option<&'static str> {
    Some(match axis {
        Axis::LeftStickX => "left_x",
        Axis::LeftStickY => "left_y",
        Axis::RightStickX => "right_x",
        Axis::RightStickY => "right_y",
        _ => return None,
    })
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
        let sticks: Vec<&str> = [Axis::LeftStickX, Axis::LeftStickY, Axis::RightStickX, Axis::RightStickY]
            .into_iter()
            .filter_map(axis_name)
            .collect();
        let mut known: Vec<&str> = sticks.clone();
        known.extend(["left_trigger", "right_trigger"]);
        known.sort_unstable();
        let mut axes = actcraft_engine::gamepad::GamepadFader::AXES.to_vec();
        axes.sort_unstable();
        assert_eq!(known, axes, "every stick and trigger the show knows comes in");
    }
}
