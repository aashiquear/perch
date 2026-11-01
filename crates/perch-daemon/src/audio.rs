//! Audio output switching for the control panel. The Rokid Max is a USB-C
//! DisplayPort device, so its speakers appear to PipeWire as the GPU's
//! HDMI/DP audio sink. Switching the *default* sink moves every playing
//! stream (WirePlumber follows the default), independent of what the
//! glasses display. Implemented by shelling out to the PipeWire tools
//! (`pw-dump`, `pw-metadata`, `wpctl`) that ship with every PipeWire
//! desktop; nothing here is on a latency path.

// The panel (feature `render`) is the only reader of the sink list; the
// switch itself is reachable from any build through `UiMsg::SetAudioSink`.
#![allow(dead_code)]

use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One audio output as the panel lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sink {
    /// PipeWire node id (what `wpctl set-default` takes).
    pub id: u32,
    /// `node.name`, stable across reboots (what the default metadata holds).
    pub name: String,
    /// Human description, e.g. "Navi 10 HDMI Audio Digital Stereo (HDMI)".
    pub description: String,
    /// Current default output.
    pub is_default: bool,
}

impl Sink {
    /// Best guess that this sink is the glasses: DisplayPort/HDMI audio of
    /// the GPU, or anything naming the glasses.
    pub fn looks_like_glasses(&self) -> bool {
        let d = self.description.to_ascii_lowercase();
        let n = self.name.to_ascii_lowercase();
        d.contains("rokid")
            || n.contains("rokid")
            || d.contains("hdmi")
            || d.contains("displayport")
            || n.contains("hdmi")
    }
}

/// Shared, periodically refreshed sink list.
pub type Sinks = Arc<Mutex<Vec<Sink>>>;

/// Start a thread that refreshes the sink list every few seconds.
pub fn spawn_watcher() -> Sinks {
    let sinks: Sinks = Arc::new(Mutex::new(Vec::new()));
    let s2 = Arc::clone(&sinks);
    std::thread::Builder::new()
        .name("perch-audio".into())
        .spawn(move || loop {
            let list = list_sinks();
            if let Ok(mut g) = s2.lock() {
                *g = list;
            }
            std::thread::sleep(Duration::from_secs(3));
        })
        .ok();
    sinks
}

/// Enumerate `Audio/Sink` nodes and mark the default.
pub fn list_sinks() -> Vec<Sink> {
    let dump = match Command::new("pw-dump").output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    let Ok(objs) = serde_json::from_slice::<Vec<serde_json::Value>>(&dump) else {
        return Vec::new();
    };
    let default_name = default_sink_name();
    let mut out = Vec::new();
    for o in objs {
        let props = &o["info"]["props"];
        if props["media.class"].as_str() != Some("Audio/Sink") {
            continue;
        }
        let (Some(id), Some(name)) = (o["id"].as_u64(), props["node.name"].as_str()) else {
            continue;
        };
        let description = props["node.description"]
            .as_str()
            .or_else(|| props["node.nick"].as_str())
            .unwrap_or(name)
            .to_string();
        out.push(Sink {
            id: id as u32,
            name: name.to_string(),
            description,
            is_default: default_name.as_deref() == Some(name),
        });
    }
    out
}

fn default_sink_name() -> Option<String> {
    let o = Command::new("pw-metadata")
        .args(["0", "default.audio.sink"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&o.stdout);
    // ... value:'{"name":"alsa_output...."}' ...
    let v = text.split("value:'").nth(1)?.split('\'').next()?;
    serde_json::from_str::<serde_json::Value>(v)
        .ok()?
        .get("name")?
        .as_str()
        .map(ToOwned::to_owned)
}

/// Make `id` the default output (streams follow).
pub fn set_default(id: u32) -> Result<(), String> {
    let st = Command::new("wpctl")
        .args(["set-default", &id.to_string()])
        .status()
        .map_err(|e| format!("wpctl: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("wpctl set-default {id} failed: {st}"))
    }
}
