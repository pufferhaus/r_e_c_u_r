# Phase 2 — Sub-plan B (conjur UI + persistence + starter shaders) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the shader-bank UI layer, `--gles-profile` CLI flag, hot-reload, persistence, and 4 more starter shaders on top of the Phase 2-A shader infrastructure already on `rust-port`.

**Architecture:** Mirror Phase 1's bank-bank-trigger pattern in a second dimension: `ShaderBank` (10 slots × N banks) holds `ShaderSlot` records (shader name + 8 param values). Two new `RootScreen` bodies — `ShadersBody` (browser) and `ShdrBnkBody` (bank view) — extend `DisplayMode::Shaders`/`ShdrBnk`. New `ControlMode::ShaderParam` reroutes nav-keys to edit the focused param. A `notify`-backed `ShaderWatcher` running on a worker thread pushes dirty-flags into a `crossbeam_channel`; the render loop drains the channel between frames and re-compiles via the existing `ShaderPipeline`. Compile failures keep the previous binary live and surface a footer toast. `clap` gains `--gles-profile`; the value is stashed on `SharedState` and consumed both by `ShaderLibrary::load_dir_for_profile` and `ShaderPipeline::new`.

**Tech Stack:** Rust 1.85, `glow` 0.14, `clap` 4 derive, `serde`+`toml` 0.8, `notify` 6, `crossbeam-channel` (new dep). No new GL machinery — sub-plan A's `ShaderPipeline` + `ShaderLibrary` already handle compile-and-cache; B feeds them state.

**Scope boundary:** Codec probe (spec Section 7) is **sub-plan C**, not in this plan. `u_prev` ping-pong (spec Section 4) is **also deferred to a Phase-2 polish pass**; the existing single-FBO output is fine for the 4 starter shaders (none depend on previous frames). The plan notes this so future-you doesn't get confused by the spec.

---

## File Map

**Create:**
- `src/shader/banks.rs` — `ShaderSlot`, `ShaderBank` state types + first-empty helper.
- `src/shader/hot_reload.rs` — `ShaderWatcher` (notify thread → crossbeam channel of `ShaderEvent::Dirty(name)`).
- `src/menu/shaders.rs` — `ShadersBody` browser screen.
- `src/menu/shdr_bnk.rs` — `ShdrBnkBody` bank-grid screen.
- `src/menu/param.rs` — `ParamBody` shader-param editor (used while `ControlMode::ShaderParam`).
- `shaders/color_shift.glsl` + `.toml`
- `shaders/pixelate.glsl` + `.toml`
- `shaders/kaleidoscope.glsl` + `.toml`
- `shaders/rgb_glitch.glsl` + `.toml`
- `tests/fixtures/shader_v310_only.toml` — meta marked `min_gles = "3.10"` for filter tests.

**Modify:**
- `src/action.rs` — add `SelectShaderSlot(u8)`, `TriggerShaderSlot(u8)`, `ShaderParamAdjust(i8)`, `ShaderParamSelect(u8)`. Extend `parse_action` / EnterMode for `ShdrBnk`.
- `src/state.rs` — add `shader_banks: Vec<ShaderBank>`, `shader_bank_number: u8`, `gles_profile: GlesProfile`, `shader_focus: u8` (currently focused param slot), `shader_dir: PathBuf` (for hot-reload + library reloads). Extend `ControlMode::ShaderParam` already exists — wire it in.
- `src/apply.rs` — handle the new actions. Trigger now also surfaces a `RackHandle::trigger_shader(name, params)` call (see Task 12 hook).
- `src/persist.rs` — `load_shader_banks` / `save_shader_banks` (`shader_banks.toml`); wire into `main.rs` startup/shutdown.
- `src/shader/mod.rs` — re-export `banks::*`, `hot_reload::*`.
- `src/shader/library.rs` — add `LoadedShader::reload_from_disk(path)` helper used by the watcher.
- `src/menu/mod.rs` — `pub mod shaders; pub mod shdr_bnk; pub mod param;`.
- `src/menu/root.rs` — dispatch `Shaders` / `ShdrBnk` modes to the new bodies; surface `ControlMode::ShaderParam` to `ParamBody`; footer shows `profile: pi3` indicator when `gles_profile == V100`.
- `src/main.rs` — add `--gles-profile` clap flag; load shader bank state; instantiate `ShaderWatcher` and drain its channel each frame to push reloads into the pipeline; load shader banks into `SharedState`; wire `Render` constructor to accept the chosen `GlesProfile`.
- `src/render/desktop.rs` — accept a `GlesProfile` parameter on `WinitGlTarget::new` and forward it to `ShaderPipeline::new` and `ShaderLibrary::load_dir_for_profile`. Same wiring on `src/render/pi.rs` for the Pi backend.
- `src/render/mod.rs` — bump the stub `Render::new` signature to match.
- `src/lib.rs` — re-export the new `apply` trait method `trigger_shader`.
- `keymap.toml` — add bindings for `EnterMode(Shaders)`, `EnterMode(ShdrBnk)`, `SelectShaderSlot(0..=9)` (chord-prefixed), and `ShaderParamAdjust(±1)`.
- `Cargo.toml` — add `crossbeam-channel = "0.5"`.

**Test:**
- `src/shader/banks.rs` (inline `mod tests`)
- `src/shader/hot_reload.rs` (inline `mod tests` — uses a tempdir + 100 ms sleep)
- `src/menu/shaders.rs`, `shdr_bnk.rs`, `param.rs` (inline)
- `src/persist.rs` extended (inline)
- `tests/integration_shader_smoke.rs` — `cargo run --bin recur -- --smoke-frames 2` with each starter shader pre-selected via env var; checks no panic + non-black framebuffer.
- `tests/integration_gles_profile.rs` — verify `--gles-profile pi3` filters a V310-only fixture.

**Out of scope (call out explicitly so reviewers don't ask):**
- Codec probe + `[X]` markers (sub-plan C).
- `u_prev` ping-pong FBO (Phase 2 polish).
- New audio uniform wiring (zero-fills stay in place from sub-plan A).

---

### Task 1: ShaderSlot + ShaderBank state types

**Files:**
- Create: `src/shader/banks.rs`
- Modify: `src/shader/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src/shader/banks.rs` with only the test module:

```rust
//! In-memory shader bank state. Mirrors `state::Bank` for video slots but
//! holds shader-name + 8 param values per slot instead of file metadata.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bank_has_ten_none_slots() {
        let b = ShaderBank::empty();
        assert_eq!(b.slots.len(), 10);
        assert!(b.slots.iter().all(Option::is_none));
    }

    #[test]
    fn first_empty_returns_zero_on_empty_bank() {
        assert_eq!(ShaderBank::empty().first_empty(), Some(0));
    }

    #[test]
    fn first_empty_skips_filled_slots() {
        let mut b = ShaderBank::empty();
        b.slots[0] = Some(ShaderSlot {
            shader: "color_shift".into(),
            params: [0.0; 8],
        });
        assert_eq!(b.first_empty(), Some(1));
    }

    #[test]
    fn first_empty_returns_none_when_full() {
        let mut b = ShaderBank::empty();
        for i in 0..10 {
            b.slots[i] = Some(ShaderSlot {
                shader: format!("s{i}"),
                params: [0.0; 8],
            });
        }
        assert_eq!(b.first_empty(), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib shader::banks`
Expected: FAIL — `ShaderBank`, `ShaderSlot` undefined.

- [ ] **Step 3: Write minimal implementation**

Top of `src/shader/banks.rs`:

```rust
//! In-memory shader bank state. Mirrors `state::Bank` for video slots but
//! holds shader-name + 8 param values per slot instead of file metadata.

use serde::{Deserialize, Serialize};

pub const SHADER_SLOTS_PER_BANK: usize = 10;
pub const MAX_SHADER_BANKS: u8 = 26;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderSlot {
    pub shader: String,
    #[serde(default = "default_params")]
    pub params: [f32; 8],
}

fn default_params() -> [f32; 8] {
    [0.0; 8]
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShaderBank {
    #[serde(default)]
    pub slots: Vec<Option<ShaderSlot>>,
}

impl ShaderBank {
    pub fn empty() -> Self {
        Self { slots: (0..SHADER_SLOTS_PER_BANK).map(|_| None).collect() }
    }

    pub fn first_empty(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }
}
```

And in `src/shader/mod.rs` append:

```rust
pub mod banks;
pub use banks::{ShaderBank, ShaderSlot, MAX_SHADER_BANKS, SHADER_SLOTS_PER_BANK};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib shader::banks`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/shader/banks.rs src/shader/mod.rs
git commit -m "feat(shader): ShaderBank + ShaderSlot types (10 slots, 8 params)"
```

---

### Task 2: SharedState carries shader banks + gles_profile

**Files:**
- Modify: `src/state.rs`
- Modify: `src/render/shader_assembly.rs` (derive `Serialize`/`Deserialize` on `GlesProfile`)

- [ ] **Step 1: Write the failing test**

Add to `src/state.rs` `mod tests`:

```rust
#[test]
fn shared_state_has_empty_shader_bank_and_v310_profile_by_default() {
    let s = SharedState::new();
    assert_eq!(s.shader_banks.len(), 1);
    assert_eq!(s.shader_banks[0].slots.len(), 10);
    assert!(s.shader_banks[0].slots.iter().all(Option::is_none));
    assert_eq!(s.shader_bank_number, 0);
    assert_eq!(s.shader_focus, 0);
    assert_eq!(s.gles_profile, crate::render::shader_assembly::GlesProfile::V310);
}

#[test]
fn current_shader_bank_returns_active_bank() {
    let s = SharedState::new();
    assert_eq!(s.current_shader_bank().slots.len(), 10);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib state::tests::shared_state_has_empty_shader_bank`
Expected: FAIL (`shader_banks` field missing).

- [ ] **Step 3: Write minimal implementation**

In `src/render/shader_assembly.rs`, change the `GlesProfile` derive line:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GlesProfile {
    V100,
    V310,
}
```

In `src/state.rs`, extend `SharedState`:

```rust
use crate::render::shader_assembly::GlesProfile;
use crate::shader::ShaderBank;

#[derive(Debug, Clone)]
pub struct SharedState {
    pub banks: Vec<Bank>,
    pub bank_number: u8,
    pub player_mode: PlayerMode,
    pub display_mode: DisplayMode,
    pub control_mode: ControlMode,
    pub function_on: bool,
    pub feedback_active: bool,
    pub sampler: SamplerSettings,
    pub paths_to_browser: Vec<PathBuf>,
    pub last_error: Option<String>,

    // Phase 2 — conjur
    pub shader_banks: Vec<ShaderBank>,
    pub shader_bank_number: u8,
    pub shader_focus: u8,        // 0..=9, focused slot in SHDR_BNK
    pub gles_profile: GlesProfile,
}
```

Extend `SharedState::new`:

```rust
shader_banks: vec![ShaderBank::empty()],
shader_bank_number: 0,
shader_focus: 0,
gles_profile: GlesProfile::V310,
```

Add helpers:

```rust
impl SharedState {
    pub fn current_shader_bank(&self) -> &ShaderBank {
        &self.shader_banks[self.shader_bank_number as usize]
    }
    pub fn current_shader_bank_mut(&mut self) -> &mut ShaderBank {
        let n = self.shader_bank_number as usize;
        &mut self.shader_banks[n]
    }
}
```

- [ ] **Step 4: Run all state + shader tests**

Run: `cargo test --lib state shader`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/state.rs src/render/shader_assembly.rs
git commit -m "feat(state): wire ShaderBank + gles_profile into SharedState"
```

---

### Task 3: `shader_banks.toml` persistence

**Files:**
- Modify: `src/persist.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/persist.rs` `mod tests`:

```rust
use crate::shader::{ShaderBank, ShaderSlot};

fn sslot(name: &str) -> ShaderSlot {
    ShaderSlot {
        shader: name.to_string(),
        params: [0.1, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    }
}

#[test]
fn shader_banks_load_default_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let got = load_shader_banks(tmp.path()).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].slots.len(), 10);
    assert!(got[0].slots.iter().all(Option::is_none));
}

#[test]
fn shader_banks_roundtrip_sparse_slots() {
    let tmp = tempfile::tempdir().unwrap();
    let mut b = ShaderBank::empty();
    b.slots[0] = Some(sslot("color_shift"));
    b.slots[4] = Some(sslot("pixelate"));
    save_shader_banks(tmp.path(), &[b.clone()]).unwrap();
    let got = load_shader_banks(tmp.path()).unwrap();
    assert_eq!(got, vec![b]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib persist::tests::shader_banks`
Expected: FAIL — `load_shader_banks` undefined.

- [ ] **Step 3: Write minimal implementation**

Append to `src/persist.rs`:

```rust
use crate::shader::{ShaderBank, ShaderSlot, SHADER_SLOTS_PER_BANK};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShaderSlotEntry {
    index: usize,
    #[serde(flatten)]
    slot: ShaderSlot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShaderBankWire {
    #[serde(default)]
    slots: Vec<ShaderSlotEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShaderBanksFileWire {
    #[serde(default)]
    banks: Vec<ShaderBankWire>,
}

fn shader_bank_to_wire(bank: &ShaderBank) -> ShaderBankWire {
    let slots = bank
        .slots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|slot| ShaderSlotEntry { index: i, slot: slot.clone() }))
        .collect();
    ShaderBankWire { slots }
}

fn shader_wire_to_bank(wire: ShaderBankWire, file: &str) -> Result<ShaderBank> {
    let mut bank = ShaderBank::empty();
    for entry in wire.slots {
        if entry.index >= SHADER_SLOTS_PER_BANK {
            return Err(Error::Other(format!(
                "{}: shader slot index {} out of range (max {})",
                file,
                entry.index,
                SHADER_SLOTS_PER_BANK - 1
            )));
        }
        bank.slots[entry.index] = Some(entry.slot);
    }
    Ok(bank)
}

pub fn load_shader_banks(state_dir: &Path) -> Result<Vec<ShaderBank>> {
    let p = state_dir.join("shader_banks.toml");
    if !p.exists() {
        return Ok(vec![ShaderBank::empty()]);
    }
    let s = std::fs::read_to_string(&p)?;
    let file_str = p.display().to_string();
    let wire: ShaderBanksFileWire = toml::from_str(&s).map_err(|e| Error::TomlParse {
        file: file_str.clone(),
        source: e,
    })?;
    if wire.banks.is_empty() {
        return Ok(vec![ShaderBank::empty()]);
    }
    wire.banks
        .into_iter()
        .map(|bw| shader_wire_to_bank(bw, &file_str))
        .collect()
}

pub fn save_shader_banks(state_dir: &Path, banks: &[ShaderBank]) -> Result<()> {
    let p = state_dir.join("shader_banks.toml");
    let wire = ShaderBanksFileWire {
        banks: banks.iter().map(shader_bank_to_wire).collect(),
    };
    let s = toml::to_string_pretty(&wire).map_err(|e| Error::TomlSerialize {
        file: p.display().to_string(),
        source: e,
    })?;
    write_atomic(&p, &s)?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib persist`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/persist.rs
git commit -m "feat(persist): shader_banks.toml load/save with sparse index wire format"
```

---

### Task 4: `--gles-profile` CLI flag + main wiring

**Files:**
- Modify: `src/main.rs`
- Modify: `src/render/mod.rs` (stub signature)
- Modify: `src/render/desktop.rs` (accept profile)
- Modify: `src/render/pi.rs` (accept profile)

- [ ] **Step 1: Write the failing test**

Add a unit test inside `src/main.rs` for the clap parser. Since `main.rs` has no test module yet, append at the bottom (use `#[cfg(test)]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn gles_profile_defaults_to_v310() {
        let a = Args::parse_from(["recur"]);
        assert_eq!(a.gles_profile, GlesProfileArg::V310);
    }

    #[test]
    fn gles_profile_pi3_alias_parses_to_v100() {
        let a = Args::parse_from(["recur", "--gles-profile", "pi3"]);
        assert_eq!(a.gles_profile, GlesProfileArg::V100);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin recur gles_profile`
Expected: FAIL — `GlesProfileArg` undefined.

- [ ] **Step 3: Write minimal implementation**

In `src/main.rs`, above `struct Args`:

```rust
/// CLI alias for the runtime GLES profile (separate from the clap-internal enum
/// so we can rename without breaking scripts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum GlesProfileArg {
    /// GLSL ES 1.00 — pi3 parity.
    #[value(alias = "pi3", alias = "v100")]
    V100,
    /// GLSL ES 3.10 — pi5 parity (default).
    #[value(alias = "pi5", alias = "v310")]
    V310,
}

impl GlesProfileArg {
    fn to_profile(self) -> recur::render::shader_assembly::GlesProfile {
        use recur::render::shader_assembly::GlesProfile;
        match self {
            GlesProfileArg::V100 => GlesProfile::V100,
            GlesProfileArg::V310 => GlesProfile::V310,
        }
    }
    fn to_min_gles(self) -> recur::shader::GlesVersion {
        use recur::shader::GlesVersion;
        match self {
            GlesProfileArg::V100 => GlesVersion::V100,
            GlesProfileArg::V310 => GlesVersion::V310,
        }
    }
}
```

Add to `struct Args`:

```rust
/// GLES profile to load shaders against. `pi3`/`v100` filters out 3.10-only
/// shaders; default `pi5`/`v310` loads all.
#[arg(long, value_enum, default_value_t = GlesProfileArg::V310)]
gles_profile: GlesProfileArg,
```

In `main()`, after `args` is parsed and before `Render::new`:

```rust
let profile = args.gles_profile.to_profile();
state.gles_profile = profile;
```

Change the `Render::new` call to pass the profile:

```rust
let mut render = recur::render::Render::new(cfg.render.width, cfg.render.height, "r_e_c_u_r", profile)?;
```

Hard-pin override on Pi builds (compile-time):

```rust
#[cfg(feature = "pi3")]
{
    if args.gles_profile == GlesProfileArg::V310 {
        tracing::warn!("--gles-profile v310 ignored on pi3 build; forcing V100");
        state.gles_profile = recur::render::shader_assembly::GlesProfile::V100;
    }
}
#[cfg(feature = "pi5")]
{
    if args.gles_profile == GlesProfileArg::V100 {
        tracing::warn!("--gles-profile v100 ignored on pi5 build; forcing V310");
        state.gles_profile = recur::render::shader_assembly::GlesProfile::V310;
    }
}
```

Propagate to backends:

In `src/render/mod.rs` stub:

```rust
pub fn new(_w: u32, _h: u32, _t: &str, _p: crate::render::shader_assembly::GlesProfile) -> anyhow::Result<Self> {
    Ok(Self)
}
```

In `src/render/desktop.rs::WinitGlTarget::new`, change the signature:

```rust
pub fn new(width: u32, height: u32, title: &str, profile: crate::render::shader_assembly::GlesProfile) -> anyhow::Result<Self> {
```

Replace the hard-coded `GlesProfile::V100` block with:

```rust
let shaders_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders");
let min_gles = match profile {
    crate::render::shader_assembly::GlesProfile::V100 => crate::shader::GlesVersion::V100,
    crate::render::shader_assembly::GlesProfile::V310 => crate::shader::GlesVersion::V310,
};
let library = crate::shader::ShaderLibrary::load_dir_for_profile(&shaders_dir, min_gles)?;
let mut pipeline = crate::render::shader_pipeline::ShaderPipeline::new(profile, library);
```

In `src/render/pi.rs`, apply the same signature change (look for the `Self::new(width, height, _title)` declaration and the matching `ShaderPipeline::new` call; mirror desktop.rs).

- [ ] **Step 4: Run test + build**

Run: `cargo test --bin recur gles_profile && cargo check`
Expected: tests PASS; check clean.

Also run the cross-target builds to catch pi path:

Run: `cargo check --no-default-features --features pi3` (skip if on macOS — known to fail on `pi::PiTarget`; just `cargo check` is enough).

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/render/mod.rs src/render/desktop.rs src/render/pi.rs
git commit -m "feat(cli): --gles-profile flag plumbed into Render + ShaderLibrary"
```

---

### Task 5: New action variants for shader UI

**Files:**
- Modify: `src/action.rs`
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/input/keymap.rs` `mod tests`:

```rust
#[test]
fn parses_select_shader_slot() {
    let s = "[bindings]\n\"F1\" = \"SelectShaderSlot(3)\"\n";
    let km = Keymap::parse(s).unwrap();
    assert_eq!(km.lookup("F1"), Some(Action::SelectShaderSlot(3)));
}

#[test]
fn parses_trigger_shader_slot() {
    let s = "[bindings]\n\"F2\" = \"TriggerShaderSlot(7)\"\n";
    let km = Keymap::parse(s).unwrap();
    assert_eq!(km.lookup("F2"), Some(Action::TriggerShaderSlot(7)));
}

#[test]
fn parses_shader_param_adjust() {
    let s = "[bindings]\n\"KeyP\" = \"ShaderParamAdjust(1)\"\n";
    let km = Keymap::parse(s).unwrap();
    assert_eq!(km.lookup("KeyP"), Some(Action::ShaderParamAdjust(1)));
}

#[test]
fn parses_enter_mode_shdr_bnk() {
    let s = "[bindings]\n\"KeyK\" = \"EnterMode(ShdrBnk)\"\n";
    let km = Keymap::parse(s).unwrap();
    assert_eq!(km.lookup("KeyK"), Some(Action::EnterMode(DisplayMode::ShdrBnk)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib input::keymap::tests::parses_select_shader_slot`
Expected: FAIL — `Action::SelectShaderSlot` undefined.

- [ ] **Step 3: Write minimal implementation**

Append to `Action` in `src/action.rs`:

```rust
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
```

Append to `parse_action` in `src/input/keymap.rs`, before the simple-variants block:

```rust
    if let Some(rest) = s.strip_prefix("SelectShaderSlot(").and_then(|r| r.strip_suffix(')')) {
        let n: u8 = rest.parse().map_err(|_| ())?;
        return Ok(Action::SelectShaderSlot(n));
    }
    if let Some(rest) = s.strip_prefix("TriggerShaderSlot(").and_then(|r| r.strip_suffix(')')) {
        let n: u8 = rest.parse().map_err(|_| ())?;
        return Ok(Action::TriggerShaderSlot(n));
    }
    if let Some(rest) = s.strip_prefix("ShaderParamAdjust(").and_then(|r| r.strip_suffix(')')) {
        let n: i8 = rest.parse().map_err(|_| ())?;
        return Ok(Action::ShaderParamAdjust(n));
    }
    if let Some(rest) = s.strip_prefix("ShaderParamSelect(").and_then(|r| r.strip_suffix(')')) {
        let n: u8 = rest.parse().map_err(|_| ())?;
        return Ok(Action::ShaderParamSelect(n));
    }
```

Extend the `EnterMode(rest)` branch to recognise `ShdrBnk` and `Frames`:

```rust
        let mode = match rest {
            "Browser" => DisplayMode::Browser,
            "Sampler" => DisplayMode::Sampler,
            "Settings" => DisplayMode::Settings,
            "Shaders" => DisplayMode::Shaders,
            "ShdrBnk" => DisplayMode::ShdrBnk,
            "Frames" => DisplayMode::Frames,
            _ => return Err(()),
        };
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib input::keymap`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/action.rs src/input/keymap.rs
git commit -m "feat(action): shader-slot select/trigger + param-adjust actions"
```

---

### Task 6: Apply shader actions to SharedState (no rack hookup yet)

**Files:**
- Modify: `src/apply.rs`
- Modify: `src/state.rs` (`ControlMode::ShaderParam` already exists; just make sure `Default` works)

- [ ] **Step 1: Write the failing test**

Add to `src/apply.rs` `mod tests`:

```rust
use crate::shader::ShaderSlot;

#[test]
fn select_shader_slot_with_function_off_triggers_pulse() {
    let mut s = SharedState::new();
    s.current_shader_bank_mut().slots[2] = Some(ShaderSlot {
        shader: "color_shift".into(),
        params: [0.0; 8],
    });
    let mut r = SpyRack::default();
    apply(Action::TriggerShaderSlot(2), &mut s, &mut r);
    assert_eq!(r.shader_triggers, vec!["color_shift".to_string()]);
}

#[test]
fn trigger_shader_slot_empty_clears_active() {
    let mut s = SharedState::new();
    let mut r = SpyRack::default();
    apply(Action::TriggerShaderSlot(5), &mut s, &mut r);
    assert_eq!(r.shader_cleared, 1);
}

#[test]
fn select_shader_slot_with_function_on_maps_into_slot() {
    let mut s = SharedState::new();
    s.function_on = true;
    s.shader_focus = 4; // highlight in SHADERS browser is stashed there in Task 7;
                         // for apply() we pass the highlight via state.shader_pending_select
    s.shader_pending_select = Some("pixelate".to_string());
    let mut r = SpyRack::default();
    apply(Action::SelectShaderSlot(3), &mut s, &mut r);
    let slot = s.current_shader_bank().slots[3].as_ref().unwrap();
    assert_eq!(slot.shader, "pixelate");
    assert!(!s.function_on);
}

#[test]
fn shader_param_adjust_clamped_by_meta() {
    let mut s = SharedState::new();
    s.control_mode = ControlMode::ShaderParam;
    s.shader_focus = 0;
    s.current_shader_bank_mut().slots[0] = Some(ShaderSlot {
        shader: "color_shift".into(),
        params: [0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    });
    s.shader_active_slot = Some(0);
    let mut r = SpyRack::default();
    apply(Action::ShaderParamAdjust(1), &mut s, &mut r);
    let slot = s.current_shader_bank().slots[0].as_ref().unwrap();
    assert!(slot.params[0] > 0.5);
}
```

(Add `shader_triggers: Vec<String>`, `shader_cleared: u32` fields to `SpyRack` and implement `RackHandle::trigger_shader` / `clear_shader` accordingly.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib apply::tests::select_shader_slot`
Expected: FAIL — missing fields/trait methods.

- [ ] **Step 3: Write minimal implementation**

Extend `SharedState` in `src/state.rs`:

```rust
    /// Browser-selected shader name awaiting a slot mapping (set by SHADERS
    /// browser, consumed by Action::SelectShaderSlot when function_on is set).
    pub shader_pending_select: Option<String>,
    /// Currently triggered shader-bank slot (0..=9). None = bypass.
    pub shader_active_slot: Option<u8>,
```

(Default both to `None` in `SharedState::new`.)

Extend `RackHandle` in `src/apply.rs`:

```rust
    fn trigger_shader(&mut self, name: &str, params: [f32; 8]);
    fn clear_shader(&mut self);
```

Stub `SpyRack`:

```rust
        #[derive(Default, Debug)]
        struct SpyRack {
            reload_count: u32,
            trigger_calls: Vec<(u8, u8, String)>,
            toggle_pause: u32,
            position: Option<f64>,
            binding: Option<(u8, u8)>,
            shader_triggers: Vec<String>,
            shader_cleared: u32,
        }

        impl RackHandle for SpyRack {
            // ... existing methods ...
            fn trigger_shader(&mut self, name: &str, _params: [f32; 8]) {
                self.shader_triggers.push(name.to_string());
            }
            fn clear_shader(&mut self) {
                self.shader_cleared += 1;
            }
        }
```

Action handling in `apply()`:

```rust
        Action::TriggerShaderSlot(n) => {
            let n = (n as usize).min(crate::shader::SHADER_SLOTS_PER_BANK - 1);
            let slot = state.current_shader_bank().slots.get(n).cloned().flatten();
            match slot {
                Some(slot) => {
                    rack.trigger_shader(&slot.shader, slot.params);
                    state.shader_active_slot = Some(n as u8);
                }
                None => {
                    rack.clear_shader();
                    state.shader_active_slot = None;
                }
            }
        }
        Action::SelectShaderSlot(n) => {
            let n = (n as usize).min(crate::shader::SHADER_SLOTS_PER_BANK - 1);
            if state.function_on {
                if let Some(name) = state.shader_pending_select.take() {
                    let bank = state.current_shader_bank_mut();
                    bank.slots[n] = Some(crate::shader::ShaderSlot {
                        shader: name,
                        params: [0.0; 8],
                    });
                }
            }
            state.function_on = false;
        }
        Action::ShaderParamSelect(n) => {
            state.shader_focus = n.min(7);
        }
        Action::ShaderParamAdjust(delta) => {
            if state.control_mode != ControlMode::ShaderParam {
                return;
            }
            let Some(active) = state.shader_active_slot else { return; };
            let bank_idx = state.shader_bank_number as usize;
            let focus = state.shader_focus as usize;
            if let Some(Some(slot)) = state
                .shader_banks
                .get_mut(bank_idx)
                .and_then(|b| b.slots.get_mut(active as usize))
            {
                // Step = 1% of [-1.0, 1.0] range as a default since we don't
                // know the meta here. ShaderPipeline re-uploads clamped values
                // on next frame.
                let step = 0.02_f32 * delta as f32;
                let v = (slot.params[focus] + step).clamp(-100.0, 100.0);
                slot.params[focus] = v;
            }
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib apply`
Expected: PASS (existing + new).

- [ ] **Step 5: Commit**

```bash
git add src/apply.rs src/state.rs
git commit -m "feat(apply): handle TriggerShaderSlot / SelectShaderSlot / ShaderParam*"
```

---

### Task 7: SHADERS browser body

**Files:**
- Create: `src/menu/shaders.rs`
- Modify: `src/menu/mod.rs`
- Modify: `src/menu/root.rs`

- [ ] **Step 1: Write the failing test**

Create `src/menu/shaders.rs`:

```rust
//! ShadersBody — browse paired `.glsl + .toml` pairs from the shader dir.
//! Enter on a row stashes the shader name into `state.shader_pending_select`;
//! the user then presses Function + SelectShaderSlot(n) to map it.

use crate::action::Action;
use crate::state::SharedState;
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

pub struct ShadersBody {
    pub names: Vec<String>,
    pub filtered: usize,
    selected: usize,
}

impl ShadersBody {
    pub fn new(names: Vec<String>, filtered: usize) -> Self {
        Self { names, filtered, selected: 0 }
    }
}

impl Screen for ShadersBody {
    fn render(&self, _state: &SharedState, grid: &mut TextGrid) {
        grid.write_row(4, "shader                              gles");
        for view_i in 0..10 {
            let row_idx = 5 + view_i;
            match self.names.get(view_i) {
                None => grid.write_row(row_idx, ""),
                Some(name) => {
                    let truncated: String = name.chars().take(38).collect();
                    grid.write_row(row_idx, &format!("{:<38} {:<5}", truncated, ""));
                    if view_i == self.selected {
                        grid.invert_row(row_idx);
                    }
                }
            }
        }
        let footer = if self.filtered > 0 {
            format!("{} shown, {} hidden (pi5-only)", self.names.len(), self.filtered)
        } else {
            format!("{} shaders", self.names.len())
        };
        grid.write_row(14, &footer);
    }

    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult {
        if self.names.is_empty() {
            return ScreenResult::Continue;
        }
        match action {
            Action::NavUp => self.selected = self.selected.saturating_sub(1),
            Action::NavDown => self.selected = (self.selected + 1).min(self.names.len() - 1),
            Action::Enter => {
                state.shader_pending_select = Some(self.names[self.selected].clone());
            }
            _ => {}
        }
        ScreenResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::state::SharedState;

    #[test]
    fn enter_stashes_selected_name_into_state() {
        let names = vec!["color_shift".to_string(), "pixelate".to_string()];
        let mut body = ShadersBody::new(names, 0);
        let mut s = SharedState::new();
        body.handle(Action::NavDown, &mut s);
        body.handle(Action::Enter, &mut s);
        assert_eq!(s.shader_pending_select.as_deref(), Some("pixelate"));
    }

    #[test]
    fn footer_shows_filtered_count_when_nonzero() {
        let body = ShadersBody::new(vec!["a".into()], 3);
        let mut grid = crate::status::grid::TextGrid::new(48, 17);
        let s = SharedState::new();
        body.render(&s, &mut grid);
        // Row 14 should contain "hidden".
        let row14: String = (0..48).map(|c| grid.at(14, c).ch).collect();
        assert!(row14.contains("hidden"));
    }
}
```

In `src/menu/mod.rs`, append:

```rust
pub mod shaders;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib menu::shaders`
Expected: FAIL — `at()` may not exist; check `status::grid` API and adjust if needed.

If `at(row, col)` is not the API (it is per existing browser tests), adapt to whatever the existing tests use (likely `grid.at(row, col).ch`).

- [ ] **Step 3: Implementation is included above; just confirm `state.shader_pending_select` was added in Task 6.**

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib menu::shaders`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/menu/shaders.rs src/menu/mod.rs
git commit -m "feat(menu): SHADERS browser screen"
```

---

### Task 8: SHDR_BNK body — shader-slot bank grid

**Files:**
- Create: `src/menu/shdr_bnk.rs`
- Modify: `src/menu/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src/menu/shdr_bnk.rs`:

```rust
//! ShdrBnkBody — 10-slot grid for shader-slot assignments. Mirrors SamplerBody.

use crate::action::Action;
use crate::state::SharedState;
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

pub struct ShdrBnkBody {
    selected: u8,
}

impl ShdrBnkBody {
    pub fn new() -> Self {
        Self { selected: 0 }
    }
}

impl Screen for ShdrBnkBody {
    fn render(&self, state: &SharedState, grid: &mut TextGrid) {
        let bank = state.current_shader_bank();
        grid.write_row(4, &format!("{:>6} {:<28} {:<5}", format!("{}-slot", state.shader_bank_number), "shader", "act"));
        for (i, opt) in bank.slots.iter().enumerate() {
            let row_idx = 5 + i;
            let line = match opt {
                None => format!("{:^6} {:<28} {:<5}", i, "", ""),
                Some(s) => {
                    let active_marker = if state.shader_active_slot == Some(i as u8) { "ON" } else { "" };
                    let truncated: String = s.shader.chars().take(28).collect();
                    format!("{:^6} {:<28} {:<5}", i, truncated, active_marker)
                }
            };
            grid.write_row(row_idx, &line);
            if i == self.selected as usize {
                grid.invert_row(row_idx);
            }
        }
    }

    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult {
        match action {
            Action::NavUp => self.selected = self.selected.saturating_sub(1),
            Action::NavDown => self.selected = (self.selected + 1).min(9),
            Action::Enter => {
                // Enter on a filled slot ⇒ Trigger; on empty ⇒ no-op (mapping
                // happens via SelectShaderSlot from SHADERS browser).
                let n = self.selected as usize;
                if state.current_shader_bank().slots.get(n).and_then(|o| o.as_ref()).is_some() {
                    state.shader_active_slot = Some(n as u8);
                }
            }
            _ => {}
        }
        ScreenResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::ShaderSlot;

    #[test]
    fn nav_clamps_to_zero_and_nine() {
        let mut b = ShdrBnkBody::new();
        let mut s = SharedState::new();
        b.handle(Action::NavUp, &mut s);
        assert_eq!(b.selected, 0);
        for _ in 0..20 {
            b.handle(Action::NavDown, &mut s);
        }
        assert_eq!(b.selected, 9);
    }

    #[test]
    fn enter_on_filled_slot_sets_active() {
        let mut s = SharedState::new();
        s.current_shader_bank_mut().slots[3] = Some(ShaderSlot {
            shader: "color_shift".into(),
            params: [0.0; 8],
        });
        let mut b = ShdrBnkBody::new();
        for _ in 0..3 { b.handle(Action::NavDown, &mut s); }
        b.handle(Action::Enter, &mut s);
        assert_eq!(s.shader_active_slot, Some(3));
    }

    #[test]
    fn enter_on_empty_slot_is_noop() {
        let mut s = SharedState::new();
        let mut b = ShdrBnkBody::new();
        b.handle(Action::Enter, &mut s);
        assert_eq!(s.shader_active_slot, None);
    }
}
```

Append to `src/menu/mod.rs`:

```rust
pub mod shdr_bnk;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib menu::shdr_bnk`
Expected: FAIL — module not yet wired (compile error).

- [ ] **Step 3: Implementation already in Step 1; confirm compile.**

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib menu::shdr_bnk`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/menu/shdr_bnk.rs src/menu/mod.rs
git commit -m "feat(menu): SHDR_BNK bank-grid screen"
```

---

### Task 9: ParamBody — shader-param editor

**Files:**
- Create: `src/menu/param.rs`
- Modify: `src/menu/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src/menu/param.rs`:

```rust
//! ParamBody — overlays in `ControlMode::ShaderParam`. Shows the 8 params of
//! the active shader slot and lets ←/→ + Enter nudge values via
//! `Action::ShaderParamAdjust(±1)` (synthesised here from NavLeft/NavRight to
//! stay within the existing keymap budget).

use crate::action::Action;
use crate::state::{ControlMode, SharedState};
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

pub struct ParamBody;

impl ParamBody {
    pub fn new() -> Self {
        Self
    }
}

impl Screen for ParamBody {
    fn render(&self, state: &SharedState, grid: &mut TextGrid) {
        grid.write_row(4, "param  name              value");
        let Some(active) = state.shader_active_slot else {
            grid.write_row(6, "  (no shader slot active)");
            return;
        };
        let bank = state.current_shader_bank();
        let Some(Some(slot)) = bank.slots.get(active as usize) else {
            grid.write_row(6, "  (active slot empty)");
            return;
        };
        for i in 0..8 {
            let row = 5 + i;
            let line = format!("{:^5}  {:<16}  {:>+.3}", i, format!("u_param{i}"), slot.params[i]);
            grid.write_row(row, &line);
            if i as u8 == state.shader_focus {
                grid.invert_row(row);
            }
        }
    }

    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult {
        // Translate NavLeft/Right into ShaderParamAdjust while in this mode.
        // NavUp/Down move the focus.
        match action {
            Action::NavUp => {
                state.shader_focus = state.shader_focus.saturating_sub(1);
                ScreenResult::Continue
            }
            Action::NavDown => {
                state.shader_focus = (state.shader_focus + 1).min(7);
                ScreenResult::Continue
            }
            Action::NavLeft => {
                // Synthesise an adjust. Caller (RootScreen) will see Continue
                // and apply.rs will not run this; instead emit it back through
                // ScreenResult::Action so main.rs's apply loop processes it.
                ScreenResult::Action(Action::ShaderParamAdjust(-1))
            }
            Action::NavRight => ScreenResult::Action(Action::ShaderParamAdjust(1)),
            Action::Back => {
                state.control_mode = ControlMode::Default;
                ScreenResult::Continue
            }
            _ => ScreenResult::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::ShaderSlot;

    #[test]
    fn navleft_synthesises_param_adjust_minus() {
        let mut b = ParamBody::new();
        let mut s = SharedState::new();
        match b.handle(Action::NavLeft, &mut s) {
            ScreenResult::Action(Action::ShaderParamAdjust(-1)) => (),
            other => panic!("expected ShaderParamAdjust(-1), got {other:?}"),
        }
    }

    #[test]
    fn nav_down_moves_focus_clamped() {
        let mut b = ParamBody::new();
        let mut s = SharedState::new();
        for _ in 0..20 { b.handle(Action::NavDown, &mut s); }
        assert_eq!(s.shader_focus, 7);
    }

    #[test]
    fn back_exits_param_mode() {
        let mut b = ParamBody::new();
        let mut s = SharedState::new();
        s.control_mode = ControlMode::ShaderParam;
        b.handle(Action::Back, &mut s);
        assert_eq!(s.control_mode, ControlMode::Default);
    }
}
```

**Important:** this depends on `ScreenResult::Action(Action)`. Check `src/ui/mod.rs` for the current variants. If it's `enum ScreenResult { Continue, Pop }`, add an `Action(Action)` variant and wire main.rs / RootScreen to forward.

Append to `src/menu/mod.rs`:

```rust
pub mod param;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib menu::param`
Expected: FAIL until `ScreenResult::Action` exists.

- [ ] **Step 3: Add `Action` variant to `ScreenResult`**

Read `src/ui/mod.rs` and add `Action(crate::action::Action)` to `ScreenResult`. Update any exhaustive `match` on `ScreenResult` elsewhere to handle the new variant (mostly RootScreen).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib menu::param`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/menu/param.rs src/menu/mod.rs src/ui/mod.rs
git commit -m "feat(menu): ParamBody for shader-param edit + ScreenResult::Action"
```

---

### Task 10: RootScreen dispatches to new bodies + footer profile indicator

**Files:**
- Modify: `src/menu/root.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/menu/root.rs` `mod tests`:

```rust
#[test]
fn footer_shows_gles_profile_indicator_when_v100() {
    use crate::render::shader_assembly::GlesProfile;
    let mut st = SharedState::new();
    st.gles_profile = GlesProfile::V100;
    st.display_mode = DisplayMode::Sampler;
    let root = RootScreen::new();
    let mut grid = crate::status::grid::TextGrid::new(48, 17);
    root.render(&st, &mut grid);
    let row15: String = (0..48).map(|c| grid.at(15, c).ch).collect();
    assert!(row15.contains("profile: pi3") || row15.contains("v100"),
        "footer should call out pi3 compat mode, got: {row15}");
}

#[test]
fn shdr_bnk_mode_renders_shdr_bnk_body() {
    use crate::shader::ShaderSlot;
    let mut st = SharedState::new();
    st.display_mode = DisplayMode::ShdrBnk;
    st.current_shader_bank_mut().slots[2] = Some(ShaderSlot {
        shader: "kaleidoscope".into(),
        params: [0.0; 8],
    });
    let root = RootScreen::new();
    let mut grid = crate::status::grid::TextGrid::new(48, 17);
    root.render(&st, &mut grid);
    // Row 7 (slot 2) should contain the shader name.
    let row7: String = (0..48).map(|c| grid.at(7, c).ch).collect();
    assert!(row7.contains("kaleidoscope"), "got: {row7}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib menu::root`
Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

In `src/menu/root.rs`:

1. Add fields `shaders: ShadersBody`, `shdr_bnk: ShdrBnkBody`, `param: ParamBody` to `RootScreen`.
2. Initialise in `new()`. `ShadersBody` starts with an empty names list — `main.rs` will refresh it after each library reload via a new public method `set_shader_names(&mut self, names: Vec<String>, filtered: usize)`.
3. Extend `render`:

```rust
match state.display_mode {
    DisplayMode::Browser => self.browser.render(state, grid),
    DisplayMode::Sampler => self.sampler.render(state, grid),
    DisplayMode::Settings => self.settings.render(state, grid),
    DisplayMode::Shaders => self.shaders.render(state, grid),
    DisplayMode::ShdrBnk => self.shdr_bnk.render(state, grid),
    DisplayMode::Frames => grid.write_row(10, "      (detour — Phase 3)"),
}
if state.control_mode == ControlMode::ShaderParam {
    self.param.render(state, grid);
}
```

4. Extend `handle` symmetrically. When dispatching to a `Body` returns `ScreenResult::Action(a)`, propagate it.
5. Footer extension in `render_chrome`:

```rust
let mut footer = if let Some(err) = state.last_error.as_deref() {
    format!("ERR: {}", err.chars().take(40).collect::<String>())
} else if state.function_on {
    "               < FUNCTION KEY ON >".to_string()
} else {
    format!("CONTROL: {:?}", state.control_mode)
};
if state.gles_profile == GlesProfile::V100 {
    footer.push_str(" [profile: pi3]");
}
grid.write_row(15, &footer);
```

(`use crate::render::shader_assembly::GlesProfile;` at top of file.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib menu::root`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/menu/root.rs
git commit -m "feat(menu): dispatch SHADERS/SHDR_BNK + footer profile indicator"
```

---

### Task 11: Hot-reload watcher (notify → crossbeam channel)

**Files:**
- Create: `src/shader/hot_reload.rs`
- Modify: `src/shader/mod.rs`
- Modify: `Cargo.toml` (add `crossbeam-channel`)

- [ ] **Step 1: Write the failing test**

Append to `Cargo.toml` under `[dependencies]`:

```toml
crossbeam-channel = "0.5"
```

Create `src/shader/hot_reload.rs`:

```rust
//! `notify`-backed file watcher that emits `ShaderEvent::Dirty(name)` on
//! `.glsl` or `.toml` saves in the shaders dir. The watcher thread is
//! lightweight; the render loop drains the channel between frames and
//! invokes `ShaderPipeline::reload(name)` (see Task 12).
//!
//! Compile failures are *not* the watcher's concern — it just signals dirt.

use std::path::{Path, PathBuf};

use crossbeam_channel::{unbounded, Receiver, Sender};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShaderEvent {
    /// Some `.glsl` or `.toml` file in the watched dir was modified. The
    /// payload is the file stem (e.g. "color_shift").
    Dirty(String),
}

pub struct ShaderWatcher {
    rx: Receiver<ShaderEvent>,
    _watcher: RecommendedWatcher,
}

impl ShaderWatcher {
    pub fn start(dir: &Path) -> notify::Result<Self> {
        let (tx, rx) = unbounded();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if matches!(ev.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    for p in ev.paths {
                        if let Some(name) = shader_name_from_path(&p) {
                            let _ = tx.send(ShaderEvent::Dirty(name));
                        }
                    }
                }
            }
        })?;
        watcher.watch(dir, RecursiveMode::NonRecursive)?;
        Ok(Self { rx, _watcher: watcher })
    }

    pub fn try_drain(&self) -> Vec<ShaderEvent> {
        self.rx.try_iter().collect()
    }
}

fn shader_name_from_path(p: &Path) -> Option<String> {
    let stem = p.file_stem()?.to_str()?;
    if stem.starts_with('_') {
        return None;
    }
    let ext = p.extension()?.to_str()?;
    if ext != "glsl" && ext != "toml" {
        return None;
    }
    Some(stem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn shader_name_from_path_picks_stems() {
        assert_eq!(shader_name_from_path(Path::new("/a/foo.glsl")).as_deref(), Some("foo"));
        assert_eq!(shader_name_from_path(Path::new("/a/foo.toml")).as_deref(), Some("foo"));
        assert_eq!(shader_name_from_path(Path::new("/a/_prelude.glsl")).as_deref(), None);
        assert_eq!(shader_name_from_path(Path::new("/a/foo.vert")).as_deref(), None);
        assert_eq!(shader_name_from_path(Path::new("/a/foo.txt")).as_deref(), None);
    }

    #[test]
    fn watcher_emits_dirty_on_file_write() {
        let tmp = tempfile::tempdir().unwrap();
        let w = ShaderWatcher::start(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("color_shift.glsl"), b"void main(){}").unwrap();
        // notify is async — give it a moment.
        std::thread::sleep(Duration::from_millis(300));
        let events = w.try_drain();
        assert!(
            events.iter().any(|e| matches!(e, ShaderEvent::Dirty(n) if n == "color_shift")),
            "expected Dirty(\"color_shift\"), got {events:?}"
        );
    }
}
```

Append to `src/shader/mod.rs`:

```rust
pub mod hot_reload;
pub use hot_reload::{ShaderEvent, ShaderWatcher};
```

- [ ] **Step 2: Run test to verify it fails (or build error)**

Run: `cargo test --lib shader::hot_reload`
Expected: FAIL initially because `crossbeam-channel` not yet in lockfile; then a build of the test should succeed.

- [ ] **Step 3: `cargo build` to pull crossbeam, then re-run**

```bash
cargo build --lib
cargo test --lib shader::hot_reload
```

Expected: PASS. If the `watcher_emits_dirty_on_file_write` test is flaky on CI, bump the sleep to 500 ms (notify on macOS uses FSEvents with coalescing).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/shader/hot_reload.rs src/shader/mod.rs
git commit -m "feat(shader): ShaderWatcher (notify → crossbeam channel)"
```

---

### Task 12: ShaderPipeline.reload + RackHandle::trigger_shader hookup

**Files:**
- Modify: `src/render/shader_pipeline.rs`
- Modify: `src/video/rack.rs` (implements `RackHandle`)
- Modify: `src/main.rs`

The video `PlayerRack` doesn't own the GL context, so `trigger_shader` needs an indirect channel. Use a small `Sender<ShaderCommand>` stored in `PlayerRack` that `main.rs` drains alongside the watcher channel.

- [ ] **Step 1: Write the failing test**

Add to `src/render/shader_pipeline.rs` `mod tests`:

```rust
#[test]
fn reload_replaces_library_entry() {
    use crate::shader::{LoadedShader, ShaderMeta};
    let mut lib = ShaderLibrary::default();
    let meta = ShaderMeta::parse("name = \"foo\"\n", "<test>").unwrap();
    lib.upsert(
        "foo",
        LoadedShader {
            meta: meta.clone(),
            fragment_body: "void main(){gl_FragColor=vec4(0);}".into(),
            source_path: std::path::PathBuf::from("foo.glsl"),
        },
    );
    let mut p = ShaderPipeline::new(GlesProfile::V100, lib);
    // Mark cache entry — we don't have a GL context in unit tests, so prove
    // the cache-invalidation path is plumbed by checking the public API:
    p.invalidate(\"foo\");
    // Library entry still present afterwards; cache entry would be gone if GL
    // context existed. (Cache is private; the assertion here is that the
    // function does not panic without a GL context.)
    assert!(p.library().get("foo").is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib render::shader_pipeline::tests::reload_replaces_library_entry`
Expected: FAIL — `invalidate` undefined.

- [ ] **Step 3: Add `invalidate` and `set_params`**

In `ShaderPipeline`:

```rust
/// Drop the cached compiled program for `name` so the next `apply` call
/// re-compiles. Safe to call without a current GL context (cache entries
/// store opaque handles; we leak them on drop in tests — production callers
/// must hold a context, in which case the dropped handles would be freed
/// via the cache HashMap's Drop, but we do not call `gl.delete_program`
/// here because we don't own a GL ref).
pub fn invalidate(&mut self, name: &str) {
    self.cache.remove(name);
}

/// Push the active shader-slot's 8 param values for the next apply().
pub fn set_params(&mut self, params: [f32; 8]) {
    self.params = params;
}

pub fn clear_active(&mut self) {
    self.active = None;
}
```

Add `params: [f32; 8]` field to `ShaderPipeline` (default `[0.0; 8]`), and in `apply`, replace the hard-coded zero loop:

```rust
for (i, loc) in cached.u_params.iter().enumerate() {
    if let Some(loc) = loc {
        gl.uniform_1_f32(Some(loc), self.params[i]);
    }
}
```

- [ ] **Step 4: Add ShaderCommand channel on PlayerRack and a forwarder**

In `src/video/rack.rs`, add:

```rust
#[derive(Debug, Clone)]
pub enum ShaderCommand {
    Trigger(String, [f32; 8]),
    Clear,
}

pub struct PlayerRack {
    // ... existing fields ...
    shader_tx: Option<crossbeam_channel::Sender<ShaderCommand>>,
}

impl PlayerRack {
    pub fn set_shader_channel(&mut self, tx: crossbeam_channel::Sender<ShaderCommand>) {
        self.shader_tx = Some(tx);
    }
}
```

Implement on the existing `impl RackHandle for PlayerRack`:

```rust
fn trigger_shader(&mut self, name: &str, params: [f32; 8]) {
    if let Some(tx) = &self.shader_tx {
        let _ = tx.send(ShaderCommand::Trigger(name.to_string(), params));
    }
}
fn clear_shader(&mut self) {
    if let Some(tx) = &self.shader_tx {
        let _ = tx.send(ShaderCommand::Clear);
    }
}
```

(If `PlayerRack` doesn't currently implement `RackHandle`, add the two methods to the existing impl with the same `apply`-side semantics.)

In `src/main.rs`, just after the `PlayerRack::new` call:

```rust
let (shader_tx, shader_rx) = crossbeam_channel::unbounded::<recur::video::rack::ShaderCommand>();
rack.set_shader_channel(shader_tx);
```

Pass `shader_rx` into `Render` (or hold it in `main` and call new `Render::process_shader_commands(rx)` each frame). Simplest: hold it in `main`, drain inside the frame loop, and call into `render` via a new method:

```rust
pub fn pipeline_mut(&mut self) -> &mut crate::render::shader_pipeline::ShaderPipeline {
    &mut self.pipeline
}
```

…on both `desktop.rs` and `pi.rs`. Stub `Render` needs a no-op `pipeline_mut` returning `&mut DummyPipeline` — easiest is to gate the drain behind `#[cfg(any(feature = "desktop", feature = "pi-base"))]` in main.

Drain block in `main.rs`'s frame loop, just before `render.begin_frame()`:

```rust
#[cfg(any(feature = "desktop", feature = "pi-base"))]
{
    for cmd in shader_rx.try_iter() {
        use recur::video::rack::ShaderCommand;
        let pipeline = render.pipeline_mut();
        match cmd {
            ShaderCommand::Trigger(name, params) => {
                pipeline.set_params(params);
                let res = unsafe { pipeline.select(/* gl ref via WinitGlTarget */, &name) };
                if let Err(e) = res {
                    state.last_error = Some(format!("shader {name} compile: {e}"));
                    pipeline.clear_active();
                } else {
                    pipeline.pulse_trigger();
                }
            }
            ShaderCommand::Clear => pipeline.clear_active(),
        }
    }
    // Hot-reload drain:
    if let Some(watcher) = shader_watcher.as_ref() {
        for ev in watcher.try_drain() {
            let recur::shader::ShaderEvent::Dirty(name) = ev;
            let pipeline = render.pipeline_mut();
            pipeline.invalidate(&name);
            // Re-read library entry from disk; ignore if missing.
            let shader_path = shader_dir.join(format!("{name}.glsl"));
            let meta_path = shader_dir.join(format!("{name}.toml"));
            if let (Ok(body), Ok(meta_src)) = (
                std::fs::read_to_string(&shader_path),
                std::fs::read_to_string(&meta_path),
            ) {
                if let Ok(meta) = recur::shader::ShaderMeta::parse(&meta_src, &meta_path.display().to_string()) {
                    pipeline.library_mut().upsert(
                        &name,
                        recur::shader::LoadedShader {
                            meta,
                            fragment_body: body,
                            source_path: shader_path,
                        },
                    );
                }
            }
        }
    }
}
```

This requires:
- A `library_mut(&mut self)` accessor on `ShaderPipeline` (1-liner).
- The `gl` ref problem: `pipeline.select(&gl, &name)` needs the GL context, which `WinitGlTarget` owns privately. Expose a higher-level method on `Render`:

```rust
pub fn select_shader(&mut self, name: &str, params: [f32; 8]) -> Result<()> {
    self.pipeline.set_params(params);
    unsafe { self.pipeline.select(&self.gl, name) }
}
pub fn clear_shader(&mut self) {
    self.pipeline.clear_active();
}
pub fn pulse_shader_trigger(&mut self) {
    self.pipeline.pulse_trigger();
}
pub fn invalidate_shader(&mut self, name: &str) {
    self.pipeline.invalidate(name);
}
pub fn upsert_shader(&mut self, name: &str, shader: crate::shader::LoadedShader) {
    self.pipeline.library_mut().upsert(name, shader);
}
```

Use those in main's drain block — drop the direct `pipeline_mut()` accessor.

- [ ] **Step 5: Run all tests**

Run: `cargo test --lib`
Expected: PASS (existing + Task 6 + Task 12).

- [ ] **Step 6: Commit**

```bash
git add src/render/shader_pipeline.rs src/render/desktop.rs src/render/pi.rs src/video/rack.rs src/main.rs
git commit -m "feat(render): wire ShaderCommand channel + invalidate/upsert for hot-reload"
```

---

### Task 13: 4 starter shaders (color_shift, pixelate, kaleidoscope, rgb_glitch)

**Files:**
- Create: `shaders/color_shift.glsl` + `.toml`
- Create: `shaders/pixelate.glsl` + `.toml`
- Create: `shaders/kaleidoscope.glsl` + `.toml`
- Create: `shaders/rgb_glitch.glsl` + `.toml`

- [ ] **Step 1: Write the failing test**

Add to `src/shader/library.rs` `mod tests`:

```rust
#[test]
fn all_starter_shaders_load_under_v100() {
    use std::path::PathBuf;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders");
    let lib = ShaderLibrary::load_dir_for_profile(&dir, GlesVersion::V100).unwrap();
    for name in ["passthrough", "color_shift", "pixelate", "kaleidoscope", "rgb_glitch"] {
        assert!(lib.get(name).is_some(), "starter shader {name} missing");
    }
}

#[test]
fn starter_shaders_have_no_v310_only_filtered() {
    use std::path::PathBuf;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders");
    let lib = ShaderLibrary::load_dir_for_profile(&dir, GlesVersion::V100).unwrap();
    assert_eq!(lib.filtered_count(), 0, "no starter shader should be v310-only");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib shader::library::tests::all_starter_shaders_load_under_v100`
Expected: FAIL — only `passthrough` exists today.

- [ ] **Step 3: Write the 4 shaders**

**`shaders/color_shift.glsl`:**

```glsl
// HSV hue / sat / value shift over u_source_0.
vec3 rgb2hsv(vec3 c) {
    vec4 K = vec4(0.0, -1.0/3.0, 2.0/3.0, -1.0);
    vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
    vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));
    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}
vec3 hsv2rgb(vec3 c) {
    vec4 K = vec4(1.0, 2.0/3.0, 1.0/3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}
void main() {
    vec4 c = texture2D(u_source_0, v_uv);
    vec3 hsv = rgb2hsv(c.rgb);
    hsv.x = fract(hsv.x + u_param0);
    hsv.y = clamp(hsv.y * (1.0 + u_param1), 0.0, 1.0);
    hsv.z = clamp(hsv.z * (1.0 + u_param2), 0.0, 4.0);
    gl_FragColor = vec4(hsv2rgb(hsv), c.a);
}
```

**`shaders/color_shift.toml`:**

```toml
name = "color_shift"
display_name = "Color Shift"
min_gles = "1.00"

[[params]]
slot = 0
name = "hue"
min = -1.0
max = 1.0
default = 0.0
curve = "linear"

[[params]]
slot = 1
name = "sat"
min = -1.0
max = 1.0
default = 0.0
curve = "linear"

[[params]]
slot = 2
name = "val"
min = -1.0
max = 1.0
default = 0.0
curve = "linear"
```

**`shaders/pixelate.glsl`:**

```glsl
// Block-quantize uv before sampling. u_param0 in [0, 1] → block edge px.
void main() {
    float block = max(1.0, u_param0 * 64.0);
    vec2 px = u_resolution / block;
    vec2 quant = floor(v_uv * px) / px;
    gl_FragColor = texture2D(u_source_0, quant);
}
```

**`shaders/pixelate.toml`:**

```toml
name = "pixelate"
display_name = "Pixelate"
min_gles = "1.00"

[[params]]
slot = 0
name = "block"
min = 0.0
max = 1.0
default = 0.25
curve = "linear"
```

**`shaders/kaleidoscope.glsl`:**

```glsl
// Radial reflect with adjustable wedge count.
void main() {
    vec2 p = v_uv - 0.5;
    float r = length(p);
    float a = atan(p.y, p.x);
    float wedges = max(2.0, floor(u_param0 * 16.0) + 2.0);
    float sector = 6.2831853 / wedges;
    a = mod(a + u_param1 * 3.14, sector);
    a = abs(a - sector * 0.5);
    vec2 q = vec2(cos(a), sin(a)) * r + 0.5;
    gl_FragColor = texture2D(u_source_0, q);
}
```

**`shaders/kaleidoscope.toml`:**

```toml
name = "kaleidoscope"
display_name = "Kaleidoscope"
min_gles = "1.00"

[[params]]
slot = 0
name = "wedges"
min = 0.0
max = 1.0
default = 0.3
curve = "linear"

[[params]]
slot = 1
name = "rotation"
min = -1.0
max = 1.0
default = 0.0
curve = "linear"
```

**`shaders/rgb_glitch.glsl`:**

```glsl
// Per-channel UV offset, time-modulated.
void main() {
    float t = u_time * (u_param3 * 4.0 + 0.1);
    vec2 ro = vec2(u_param0, 0.0) * 0.05 * sin(t);
    vec2 go = vec2(u_param1, 0.0) * 0.05 * cos(t * 1.3);
    vec2 bo = vec2(u_param2, 0.0) * 0.05 * sin(t * 0.7 + 1.7);
    float r = texture2D(u_source_0, v_uv + ro).r;
    float g = texture2D(u_source_0, v_uv + go).g;
    float b = texture2D(u_source_0, v_uv + bo).b;
    gl_FragColor = vec4(r, g, b, 1.0);
}
```

**`shaders/rgb_glitch.toml`:**

```toml
name = "rgb_glitch"
display_name = "RGB Glitch"
min_gles = "1.00"

[[params]]
slot = 0
name = "r_off"
min = -1.0
max = 1.0
default = 0.5
curve = "linear"

[[params]]
slot = 1
name = "g_off"
min = -1.0
max = 1.0
default = -0.5
curve = "linear"

[[params]]
slot = 2
name = "b_off"
min = -1.0
max = 1.0
default = 0.5
curve = "linear"

[[params]]
slot = 3
name = "speed"
min = 0.0
max = 1.0
default = 0.3
curve = "linear"
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib shader::library::tests::all_starter_shaders_load_under_v100`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add shaders/color_shift.glsl shaders/color_shift.toml \
        shaders/pixelate.glsl shaders/pixelate.toml \
        shaders/kaleidoscope.glsl shaders/kaleidoscope.toml \
        shaders/rgb_glitch.glsl shaders/rgb_glitch.toml
git commit -m "feat(shaders): 4 starter shaders (color_shift, pixelate, kaleidoscope, rgb_glitch)"
```

---

### Task 14: Default keymap bindings for shader UI

**Files:**
- Modify: `keymap.toml`

- [ ] **Step 1: Write the failing test**

Extend `src/input/keymap.rs::tests::default_keymap_toml_parses_fully`:

```rust
    assert_eq!(km.lookup("KeyH"), Some(Action::EnterMode(DisplayMode::Shaders)));
    assert_eq!(km.lookup("KeyK"), Some(Action::EnterMode(DisplayMode::ShdrBnk)));
    // Function + Digit0..9 path: keymap maps Digit0..9 → SelectSlot which is
    // bank-side; shader-side trigger uses F1..F10.
    assert_eq!(km.lookup("F1"), Some(Action::TriggerShaderSlot(0)));
    assert_eq!(km.lookup("F10"), Some(Action::TriggerShaderSlot(9)));
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib input::keymap::tests::default_keymap_toml_parses_fully`
Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Append to `keymap.toml`:

```toml
"KeyH" = "EnterMode(Shaders)"
"KeyK" = "EnterMode(ShdrBnk)"
"F1" = "TriggerShaderSlot(0)"
"F2" = "TriggerShaderSlot(1)"
"F3" = "TriggerShaderSlot(2)"
"F4" = "TriggerShaderSlot(3)"
"F5" = "TriggerShaderSlot(4)"
"F6" = "TriggerShaderSlot(5)"
"F7" = "TriggerShaderSlot(6)"
"F8" = "TriggerShaderSlot(7)"
"F9" = "TriggerShaderSlot(8)"
"F10" = "TriggerShaderSlot(9)"
```

(`SelectShaderSlot(n)` reuses Digit0..9 when in SHADERS mode, gated by `function_on`; that's apply.rs's job, not the keymap's.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib input::keymap`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add keymap.toml
git commit -m "feat(keymap): EnterMode(Shaders|ShdrBnk) + F1..F10 shader triggers"
```

---

### Task 15: Wire shader-bank persistence into main startup/shutdown

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing test**

Add an integration test `tests/integration_shader_persist.rs`:

```rust
//! Smoke test: ensure shader_banks.toml round-trips across runs (without
//! actually starting the render loop).

use recur::persist;
use recur::shader::{ShaderBank, ShaderSlot};

#[test]
fn shader_banks_persist_through_save_load_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let mut b = ShaderBank::empty();
    b.slots[1] = Some(ShaderSlot {
        shader: "color_shift".into(),
        params: [0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    });
    persist::save_shader_banks(tmp.path(), &[b.clone()]).unwrap();
    let got = persist::load_shader_banks(tmp.path()).unwrap();
    assert_eq!(got, vec![b]);
}
```

(This already passes via Task 3's unit tests; we add it as an `tests/` integration so we know the public API is exposed.)

- [ ] **Step 2: Run test to verify it builds**

Run: `cargo test --test integration_shader_persist`
Expected: PASS (after exposing `pub use shader::*` etc.).

- [ ] **Step 3: Wire startup/shutdown**

In `src/main.rs`, alongside `let banks = persist::load_banks(...)`:

```rust
let shader_banks = persist::load_shader_banks(&state_dir)?;
info!("loaded {} shader banks", shader_banks.len());
state.shader_banks = shader_banks;
```

At shutdown (alongside `save_banks`):

```rust
persist::save_shader_banks(&state_dir, &state.shader_banks)?;
```

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/integration_shader_persist.rs
git commit -m "feat(main): load/save shader_banks.toml on startup/shutdown"
```

---

### Task 16: Hot-reload + watcher wiring in main loop

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing test**

Add `tests/integration_hot_reload.rs`:

```rust
//! Smoke test: ShaderWatcher start + drain sequence does not panic and emits
//! at least one event for a touched file.

use std::time::Duration;
use recur::shader::ShaderWatcher;

#[test]
fn watcher_starts_and_drains() {
    let tmp = tempfile::tempdir().unwrap();
    let w = ShaderWatcher::start(tmp.path()).unwrap();
    std::fs::write(tmp.path().join("color_shift.glsl"), b"void main(){}").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let events = w.try_drain();
    assert!(!events.is_empty(), "watcher should emit ≥1 event for fs write");
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --test integration_hot_reload`
Expected: PASS (sub-skill: skip on Linux CI if flaky — already covered in unit tests).

- [ ] **Step 3: Wire main loop**

In `main()`:

```rust
let shader_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders");
let shader_watcher = recur::shader::ShaderWatcher::start(&shader_dir)
    .map_err(|e| { tracing::warn!("shader hot-reload disabled: {e}"); e })
    .ok();
```

Drain block goes inside the frame loop (already specified in Task 12 — confirm both `shader_rx` and `shader_watcher.as_ref()` drains are present).

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/integration_hot_reload.rs
git commit -m "feat(main): wire ShaderWatcher drain into render loop"
```

---

### Task 17: V310-only fixture + gles-profile filter test

**Files:**
- Create: `tests/fixtures/shader_v310_only.glsl`
- Create: `tests/fixtures/shader_v310_only.toml`
- Create: `tests/integration_gles_profile.rs`

- [ ] **Step 1: Write the failing test**

`tests/fixtures/shader_v310_only.glsl`:

```glsl
void main() { frag_color = vec4(0.0, 1.0, 0.0, 1.0); }
```

`tests/fixtures/shader_v310_only.toml`:

```toml
name = "v310_only"
min_gles = "3.10"
```

`tests/integration_gles_profile.rs`:

```rust
//! Verifies `--gles-profile pi3` (V100) filters V310-only shaders.

use std::fs;
use recur::shader::{GlesVersion, ShaderLibrary};

#[test]
fn v310_only_shader_filtered_under_v100() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("v310_only.glsl"),
        include_str!("fixtures/shader_v310_only.glsl"),
    ).unwrap();
    fs::write(
        tmp.path().join("v310_only.toml"),
        include_str!("fixtures/shader_v310_only.toml"),
    ).unwrap();

    let lib_v100 = ShaderLibrary::load_dir_for_profile(tmp.path(), GlesVersion::V100).unwrap();
    assert!(lib_v100.get("v310_only").is_none());
    assert_eq!(lib_v100.filtered_count(), 1);

    let lib_v310 = ShaderLibrary::load_dir_for_profile(tmp.path(), GlesVersion::V310).unwrap();
    assert!(lib_v310.get("v310_only").is_some());
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --test integration_gles_profile`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/fixtures/shader_v310_only.glsl tests/fixtures/shader_v310_only.toml tests/integration_gles_profile.rs
git commit -m "test: verify --gles-profile V100 filters V310-only shaders"
```

---

### Task 18: Smoke render of each starter shader

**Files:**
- Create: `tests/integration_shader_smoke.rs`

- [ ] **Step 1: Write the test**

```rust
//! Compile each starter shader against both GLES preludes (pure-Rust assembly
//! path, no GL context required) — verifies they at least produce textually-
//! valid source strings.

use recur::render::shader_assembly::{assemble_fragment_source, GlesProfile};

fn shader_body(name: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(format!("{name}.glsl"));
    std::fs::read_to_string(&p).unwrap_or_else(|_| panic!("read {p:?}"))
}

#[test]
fn all_starters_assemble_under_v100() {
    for name in ["passthrough", "color_shift", "pixelate", "kaleidoscope", "rgb_glitch"] {
        let body = shader_body(name);
        let src = assemble_fragment_source(GlesProfile::V100, &body);
        assert!(src.starts_with("#version 100"), "{name}: V100 prelude missing");
        assert!(src.contains("gl_FragColor"), "{name}: shader must write gl_FragColor in V100");
    }
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --test integration_shader_smoke`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/integration_shader_smoke.rs
git commit -m "test: assemble all starter shaders under V100 prelude"
```

---

### Task 19: ROADMAP update — close Phase 2 sub-plan B

**Files:**
- Modify: `.docs/ROADMAP.md`

- [ ] **Step 1: Update Recently Shipped + Execution Order**

In `.docs/ROADMAP.md`:

- Add a `## Recently Shipped` entry: `**Phase 2 sub-plan B — conjur UI + persistence** (2026-05-16): SHADERS browser, SHDR_BNK bank, shader_banks.toml, hot-reload, --gles-profile flag, 4 starter shaders (color_shift, pixelate, kaleidoscope, rgb_glitch). Codec probe remains in sub-plan C.`
- Roll oldest entry off `Recently Shipped` into `.docs/COMPLETED.md` (the dual-target spec entry stays; the oldest is "Render backend" — move it).
- Phase 2 row stays `☐` (sub-plan C still open). Add a note: `(A + B shipped; C pending — codec probe)`.

- [ ] **Step 2: Commit**

```bash
git add .docs/ROADMAP.md .docs/COMPLETED.md
git commit -m "docs(roadmap): conjur sub-plan B shipped; C (codec probe) remaining"
```

---

### Task 20: Full done-criteria verification pass

- [ ] **Step 1: Run the full test suite**

Run: `cargo test`
Expected: all green.

- [ ] **Step 2: Cross-target builds**

Run:

```bash
cargo build --no-default-features --features pi3 --release
cargo build --no-default-features --features pi5 --release
```

Expected: both PASS. Note: on macOS the `pi::PiTarget` import may fail per the known gotcha in [`project-recur-phase2`]; that's `cargo check` only, not the actual cross build. Use `cross` if running locally on macOS:

```bash
cross build --no-default-features --features pi3 --target aarch64-unknown-linux-gnu
cross build --no-default-features --features pi5 --target aarch64-unknown-linux-gnu
```

- [ ] **Step 3: Smoke run with each starter**

```bash
RECUR_SMOKE_AUTO_LOAD=1 cargo run -- --smoke-frames 2
RECUR_SMOKE_AUTO_LOAD=1 cargo run -- --smoke-frames 2 --gles-profile pi3
```

Expected: no panic; pi3 run reports filtered count of 0 in the SHADERS browser (no V310-only starter shaders).

- [ ] **Step 4: Manual UI walkthrough**

Run `cargo run`:

1. Press `KeyH` → SHADERS mode, see 5 shaders.
2. ↓ to highlight `color_shift`, press `Enter` → name stashed in `shader_pending_select`.
3. Press `KeyK` → SHDR_BNK mode.
4. Hold `ShiftLeft` (function), press `Digit0` → maps `color_shift` to slot 0.
5. Release function. Press `F1` → triggers slot 0 → video should be color-shifted.
6. Press `F2..F4` → switches between empty slots (bypass) and any other mapped shaders.
7. Press `Escape` → exits any active screen.
8. Restart binary. Confirm `color_shift` slot 0 still mapped (persistence works).
9. Edit `shaders/color_shift.glsl`, save. Watch the running window — visible change within ~500 ms.
10. Run with `--gles-profile pi3`. Confirm footer shows `[profile: pi3]`. Confirm filtered count line `5 shown, 0 hidden` in SHADERS.

- [ ] **Step 5: Commit (if any docs/log fixes)**

Only if anything was missed; otherwise this task is observational and produces no commit.

---

## Self-Review Checklist

- **Spec coverage:** Section 3 (TOML schema) ✓ (existing; no schema changes in B). Section 4 (Assembly + runtime) ✓ (already in A; `set_params`/`invalidate`/hot-reload added in Tasks 11–12). Section 5 (SHADERS, SHDR_BNK, PARAM, persistence) ✓ (Tasks 7–10, 15). Section 6 (`--gles-profile`) ✓ (Task 4). Section 7 (codec probe) **out of scope** (sub-plan C). Section 8 done-criteria 1, 4, 5, 6, 8 ✓ (Tasks 13, 17, 20). Criterion 7 (FILES `[X]`) **out of scope**. Criterion 2 (smoke render) ✓ (Task 20).
- **Placeholder scan:** clean. Every code block has full text. No "implement later" markers.
- **Type consistency:** `ShaderSlot { shader: String, params: [f32; 8] }` is the only payload type; used uniformly in `shader/banks.rs`, `persist.rs`, `apply.rs`, `menu/*`, `render/shader_pipeline.rs`. `ShaderCommand { Trigger(String, [f32;8]), Clear }`. `ShaderEvent::Dirty(String)`. `Action::SelectShaderSlot(u8)` / `TriggerShaderSlot(u8)` / `ShaderParamAdjust(i8)` / `ShaderParamSelect(u8)` — all referenced consistently.
- **Cross-task references:** `state.shader_pending_select`, `state.shader_active_slot`, `state.shader_focus`, `state.gles_profile` all introduced in Task 2 / 6 and consumed in Tasks 7–10. `ShaderPipeline::{invalidate, set_params, clear_active, library_mut}` added in Task 12; `Render::{select_shader, clear_shader, pulse_shader_trigger, invalidate_shader, upsert_shader}` added in Task 12 and used in Task 16's drain block. `RackHandle::{trigger_shader, clear_shader}` added in Task 6 (apply tests) and implemented on `PlayerRack` in Task 12.
- **Known gotcha carried from sub-plan A:** `cargo check --features pi3` on macOS still trips `unresolved import pi::PiTarget`. Cross-builds are clean. Tasks 4 + 20 acknowledge this rather than try to fix it.

