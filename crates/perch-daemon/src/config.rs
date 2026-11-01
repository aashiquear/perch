//! `perchd` configuration: `~/.config/perch/perchd.toml` (XDG) and the
//! persisted state next to it (`workspace.json`, `calibration-<serial>.json`).

use perch_tracking::TrackerConfig;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use uuid::Uuid;

/// Top-level config. Every field has a default so an empty file is valid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Stable id for this install (generated on first run).
    pub peer_id: Uuid,
    /// Name shown to other peers.
    pub name: String,
    /// Pairing token controllers must present (empty = none; LAN only!).
    pub pairing_token: String,
    /// Control WebSocket bind.
    pub control_bind: SocketAddr,
    /// Stream UDP bind.
    pub stream_bind: SocketAddr,
    /// Advertise on mDNS.
    pub mdns: bool,
    /// Display optics / modes.
    pub display: DisplayConfig,
    /// Tracker tuning.
    pub tracker: TrackerConfig,
    /// Input sharing.
    pub input: InputConfig,
    /// Glasses buttons → actions.
    pub buttons: ButtonsConfig,
    /// Screen capture.
    pub capture: CaptureConfig,
}

/// Capture settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    /// Opaque token from the last portal pick; lets perchd restore the same
    /// windows on start without a dialog. Cleared by `perch pick`.
    pub restore_token: String,
    /// Requested capture frame rate.
    pub fps: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            restore_token: String::new(),
            fps: 60,
        }
    }
}

/// Display section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// Substring of the monitor name to render on.
    pub monitor_name: String,
    /// Horizontal FOV override in degrees (ar-drivers reports 23 for Max; the
    /// 50° diagonal spec implies ~43–46). Tune until a 1 m-wide window at
    /// 2 m looks 1 m wide.
    pub fov_h_deg: f32,
    /// Display tilt relative to IMU, degrees.
    pub tilt_deg: f32,
    /// Mode to request on connect: mono | stereo | mono120 | stereo120.
    pub mode: String,
    /// IPD in metres for stereo.
    pub ipd_m: f32,
    /// Extra prediction in milliseconds on top of the device's display delay.
    pub extra_prediction_ms: f32,
}

/// Input section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// Enable local keyboard/mouse capture + routing to remote windows.
    pub share_local_devices: bool,
    /// Pixels of resistance before an edge switch.
    pub edge_resistance_px: f32,
    /// Look-to-focus: focus follows gaze after `gaze_focus_ms` dwell (0 = off).
    pub gaze_focus_ms: u32,
    /// Hotkey (HID usage) that toggles routing back to local, e.g. ScrollLock.
    pub panic_key_usage: u16,
    /// Second toggle for keyboards without Scroll Lock: this HID usage pressed
    /// with Ctrl+Alt held (default `P`). 0 disables.
    pub grab_chord_usage: u16,
    /// Ctrl+Alt+<this HID usage> moves the focused window to a new glasses
    /// display (default `G`). 0 disables.
    pub move_chord_usage: u16,
    /// Ctrl+Alt+<this HID usage> moves the focused window to a half-width
    /// glasses display (default `H`). 0 disables.
    pub move_half_chord_usage: u16,
    /// Touchpad two-finger scrolling: content follows the fingers (true) or
    /// the reverse. Toggle in the panel; saved here.
    pub natural_scroll: bool,
}

/// Buttons section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ButtonsConfig {
    /// Action for temple button 0 ("recenter" | "focus_next" | "none").
    pub button0: String,
    /// Action for button 1.
    pub button1: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            peer_id: Uuid::new_v4(),
            name: hostname(),
            pairing_token: String::new(),
            control_bind: perch_net::default_control_bind(),
            stream_bind: perch_net::default_stream_bind(),
            mdns: true,
            display: DisplayConfig::default(),
            tracker: TrackerConfig::default(),
            input: InputConfig::default(),
            buttons: ButtonsConfig::default(),
            capture: CaptureConfig::default(),
        }
    }
}
impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            monitor_name: "Rokid".into(),
            fov_h_deg: 46.0,
            tilt_deg: 4.0,
            mode: "mono120".into(),
            ipd_m: 0.064,
            extra_prediction_ms: 4.0,
        }
    }
}
impl Default for InputConfig {
    fn default() -> Self {
        Self {
            share_local_devices: true,
            edge_resistance_px: 40.0,
            gaze_focus_ms: 0,
            panic_key_usage: 0x47,       // Scroll Lock
            grab_chord_usage: 0x13,      // Ctrl+Alt+P
            move_chord_usage: 0x0a,      // Ctrl+Alt+G
            move_half_chord_usage: 0x0b, // Ctrl+Alt+H
            natural_scroll: true,
        }
    }
}
impl Default for ButtonsConfig {
    fn default() -> Self {
        Self {
            button0: "recenter".into(),
            button1: "focus_next".into(),
        }
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "perch-host".into())
}

/// Where files live.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Config dir.
    pub dir: PathBuf,
}

impl Paths {
    /// XDG-aware default (`~/.config/perch` on Linux).
    pub fn default_paths() -> Self {
        let dir = directories::ProjectDirs::from("io", "perch", "perch")
            .map(|p| p.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".perch"));
        Self { dir }
    }
    /// Explicit dir.
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }
    /// perchd.toml
    pub fn config_file(&self) -> PathBuf {
        self.dir.join("perchd.toml")
    }
    /// workspace.json
    pub fn workspace_file(&self) -> PathBuf {
        self.dir.join("workspace.json")
    }
    /// calibration-<serial>.json
    pub fn calibration_file(&self, serial: &str) -> PathBuf {
        let safe: String = serial.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        self.dir.join(format!("calibration-{safe}.json"))
    }

    /// Load config, creating a default file on first run.
    pub fn load_config(&self) -> anyhow::Result<Config> {
        let path = self.config_file();
        if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            let cfg: Config = toml::from_str(&text)?;
            Ok(cfg)
        } else {
            let cfg = Config::default();
            std::fs::create_dir_all(&self.dir)?;
            std::fs::write(&path, toml::to_string_pretty(&cfg)?)?;
            tracing::info!(path = %path.display(), "wrote default config");
            Ok(cfg)
        }
    }

    /// Overwrite the config file.
    pub fn save_config(&self, cfg: &Config) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.config_file(), toml::to_string_pretty(cfg)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrips_toml() {
        let c = Config::default();
        let t = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&t).unwrap();
        assert_eq!(back.peer_id, c.peer_id);
        assert_eq!(back.display.fov_h_deg, 46.0);
        // Partial files are fine.
        let partial: Config = toml::from_str("name = \"lab\"\n[display]\nfov_h_deg = 44\n").unwrap();
        assert_eq!(partial.name, "lab");
        assert_eq!(partial.display.fov_h_deg, 44.0);
        assert_eq!(partial.display.mode, "mono120");
    }
}
