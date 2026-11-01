//! `perch-capture` — where pixels come from.
//!
//! A [`CaptureSource`] produces [`Frame`]s for one [`Source`] (a display, a
//! window, a virtual display, a phone screen). The compositor uploads frames
//! to textures and draws them on the projected quads from `perch-space`.
//!
//! Backend status (see docs/PLATFORMS.md):
//!
//! | Platform | Mechanism | Status |
//! |----------|-----------|--------|
//! | Linux (Wayland/X11) | xdg-desktop-portal ScreenCast → PipeWire DMA-BUF | scaffold (`linux-pipewire`) |
//! | Linux virtual displays | GNOME Mutter `CreateVirtualMonitor` / `wlr-virtual-output` / `evdi` | planned |
//! | Windows | Windows.Graphics.Capture | scaffold |
//! | macOS | ScreenCaptureKit | scaffold |
//! | Android | MediaProjection in the host app → `perch-ffi::push_frame` | implemented app-side |
//! | Any | [`TestPattern`] | implemented |
//!
//! Zero-copy is the goal: DMA-BUF/IOSurface/D3D11 textures go straight to
//! wgpu via external-memory import. v0.1 copies through CPU memory
//! ([`Frame::Rgba`]), which is fine for 1–3 windows at 1080p/60.

#![warn(missing_docs)]

use perch_proto::Source;
use std::sync::Arc;

/// Errors.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// Backend not compiled in / not available on this platform.
    #[error("capture backend unavailable: {0}")]
    Unavailable(String),
    /// User denied the portal / permission prompt.
    #[error("capture permission denied")]
    Denied,
    /// Source went away (window closed, display unplugged).
    #[error("capture source ended")]
    Ended,
    /// Other.
    #[error("{0}")]
    Other(String),
}

/// Pixel format of a CPU frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 8-bit RGBA, straight alpha.
    Rgba8,
    /// 8-bit BGRA (Windows/macOS native).
    Bgra8,
}

/// One captured frame.
#[derive(Debug, Clone)]
pub enum Frame {
    /// CPU pixels.
    Rgba {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
        /// Bytes per row (may include padding).
        stride: u32,
        /// Format.
        format: PixelFormat,
        /// Pixel data.
        data: Arc<[u8]>,
        /// Presentation timestamp, µs, host monotonic clock.
        t_us: u64,
    },
    /// A GPU-side handle the compositor can import without copying.
    /// Opaque per-platform (DMA-BUF fd, IOSurface id, D3D11 shared handle).
    External {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
        /// Platform handle.
        handle: u64,
        /// Timestamp.
        t_us: u64,
    },
}

impl Frame {
    /// Width in pixels.
    pub fn width(&self) -> u32 {
        match self {
            Frame::Rgba { width, .. } | Frame::External { width, .. } => *width,
        }
    }
    /// Height in pixels.
    pub fn height(&self) -> u32 {
        match self {
            Frame::Rgba { height, .. } | Frame::External { height, .. } => *height,
        }
    }
}

/// A running capture of one source.
pub trait CaptureSource: Send {
    /// The source being captured.
    fn source(&self) -> &Source;
    /// Block (up to `timeout`) for the next frame. `Ok(None)` on timeout.
    fn next_frame(&mut self, timeout: std::time::Duration) -> Result<Option<Frame>, CaptureError>;
    /// Current size.
    fn size(&self) -> (u32, u32);
}

/// Something that lists and opens capture sources on this machine.
pub trait CaptureBackend: Send {
    /// Enumerate what can be captured (displays, windows).
    fn enumerate(&mut self) -> Result<Vec<Source>, CaptureError>;
    /// Start capturing a source.
    fn open(&mut self, source: &Source, fps: u32) -> Result<Box<dyn CaptureSource>, CaptureError>;
    /// Create a virtual display of the given size (for "extend" mode).
    fn create_virtual_display(&mut self, _width: u32, _height: u32) -> Result<Source, CaptureError> {
        Err(CaptureError::Unavailable("virtual displays".into()))
    }
}

/// Pick the backend for this OS.
pub fn default_backend() -> Result<Box<dyn CaptureBackend>, CaptureError> {
    #[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
    {
        return Ok(Box::new(linux::PortalBackend::new()?));
    }
    #[allow(unreachable_code)]
    Err(CaptureError::Unavailable(
        "no capture backend compiled for this platform; use --features linux-pipewire or the test pattern".into(),
    ))
}

// ---------------------------------------------------------------------------
// Test pattern (always available)
// ---------------------------------------------------------------------------

/// Generates a moving gradient with a frame counter, so you can verify the
/// whole pipeline (tracking → projection → compositor) without a desktop.
pub struct TestPattern {
    source: Source,
    width: u32,
    height: u32,
    frame: u64,
    period: std::time::Duration,
    last: std::time::Instant,
}

impl TestPattern {
    /// New pattern at `fps`.
    pub fn new(source: Source, width: u32, height: u32, fps: u32) -> Self {
        Self {
            source,
            width,
            height,
            frame: 0,
            period: std::time::Duration::from_secs_f64(1.0 / fps.max(1) as f64),
            last: std::time::Instant::now(),
        }
    }

    /// Render frame `n` into RGBA. Public so the compositor's tests can use it.
    pub fn render(width: u32, height: u32, n: u64) -> Vec<u8> {
        let mut data = vec![0u8; (width * height * 4) as usize];
        let shift = (n % width as u64) as u32;
        for y in 0..height {
            for x in 0..width {
                let i = ((y * width + x) * 4) as usize;
                let xx = (x + shift) % width;
                let border = x < 8 || y < 8 || x >= width - 8 || y >= height - 8;
                let grid = (x % 120 < 2) || (y % 120 < 2);
                let (r, g, b) = if border {
                    (255, 255, 255)
                } else if grid {
                    (60, 60, 60)
                } else {
                    ((xx * 255 / width) as u8, (y * 255 / height) as u8, 128)
                };
                data[i] = r;
                data[i + 1] = g;
                data[i + 2] = b;
                data[i + 3] = 255;
            }
        }
        data
    }
}

impl CaptureSource for TestPattern {
    fn source(&self) -> &Source {
        &self.source
    }
    fn next_frame(&mut self, timeout: std::time::Duration) -> Result<Option<Frame>, CaptureError> {
        let due = self.last + self.period;
        let now = std::time::Instant::now();
        if due > now {
            let wait = due - now;
            if wait > timeout {
                std::thread::sleep(timeout);
                return Ok(None);
            }
            std::thread::sleep(wait);
        }
        self.last = std::time::Instant::now();
        self.frame += 1;
        let data = Self::render(self.width, self.height, self.frame);
        Ok(Some(Frame::Rgba {
            width: self.width,
            height: self.height,
            stride: self.width * 4,
            format: PixelFormat::Rgba8,
            data: data.into(),
            t_us: now_us(),
        }))
    }
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Host monotonic time in µs.
pub fn now_us() -> u64 {
    use std::sync::OnceLock;
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_micros() as u64
}

// ---------------------------------------------------------------------------
// Linux: xdg-desktop-portal + PipeWire
// ---------------------------------------------------------------------------

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
pub mod linux {
    //! Flow (works on GNOME, KDE, Sway/wlroots, and X11 via the portal):
    //!
    //! 1. `ashpd::desktop::screencast::Screencast::create_session()`
    //! 2. `select_sources(types = MONITOR | WINDOW, cursor_mode = EMBEDDED, persist = ExplicitlyRevoked)`
    //!    — persistence token is stored in the config so the user is asked once.
    //! 3. `start()` → PipeWire node ids.
    //! 4. `pipewire::stream::Stream` on that node; negotiate `DMA-BUF` with
    //!    modifiers if available, else `MemFd`.
    //! 5. Each buffer → [`Frame::External`] (DMA-BUF fd) or [`Frame::Rgba`].
    //!
    //! Virtual monitors: on GNOME ≥ 44 use the Mutter
    //! `org.gnome.Mutter.ScreenCast` `RecordVirtual` D-Bus call; on wlroots use
    //! `wlr-virtual-output-unstable-v1`; fallback is the `evdi` kernel module.
    use super::*;

    /// Portal-backed capture.
    pub struct PortalBackend {}

    impl PortalBackend {
        /// Connect to the portal.
        pub fn new() -> Result<Self, CaptureError> {
            Ok(Self {})
        }
    }

    impl CaptureBackend for PortalBackend {
        fn enumerate(&mut self) -> Result<Vec<Source>, CaptureError> {
            // The portal does not enumerate; the user picks in the portal
            // dialog during `open`. Return a single placeholder so the UI can
            // offer "Share screen…".
            Ok(vec![Source::Display {
                peer: uuid::Uuid::nil(),
                display_index: 0,
            }])
        }
        fn open(&mut self, _source: &Source, _fps: u32) -> Result<Box<dyn CaptureSource>, CaptureError> {
            Err(CaptureError::Unavailable(
                "PipeWire capture is scaffolded; see crates/perch-capture/src/lib.rs::linux and docs/ROADMAP.md M2"
                    .into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_produces_frames() {
        let src = Source::Virtual {
            peer: uuid::Uuid::nil(),
            width: 64,
            height: 32,
        };
        let mut tp = TestPattern::new(src, 64, 32, 1000);
        let f = tp.next_frame(std::time::Duration::from_millis(50)).unwrap().unwrap();
        assert_eq!((f.width(), f.height()), (64, 32));
        if let Frame::Rgba { data, .. } = f {
            assert_eq!(data.len(), 64 * 32 * 4);
            assert_eq!(data[3], 255);
        }
    }

    #[test]
    fn default_backend_without_features_is_unavailable() {
        assert!(matches!(default_backend(), Err(CaptureError::Unavailable(_))));
    }
}
