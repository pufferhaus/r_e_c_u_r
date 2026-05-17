# r_e_c_u_r — Completed Work

Rolled-off entries from `Recently Shipped` in [ROADMAP.md](ROADMAP.md). Newest first.

## 2026-05-16

- **Phase 2 sub-plan A — shader infrastructure**: `src/shader/` module, GLES-split preludes, shader_assembly, ShaderPipeline (FBO + compile cache), wired into both render backends, passthrough demo. See `docs/superpowers/specs/2026-05-16-conjur-design.md`.
- **Dual-target spec**: pi3 + pi5 cargo features, per-shader GLES gating rules, byte-budgeted detour ring rules, desktop dev defaults to pi5 parity. Foundation only — phase-2/3 implementation pending. See `docs/superpowers/specs/2026-05-16-pi5-target-revision-design.md`.

## 2026-05-12

- **Render backend**: real desktop GL render via winit + glutin + glow; video frames now display in a window. Pi cross-build via `cross build --no-default-features --features pi` verified compiling to `aarch64-unknown-linux-gnu` (real-Pi runtime testing pending hardware access).
- **Phase 1 — r_e_c_u_r-core**: file playback, sample bank, loop points, sampler modes, Browser/Sampler/Settings menus, desktop keyboard control. 80+ unit tests; headless smoke runs `--smoke-frames N`. GL render backend + Pi cross-build deferred to a follow-up; ScreenStack + apply pipeline + GStreamer player rack all in place.
