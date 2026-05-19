# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Desktop dev
cargo run
cargo run -- --smoke-frames 60          # headless smoke test, exits after 60 frames
cargo run -- --gles-profile pi3         # force GLSL ES 1.00 parity on desktop

# Tests
cargo test --lib                         # all unit tests (257)
cargo test --lib -- module::test_name    # single test

# Cross-compile for Pi (requires `cross` installed)
cross build --release --no-default-features --features pi3 --target aarch64-unknown-linux-gnu
cross build --release --no-default-features --features pi5 --target aarch64-unknown-linux-gnu
```

## Architecture

### Dispatch model

`apply()` in `src/apply.rs` is the single dispatch point for all user intent. Every key press becomes an `Action` (defined in `src/action.rs`), flows through `src/input/keymap.rs` → `double_tap` filter → `apply()`. No logic lives in the input or render layers.

### 3-pipeline rotation

`PlayerRack` (`src/video/rack.rs`) holds `last / current / next` GStreamer pipelines. When a slot fires: `last` tears down, `current` → `last`, `next` → `current`, new `next` preloads. This hides load latency — the next clip is in a paused pipeline while the current one plays.

### Feature flags (mutually exclusive — enforced in `build.rs`)

| Feature | Target | GLSL ES | GL |
|---|---|---|---|
| `desktop` (default) | macOS / Linux x86_64 | 3.10 (parity with pi5) | winit + glutin |
| `pi3` | Pi 3 B+ | 1.00 | DRM/KMS + EGL |
| `pi5` | Pi 5 | 3.10 | DRM/KMS + EGL |

`pi-base` is a shared dep group (`khronos-egl`, `gbm`, `drm`, `evdev`) pulled in by both `pi3` and `pi5`. `--gles-profile pi3` on a desktop build simulates pi3 shader restrictions without cross-compiling.

### Shader assembly

Shaders in `shaders/` get a platform prelude prepended at load time (`render/shader_assembly.rs`). Shaders with `min_gles = "3.10"` in their TOML sidecar are hidden on pi3 builds. Hot-reload failures fall back to the `__safe__` passthrough shader rather than crashing.

### FrameRing (detour)

Byte-budgeted in-memory ring: pi3 128 MB, pi5 256 MB, desktop 512 MB. Override via `ring_budget_mb` in `config.toml`. `sysinfo` enforces a 50%-free-RAM ceiling at startup. Scrub mode (`DetourScrub`) alpha-blends the ring frame over live.

### State persistence

All TOML state written atomically (`write tmp → rename`). `banks.toml` accepts both the current tagged `source = { kind = "file", value = "..." }` form and the legacy `location = "..."` form for backward compatibility. State search order: `$RECUR_STATE_DIR` → `<exec>/.config/recur/` → `./.config/recur/`.

### Capture pipeline (`captur`)

`build_for_capture` in `src/video/pipeline_factory.rs` builds v4l2src (linux) / avfvideosrc (macOS) pipelines. Recording is a hot-swapped tee branch using `splitmuxsink`; finalized on EOS. Auto-imports the saved file into the first empty bank slot. Hardware verification requires a Pi + USB camera.

## Roadmap

Active phase: **Phase 5 — Pi inputs** (GPIO matrix via `rppal`, USB MIDI via `midir`, ADC knobs over I2C — all behind `pi-base` feature). See `.docs/ROADMAP.md` for full execution order.

Full design specs per phase: `docs/superpowers/specs/`.
