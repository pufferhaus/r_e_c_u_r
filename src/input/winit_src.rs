//! Desktop keyboard input via winit `KeyEvent`.
//!
//! `WinitSource` accumulates `Action` values translated from winit key events.
//! The event loop (Task 13) calls `push_key_event` for each `WindowEvent::KeyboardInput`,
//! then calls `poll()` at the start of each frame to drain the accumulated actions.

use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

use super::keymap::Keymap;
use crate::action::Action;

/// Accumulated input state fed from winit events.
pub struct WinitSource {
    keymap: Keymap,
    buffer: Vec<String>, // raw key strings, resolved at poll() time
}

impl WinitSource {
    pub fn new(keymap: Keymap) -> Self {
        Self {
            keymap,
            buffer: Vec::new(),
        }
    }

    /// Call this from the winit event loop for each `KeyboardInput` event.
    pub fn push_key_event(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        if let Some(raw) = key_to_raw(&event.logical_key, event.physical_key) {
            self.buffer.push(raw);
        }
    }

    /// Resolve all buffered keys against the keymap, using the supplied mode
    /// for overrides (e.g. `KeyR` remap inside `DetourScrub`). Drains the buffer.
    pub fn poll(&mut self, mode: crate::state::ControlMode) -> Vec<Action> {
        let raws = std::mem::take(&mut self.buffer);
        raws.into_iter()
            .filter_map(|k| self.keymap.lookup_with_mode(&k, mode))
            .collect()
    }
}

/// Map a winit key event to our raw key-code string (matches keymap.toml names).
///
/// We use `PhysicalKey::Code` names (DigitN, KeyX, ArrowUp, etc.) which are
/// layout-independent and match the keymap.toml naming convention.
fn key_to_raw(logical: &Key, physical: PhysicalKey) -> Option<String> {
    // Named logical keys first.
    if let Key::Named(named) = logical {
        return Some(match named {
            NamedKey::ArrowUp => "ArrowUp".into(),
            NamedKey::ArrowDown => "ArrowDown".into(),
            NamedKey::ArrowLeft => "ArrowLeft".into(),
            NamedKey::ArrowRight => "ArrowRight".into(),
            NamedKey::Enter => "Enter".into(),
            NamedKey::Escape => "Escape".into(),
            NamedKey::Backspace => "Backspace".into(),
            NamedKey::Space => "Space".into(),
            NamedKey::Tab => "Tab".into(),
            NamedKey::Shift => "ShiftLeft".into(),
            _ => return None,
        });
    }

    // Physical key codes (layout-independent).
    if let PhysicalKey::Code(code) = physical {
        return Some(match code {
            KeyCode::Digit0 => "Digit0".into(),
            KeyCode::Digit1 => "Digit1".into(),
            KeyCode::Digit2 => "Digit2".into(),
            KeyCode::Digit3 => "Digit3".into(),
            KeyCode::Digit4 => "Digit4".into(),
            KeyCode::Digit5 => "Digit5".into(),
            KeyCode::Digit6 => "Digit6".into(),
            KeyCode::Digit7 => "Digit7".into(),
            KeyCode::Digit8 => "Digit8".into(),
            KeyCode::Digit9 => "Digit9".into(),
            KeyCode::BracketLeft => "BracketLeft".into(),
            KeyCode::BracketRight => "BracketRight".into(),
            KeyCode::Backslash => "Backslash".into(),
            KeyCode::Comma => "Comma".into(),
            KeyCode::Period => "Period".into(),
            KeyCode::KeyA => "KeyA".into(),
            KeyCode::KeyB => "KeyB".into(),
            KeyCode::KeyC => "KeyC".into(),
            KeyCode::KeyD => "KeyD".into(),
            KeyCode::KeyE => "KeyE".into(),
            KeyCode::KeyF => "KeyF".into(),
            KeyCode::KeyG => "KeyG".into(),
            KeyCode::KeyH => "KeyH".into(),
            KeyCode::KeyI => "KeyI".into(),
            KeyCode::KeyJ => "KeyJ".into(),
            KeyCode::KeyK => "KeyK".into(),
            KeyCode::KeyL => "KeyL".into(),
            KeyCode::KeyM => "KeyM".into(),
            KeyCode::KeyN => "KeyN".into(),
            KeyCode::KeyO => "KeyO".into(),
            KeyCode::KeyP => "KeyP".into(),
            KeyCode::KeyQ => "KeyQ".into(),
            KeyCode::KeyR => "KeyR".into(),
            KeyCode::KeyS => "KeyS".into(),
            KeyCode::KeyT => "KeyT".into(),
            KeyCode::KeyU => "KeyU".into(),
            KeyCode::KeyV => "KeyV".into(),
            KeyCode::KeyW => "KeyW".into(),
            KeyCode::KeyX => "KeyX".into(),
            KeyCode::KeyY => "KeyY".into(),
            KeyCode::KeyZ => "KeyZ".into(),
            KeyCode::ShiftLeft => "ShiftLeft".into(),
            KeyCode::ShiftRight => "ShiftRight".into(),
            _ => return None,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // winit::event::KeyEvent cannot be constructed from outside the crate
    // (platform_specific field is pub(crate)). Tests that exercise
    // push_key_event directly are marked ignore; coverage comes from the
    // live desktop loop in Task 13.

    #[test]
    fn poll_on_empty_returns_empty_vec() {
        let km = Keymap::default();
        let mut src = WinitSource::new(km);
        assert!(src.poll(crate::state::ControlMode::Default).is_empty());
    }

    #[test]
    #[ignore = "winit::event::KeyEvent cannot be constructed externally"]
    fn key_event_routes_to_action() {}
}
