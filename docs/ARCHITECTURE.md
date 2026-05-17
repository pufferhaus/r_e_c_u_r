# r_e_c_u_r — Architecture Reference

## Module map

```
src/
  main.rs            — CLI args, init, render loop, event pump
  lib.rs             — crate root, re-exports
  action.rs          — Action enum: every user-issued intent
  apply.rs           — apply(action, state, rack) — pure dispatch
  state.rs           — SharedState, Slot, Bank, DisplayMode, ControlMode, settings enums
  config.rs          — config.toml schema + user_state_dir() resolution
  persist.rs         — TOML round-trip: banks, settings, paths, shader_banks
  error.rs           — crate-wide Error enum

  video/
    rack.rs           — PlayerRack: 3-pipeline rotation (last / current / next)
    player.rs         — single GStreamer pipeline lifecycle
    pipeline_factory.rs — build_for_file / build_for_capture pipeline strings
    probe.rs          — gstreamer-pbutils codec probe worker + ProbeCache
    mod.rs

  capture/
    device.rs         — CaptureDevice, enumerate_capture_devices (v4l2 / avf)
    recording.rs      — Target enum, ActiveRecording, build_record_bin_desc,
                        generate_recording_path, check_disk_space
    mod.rs

  detour/
    ring.rs           — FrameRing: byte-budgeted contiguous RGBA ring
    budget.rs         — ring_budget_bytes() per-target + sysinfo ceiling
    settings.rs       — DetourSettings: speed/direction/mix/markers/auto-play
    mod.rs

  shader/
    library.rs        — scan shaders/, filter by GLES profile
    meta.rs           — ShaderMeta: TOML sidecar schema
    banks.rs          — ShaderBank, ShaderSlot (10-slot model mirrors video)
    hot_reload.rs     — notify watcher → channel → recompile
    params.rs         — param range + step helpers
    mod.rs

  render/
    desktop.rs        — winit + glutin window + GL context
    pi.rs             — DRM/KMS + EGL context (pi3 / pi5)
    shader_assembly.rs — GlesProfile, prelude selection, shader source build
    shader_pipeline.rs — compiled GL shader program lifecycle
    shader.rs         — composite + shader pass
    text.rs           — 17×48 amber text grid rasterizer
    mod.rs

  menu/
    root.rs           — RootScreen: top-level screen dispatcher
    browser.rs        — file tree + codec-probe markers
    sampler.rs        — bank × slot grid
    settings.rs       — settings cycle screen
    shaders.rs        — shader file browser
    shdr_bnk.rs       — shader bank screen
    frames.rs         — detour ring stats + scrub display
    param.rs          — per-shader param editor
    mod.rs

  input/
    mod.rs            — InputSource trait, RawEvent
    winit_src.rs      — desktop keyboard → RawEvent
    keymap.rs         — keymap.toml → Action dispatch
    double_tap.rs     — double-tap Esc/Backspace → Panic
    mock.rs           — test stub

  status/
    grid.rs           — TextGrid: 17×48 char buffer
    mod.rs

  ui/
    mod.rs            — ScreenStack, Screen trait
```

---

## Core data flow

```
InputSource (WinitSource / GPIO / MIDI)
    ↓  RawEvent
Keymap::translate()
    ↓  Option<Action>
double_tap filter
    ↓  Action
apply(action, &mut SharedState, &mut RackHandle)
    ↓  side effects on state, GStreamer pipeline commands
PlayerRack (video) + ShaderPipeline (GL) + FrameRing (detour)
    ↓  rendered frame
Render backend (desktop window / Pi DRM framebuffer)
    ↓
SPI LCD (Pi) / on-screen text overlay (desktop)
```

`apply()` in `src/apply.rs` is the single dispatch point for all user intent. It takes an `Action`, mutates `SharedState` directly, and sends commands to the rack via `RackHandle` (a channel-based handle). All business logic lives here; no logic lives in the input layer or render layer.

---

## 3-pipeline rotation

`PlayerRack` always holds three named pipeline slots: `last`, `current`, `next`.

```
last:    most recently active player — kept alive for crossfade or instant rollback
current: the playing player — composited to output
next:    preloaded player (if load_next = auto) — ready for zero-gap switch
```

When a new slot is triggered:
1. `last` is torn down (or held for crossfade).
2. `current` → `last`.
3. `next` → `current` (if preloaded and matching), otherwise a new pipeline is built immediately.
4. A new `next` is preloaded from the subsequent sequential slot (if `load_next = auto`).

This rotation is what hides load latency — the next clip is buffered in a paused pipeline while the current one plays.

---

## GStreamer pipeline shapes

### File playback

```
uridecodebin uri=file:///... ! videoconvert ! glupload ! glsinkbin
```

On Pi, `v4l2h264dec` / `v4l2h265dec` are selected automatically by `uridecodebin` for hardware decode; dmabuf → EGLImage zero-copy path used when available.

### Live capture

```
v4l2src device=/dev/videoN [! video/x-raw,...] ! videoconvert ! glupload ! glsinkbin
```

On macOS: `avfvideosrc` replaces `v4l2src`.

### Capture with concurrent recording (Phase 4b)

```
v4l2src device=/dev/videoN
    ! videoconvert
    ! tee name=cap_t

cap_t. ! queue ! videoscale ! glupload ! glsinkbin       ← live preview branch
cap_t. ! [record bin]                                    ← record branch (hot-swappable)
```

The record bin is:
```
queue ! <encoder> ! <parser> ! splitmuxsink muxer-factory=mp4mux max-size-time=0 location="..."
```

The record bin is added to the pipeline at recording start (hot-swap: `gst_bin_add` + `sync_state_with_parent`) and removed by sending EOS down the queue's sink pad, waiting for the EOS event on the splitmuxsink's bus, then removing it.

The tee's request pad is released in `finalize()` after the bin is removed.

---

## Build targets and feature flags

```
Cargo features:
  desktop  (default) — winit + glutin + softbuffer; WinitSource input
  pi-base             — khronos-egl + gbm + drm + evdev + memmap2; DRM/KMS backend
  pi3                 — pi-base; VideoCore IV, GLES 2.0, GLSL ES 1.00
  pi5                 — pi-base; VideoCore VII, GLES 3.1, GLSL ES 3.10
  pi                  — deprecated alias → pi3 (removed in 0.2.0)
```

`build.rs` enforces exactly one of `{desktop, pi3, pi5}` at compile time.

GLSL ES profile affects:
- Which shaders are visible in the browser (`min_gles = "3.10"` shaders hidden on `pi3`)
- Which prelude is prepended to shader source (`_prelude_100.glsl` vs `_prelude_310.glsl`)
- `--gles-profile pi3` on a desktop build forces pi3 parity for testing

---

## Shader assembly

Each shader is composed as:

```
<prelude>           — platform precision + extension declarations
<shared uniforms>   — u_time, u_resolution, u_audio, u_param0..7, u_source_0..N
<shader body>       — user .glsl
```

The `ShaderAssembler` in `render/shader_assembly.rs` performs this concatenation at load time. Hot-reload re-runs the assembler and recompiles the GL program; failures fall back to the `__safe__` baked passthrough rather than crashing.

---

## State persistence model

All user state is written atomically: write to `<name>.toml.tmp`, then `rename()`. This means a crash mid-write leaves the old file intact.

`banks.toml` uses a sparse index-preserving wire format:

```toml
[[banks]]
[[banks.slots]]
index = 3
source = { kind = "file", value = "/clips/loop.mp4" }
name = "loop.mp4"
start = 0.0
end   = 4.2
length = 10.0
rate  = 1.0
```

The `Slot` deserializer accepts both the new tagged `source` form and the legacy `location = "..."` form for backward compatibility.

`shader_banks.toml` follows the same sparse pattern, with `shader` (name string) and `params` (8-element f32 array) per entry.

---

## Phase history

| Phase | Key additions |
|---|---|
| 1 — core | PlayerRack, 3-pipeline rotation, Slot/Bank/SharedState, keymap dispatch, Browser/Sampler/Settings menus, SPI LCD text grid, DRM/KMS Pi render, desktop winit render |
| 2 — conjur | ShaderLibrary, ShaderBank, hot-reload, GlesProfile / GLSL ES prelude split, codec probe (gstreamer-pbutils), SHADERS/SHDR_BNK menus |
| 3 — detour | FrameRing, byte-budget + sysinfo ceiling, DetourSettings (speed/dir/mix/markers/auto-play), DetourScrub control mode, FRAMES menu |
| 4a — captur | CaptureDevice, enumerate_capture_devices, build_for_capture pipeline factory, AddCaptureSlot action |
| 4b — captur recording | build_record_bin_desc, record bin hot-swap, EOS finalize, auto-import to bank, disk-space gate (10 MB, 10s poll), `<REC>` / `<SAV>` status indicators |

Full design specs: `docs/superpowers/specs/`.

## What's next

**Phase 5 — Pi inputs:** GPIO matrix via `rppal` (i_n_c_u_r PCB numpad), USB MIDI via `midir`, analog ADC (ADS1115-class I2C pots) → `RawEvent::Knob`. All behind `--features pi`; desktop keeps `WinitSource` only. `keymap.toml` will gain `[midi]` and `[gpio]` sections. No pi3/pi5 divergence.
