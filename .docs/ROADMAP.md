# r_e_c_u_r (Rust port) — Roadmap

Rust re-imagining of [langolierz/r_e_c_u_r](https://github.com/langolierz/r_e_c_u_r). Targets Pi 3 B+ / Pi 4 / Pi 5, with macOS + Linux x86_64 dev. Same render backend + LCD pathway as `/Users/cody/Dev/mandleROT`.

The original Python source is preserved at `.old/` (gitignored).

## Bugs / Blockers

_(none)_

## Recently Shipped

- **Phase 4b — captur (recording)** (2026-05-17): `splitmuxsink` tee'd record branch (post-`videoconvert`, pre-`videoscale`) on the live capture pipeline; per-target encoder (pi3 h264, pi5 h265, mac vtenc, linux desktop x264enc); `KeyR` mode-aware (`DetourScrub`→`DetourTogglePlay`, else `RecordToggle`); `<REC> MM:SS` / `<SAV>` indicators; disk-space gate (10 MB, polled 10s); auto-import finalized MP4 into first empty bank slot. **Hardware verification pending** — software smoke + macOS `vtenc_h264` integration test pass; v4l2h264/h265 requires Pi. See `docs/superpowers/specs/2026-05-17-captur-4b-design.md`.
- **Phase 4a — captur (live capture, no recording)** (2026-05-16): `Slot.source: SourceKind` enum (File | Capture) with back-compat serde shim for legacy `location =` form; `CaptureDevice` + `enumerate_capture_devices` (linux v4l2 / macos avf); `build_for_capture` pipeline factory; `KeyC` = `AddCaptureSlot`; SamplerBody `[cap]` marker. **Hardware verification pending** — software smoke covers slot model + pipeline-string generation only; real v4l2/avfvideosrc runtime requires a Pi or USB camera. Recording → Phase 4b.
- **Phase 3 — detour** (2026-05-16): in-memory frame ring (byte-budgeted: pi3 128 MB, pi5 256 MB, desktop 512 MB; `config.toml` override; 50%-free-RAM ceiling via `sysinfo`). `ControlMode::DetourScrub` mode, FRAMES display body, scrub controls (frame ±1, speed cycle 0.25/0.5/1/2/4×, direction, auto-play, start/end markers, mix 0–100%). Compose layer alpha-blends ring frame over live. GStreamer scales source frames to render resolution so ring captures consistently. See `docs/superpowers/specs/2026-05-16-detour-design.md`.

## Design Notes

- **Decoder**: GStreamer (`gstreamer-rs`). `uridecodebin ! videoconvert ! glupload ! glsinkbin`. On Pi `v4l2h264dec` auto-selects, dmabuf → EGLImage zero-copy.
- **Render**: copy mandleROT's `src/render/` verbatim. `desktop` / `pi3` / `pi5` feature split, sharing the `pi-base` deps (`khronos-egl` + `gbm` + `drm`) on both Pi targets. Desktop uses `glow` + `winit` + `glutin`.
- **UI**: 17×48 amber text grid on SPI LCD. Reuse mandleROT's `src/status/` and `src/ui/` (Screen trait + ScreenStack).
- **Playback model**: 3 GStreamer pipelines rotated as `last / current / next` to hide load latency. Mirrors the Python original's player rotation.
- **State files**: TOML in `user_state_dir()` (precedence `$RECUR_STATE_DIR` → `<exec>/.config/recur/` → `./.config/recur/`).
- **Panic semantics**: `panic = "abort"`; systemd restarts on Pi. Esc / Backspace ×2 ≤ 400 ms = `Action::Panic` resets the rack.
- **Targets**: `pi3` (baseline, original r_e_c_u_r replacement) and `pi5` (forward path). Compile-time feature split; `pi-base` shared deps; `build.rs` enforces exactly-one-of. Deprecated `pi` alias maps to `pi3` for one release.

## Execution Order

Each phase = its own design spec + implementation plan + ship cycle.

| ID | Phase | Status | Key files / dirs |
|---|---|---|---|
| 1 | **r_e_c_u_r-core** — file playback, sample bank, loop points, sampler modes, Browser/Sampler/Settings menus, desktop keyboard control | ✅ | `src/video/`, `src/sample/`, `src/menu/`, `src/input/winit_src.rs` |
| 2 | **conjur** — GLSL shader layer + codec probe. All 3 sub-plans (A: infra, B: UI+persistence, C: codec probe) shipped 2026-05-16 | ✅ | `src/shader/`, `shaders/`, `src/menu/{shaders,shdr_bnk,param}.rs`, `src/video/probe.rs` |
| 3 | **detour** — in-memory frame ring + scrub mode + mix compose. Byte-budgeted ring (per-target defaults; sysinfo ceiling); DetourScrub control mode; FRAMES display body | ✅ | `src/detour/`, `src/menu/frames.rs` |
| 4 | **captur** — USB v4l2 / CSI live-capture as a video source, slot-mapped. *4a (live capture, no recording) shipped 2026-05-16 pending hardware verification; 4b (recording) shipped 2026-05-17 pending hardware verification on Pi* | ✅ | `src/capture/`, `src/video/pipeline_factory.rs` |
| 5 | **Pi inputs** — GPIO matrix (`i_n_c_u_r` PCB), USB MIDI, analog ADC over I2C (`pi-base` feature) | ☐ | `src/input/{gpio,midi,adc}.rs` |

Active phase = lowest-numbered row with ☐.

## Backlog

- `recur import-old-banks <dir>` — migrate from original Python `json_objects/`.
- Bench composite output to native composite TRRS jack on Pi 3 B+ vs Pi 5.
- Auto-discover `paths_to_browser` from common mount points (`/media/*`, USB).
- MIDI clock sync for `LoopType::Parallel` (line up loop restarts to bar).
