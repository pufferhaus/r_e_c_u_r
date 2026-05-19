# Render backend — follow-up plan

> **For agentic workers:** Use superpowers:subagent-driven-development. Tasks use `- [ ]` checkboxes.

**Goal:** Replace the Phase-1 `Render` stub with a working GL backend so video frames actually display. Verify Pi cross-build.

**Architecture:** Small `WinitGlTarget` owns `winit::EventLoop` + `glutin::Surface` + `glow::Context`. Single RGBA texture + single fullscreen-quad shader. `Player::pull_latest_rgba` extracts decoded frames from the gst appsink. `main.rs` pumps events + draws per frame.

**Tech:** `glow 0.14`, `winit 0.30` (pump_events), `glutin 0.32`, `glutin-winit 0.5`, `raw-window-handle 0.6`. All already in `Cargo.toml`. Pi path uses `khronos-egl`, `gbm`, `drm`.

**Spec:** Resolves Bugs/Blockers from `.docs/ROADMAP.md`.

---

## Task R1 — Real desktop `Render` backend

**Files:**
- Replace: `src/render/mod.rs` (currently a stub from T11)
- Create: `src/render/desktop.rs`
- Create: `src/render/shader.rs` (vertex + fragment shaders inline as `&'static str`)
- Modify: `src/main.rs` (construct `Render` with window dims; remove `--headless` no-op; integrate `pump`)

- [ ] **Step 1: Write `src/render/shader.rs`**

```rust
//! GLSL ES 1.00 shader pair for the fullscreen video quad.

pub const VERT: &str = r#"
#version 100
attribute vec2 a_pos;
attribute vec2 a_uv;
varying vec2 v_uv;
void main() {
    v_uv = a_uv;
    gl_Position = vec4(a_pos, 0.0, 1.0);
}
"#;

pub const FRAG: &str = r#"
#version 100
precision mediump float;
varying vec2 v_uv;
uniform sampler2D u_tex;
uniform float u_alpha;
void main() {
    vec4 c = texture2D(u_tex, v_uv);
    gl_FragColor = vec4(c.rgb, c.a * u_alpha);
}
"#;
```

- [ ] **Step 2: Write `src/render/desktop.rs`**

Build a `WinitGlTarget` that owns the GL context + texture + program. Reference the pattern in `/Users/cody/Dev/mandleROT/src/render/desktop.rs` lines 1-200 (window + glutin setup) — but **do not copy the status preview window, softbuffer, FBO, postfx, or scene paths**.

API:

```rust
pub struct WinitGlTarget {
    event_loop: Option<winit::event_loop::EventLoop<()>>,
    window: std::rc::Rc<winit::window::Window>,
    surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    gl_context: glutin::context::PossiblyCurrentContext,
    gl: std::sync::Arc<glow::Context>,
    tex: <glow::Context as glow::HasContext>::Texture,
    program: <glow::Context as glow::HasContext>::Program,
    vbo: <glow::Context as glow::HasContext>::Buffer,
    u_alpha: <glow::Context as glow::HasContext>::UniformLocation,
    last_tex_w: u32,
    last_tex_h: u32,
}

impl WinitGlTarget {
    pub fn new(w: u32, h: u32, title: &str) -> anyhow::Result<Self>;
    pub fn pump(&mut self) -> Vec<winit::event::KeyEvent>;
    pub fn begin_frame(&mut self);
    pub fn draw_video_layer(&mut self, rgba: &[u8], w: u32, h: u32, alpha: f32);
    pub fn end_frame(&mut self);
}
```

Implementation notes:

- Use the `glutin-winit::DisplayBuilder` + `ConfigTemplateBuilder` dance verbatim from mandleROT's desktop.rs (it's the canonical winit 0.30 + glutin 0.32 incantation).
- VBO: 6 vertices (two triangles), interleaved `(x, y, u, v)`. Static buffer, write once in `new`.
- Texture: `GL_TEXTURE_2D`, format `GL_RGBA`, `GL_UNSIGNED_BYTE`. On the first `draw_video_layer` call (and when `w/h` change), allocate with `tex_image_2d`; subsequent same-size frames use `tex_sub_image_2d` (faster, no realloc).
- `pump()` uses `EventLoopExtPumpEvents::pump_events(Some(Duration::ZERO), ...)` so it never blocks. Drains and buffers `WindowEvent::KeyboardInput` events.
- On `WindowEvent::CloseRequested`, set an internal `should_close` flag the caller can check via a `pub fn should_close(&self) -> bool` accessor — wire it into the main loop's exit condition.
- On `WindowEvent::Resized`, update glutin surface dimensions and `gl.viewport(0, 0, w, h)`.

- [ ] **Step 3: Rewrite `src/render/mod.rs`**

```rust
//! Video render — desktop backend gated by `desktop` feature.

#[cfg(feature = "desktop")]
mod desktop;
mod shader;

#[cfg(feature = "desktop")]
pub use desktop::WinitGlTarget as Render;

#[cfg(not(feature = "desktop"))]
mod stub {
    /// No-op stub used when neither `desktop` nor `pi` feature is enabled.
    /// Lets the crate compile for unit-test-only invocations.
    pub struct Render;
    impl Render {
        pub fn new(_w: u32, _h: u32, _t: &str) -> anyhow::Result<Self> {
            Ok(Self)
        }
        pub fn pump(&mut self) -> Vec<winit::event::KeyEvent> {
            Vec::new()
        }
        pub fn should_close(&self) -> bool {
            false
        }
        pub fn begin_frame(&mut self) {}
        pub fn draw_video_layer(&mut self, _: &[u8], _: u32, _: u32, _: f32) {}
        pub fn end_frame(&mut self) {}
    }
}
#[cfg(not(feature = "desktop"))]
pub use stub::Render;
```

(Pi feature lands in Task R3.)

- [ ] **Step 4: `cargo build` green**

Run: `cargo build`
Expected: clean. Lots of new glow / winit / glutin code — first build may take a minute.

- [ ] **Step 5: Wire into `main.rs`**

Add right after `gst::init()?`:

```rust
let mut render = recur::render::Render::new(cfg.render.width, cfg.render.height, "r_e_c_u_r")?;
```

Replace the existing input-drain block with:

```rust
// 1. Drain input → Actions
for ev in render.pump() {
    input.push_key_event(&ev);
}
if render.should_close() {
    info!("window closed, exiting");
    break;
}
for action in input.poll() {
    let _consumed = stack.dispatch(action.clone(), &mut state);
    apply(action, &mut state, &mut rack);
}
```

Remove the `--headless` arg handling (or keep the flag and route to the existing stub by simply not pulling frames into Render — your call). For simplicity, **remove the `--headless` flag** entirely.

Where the comment `// 4. Render frame (window or pi) — stubbed for Phase 1.` lives, replace with:

```rust
// 4. Pull latest frame from current player and draw.
render.begin_frame();
if let Some((rgba, w, h)) = rack.current.pull_latest_rgba() {
    render.draw_video_layer(&rgba, w, h, 1.0);
}
render.end_frame();
```

(`pull_latest_rgba` lands in Task R2 — until then, this block won't compile. **Land R2 before re-building main.rs.** Order: R1 step 1-4 → R2 → R1 step 5-7.)

- [ ] **Step 6: Smoke run shows a video frame**

After R2 is in, run:

```bash
cargo run -- --smoke-frames 120
```

Expected: a window opens at 720×480; the SMPTE test clip plays for 4 seconds; exits.

If frames decode but nothing displays, suspect texture upload format or shader compile. Inspect `gl.get_error()` after each upload during dev.

- [ ] **Step 7: Commit**

```bash
git add src/render/ src/main.rs
git commit -m "real desktop render backend; draw video to fullscreen quad"
```

---

## Task R2 — `Player::pull_latest_rgba`

**Files:**
- Modify: `src/video/player.rs`

- [ ] **Step 1: Add method**

In `src/video/player.rs`, add this method to the `impl Player` block:

```rust
use gstreamer::prelude::*;
use gstreamer_app::AppSink;

/// Pull the most recent decoded RGBA frame from the appsink, if one is
/// available right now. Returns `None` when no sample is ready or the
/// pipeline is not in a playing/paused state.
pub fn pull_latest_rgba(&self) -> Option<(Vec<u8>, u32, u32)> {
    let appsink = self.appsink.as_ref()?;
    let sample = appsink.try_pull_sample(gst::ClockTime::ZERO)?;
    let buffer = sample.buffer()?;
    let caps = sample.caps()?;
    let s = caps.structure(0)?;
    let w: i32 = s.get("width").ok()?;
    let h: i32 = s.get("height").ok()?;
    let map = buffer.map_readable().ok()?;
    Some((map.as_slice().to_vec(), w as u32, h as u32))
}
```

(`use gstreamer::prelude::*` is already in the file. Add `use gstreamer_app::AppSink;` if not already.)

- [ ] **Step 2: Test**

Add to the existing `#[cfg(test)] mod tests` block in `player.rs`:

```rust
#[test]
#[ignore] // requires gst plugins + bundled clip
fn pulls_an_rgba_frame_after_load() {
    init_gst();
    let mut p = Player::empty(0);
    let slot = Slot {
        location: test_clip(),
        name: "test_smpte.mp4".into(),
        start: -1.0,
        end: -1.0,
        length: 0.0,
        rate: 1.0,
    };
    p.try_load(slot).unwrap();
    for _ in 0..200 {
        p.tick();
        if p.status == PlayerStatus::Loaded {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    p.play();
    for _ in 0..200 {
        p.tick();
        if let Some((rgba, w, h)) = p.pull_latest_rgba() {
            assert_eq!(rgba.len(), (w * h * 4) as usize);
            assert_eq!(w, 720);
            assert_eq!(h, 480);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("never pulled an RGBA sample");
}
```

- [ ] **Step 3: Run both tests**

```bash
cargo test --lib video::player
cargo test --lib video::player -- --ignored
```

Both should pass.

- [ ] **Step 4: Commit**

```bash
git add src/video/player.rs
git commit -m "add Player::pull_latest_rgba for render integration"
```

---

## Task R3 — Pi backend + cross-build

**Files:**
- Create: `src/render/pi.rs`
- Modify: `src/render/mod.rs`
- Create: `Cross.toml` (copy from mandleROT)

This task requires Pi hardware to verify end-to-end. Local-only goal: `cross build --target aarch64-unknown-linux-gnu --features pi --release` succeeds.

- [ ] **Step 1: Copy pi backend from mandleROT**

Open `/Users/cody/Dev/mandleROT/src/render/pi.rs` (402 lines). It does KMS/DRM/GBM/EGL on the Pi. Copy verbatim into `src/render/pi.rs`. Trim any `use crate::scene::*` or audio-related imports. The contract is the same as `WinitGlTarget`:

```rust
pub struct PiTarget { /* ... */ }
impl PiTarget {
    pub fn new(w: u32, h: u32) -> anyhow::Result<Self>;
    pub fn pump(&mut self) -> Vec<()>;          // pi reads input via evdev (Phase 5), so empty here
    pub fn should_close(&self) -> bool;
    pub fn begin_frame(&mut self);
    pub fn draw_video_layer(&mut self, rgba: &[u8], w: u32, h: u32, alpha: f32);
    pub fn end_frame(&mut self);
}
```

Note the `pump()` return type. The Pi backend has no winit; key input on the Pi comes from `evdev` (Phase 5). For now `pump` returns an empty Vec. The desktop `pump` returns `Vec<winit::event::KeyEvent>`. Bridge via a feature-gated type alias in `src/render/mod.rs`:

```rust
#[cfg(feature = "desktop")]
pub type KeyEvent = winit::event::KeyEvent;
#[cfg(all(not(feature = "desktop"), feature = "pi"))]
pub type KeyEvent = ();
```

- [ ] **Step 2: Update `src/render/mod.rs`**

```rust
#[cfg(feature = "desktop")]
mod desktop;
#[cfg(feature = "pi")]
mod pi;
mod shader;

#[cfg(feature = "desktop")]
pub use desktop::WinitGlTarget as Render;
#[cfg(all(feature = "pi", not(feature = "desktop")))]
pub use pi::PiTarget as Render;
```

- [ ] **Step 3: Install `cross`**

```bash
cargo install cross --git https://github.com/cross-rs/cross
```

- [ ] **Step 4: Copy `Cross.toml`**

```bash
cp /Users/cody/Dev/mandleROT/Cross.toml ./Cross.toml
```

Edit for `recur` if any project name shows up.

- [ ] **Step 5: Cross-build**

```bash
cross build --target aarch64-unknown-linux-gnu --no-default-features --features pi --release 2>&1 | tail -20
```

Expected: clean build producing `target/aarch64-unknown-linux-gnu/release/recur`.

If it fails on missing system libs in the cross image, the most common fix is adding lines to `Cross.toml`:

```toml
[target.aarch64-unknown-linux-gnu]
pre-build = [
    "apt-get update && apt-get install -y libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libgles2-mesa-dev libegl1-mesa-dev libgbm-dev libdrm-dev pkg-config",
]
```

- [ ] **Step 6: Manual Pi run** (skip if no Pi nearby)

```bash
scp target/aarch64-unknown-linux-gnu/release/recur pi@<host>:/tmp/
ssh pi@<host> /tmp/recur --smoke-frames 60
```

Expected: process exits with `smoke complete: rendered 60 frames`. If SMPTE test clip is bundled into the binary, visible playback to HDMI.

- [ ] **Step 7: Update `.docs/ROADMAP.md`**

Remove the two Bugs/Blockers entries (Render stub + Pi cross-build unverified). Add a Recently Shipped entry:

```markdown
- **Render backend** (2026-05-DD): real desktop GL render via winit+glutin+glow; Pi cross-build verified via `cross build --features pi`. Video frames now reach the screen.
```

- [ ] **Step 8: Commit**

```bash
git add src/render/ Cross.toml .docs/ROADMAP.md
git commit -m "real pi render backend; cross-build verified"
```
