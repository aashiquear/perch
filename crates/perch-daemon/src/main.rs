//! `perchd` — Perch host daemon.
//!
//! ```text
//! perchd                      # real glasses, headless unless built with --features render
//! perchd --mock synthetic     # no hardware: synthetic IMU
//! perchd --mock rec.jsonl     # replay a recording from `perch record`
//! perchd --config-dir ./dev   # keep state out of ~/.config while hacking
//! ```

mod capture;
mod config;
mod device;
mod engine;
#[cfg(feature = "render")]
mod interact;
#[cfg(feature = "render")]
mod panel;
#[cfg(feature = "render")]
mod render;
mod server;

use clap::Parser;
use config::Paths;
use perch_space::DisplayOptics;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
#[command(name = "perchd", version, about = "Perch host daemon")]
struct Args {
    /// Config/state directory (default: XDG config dir).
    #[arg(long)]
    config_dir: Option<PathBuf>,
    /// Use a mock device: `synthetic` or a path to a JSON-lines recording.
    #[arg(long)]
    mock: Option<String>,
    /// Don't open a render window even if built with `render`.
    #[arg(long)]
    headless: bool,
    /// Log filter (RUST_LOG syntax).
    #[arg(
        long,
        default_value = "info,zbus=error,wgpu_hal=warn,wgpu_core=warn,sctk_adwaita=error,naga=warn"
    )]
    log: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_new(&args.log).unwrap_or_else(|_| "info".into()))
        .init();

    let paths = args
        .config_dir
        .clone()
        .map(Paths::at)
        .unwrap_or_else(Paths::default_paths);
    let cfg = paths.load_config()?;
    tracing::info!(name = %cfg.name, peer = %cfg.peer_id, dir = %paths.dir.display(), "perchd starting");

    let optics = DisplayOptics {
        fov_h: cfg.display.fov_h_deg.to_radians(),
        tilt: cfg.display.tilt_deg.to_radians(),
        ..if cfg.display.mode.contains("1200") {
            DisplayOptics::rokid_max_1200p()
        } else {
            DisplayOptics::rokid_max_1080p()
        }
    };

    let source = match args.mock.as_deref() {
        None => device::DeviceSource::Hardware,
        Some("synthetic") => device::DeviceSource::Mock(Box::new(|| {
            Box::new(perch_hal::mock::MockGlasses::synthetic(2.0, 3600.0, 0.0).realtime(true))
        })),
        Some(path) => {
            let text = std::fs::read_to_string(path)?;
            device::DeviceSource::Mock(Box::new(move || {
                Box::new(perch_hal::mock::MockGlasses::from_jsonl(&text, true).expect("valid recording"))
            }))
        }
    };

    let engine = Arc::new(Mutex::new(engine::Engine::new(
        server::self_hello(&cfg),
        optics,
        Some(paths.clone()),
    )));
    let dev = device::spawn(cfg.clone(), Some(paths.clone()), source);
    let backend = capture::init(cfg.peer_id);
    // Commands and raw input from the on-glasses UI → engine loop.
    let (ui_tx, ui_rx) = tokio::sync::mpsc::unbounded_channel::<capture::UiMsg>();
    // Physical mouse/keyboard, grabbed from the OS while `grab` is set, feed
    // the spatial pointer on the glasses. Scroll Lock or Ctrl+Alt+P toggles it.
    let grab = Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(all(target_os = "linux", feature = "linux-input", feature = "render"))]
    let device_rx = {
        let (tx, rx) = std::sync::mpsc::channel::<perch_proto::InputEvent>();
        let grab = Arc::clone(&grab);
        let panic_key = cfg.input.panic_key_usage;
        let chord_key = cfg.input.grab_chord_usage;
        if cfg.input.share_local_devices {
            std::thread::Builder::new()
                .name("perch-evdev".into())
                .spawn(move || {
                    use perch_input::InputSource;
                    use std::sync::atomic::Ordering;
                    let mut src = match perch_input::linux::EvdevSource::open_all() {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("physical input capture unavailable: {e}");
                            return;
                        }
                    };
                    let mut grabbed = false;
                    loop {
                        let want = grab.load(Ordering::Relaxed);
                        if want != grabbed {
                            match src.set_grabbed(want) {
                                Ok(()) => {
                                    grabbed = want;
                                    tracing::info!(grabbed, "input grab");
                                }
                                Err(e) => {
                                    tracing::warn!("input grab: {e}");
                                    grab.store(grabbed, Ordering::Relaxed);
                                }
                            }
                        }
                        let ev = match src.next() {
                            Ok(ev) => ev,
                            Err(e) => {
                                tracing::warn!("evdev: {e}");
                                break;
                            }
                        };
                        if let perch_proto::InputEvent::Key {
                            usage,
                            down: true,
                            mods,
                        } = ev
                        {
                            // Ctrl+Alt+<chord> (either side) or the panic key.
                            let ctrl_alt = mods & 1 != 0 && mods & 4 != 0;
                            if usage == panic_key || (chord_key != 0 && usage == chord_key && ctrl_alt) {
                                grab.store(!grabbed, Ordering::Relaxed);
                                continue;
                            }
                        }
                        if grabbed && tx.send(ev).is_err() {
                            break;
                        }
                    }
                })
                .expect("spawn evdev thread");
        }
        rx
    };

    let local_sink: server::LocalSink = {
        #[cfg(all(target_os = "linux", feature = "linux-input"))]
        {
            match perch_input::linux::UinputSink::new((1920.0, 1080.0)) {
                Ok(s) => Some(Box::new(s)),
                Err(e) => {
                    tracing::warn!("local input injection unavailable: {e}");
                    None
                }
            }
        }
        #[cfg(not(all(target_os = "linux", feature = "linux-input")))]
        {
            None
        }
    };

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

    #[cfg(feature = "render")]
    if !args.headless {
        // Networking on the runtime thread; winit on the main thread.
        let eng2 = Arc::clone(&engine);
        let tracker = Arc::clone(&dev.tracker);
        let cfg2 = cfg.clone();
        let backend2 = Arc::clone(&backend);
        let backend3 = Arc::clone(&backend);
        std::thread::spawn(move || {
            if let Err(e) = rt.block_on(server::run(cfg2, eng2, dev, local_sink, Arc::clone(&backend2), ui_rx)) {
                tracing::error!("server: {e}");
            }
            capture::close(&backend2);
            std::process::exit(0);
        });
        let host = cfg.name.clone();
        let res = render::RenderApp::new(
            engine,
            tracker,
            cfg.display.monitor_name.clone(),
            cfg.display.ipd_m,
            render::CaptureSetup {
                backend,
                me: cfg.peer_id,
                fps: cfg.capture.fps,
                commands: ui_tx,
                grab,
                #[cfg(all(target_os = "linux", feature = "linux-input"))]
                device_rx: Some(device_rx),
                #[cfg(not(all(target_os = "linux", feature = "linux-input")))]
                device_rx: None,
            },
        )
        .with_host(host)
        .run();
        capture::close(&backend3);
        return res;
    }

    drop(ui_tx);
    drop(grab);
    let res = rt.block_on(server::run(cfg, engine, dev, local_sink, Arc::clone(&backend), ui_rx));
    capture::close(&backend);
    res
}
