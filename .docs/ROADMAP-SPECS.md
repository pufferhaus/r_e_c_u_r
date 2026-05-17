# r_e_c_u_r (Rust port) — phase specs

Full specs per phase from the Execution Order table in `ROADMAP.md`. Each is a one-paragraph summary; the detailed design lives in `docs/superpowers/specs/`.

## 1 — r_e_c_u_r-core

**Spec**: [`docs/superpowers/specs/2026-05-12-recur-rust-phase1-core-design.md`](../docs/superpowers/specs/2026-05-12-recur-rust-phase1-core-design.md)

Single-binary Rust port of the core video sampler. GStreamer for decode (3-pipeline rotation: `last / current / next`). Per-slot loop in / out, rate, layer. Sampler modes: sequential, parallel, fixed-length, random-start. Banks of 10 slots each, persisted to `banks.toml`. Menus: Browser (file tree + slot annotation), Sampler (bank view), Settings (nested cycle-through). 17×48 char SPI LCD UI. Desktop input via `winit` keyboard (mac-friendly for local dev). GL composite output to HDMI/composite on Pi; window on desktop. Render backend, LCD status grid, and screen-stack framework copied from mandleROT.

**Done when**: `cargo test --lib` green; `cargo run -- --smoke-frames 60` plays the bundled SMPTE test clip; user can map → trigger → loop slots via keyboard; settings cycle persists; `cross build --features pi` runs on a real Pi.

**Carried forward**: the Phase 1 build used the single `pi` feature; the 2026-05-16 dual-target revision renames it to `pi3` (with `pi` retained as a deprecated alias for one release).

## 2 — conjur (shader layer)

GLSL shader layer applied over the video sources. Each shader = one `.glsl` + `.toml` pair in `shaders/` (same scheme as mandleROT). Shader can read up to N video sources as `sampler2D` uniforms (`u_source_0`, `u_source_1`, …) plus the standard mandleROT uniforms (`u_time`, `u_resolution`, `u_audio.xyzw`, `u_param0..7`, etc.). New display modes: `SHADERS` (shader file browser) and `SHDR_BNK` (per-layer shader slot bank). Hot-reload via `notify`. Reuses mandleROT's `src/scene/` + `src/render/` shader-assembly pipeline.

**Dual-target rules** (per 2026-05-16 spec revision):
- Each shader `.toml` declares `min_gles = "1.00"` (default) or `"3.10"`.
- Shader assembler picks prelude per build: `shaders/_prelude_100.glsl` for `pi3`, `shaders/_prelude_310.glsl` for `pi5` and desktop default.
- On a `pi3` binary, the `SHADERS` browser filters out `min_gles = "3.10"` shaders; status line reports the hidden count.
- Pi 5–only features available behind `min_gles = "3.10"`: compute shaders + SSBOs, MRT, `textureLod()`, integer samplers.
- Desktop dev build defaults to `pi5` parity; `--gles-profile pi3` CLI flag forces `1.00` emit + filtering for regression checking.

**Depends on**: Phase 1 (need the player rack + composite output to feed shader inputs).

## 3 — detour (frame ring)

In-memory ring buffer of decoded RGBA frames (target ~500 frames at the configured render resolution; size-cap by megabytes, not frame count, so it scales sanely). Captures from `current` player. New display mode `FRAMES` shows: ring size, scrub position, start/end markers, mix amount, playback speed / direction (`detour_settings` from the original). New control mode `DetourScrub` re-maps inputs to scrub controls. Compose pass blends ring output with live `current` per `detour_mix`. Reuses Phase 1's GL composite.

**Ring sizing** (revised 2026-05-16 — Pi 5 1GB baseline):
- Ring size = byte-budget, not frame count.
- Per-target defaults:
  - `pi3`: 128 MB (~34 frames @ 720p RGBA, ~107 @ 480p).
  - `pi5`: 256 MB (~68 frames @ 720p RGBA, ~213 @ 480p) — sized for 1GB Pi 5 baseline.
  - `desktop`: 512 MB.
- Override via `[detour] ring_budget_mb = N` in `config.toml`. Larger Pi 5 SKUs (2/4/8/16 GB) bump this up explicitly.
- Hard ceiling: 50% of detected free RAM at startup (`sysinfo` or `/proc/meminfo` read — no new heavy dep).
- Ring is contiguous heap allocation, pre-allocated at startup, no reallocation during playback.

**Depends on**: Phase 1.

## 4 — captur (live capture)

USB v4l2 / CSI live-capture as a video source, mappable into bank slots like a file. GStreamer source becomes `v4l2src device=/dev/videoN ! ...` instead of `uridecodebin uri=file://...`. Recording (`<REC>` indicator from the original) writes a file via `splitmuxsink` while still feeding the live preview.

**Dual-target divergence** (per 2026-05-16 spec revision):
- `pi5`: USB 3.0 + libcamerasrc make captur first-class. Recording uses `splitmuxsink` with `v4l2h265enc` for H.265 output.
- `pi3`: USB-2 bus contention with Ethernet keeps captur fragile (per the original project's notes). Recording uses H.264 only via `splitmuxsink`. CSI via `libcamerasrc` available.
- Captur stays Phase 4 ordering on both targets; the "intentionally near-last" justification is dropped for `pi5`.

**Depends on**: Phase 1. Optional benefit from Phase 2 if shaders are used to clean up capture.

## 5 — Pi inputs (`pi` feature flag)

GPIO matrix scan for the original `i_n_c_u_r` PCB numpad via `rppal`. USB MIDI in via `midir` (note → SelectSlot, CC → CycleSetting / knob mapping). Analog pots via I2C ADC (ADS1115-class), feeding `RawEvent::Knob`. All behind `cargo build --features pi`; macOS / desktop builds keep `WinitSource` as the only input. `keymap.toml` extended with `[midi]` and `[gpio]` sections.

**Dual-target rules**: No divergence between `pi3` and `pi5`. `rppal`, `midir`, and the ADS1115 I²C ADC code path are identical on both Pis. `keymap.toml` shape unchanged.

**Depends on**: Phase 1.

## Source format handling (cross-phase)

Per the 2026-05-16 dual-target revision: video files are codec-probed via `gstreamer-pbutils` discoverer when they enter the Browser. Per-target unsupported lists:

- `pi3` / `desktop --gles-profile pi3`: H.265 (HEVC), VP9, AV1.
- `pi5` / `desktop` (default): none currently.

Unsupported files appear in the Browser with an `[X]` marker; selecting them shows the codec name in the status line. The map-to-slot action is refused with `cannot map: pi3 build does not support hevc`. No silent CPU fallback and no auto-transcode. README documents external workaround (`ffmpeg -c:v libx264 ...`).

Backlog: `recur convert <in> <out>` ffmpeg-wrapper subcommand.
