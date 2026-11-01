//! The one capture backend shared by the server (which runs the portal pick)
//! and the render loop (which opens streams). `None` when no backend is
//! compiled in for this platform; windows then show the test pattern.

use perch_capture::CaptureBackend;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Shared handle.
pub type SharedBackend = Arc<Mutex<Option<Box<dyn CaptureBackend>>>>;

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
