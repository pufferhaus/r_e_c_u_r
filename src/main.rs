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

    /// Run headless (no window). Useful for unit-test-style smoke runs.
    #[arg(long)]
    headless: bool,
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

    let bank0 = state.banks[0].clone();
    let sampler_settings = state.sampler.clone();
    let mut rack = PlayerRack::new(bank0, sampler_settings);
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

    let target_fps = cfg.render.fps as u64;
    let frame_dt = Duration::from_micros(1_000_000 / target_fps.max(1));
    let max_frames = args.smoke_frames;
    let mut frame_count: u64 = 0;
    let mut t_next = Instant::now();

    loop {
        // 1. Drain input → Actions
        #[cfg(feature = "desktop")]
        let actions: Vec<Action> = input.poll();

        #[cfg(not(feature = "desktop"))]
        let actions: Vec<Action> = Vec::new();

        for action in actions {
            let consumed = stack.dispatch(action.clone(), &mut state);
            if !consumed {
                apply(action, &mut state, &mut rack);
            } else {
                // Some actions (e.g. EnterMode) need both screen and state mutation.
                apply(action, &mut state, &mut rack);
            }
        }

        // 2. Rack tick
        rack.tick();

        // 3. Re-render text grid
        grid.clear();
        if let Some(top) = stack.top() {
            top.render(&state, &mut grid);
        }

        // 4. Render frame (window or pi) — stubbed for Phase 1.
        // Real GL backend is a follow-up.

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
