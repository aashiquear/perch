//! `perch` — talk to a running `perchd`.
//!
//! ```text
//! perch status                       # glasses, calibration, windows
//! perch recenter
//! perch calibrate --seconds 30       # glasses on a table, still
//! perch mode mono120
//! perch preset coding
//! perch windows                      # list ids
//! perch pin <id>                     # pin where you're looking
//! perch place <id> --yaw 30 --pitch -5 --distance 2 --width 1.6 --height 0.9
//! perch anchor <id> smooth_follow
//! perch focus next | <id>
//! perch layout save work | load work
//! perch watch                        # stream events as JSON lines
//! perch record out.jsonl --seconds 10   # dump raw IMU (needs --features rokid)
//! perch discover                     # list perchd hosts on the LAN
//! ```

use clap::{Parser, Subcommand};
use perch_net::ControlClient;
use perch_proto::{
    Anchor, Capabilities, Command, DisplayMode, Event, Hello, Message, Placement, Platform, WindowId, PROTOCOL_VERSION,
};
use std::time::Duration;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "perch", version, about = "Perch controller")]
struct Args {
    /// perchd control URL.
    #[arg(long, default_value = "ws://127.0.0.1:7420")]
    url: String,
    /// Pairing token (if perchd has one).
    #[arg(long, default_value = "")]
    token: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show state.
    Status,
    /// Make the current gaze yaw = 0.
    Recenter,
    /// Run stationary gyro calibration.
    Calibrate {
        #[arg(long, default_value_t = 30)]
        seconds: u8,
    },
    /// Set display mode: mono | stereo | mono120 | stereo120.
    Mode { mode: String },
    /// Apply a preset: single | coding | gaming | couch.
    Preset { name: String },
    /// List windows.
    Windows,
    /// Pick local windows/monitors to show in the glasses (opens the desktop
    /// portal dialog on the host). New windows appear where you are looking.
    Pick {
        /// Let the dialog select several sources at once.
        #[arg(long)]
        multiple: bool,
    },
    /// Remove a window.
    Remove { id: String },
    /// Remove every window.
    Clear,
    /// Show or change the slot grid (lengths in metres, angles in degrees).
    /// With no options, prints it. `--arrange` also snaps every window in.
    Grid {
        #[arg(long)]
        cols: Option<u32>,
        #[arg(long)]
        rows: Option<u32>,
        /// Cell width.
        #[arg(long)]
        cell_w: Option<f32>,
        /// Cell height.
        #[arg(long)]
        cell_h: Option<f32>,
        #[arg(long)]
        gap: Option<f32>,
        #[arg(long)]
        distance: Option<f32>,
        /// Grid centre yaw (+ = left).
        #[arg(long)]
        yaw: Option<f32>,
        /// Grid centre pitch (+ = up).
        #[arg(long)]
        pitch: Option<f32>,
        /// Snap every visible window into the grid afterwards.
        #[arg(long)]
        arrange: bool,
    },
    /// Snap a window into grid slot COL ROW (0-based; col 0 = left, row 0 = top).
    Slot { id: String, col: u32, row: u32 },
    /// Snap every visible window into the grid, row-major.
    Arrange,
    /// Pin a window where you're looking.
    Pin { id: String },
    /// Place a window (angles in degrees, sizes in metres).
    Place {
        id: String,
        #[arg(long, default_value_t = 0.0)]
        yaw: f32,
        #[arg(long, default_value_t = 0.0)]
        pitch: f32,
        #[arg(long, default_value_t = 2.0)]
        distance: f32,
        #[arg(long, default_value_t = 1.6)]
        width: f32,
        #[arg(long, default_value_t = 0.9)]
        height: f32,
    },
    /// Set anchor: world | head | smooth_follow.
    Anchor { id: String, anchor: String },
    /// Focus a window by id, or `next`.
    Focus { id: String },
    /// Save/load layouts.
    Layout {
        #[command(subcommand)]
        op: LayoutOp,
    },
    /// Print events as they arrive (JSON lines).
    Watch,
    /// Record raw IMU samples to a JSON-lines file (requires `--features rokid`).
    Record {
        out: String,
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
    /// Discover perchd hosts via mDNS.
    Discover,
}

#[derive(Subcommand)]
enum LayoutOp {
    /// Save current windows under a name.
    Save { name: String },
    /// Load a saved layout.
    Load { name: String },
}

fn parse_mode(s: &str) -> anyhow::Result<DisplayMode> {
    Ok(match s {
        "mono" => DisplayMode::Mono,
        "stereo" => DisplayMode::Stereo,
        "mono120" => DisplayMode::Mono120,
        "stereo120" => DisplayMode::Stereo120,
        _ => anyhow::bail!("unknown mode {s}"),
    })
}

fn parse_anchor(s: &str) -> anyhow::Result<Anchor> {
    Ok(match s {
        "world" => Anchor::World,
        "head" => Anchor::Head,
        "smooth_follow" | "smooth" => Anchor::SmoothFollow,
        _ => anyhow::bail!("unknown anchor {s}"),
    })
}

/// Resolve `id` which may be a full UUID or a unique prefix of one.
fn resolve_id(id: &str, known: &[WindowId]) -> anyhow::Result<WindowId> {
    if let Ok(u) = Uuid::parse_str(id) {
        return Ok(u);
    }
    let matches: Vec<_> = known.iter().filter(|w| w.to_string().starts_with(id)).collect();
    match matches.as_slice() {
        [one] => Ok(**one),
        [] => anyhow::bail!("no window id starts with {id}"),
        _ => anyhow::bail!("ambiguous id prefix {id}"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Behave like a normal Unix tool when piped into `head`: die quietly on
    // SIGPIPE instead of panicking on a failed println.
    #[cfg(unix)]
    // SAFETY: resetting a signal disposition to its default at startup, before
    // any threads that might care exist.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    tracing_subscriber::fmt().with_env_filter("warn").init();
    let args = Args::parse();

    if let Cmd::Discover = args.cmd {
        let d = perch_net::Discovery::new()?;
        let mut rx = d.browse()?;
        println!("browsing for 3 s…");
        let deadline = tokio::time::sleep(Duration::from_secs(3));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                Some(p) = rx.recv() => println!("{}  {:?}  {}", p.name, p.addrs, p.peer_id.map(|u| u.to_string()).unwrap_or_default()),
                _ = &mut deadline => break,
            }
        }
        return Ok(());
    }

    if let Cmd::Record { out, seconds } = &args.cmd {
        return record(out, *seconds);
    }

    let name = if args.token.is_empty() {
        "perch-cli".to_string()
    } else {
        format!("perch-cli#{}", args.token)
    };
    let hello = Hello {
        version: PROTOCOL_VERSION,
        peer_id: Uuid::new_v4(),
        name,
        platform: Platform::Linux,
        caps: Capabilities::default(),
    };
    let mut c = ControlClient::connect(&args.url, hello).await?;

    // Always fetch state first so we can resolve id prefixes.
    c.send(Message::Command(Command::GetState));
    let state = loop {
        match tokio::time::timeout(Duration::from_secs(3), c.recv()).await {
            Ok(Some(Message::Event(Event::State(s)))) => break s,
            Ok(Some(_)) => continue,
            _ => anyhow::bail!("no state from perchd"),
        }
    };
    let ids: Vec<WindowId> = state.windows.iter().map(|w| w.id).collect();

    let cmd = match args.cmd {
        Cmd::Status => {
            println!("host: {}", c.server.name);
            println!("glasses: {}", state.glasses.as_deref().unwrap_or("not connected"));
            println!("mode: {:?}", state.display_mode);
            println!("calibration: {:?}", state.calibration);
            println!(
                "peers: {}",
                state
                    .peers
                    .iter()
                    .map(|p| format!("{} ({:?})", p.name, p.platform))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!("layouts: {}", state.layouts.join(", "));
            print_windows(&state);
            return Ok(());
        }
        Cmd::Windows => {
            print_windows(&state);
            return Ok(());
        }
        Cmd::Grid {
            cols,
            rows,
            cell_w,
            cell_h,
            gap,
            distance,
            yaw,
            pitch,
            arrange,
        } => {
            let mut g = state.grid;
            let mut changed = false;
            macro_rules! set {
                ($field:ident, $val:expr) => {
                    if let Some(v) = $val {
                        g.$field = v;
                        changed = true;
                    }
                };
            }
            set!(cols, cols);
            set!(rows, rows);
            set!(cell_w_m, cell_w);
            set!(cell_h_m, cell_h);
            set!(gap_m, gap);
            set!(distance, distance);
            set!(center_yaw, yaw.map(f32::to_radians));
            set!(center_pitch, pitch.map(f32::to_radians));
            if changed {
                c.send(Message::Command(Command::SetGrid { grid: g }));
            }
            if arrange {
                c.send(Message::Command(Command::ArrangeGrid));
            }
            let mut latest = state.clone();
            let mut want = usize::from(changed) + usize::from(arrange);
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            while want > 0 {
                match tokio::time::timeout_at(deadline, c.recv()).await {
                    Ok(Some(Message::Event(Event::State(s)))) => {
                        latest = s;
                        want -= 1;
                    }
                    Ok(Some(Message::Event(Event::Error { message }))) => anyhow::bail!("perchd: {message}"),
                    Ok(Some(_)) => continue,
                    _ => break,
                }
            }
            print_grid(&latest);
            if arrange {
                print_windows(&latest);
            }
            c.close().await;
            return Ok(());
        }
        Cmd::Clear => {
            let n = state.windows.len();
            for w in &state.windows {
                c.send(Message::Command(Command::RemoveWindow { id: w.id }));
            }
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            let mut removed = 0;
            while removed < n {
                match tokio::time::timeout_at(deadline, c.recv()).await {
                    Ok(Some(Message::Event(Event::WindowRemoved { .. }))) => removed += 1,
                    Ok(Some(_)) => continue,
                    _ => break,
                }
            }
            println!("removed {removed} of {n} windows");
            c.close().await;
            return Ok(());
        }
        Cmd::Watch => {
            while let Some(m) = c.recv().await {
                println!("{}", m.to_json()?);
            }
            return Ok(());
        }
        Cmd::Recenter => Command::Recenter,
        Cmd::Calibrate { seconds } => {
            println!("calibrating for {seconds}s — keep the glasses perfectly still");
            Command::Calibrate { seconds }
        }
        Cmd::Mode { mode } => Command::SetDisplayMode {
            mode: parse_mode(&mode)?,
        },
        Cmd::Preset { name } => Command::ApplyPreset { name },
        Cmd::Pick { multiple } => Command::PickSources { multiple },
        Cmd::Remove { id } => Command::RemoveWindow {
            id: resolve_id(&id, &ids)?,
        },
        Cmd::Slot { id, col, row } => Command::SlotWindow {
            id: resolve_id(&id, &ids)?,
            col,
            row,
        },
        Cmd::Arrange => Command::ArrangeGrid,
        Cmd::Pin { id } => Command::PinHere {
            id: resolve_id(&id, &ids)?,
        },
        Cmd::Place {
            id,
            yaw,
            pitch,
            distance,
            width,
            height,
        } => Command::PlaceWindow {
            id: resolve_id(&id, &ids)?,
            placement: Placement {
                yaw: yaw.to_radians(),
                pitch: pitch.to_radians(),
                distance,
                roll: 0.0,
                width_m: width,
                height_m: height,
            },
        },
        Cmd::Anchor { id, anchor } => Command::SetAnchor {
            id: resolve_id(&id, &ids)?,
            anchor: parse_anchor(&anchor)?,
        },
        Cmd::Focus { id } if id == "next" => Command::FocusNext,
        Cmd::Focus { id } => Command::Focus {
            id: resolve_id(&id, &ids)?,
        },
        Cmd::Layout {
            op: LayoutOp::Save { name },
        } => Command::SaveLayout { name },
        Cmd::Layout {
            op: LayoutOp::Load { name },
        } => Command::LoadLayout { name },
        Cmd::Record { .. } | Cmd::Discover => unreachable!(),
    };
    let is_pick = matches!(cmd, Command::PickSources { .. });
    let fire_and_forget = matches!(
        cmd,
        Command::Recenter | Command::Calibrate { .. } | Command::SetDisplayMode { .. } | Command::SetBrightness { .. }
    );
    c.send(Message::Command(cmd));
    if fire_and_forget {
        println!("ok");
    } else {
        // Wait for the event this command produces; ignore pose ticks and the
        // join-time state broadcast. A pick waits for the user in the dialog.
        let wait = if is_pick {
            Duration::from_secs(300)
        } else {
            Duration::from_secs(2)
        };
        let deadline = tokio::time::Instant::now() + wait;
        if is_pick {
            println!("choose window(s) in the dialog on the host…");
        }
        loop {
            match tokio::time::timeout_at(deadline, c.recv()).await {
                Ok(Some(Message::Event(Event::Pose(_)))) => continue,
                Ok(Some(Message::Event(Event::Error { message }))) => anyhow::bail!("perchd: {message}"),
                Ok(Some(Message::Event(Event::WindowChanged(w)))) => {
                    println!(
                        "window {} → yaw {:.1}° pitch {:.1}° {:?}",
                        &w.id.to_string()[..8],
                        w.placement.yaw.to_degrees(),
                        w.placement.pitch.to_degrees(),
                        w.anchor
                    );
                    if is_pick {
                        // Several windows may follow one pick; drain briefly.
                        let quiet = tokio::time::Instant::now() + Duration::from_millis(500);
                        while let Ok(Some(m)) = tokio::time::timeout_at(quiet, c.recv()).await {
                            if let Message::Event(Event::WindowChanged(w)) = m {
                                println!(
                                    "window {} → yaw {:.1}° pitch {:.1}° {:?}",
                                    &w.id.to_string()[..8],
                                    w.placement.yaw.to_degrees(),
                                    w.placement.pitch.to_degrees(),
                                    w.anchor
                                );
                            }
                        }
                    }
                    break;
                }
                Ok(Some(Message::Event(Event::WindowRemoved { id }))) => {
                    println!("removed {}", &id.to_string()[..8]);
                    break;
                }
                Ok(Some(Message::Event(Event::FocusChanged { id }))) => {
                    println!(
                        "focused: {}",
                        id.map(|i| i.to_string()[..8].to_string())
                            .unwrap_or_else(|| "none".into())
                    );
                    break;
                }
                Ok(Some(Message::Event(Event::State(s)))) => {
                    // The first State after connecting is the join broadcast; a
                    // second one is the reply to layout/preset commands.
                    if s.windows.len() != state.windows.len()
                        || s.layouts != state.layouts
                        || s.windows != state.windows
                    {
                        print_windows(&s);
                        if !s.layouts.is_empty() {
                            println!("layouts: {}", s.layouts.join(", "));
                        }
                        break;
                    }
                    continue;
                }
                Ok(Some(_)) => continue,
                _ => {
                    println!("ok (no confirmation within {} s)", wait.as_secs());
                    break;
                }
            }
        }
    }
    c.close().await;
    Ok(())
}

fn print_grid(state: &perch_proto::State) {
    let g = &state.grid;
    println!(
        "grid: {}×{} cells {:.2}×{:.2} m, gap {:.2} m, {:.1} m away, centre yaw {:.1}° pitch {:.1}°",
        g.cols,
        g.rows,
        g.cell_w_m,
        g.cell_h_m,
        g.gap_m,
        g.distance,
        g.center_yaw.to_degrees(),
        g.center_pitch.to_degrees()
    );
}

fn print_windows(state: &perch_proto::State) {
    println!("windows:");
    for w in &state.windows {
        let p = &w.placement;
        println!(
            "  {}{} {:<14} yaw {:>6.1}° pitch {:>5.1}° dist {:.1} m {:.2}×{:.2} m  {:?}{}",
            if Some(w.id) == state.focused { "*" } else { " " },
            &w.id.to_string()[..8],
            source_label(&w.source),
            p.yaw.to_degrees(),
            p.pitch.to_degrees(),
            p.distance,
            p.width_m,
            p.height_m,
            w.anchor,
            if w.visible { "" } else { " (hidden)" }
        );
    }
}

fn source_label(s: &perch_proto::Source) -> String {
    use perch_proto::Source::*;
    match s {
        Display { display_index, .. } => format!("display:{display_index}"),
        Window { title, .. } => format!("win:{}", title.chars().take(10).collect::<String>()),
        Virtual { width, height, .. } => format!("virtual {width}x{height}"),
        Dex { .. } => "dex".into(),
        PhoneScreen { .. } => "phone".into(),
    }
}

#[cfg(feature = "rokid")]
fn record(out: &str, seconds: u64) -> anyhow::Result<()> {
    use perch_hal::HalEvent;
    use std::io::Write;
    let mut g = perch_hal::open_any()?;
    println!("recording {} for {seconds}s → {out}", g.info().model);
    let mut f = std::fs::File::create(out)?;
    let start = std::time::Instant::now();
    let mut n = 0u64;
    while start.elapsed().as_secs() < seconds {
        if let HalEvent::Imu(s) = g.next_event()? {
            let r = perch_hal::mock::Record {
                t_us: s.t_us,
                accel: [s.accel.x, s.accel.y, s.accel.z],
                gyro: [s.gyro.x, s.gyro.y, s.gyro.z],
                mag: s.mag.map(|m| [m.x, m.y, m.z]),
            };
            writeln!(f, "{}", serde_json::to_string(&r)?)?;
            n += 1;
        }
    }
    println!("{n} samples ({:.0} Hz)", n as f64 / seconds as f64);
    Ok(())
}

#[cfg(not(feature = "rokid"))]
fn record(_out: &str, _seconds: u64) -> anyhow::Result<()> {
    anyhow::bail!("rebuild with `cargo build -p perch-cli --features rokid` to record from hardware")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_prefix_resolution() {
        let a = Uuid::parse_str("11111111-0000-0000-0000-000000000000").unwrap();
        let b = Uuid::parse_str("12222222-0000-0000-0000-000000000000").unwrap();
        assert_eq!(resolve_id("111", &[a, b]).unwrap(), a);
        assert!(resolve_id("1", &[a, b]).is_err());
        assert!(resolve_id("9", &[a, b]).is_err());
        assert_eq!(resolve_id(&a.to_string(), &[]).unwrap(), a);
    }
}
