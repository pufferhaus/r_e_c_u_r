//! Scripted input source for headless tests.
//!
//! Format (one event per line):
//!
//! ```text
//! 0.000  press Space
//! 0.500  press Digit1
//! 1.000  press Enter repeat 3
//! ```
//!
//! Comments start with `#`. Blank lines are ignored. Times are absolute
//! seconds from script start. `press <key>` emits a lookup of `key` through
//! the `Keymap`; if the key maps to no action it is silently skipped.

use std::time::Duration;

use super::keymap::Keymap;
use crate::action::Action;
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
struct ScriptedEvent {
    at: Duration,
    key: String,
    repeat: u32,
}

/// A scripted input source that replays key-presses from a text script.
#[derive(Debug, Default)]
pub struct MockInput {
    events: Vec<ScriptedEvent>,
    keymap: Option<Keymap>,
    cursor: usize,
}

impl MockInput {
    /// Parse a script. Use `with_keymap` to attach a keymap before draining.
    pub fn from_script(s: &str) -> Result<Self> {
        let mut events = Vec::new();
        for (lineno, line) in s.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let evt = parse_line(line, lineno + 1)?;
            events.push(evt);
        }
        Ok(Self {
            events,
            keymap: None,
            cursor: 0,
        })
    }

    /// Attach a keymap so that `drain_until` can resolve keys to actions.
    pub fn with_keymap(mut self, keymap: Keymap) -> Self {
        self.keymap = Some(keymap);
        self
    }

    /// Drain events whose `at` ≤ `now`. Returns resolved `Action`s. Keys that
    /// don't map to an action are silently skipped.
    pub fn drain_until(&mut self, now: Duration) -> Vec<Action> {
        let mut out = Vec::new();
        while let Some(evt) = self.events.get(self.cursor) {
            if evt.at > now {
                break;
            }
            if let Some(km) = &self.keymap {
                if let Some(action) = km.lookup(&evt.key) {
                    for _ in 0..evt.repeat.max(1) {
                        out.push(action.clone());
                    }
                }
            }
            self.cursor += 1;
        }
        out
    }

    pub fn finished(&self) -> bool {
        self.cursor >= self.events.len()
    }
}

fn parse_line(s: &str, lineno: usize) -> Result<ScriptedEvent> {
    let mut tokens = s.split_whitespace();
    let at_str = tokens
        .next()
        .ok_or_else(|| Error::Other(format!("line {lineno}: empty")))?;
    let at_secs: f64 = at_str
        .parse()
        .map_err(|_| Error::Other(format!("line {lineno}: bad time '{at_str}'")))?;
    let verb = tokens
        .next()
        .ok_or_else(|| Error::Other(format!("line {lineno}: missing verb")))?;
    if verb != "press" {
        return Err(Error::Other(format!(
            "line {lineno}: only 'press' supported, got '{verb}'"
        )));
    }
    let key = tokens
        .next()
        .ok_or_else(|| Error::Other(format!("line {lineno}: missing key")))?
        .to_string();

    let mut repeat = 1u32;
    while let Some(tok) = tokens.next() {
        match tok {
            "repeat" => {
                let n_str = tokens
                    .next()
                    .ok_or_else(|| Error::Other(format!("line {lineno}: 'repeat' needs count")))?;
                repeat = n_str.parse().map_err(|_| {
                    Error::Other(format!("line {lineno}: bad repeat count '{n_str}'"))
                })?;
            }
            other => {
                return Err(Error::Other(format!(
                    "line {lineno}: unexpected token '{other}'"
                )));
            }
        }
    }

    Ok(ScriptedEvent {
        at: Duration::from_secs_f64(at_secs),
        key,
        repeat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::keymap::Keymap;

    fn sample_keymap() -> Keymap {
        Keymap::parse("[bindings]\n\"Space\" = \"TogglePlayPause\"\n\"Enter\" = \"Enter\"\n")
            .unwrap()
    }

    #[test]
    fn parses_simple_script() {
        let m = MockInput::from_script("0.0 press Space\n0.5 press Enter").unwrap();
        assert_eq!(m.events.len(), 2);
    }

    #[test]
    fn drain_returns_events_at_or_before_now() {
        let mut m = MockInput::from_script("0.0 press Space\n1.0 press Enter\n2.0 press Space")
            .unwrap()
            .with_keymap(sample_keymap());
        let drained = m.drain_until(Duration::from_secs(1));
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0], Action::TogglePlayPause);
        assert_eq!(drained[1], Action::Enter);
        let rest = m.drain_until(Duration::from_secs(10));
        assert_eq!(rest.len(), 1);
        assert!(m.finished());
    }

    #[test]
    fn repeat_expands() {
        let mut m = MockInput::from_script("0.0 press Space repeat 3")
            .unwrap()
            .with_keymap(sample_keymap());
        let drained = m.drain_until(Duration::from_secs(1));
        assert_eq!(drained.len(), 3);
    }

    #[test]
    fn unknown_key_skipped_silently() {
        let mut m = MockInput::from_script("0.0 press UnknownKey")
            .unwrap()
            .with_keymap(sample_keymap());
        let drained = m.drain_until(Duration::from_secs(1));
        assert!(drained.is_empty());
    }

    #[test]
    fn empty_script_finishes_immediately() {
        let m = MockInput::from_script("").unwrap();
        assert!(m.finished());
    }
}
