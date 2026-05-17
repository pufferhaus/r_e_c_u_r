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

    // Shader bank (Phase 2 — conjur)
    /// Map the currently highlighted SHADERS browser entry into the focused
    /// shader-bank slot. Function-key gated like SelectSlot.
    SelectShaderSlot(u8),
    /// Activate shader slot `n` of the current shader bank. Empty slot →
    /// bypass; missing/uncompilable shader → fall back to baked __safe__.
    TriggerShaderSlot(u8),
    /// In ControlMode::ShaderParam, nudge the focused param up (+1) or down (-1)
    /// by a meta-defined step (default 1% of [min..max] range).
    ShaderParamAdjust(i8),
    /// Move the param-edit focus to slot 0..=7.
    ShaderParamSelect(u8),

    // Detour (Phase 3)
    DetourEnter,
    DetourExit,
    DetourScrubBy(i32),
    DetourCycleSpeed,
    DetourToggleDirection,
    DetourTogglePlay,
    DetourSetStartMarker,
    DetourSetEndMarker,
    DetourClearMarkers,
    DetourCycleMix,
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
