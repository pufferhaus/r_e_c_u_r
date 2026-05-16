use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use gstreamer as gst;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use recur::action::Action;
use recur::apply::apply;
use recur::config::{self, Config};
use recur::input::keymap::Keymap;
use recur::menu::root::RootScreen;
use recur::persist;
use recur::state::SharedState;
use recur::status::grid::TextGrid;
use recur::ui::ScreenStack;
use recur::video::rack::PlayerRack;

#[cfg(feature = "desktop")]
use recur::input::winit_src::WinitSource;

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
    #[allow(dead_code)]
    fn to_min_gles(self) -> recur::shader::GlesVersion {
        use recur::shader::GlesVersion;
        match self {
            GlesProfileArg::V100 => GlesVersion::V100,
            GlesProfileArg::V310 => GlesVersion::V310,
        }
    }
}

#[derive(Parser, Debug)]
struct Args {
    /// Render N frames then exit (for smoke tests).
    #[arg(long)]
    smoke_frames: Option<u64>,

    /// Path to config.toml.
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,

    /// Path to keymap.toml.
    #[arg(long, default_value = "keymap.toml")]
    keymap: PathBuf,

    /// GLES profile to load shaders against. `pi3`/`v100` filters out 3.10-only
    /// shaders; default `pi5`/`v310` loads all.
    #[arg(long, value_enum, default_value_t = GlesProfileArg::V310)]
    gles_profile: GlesProfileArg,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    gst::init()?;
    let args = Args::parse();

    let cfg = Config::load(&args.config)?;
    let state_dir = config::user_state_dir();
    let banks = persist::load_banks(&state_dir)?;
    let settings = persist::load_settings(&state_dir)?;
    let paths = persist::load_paths(&state_dir)?;
    let shader_banks = persist::load_shader_banks(&state_dir)?;
    info!(
        "loaded {} banks, paths_to_browser = {:?}",
        banks.len(),
        paths
    );
    info!("loaded {} shader banks", shader_banks.len());

    let mut state = SharedState::new();
    state.banks = banks;
    state.sampler = settings;
    state.paths_to_browser = paths;
    state.shader_banks = shader_banks;

    state.gles_profile = args.gles_profile.to_profile();

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
    // Desktop OpenGL 3.0 contexts (e.g. macOS) reject `#version 310 es`; clamp
    // to V100 until a GLES 3.1 desktop context is in place.
    #[cfg(all(feature = "desktop", not(any(feature = "pi3", feature = "pi5"))))]
    {
        if args.gles_profile == GlesProfileArg::V310 {
            tracing::warn!(
                "--gles-profile v310 currently unsupported on desktop builds (context is desktop GL 3.0); forcing V100"
            );
            state.gles_profile = recur::render::shader_assembly::GlesProfile::V100;
        }
    }

    let shader_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders");
    let shader_watcher = recur::shader::ShaderWatcher::start(&shader_dir)
        .map_err(|e| {
            tracing::warn!("shader hot-reload disabled: {e}");
            e
        })
        .ok();

    let sampler_settings = state.sampler.clone();
    let mut rack = PlayerRack::new(sampler_settings);
    let (shader_tx, shader_rx) = crossbeam_channel::unbounded::<recur::video::rack::ShaderCommand>();
    rack.set_shader_channel(shader_tx);
    let mut grid = TextGrid::new(48, 17);

    let mut root = RootScreen::new();
    // Populate SHADERS browser names from the initial library so the browser
    // shows entries without needing to call into Render.
    {
        let lib = recur::shader::ShaderLibrary::load_dir_for_profile(
            &shader_dir,
            match state.gles_profile {
                recur::render::shader_assembly::GlesProfile::V100 => recur::shader::GlesVersion::V100,
                recur::render::shader_assembly::GlesProfile::V310 => recur::shader::GlesVersion::V310,
            },
        ).unwrap_or_else(|e| {
            tracing::warn!("initial shader library load failed: {e}");
            recur::shader::ShaderLibrary::default()
        });
        let names: Vec<String> = lib.names().map(|s| s.to_string()).collect();
        let filtered = lib.filtered_count();
        root.set_shader_names(names, filtered);
    }
    // NOTE: once root is pushed into the stack it is owned by Box<dyn Screen>
    // and cannot be accessed directly. Hot-reload events update the shader
    // compile cache (invalidate/upsert below), but the SHADERS browser name
    // list is NOT refreshed at runtime — users can restart to pick up new
    // shader files. Updating names via hot-reload is out of scope for Task 16.
    let mut stack = ScreenStack::new();
    stack.push(Box::new(root));

    let keymap = Keymap::load(&args.keymap).unwrap_or_else(|e| {
        error!("keymap.toml: {e}; using empty bindings");
        Keymap::default()
    });

    #[cfg(feature = "desktop")]
    let mut input = WinitSource::new(keymap);

    // On non-desktop builds (pi feature), no input source is wired yet.
    // The smoke path produces zero actions each frame regardless.
    #[cfg(not(feature = "desktop"))]
    let _ = keymap; // suppress unused warning

    let mut render = recur::render::Render::new(cfg.render.width, cfg.render.height, "r_e_c_u_r", state.gles_profile)?;

    #[cfg(debug_assertions)]
    {
        // Smoke-test convenience: pre-load assets/test_smpte.mp4 into slot 0.
        if std::env::var("RECUR_SMOKE_AUTO_LOAD").is_ok() {
            let p = std::env::current_dir()?.join("assets/test_smpte.mp4");
            if p.exists() {
                let slot = recur::state::Slot {
                    location: p,
                    name: "test_smpte.mp4".into(),
                    start: -1.0,
                    end: -1.0,
                    length: 0.0,
                    rate: 1.0,
                };
                state.banks[0].slots[0] = Some(slot.clone());
                // Trigger play immediately so the smoke run shows video.
                use recur::apply::RackHandle;
                let bank_snapshot = state.banks[0].clone();
                rack.trigger_slot_with(0, 0, slot, bank_snapshot);
            }
        }
    }

    let target_fps = cfg.render.fps as u64;
    let frame_dt = Duration::from_micros(1_000_000 / target_fps.max(1));
    let max_frames = args.smoke_frames;
    let mut frame_count: u64 = 0;
    let mut t_next = Instant::now();

    loop {
        // 1. Drain input → Actions
        #[cfg(feature = "desktop")]
        for ev in render.pump() {
            input.push_key_event(&ev);
        }

        #[cfg(feature = "desktop")]
        if render.should_close() {
            info!("window closed, exiting");
            break;
        }

        #[cfg(feature = "desktop")]
        let actions: Vec<Action> = input.poll();

        #[cfg(not(feature = "desktop"))]
        let actions: Vec<Action> = Vec::new();

        for action in actions {
            let synth = stack.dispatch(action.clone(), &mut state);
            apply(action, &mut state, &mut rack);
            if let Some(synth_action) = synth {
                apply(synth_action, &mut state, &mut rack);
            }
        }

        // 2. Rack tick
        rack.tick();

        // Drain any gst error into shared state for the UI to display.
        if let Some(err) = rack.drain_last_error() {
            state.last_error = Some(err);
        }

        // Drain shader commands from the rack channel.
        for cmd in shader_rx.try_iter() {
            use recur::video::rack::ShaderCommand;
            match cmd {
                ShaderCommand::Trigger(name, params) => {
                    match render.select_shader(&name, params) {
                        Ok(()) => render.pulse_shader_trigger(),
                        Err(e) => {
                            let msg = format!("shader {name}: {e}");
                            tracing::warn!("{msg}");
                            state.last_error = Some(msg);
                            render.clear_shader();
                        }
                    }
                }
                ShaderCommand::Clear => render.clear_shader(),
            }
        }

        // Drain shader hot-reload events; invalidate and upsert into the pipeline.
        // Browser name list is NOT updated at runtime (see comment above stack setup).
        if let Some(watcher) = shader_watcher.as_ref() {
            for ev in watcher.try_drain() {
                let recur::shader::ShaderEvent::Dirty(name) = ev;
                // Re-read library entry from disk; on failure, keep the cached one.
                let shader_path = shader_dir.join(format!("{name}.glsl"));
                let meta_path = shader_dir.join(format!("{name}.toml"));
                match (
                    std::fs::read_to_string(&shader_path),
                    std::fs::read_to_string(&meta_path),
                ) {
                    (Ok(body), Ok(meta_src)) => {
                        match recur::shader::ShaderMeta::parse(&meta_src, &meta_path.display().to_string()) {
                            Ok(meta) => {
                                let shader = recur::shader::LoadedShader {
                                    meta,
                                    fragment_body: body,
                                    source_path: shader_path,
                                };
                                render.invalidate_shader(&name);
                                render.upsert_shader(&name, shader);
                                tracing::info!("hot-reloaded shader: {name}");
                            }
                            Err(e) => tracing::warn!("hot-reload {name} meta parse failed: {e}"),
                        }
                    }
                    _ => tracing::debug!("hot-reload {name}: file gone or unreadable, skipping"),
                }
            }
        }

        // 3. Re-render text grid
        grid.clear();
        if let Some(top) = stack.top() {
            top.render(&state, &mut grid);
        }

        // 4. Pull latest frame from current player and draw.
        render.begin_frame();
        if let Some(frame) = rack.current.pull_latest_rgba() {
            render.draw_video_layer(frame.data(), frame.width, frame.height, 1.0);
        }
        render.draw_text_grid(&grid);
        render.end_frame();

        // 5. Pace the loop
        t_next += frame_dt;
        if let Some(rem) = t_next.checked_duration_since(Instant::now()) {
            std::thread::sleep(rem);
        } else {
            t_next = Instant::now();
        }

        frame_count += 1;
        if let Some(max) = max_frames {
            if frame_count >= max {
                info!("smoke complete: rendered {} frames", frame_count);
                break;
            }
        }
    }

    persist::save_banks(&state_dir, &state.banks)?;
    persist::save_settings(&state_dir, &state.sampler)?;
    persist::save_paths(&state_dir, &state.paths_to_browser)?;
    persist::save_shader_banks(&state_dir, &state.shader_banks)?;
    Ok(())
}

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
