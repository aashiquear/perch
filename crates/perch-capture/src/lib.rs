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
//! | Linux (Wayland/X11) | xdg-desktop-portal ScreenCast → PipeWire (CPU frames) | implemented (`linux-pipewire`) |
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
    /// Interactively pick sources (e.g. the portal dialog). Blocks until the
    /// user has chosen; returns the newly available sources, which
    /// [`enumerate`](Self::enumerate) will also list from now on.
    fn pick(&mut self, _multiple: bool) -> Result<Vec<Source>, CaptureError> {
        Err(CaptureError::Unavailable("interactive source picking".into()))
    }
    /// Re-create the sources of a previous [`pick`](Self::pick) without a
    /// dialog, from the token that pick returned.
    fn restore(&mut self, _token: &str) -> Result<Vec<Source>, CaptureError> {
        Err(CaptureError::Unavailable("restore".into()))
    }
    /// Opaque token that [`restore`](Self::restore) accepts, if the last pick
    /// produced one. Persist it in the config.
    fn restore_token(&self) -> Option<String> {
        None
    }
    /// Pixel size of a source, if known without opening it.
    fn source_size(&self, _source: &Source) -> Option<(u32, u32)> {
        None
    }
    /// Where the source sits on the host desktop, in the OS's logical pixels
    /// (x, y, w, h), if known. Monitors usually are; individual windows
    /// usually are not.
    fn source_screen_rect(&self, _source: &Source) -> Option<(f32, f32, f32, f32)> {
        None
    }
    /// Extent of the whole desktop (all outputs) in logical pixels.
    fn screen_bounds(&self) -> Option<(f32, f32)> {
        None
    }
}

/// Pick the backend for this OS. `peer` is our own peer id (sources are
/// tagged with it).
pub fn default_backend(peer: uuid::Uuid) -> Result<Box<dyn CaptureBackend>, CaptureError> {
    #[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
    {
        return Ok(Box::new(linux::PortalBackend::new(peer)?));
    }
    #[allow(unreachable_code)]
    let _ = peer;
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
    /// Frame 0, rendered once; later frames are a cheap horizontal rotation
    /// of it so three 1080p patterns don't eat a CPU core.
    base: Vec<u8>,
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
            base: Self::render(width, height, 0),
        }
    }

    /// Frame `n`: `base` rotated left by `n` pixels, border kept in place.
    fn rotated(&self, n: u64) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let row = w * 4;
        let shift = (n % w as u64) as usize * 4;
        let mut out = vec![0u8; row * h];
        for y in 0..h {
            let src = &self.base[y * row..(y + 1) * row];
            let dst = &mut out[y * row..(y + 1) * row];
            dst[..row - shift].copy_from_slice(&src[shift..]);
            dst[row - shift..].copy_from_slice(&src[..shift]);
            if y < 8 || y >= h.saturating_sub(8) {
                dst.fill(255);
            } else {
                dst[..8 * 4].fill(255);
                dst[row - 8 * 4..].fill(255);
            }
        }
        out
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
        let data = self.rotated(self.frame);
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

/// Captured frames wider than this are halved before upload: the glasses
/// panel is 1920 px wide, so a 4K+ HiDPI monitor capture is oversampled and
/// its 40+ MB/frame upload stalls the render loop.
pub const MAX_UPLOAD_WIDTH: u32 = 2560;

/// Box-filter a tightly packed 4-byte-per-pixel image by 2 in each axis.
/// Odd trailing rows/columns are dropped. Returns `(data, w/2, h/2)`.
pub fn downscale2x(src: &[u8], w: u32, h: u32, stride: usize) -> (Vec<u8>, u32, u32) {
    let (ow, oh) = (w / 2, h / 2);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..oh as usize {
        let r0 = &src[2 * y * stride..2 * y * stride + (w as usize) * 4];
        let r1 = &src[(2 * y + 1) * stride..(2 * y + 1) * stride + (w as usize) * 4];
        let dst = &mut out[y * ow as usize * 4..(y + 1) * ow as usize * 4];
        for x in 0..ow as usize {
            let i = x * 8;
            for c in 0..4 {
                let sum = r0[i + c] as u16 + r0[i + 4 + c] as u16 + r1[i + c] as u16 + r1[i + 4 + c] as u16;
                dst[x * 4 + c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    (out, ow, oh)
}

/// Host monotonic time in µs.
pub fn now_us() -> u64 {
    use std::sync::OnceLock;
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_micros() as u64
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
pub mod linux;

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
        assert!(matches!(
            default_backend(uuid::Uuid::nil()),
            Err(CaptureError::Unavailable(_))
        ));
    }

    #[test]
    fn downscale2x_averages_blocks_and_honours_stride() {
        // 4x2 image with 4 bytes padding per row.
        let mut src = vec![0u8; 2 * 20];
        for (x, v) in [(0, 10u8), (1, 20), (2, 100), (3, 200)] {
            for row in 0..2 {
                src[row * 20 + x * 4..row * 20 + x * 4 + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let (out, w, h) = downscale2x(&src, 4, 2, 20);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, vec![15, 15, 15, 255, 150, 150, 150, 255]);
    }
}
