//! User-issued intents derived from input events.

use crate::state::DisplayMode;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // Navigation
    NavUp,
    NavDown,
    NavLeft,
    NavRight,
    Enter,
    Back,
    /// Triggered by double-tap Esc/Backspace. Resets all 3 players to Empty.
    Panic,

    // Mode switching
    EnterMode(DisplayMode),
    ToggleNowNext,
    ToggleFunction,

    // Banks & slots
    /// Numpad 0-9. When `function_on`, this *maps* the highlighted browser
    /// item to slot `n`; otherwise it *triggers* slot `n` of the current bank.
    SelectSlot(u8),
    PrevBank,
    NextBank,
    SetLoopIn,
    SetLoopOut,
    ClearLoop,

    // Playback
    TogglePlayPause,
    SeekRelative(f64),
    SetRate(f32),
    Reload,

    // Settings
    CycleSetting(SettingId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingId {
    LoopType,
    OnFinish,
    OnStart,
    OnLoad,
    LoadNext,
    RandStartMode,
    FixedLengthMode,
    FixedLengthMultiply,
    ResetPlayers,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_slot_distinguishes_by_n() {
        assert_ne!(Action::SelectSlot(0), Action::SelectSlot(1));
    }

    #[test]
    fn panic_is_distinct_from_back() {
        assert_ne!(Action::Panic, Action::Back);
    }
}
