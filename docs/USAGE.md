# r_e_c_u_r — Operator Manual

## Overview

r_e_c_u_r is a video sampler. It plays clips from a bank of slots, applies GLSL shaders over the output, captures live camera input, records it, and lets you scrub a ring buffer of captured frames. All control flows through a 17×48 character grid displayed on an SPI LCD (on Pi) or a window (on desktop).

---

## Display modes

Switch modes with the keys below. The current mode governs what you see and what the navigation keys do.

| Key | Mode | What you see |
|---|---|---|
| `S` | **Sampler** | Bank letter + 10 slot rows: name, loop in/out, status |
| `B` | **Browser** | File tree rooted at configured paths |
| `G` | **Settings** | Cycle through sampler settings |
| `H` | **Shaders** | Shader file browser |
| `K` | **ShdrBnk** | Shader bank (10 shader slots) |
| `D` | **Frames** | Detour frame ring: ring stats, scrub position, markers |

---

## Playing clips

### Map a clip

1. Press `B` → Browser mode.
2. Navigate with `↑` / `↓`. Enter directories with `Enter`, back out with `Esc`.
3. Press `Shift` (ToggleFunction) to toggle map mode on — status line shows `[FN]`.
4. Press `0`–`9` to map the highlighted file into slot N of the current bank.
5. Press `Shift` again to toggle map mode off. Map mode also clears automatically after each slot assignment.

Files unsupported by the current build show `[X]`. Mapping them is refused.

### Trigger a slot

Press `0`–`9` (without Shift) to load and play slot N. The sampler mode (sequential, parallel, etc.) governs what happens to the previous player.

### Loop points

| Key | Action |
|---|---|
| `[` | Set loop in at current playback position |
| `]` | Set loop out at current playback position |
| `\` | Clear loop (resets to full file) |

### Playback controls

| Key | Action |
|---|---|
| `Space` | Toggle play / pause |
| `Tab` | Toggle Now / Next player focus |

---

## Banks

Up to 26 banks (A–Z), each with 10 slots. All banks persist across restarts.

| Key | Action |
|---|---|
| `,` | Previous bank |
| `.` | Next bank |

---

## Sampler settings (`G` mode)

Navigate with `↑` / `↓` to focus a setting. Press `Enter` to cycle its value.

| Setting | Values | Effect |
|---|---|---|
| **loop_type** | `sequential` / `parallel` | Sequential: new slot stops old. Parallel: both play simultaneously. |
| **on_finish** | `switch` / `repeat` | What happens when a clip reaches its loop out point. |
| **on_start** | `play` / `show` / `play_show` | Whether triggering a slot starts playback, shows the frame, or both. |
| **on_load** | `show` / `hide` | Whether loading into Next shows the frame immediately. |
| **load_next** | `auto` / `manual` | Auto: preload next sequential slot. Manual: you choose. |
| **rand_start_mode** | on / off | Randomize start position within clip on each trigger. |
| **fixed_length_mode** | on / off | Clamp playback to `fixed_length` seconds regardless of loop points. |
| **fixed_length_multiply** | float | Scale the fixed length (e.g. 0.5 = half, 2.0 = double). |
| **reset_players** | on / off | Reset all 3 players to empty on each trigger. |

---

## Frame ring — Detour

Detour captures frames from the live player into a byte-budgeted ring buffer, then lets you scrub back through them and alpha-blend them over the live signal.

**Ring budget defaults** (can be overridden in `config.toml`):

| Build | Default budget | Approximate capacity @ 720p RGBA |
|---|---|---|
| pi3 | 128 MB | ~34 frames |
| pi5 | 256 MB | ~68 frames |
| desktop | 512 MB | ~137 frames |

### Entering / exiting Detour

| Key | Action |
|---|---|
| `D` | Enter Detour (switch to Frames display, enter DetourScrub control mode) |
| `E` | Exit Detour (restore previous display mode) |

### Scrub controls (while in DetourScrub mode)

| Key | Action |
|---|---|
| `←` | Scrub back 1 frame |
| `→` | Scrub forward 1 frame |
| `W` | Cycle playback speed: 1× → 2× → 4× → 0.25× → 0.5× → 1× |
| `J` | Toggle forward / reverse direction |
| `R` | Toggle auto-play (continuously advance at current speed/direction) |
| `N` | Set start marker at current position |
| `P` | Set end marker at current position |
| `X` | Clear start + end markers |
| `M` | Cycle mix: 0% → 25% → 50% → 75% → 100% → 0% |

Mix controls how much the ring frame blends over the live signal (0% = live only, 100% = ring only).

When markers are set, auto-play loops between them. Markers are ring-index–based, not timestamps — they drift if the ring wraps and old frames age out.

---

## Shaders — conjur

### Browser (`H` mode)

Navigate with `↑` / `↓`. Press `Enter` to preview a shader. Shaders incompatible with the current GLES profile (e.g. `min_gles = "3.10"` shaders on a `pi3` build) are hidden; a count of hidden shaders appears in the status line.

### Shader bank (`K` mode)

10 shader slots per bank, same bank/slot model as video. Navigate slots with `↑` / `↓`.

To map a shader from the browser into a slot:
1. In `H` (Shaders) browser, highlight the shader.
2. Hold `Shift`, press `0`–`9` to map into that slot.

To activate a shader slot:
- Press `F1`–`F10` to activate shader slots 0–9.
- An empty slot or a missing/broken shader falls back to the baked `__safe__` passthrough.

### Shader params

With a shader active, the param screen enters `ControlMode::ShaderParam`. In this mode: `NavLeft` / `NavRight` nudge the focused parameter down / up; `NavUp` / `NavDown` move focus between the 8 slots. The default `keymap.toml` does not bind `ShaderParamAdjust` — add bindings for `NavLeft` / `NavRight` to whichever keys you want to use for param control. Each param's range and step are declared in the shader's `.toml` sidecar.

### Shaders on disk

Shaders live in `shaders/`. Each shader is a `.glsl` + `.toml` pair. The TOML declares metadata:

```toml
name        = "color_shift"
description = "Hue rotation over time"
min_gles    = "1.00"   # or "3.10" for pi5/desktop-only features

[[params]]
name    = "speed"
default = 0.5
min     = 0.0
max     = 2.0
```

Hot-reload: editing a `.glsl` or `.toml` file on disk recompiles and applies it immediately (uses `notify` file watcher).

---

## Live capture — captur

### Add a capture slot

Press `C` → `AddCaptureSlot`. This enumerates available capture devices (v4l2 on Linux, AVFoundation on macOS) and maps the first available device into the next empty slot of the current bank. The slot appears in the Sampler as `[cap]`.

Trigger the slot normally (press its number key) to start live playback.

### Record from a capture slot

With a capture slot active:

| Key | Action |
|---|---|
| `R` | Start recording — status line shows `<REC> MM:SS` |
| `R` again | Stop recording and finalize — status line shows `<SAV>` while finalizing |

The recording is written to the state directory as `rec-YYYY-MM-DD-N.mp4`. When finalization completes, the file is auto-imported into the first empty video slot of the current bank.

**Encoder per build:**

| Build | Encoder | Container |
|---|---|---|
| pi3 | `v4l2h264enc` | MP4 / H.264 |
| pi5 | `v4l2h265enc` | MP4 / H.265 |
| macOS | `vtenc_h264` | MP4 / H.264 |
| Linux desktop | `x264enc` | MP4 / H.264 |

**Disk gate:** recording is blocked (and an error shown) if the state directory filesystem has less than 10 MB free. Disk space is polled every 10 seconds during recording; recording stops if space drops below the gate.

**Mode behaviour:** in DetourScrub mode, `R` triggers `DetourTogglePlay` instead of `RecordToggle` (same physical key, different mode). Exit Detour first if you want to record.

---

## Keyboard reference

The full binding table is in `keymap.toml`. Defaults:

```
0–9          SelectSlot(N)          trigger slot N (or map if [FN])
[            SetLoopIn
]            SetLoopOut
\            ClearLoop
Space        TogglePlayPause
Tab          ToggleNowNext          switch Now/Next player focus
,            PrevBank
.            NextBank
↑ ↓ ← →     NavUp/Down + DetourScrubBy (context-dependent)
Enter        Enter
Esc/Bksp     Back
Shift        ToggleFunction         hold for map/param mode

B            Browser
S            Sampler
G            Settings
H            Shaders
K            ShdrBnk
D            DetourEnter
E            DetourExit
W            DetourCycleSpeed
J            DetourToggleDirection
N            DetourSetStartMarker
P            DetourSetEndMarker
X            DetourClearMarkers
M            DetourCycleMix
C            AddCaptureSlot
R            RecordToggle in Default mode; DetourTogglePlay (auto-play) in DetourScrub mode
F1–F10       TriggerShaderSlot(0–9)
```

Double-tap `Esc` or `Backspace` within 400 ms → `Panic`: resets all 3 players to empty.

---

## State files and persistence

State directory location (in order of precedence):
1. `$RECUR_STATE_DIR`
2. `<binary_dir>/.config/recur/`
3. `./.config/recur/`

All writes are atomic (write to `.toml.tmp`, then rename).

| File | Saved when | Contents |
|---|---|---|
| `banks.toml` | On slot map / unmap | Bank × slot matrix: source path, name, loop in/out, length, rate |
| `settings.toml` | On setting change | Sampler settings (loop type, on-finish, etc.) |
| `paths.toml` | On path add | Browser root directories |
| `shader_banks.toml` | On shader slot change | Shader name + 8 param values per slot |

`banks.toml` supports both the current tagged `source = { kind = "file", value = "..." }` format and the legacy `location = "..."` format for backward compatibility with old state files.

---

## Config file reference

`config.toml` is the static render config. It is **not** the user state directory.

```toml
[render]
width  = 720    # output resolution width
height = 480    # output resolution height
fps    = 30     # target frame rate

[detour]
ring_budget_mb = 256   # optional: override per-target ring budget
```

Per-target ring budget defaults if `ring_budget_mb` is not set:
- `pi3`: 128 MB
- `pi5`: 256 MB
- `desktop`: 512 MB

Hard ceiling: 50% of free RAM at startup (measured via `sysinfo`).
