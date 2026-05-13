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
    info!(
        "loaded {} banks, paths_to_browser = {:?}",
        banks.len(),
        paths
    );

    let mut state = SharedState::new();
    state.banks = banks;
    state.sampler = settings;
    state.paths_to_browser = paths;

    let sampler_settings = state.sampler.clone();
    let mut rack = PlayerRack::new(sampler_settings);
    let mut grid = TextGrid::new(48, 17);
    let mut stack = ScreenStack::new();
    stack.push(Box::new(RootScreen::new()));

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

    let mut render = recur::render::Render::new(cfg.render.width, cfg.render.height, "r_e_c_u_r")?;

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
                rack.trigger_slot_with(0, 0, slot);
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
            let _consumed = stack.dispatch(action.clone(), &mut state);
            apply(action, &mut state, &mut rack);
        }

        // 2. Rack tick
        rack.tick();

        // Drain any gst error into shared state for the UI to display.
        if let Some(err) = rack.drain_last_error() {
            state.last_error = Some(err);
        }

        // 3. Re-render text grid
        grid.clear();
        if let Some(top) = stack.top() {
            top.render(&state, &mut grid);
        }

        // 4. Pull latest frame from current player and draw.
        render.begin_frame();
        if let Some((rgba, w, h)) = rack.current.pull_latest_rgba() {
            render.draw_video_layer(&rgba, w, h, 1.0);
        }
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
    Ok(())
}
