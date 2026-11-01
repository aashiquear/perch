//! The one capture backend shared by the server (which runs the portal pick)
//! and the render loop (which opens streams). `None` when no backend is
//! compiled in for this platform; windows then show the test pattern.

use perch_capture::CaptureBackend;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Shared handle.
pub type SharedBackend = Arc<Mutex<Option<Box<dyn CaptureBackend>>>>;

/// What the on-glasses UI (render thread) sends to the engine loop.
#[derive(Debug)]
pub enum UiMsg {
    /// A controller-style command (same as from the CLI/phone).
    Command(perch_proto::Command),
    /// Raw input for the application behind a window: the spatial pointer is
    /// over `window` (or it is focused, for keys). `MouseMoveAbs` is
    /// normalised to the window.
    Input {
        /// Target window.
        window: perch_proto::WindowId,
        /// Events.
        events: Vec<perch_proto::InputEvent>,
    },
    /// The physical-input grab was switched on or off.
    GrabChanged(bool),
}

/// Open the platform backend, if any.
pub fn init(peer: Uuid) -> SharedBackend {
    let b = match perch_capture::default_backend(peer) {
        Ok(b) => Some(b),
        Err(e) => {
            tracing::info!("screen capture disabled: {e}");
            None
        }
    };
    Arc::new(Mutex::new(b))
}

/// Close the capture backend's sessions before exit (see `CaptureBackend::close`).
pub fn close(backend: &SharedBackend) {
    if let Ok(mut b) = backend.lock() {
        if let Some(b) = b.as_mut() {
            b.close();
        }
    }
}
