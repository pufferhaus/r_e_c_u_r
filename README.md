# r_e_c_u_rˢ

Rust port of [cyberboy666/r_e_c_u_r](https://github.com/cyberboy666/r_e_c_u_r) — a video sampler for Raspberry Pi. Targets Pi 3 B+ and Pi 5 as separate compile-time builds; macOS and Linux x86_64 are supported for development.

_Forked from [cyberboy666/r_e_c_u_r](https://github.com/cyberboy666/r_e_c_u_r); now an independent Rust reimplementation._

**257 unit tests pass.** Hardware verification of Phase 4 (captur) requires a Pi + USB camera; all other phases verified on desktop.

## Features

| Phase | What it does |
|---|---|
| **core** | File playback, sample banks (10 slots × 26 banks), loop in/out/rate, sampler modes, Browser/Sampler/Settings menus, desktop keyboard control |
| **conjur** | GLSL shader layer over video; shader banks (10 slots); hot-reload; per-target GLSL ES profile filtering; 8 per-shader params |
| **detour** | In-memory frame ring + scrub mode; byte-budgeted (128/256/512 MB by target); speed/direction/mix/markers; alpha-blend with live |
| **captur** | USB v4l2 / CSI live capture mapped into bank slots; concurrent `splitmuxsink` recording; auto-import to bank on save |

## Install matrix

| Hardware | Binary | Cargo invocation |
|---|---|---|
| Raspberry Pi 3 B+ | `r_e_c_u_rs-aarch64-pi3` | `cross build --release --no-default-features --features pi3 --target aarch64-unknown-linux-gnu` |
| Raspberry Pi 4 | `r_e_c_u_rs-aarch64-pi3` (works; not first-class) | same as Pi 3 B+ |
| Raspberry Pi 5 | `r_e_c_u_rs-aarch64-pi5` | `cross build --release --no-default-features --features pi5 --target aarch64-unknown-linux-gnu` |
| macOS / Linux x86_64 | `r_e_c_u_rs` | `cargo build --release` |

`pi3` and `pi5` are mutually exclusive at compile time; `build.rs` fails fast if neither or both are set.
The legacy `pi` feature is a deprecated alias for `pi3` (removed in 0.2.0).

## Codec support

| Codec | pi3 | pi5 | desktop |
|---|---|---|---|
| H.264 | ✓ hardware | ✓ hardware | ✓ |
| H.265 / HEVC | ✗ | ✓ hardware | ✓ |
| VP9 | ✗ | ✓ | ✓ |
| AV1 | ✗ | ✓ software | ✓ |

Unsupported sources appear in the Browser with an `[X]` marker; mapping them to a slot is refused with an error in the status line. Re-encode with `ffmpeg -c:v libx264 -crf 20 -preset slow in.mkv out.mp4`.

## Running

```sh
# desktop (default — pi5 GLSL ES 3.10 parity)
cargo run

# emulate pi3 shader compatibility
cargo run -- --gles-profile pi3

# headless smoke test (exits after 60 frames)
cargo run -- --smoke-frames 60

# custom config / keymap paths
cargo run -- --config /path/to/config.toml --keymap /path/to/keymap.toml

# unit tests
cargo test --lib

# cross-build for Pi
cross build --release --no-default-features --features pi3 --target aarch64-unknown-linux-gnu
cross build --release --no-default-features --features pi5 --target aarch64-unknown-linux-gnu
```

## Configuration

`config.toml` (passed via `--config`; default: `config.toml` next to the binary):

```toml
[render]
width  = 720
height = 480
fps    = 30

# [detour]
# ring_budget_mb = 256   # override per-target default (pi3: 128, pi5: 256, desktop: 512)
```

`keymap.toml` (passed via `--keymap`; default: `keymap.toml` next to the binary) maps `winit` key codes to `Action` names. See `keymap.toml` for the full default binding table.

## State files

User state is written atomically to a TOML state directory. Resolution order:

1. `$RECUR_STATE_DIR` (set by the systemd unit on Pi)
2. `<binary_dir>/.config/recur/`
3. `./.config/recur/` (relative to CWD)

| File | Contents |
|---|---|
| `banks.toml` | All banks and their slot mappings (file paths, loop points, rate) |
| `settings.toml` | Sampler settings (loop type, on-finish, on-start, etc.) |
| `paths.toml` | Browser root directories |
| `shader_banks.toml` | Shader bank slot assignments and per-slot param values |

## Usage

See [docs/USAGE.md](docs/USAGE.md) for the full operator guide (display modes, workflows, keyboard reference).

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the developer reference (module map, pipeline design, dispatch flow).

## License

MIT OR Apache-2.0.
