//! `perch-compositor` — draws the spatial windows onto the glasses.
//!
//! The glasses are just a monitor to the OS. The daemon opens a borderless
//! fullscreen `winit` window on that monitor (identified by its EDID name,
//! "Rokid Max"), and every frame:
//!
//! 1. asks the tracker for a pose predicted to the display's scan-out time,
//! 2. asks `perch-space` to project each window to a [`ScreenQuad`],
//! 3. uploads any new captured frames to textures,
//! 4. draws each quad with a projective mapping (see `shader.wgsl`),
//!    back-to-front, with opacity and a focus outline.
//!
//! Stereo: in SBS modes the panel is 3840 wide; we render the scene twice
//! with the head origin shifted by ±IPD/2, which gives correct parallax so
//! windows appear at their `distance` rather than at infinity.
//!
//! This crate contains the GPU code and a pure, testable
//! [`quad_to_vertices`] that turns a quad + homography into vertex data.

#![warn(missing_docs)]

use nalgebra::Matrix3;
use perch_capture::Frame;
use perch_proto::WindowId;
use perch_space::{Projector, ScreenQuad};
use std::collections::HashMap;
use std::sync::Arc;

/// Errors.
#[derive(Debug, thiserror::Error)]
pub enum CompositorError {
    /// No GPU adapter / surface.
    #[error("gpu: {0}")]
    Gpu(String),
    /// Surface lost.
    #[error("surface: {0}")]
    Surface(#[from] wgpu::SurfaceError),
}

/// Vertex layout matching `shader.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// `(x_ndc * w, y_ndc * w, w)`.
    pub pos_w: [f32; 3],
    /// Texture coordinate.
    pub uv: [f32; 2],
}

/// Build 6 vertices (two triangles) for a projected quad on a panel of
/// `panel_w × panel_h` pixels, optionally restricted to a sub-viewport
/// (for SBS stereo halves: `vp = (x0, w)`).
///
/// The homogeneous weight for corner *i* is the third row of the homography
/// evaluated at that corner's `uv` — this is what makes interpolation
/// projective rather than affine.
pub fn quad_to_vertices(q: &ScreenQuad, h: &Matrix3<f32>, panel_w: f32, panel_h: f32, vp: (f32, f32)) -> [Vertex; 6] {
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let (vx0, vw) = vp;
    let corner = |i: usize| {
        let [u, v] = uvs[i];
        let w = h[(2, 0)] * u + h[(2, 1)] * v + h[(2, 2)];
        let p = q.corners[i];
        // pixel → NDC within the viewport
        let x_ndc = ((p.x - vx0) / vw) * 2.0 - 1.0;
        let y_ndc = 1.0 - (p.y / panel_h) * 2.0;
        let _ = panel_w;
        Vertex {
            pos_w: [x_ndc * w, y_ndc * w, w],
            uv: [u, v],
        }
    };
    let c = [corner(0), corner(1), corner(2), corner(3)];
    [c[0], c[1], c[2], c[0], c[2], c[3]]
}

/// Per-window GPU resources.
struct Layer {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    params: wgpu::Buffer,
    size: (u32, u32),
}

/// Stereo configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stereo {
    /// Mono panel.
    Off,
    /// Side-by-side; `ipd_m` is interpupillary distance in metres.
    SideBySide {
        /// IPD.
        ipd_m: f32,
    },
}

/// The renderer. Create with [`Compositor::new`] from a `winit` window.
pub struct Compositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    layers: HashMap<WindowId, Layer>,
    vbuf: wgpu::Buffer,
    vbuf_cap: usize,
    /// Stereo mode.
    pub stereo: Stereo,
    /// Draw a thin outline on the focused window.
    pub focus_outline: bool,
}

impl Compositor {
    /// Initialise wgpu on a window (must be `'static`, e.g. `Arc<winit::window::Window>`).
    pub fn new(window: Arc<winit::window::Window>) -> Result<Self, CompositorError> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|e| CompositorError::Gpu(e.to_string()))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| CompositorError::Gpu("no adapter".into()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("perch"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| CompositorError::Gpu(e.to_string()))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            // Immediate/Mailbox = lowest latency; FIFO adds up to a frame.
            present_mode: if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("perch-blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("perch-layer"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("perch-blit"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let vbuf_cap = 64 * 6;
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("perch-vbuf"),
            size: (vbuf_cap * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            device,
            queue,
            surface,
            config,
            pipeline,
            bgl,
            sampler,
            layers: HashMap::new(),
            vbuf,
            vbuf_cap,
            stereo: Stereo::Off,
            focus_outline: true,
        })
    }

    /// Handle a window resize (e.g. display mode switch 1080p → 1200p).
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Panel size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Upload a captured frame for a window (creates/resizes the texture).
    pub fn upload(&mut self, id: WindowId, frame: &Frame) {
        let Frame::Rgba {
            width,
            height,
            stride,
            format,
            data,
            ..
        } = frame
        else {
            tracing::warn!("external frames not yet supported; see docs/ROADMAP.md M3");
            return;
        };
        let (w, h) = (*width, *height);
        let needs_new = self.layers.get(&id).is_none_or(|l| l.size != (w, h));
        if needs_new {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("perch-window"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: match format {
                    perch_capture::PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8UnormSrgb,
                    perch_capture::PixelFormat::Bgra8 => wgpu::TextureFormat::Bgra8UnormSrgb,
                },
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let params = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("perch-params"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params.as_entire_binding(),
                    },
                ],
            });
            self.layers.insert(
                id,
                Layer {
                    texture,
                    bind_group,
                    params,
                    size: (w, h),
                },
            );
        }
        let layer = self.layers.get(&id).expect("just inserted");
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &layer.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(*stride),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Drop GPU resources for a window.
    pub fn remove(&mut self, id: WindowId) {
        self.layers.remove(&id);
    }

    /// Render one frame. `quads` must be back-to-front (from
    /// `Projector::project_all`). In SBS mode the caller passes the
    /// left-eye and right-eye quads separately via [`Self::render_stereo`].
    pub fn render(&mut self, quads: &[ScreenQuad]) -> Result<(), CompositorError> {
        let (w, h) = (self.config.width as f32, self.config.height as f32);
        self.render_views(&[(quads, (0.0, w))], w, h)
    }

    /// Render SBS: left quads into the left half, right into the right half.
    pub fn render_stereo(&mut self, left: &[ScreenQuad], right: &[ScreenQuad]) -> Result<(), CompositorError> {
        let (w, h) = (self.config.width as f32, self.config.height as f32);
        let half = w / 2.0;
        self.render_views(&[(left, (0.0, half)), (right, (half, half))], w, h)
    }

    fn render_views(&mut self, views: &[(&[ScreenQuad], (f32, f32))], w: f32, h: f32) -> Result<(), CompositorError> {
        // Build vertex data for every quad that has a texture.
        let mut verts: Vec<Vertex> = Vec::new();
        let mut draws: Vec<(WindowId, u32, u32, f32)> = Vec::new(); // id, first, count, opacity
        for (quads, vp) in views {
            for q in quads.iter() {
                if !self.layers.contains_key(&q.id) {
                    continue;
                }
                let hmat = Projector::homography(q);
                let v = quad_to_vertices(q, &hmat, w, h, *vp);
                let first = verts.len() as u32;
                verts.extend_from_slice(&v);
                draws.push((q.id, first, 6, q.opacity));
            }
        }
        if verts.len() > self.vbuf_cap {
            self.vbuf_cap = verts.len().next_power_of_two();
            self.vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("perch-vbuf"),
                size: (self.vbuf_cap * std::mem::size_of::<Vertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !verts.is_empty() {
            self.queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&verts));
        }
        for (id, _, _, opacity) in &draws {
            if let Some(l) = self.layers.get(id) {
                self.queue
                    .write_buffer(&l.params, 0, bytemuck::cast_slice(&[*opacity, 0.0, 0.0, 0.0]));
            }
        }

        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("perch"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Black = transparent on optical see-through glasses.
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.vbuf.slice(..));
            for (id, first, count, _) in &draws {
                if let Some(l) = self.layers.get(id) {
                    pass.set_bind_group(0, &l.bind_group, &[]);
                    pass.draw(*first..*first + *count, 0..1);
                }
            }
        }
        self.queue.submit([enc.finish()]);
        frame.present();
        Ok(())
    }
}

/// Find the monitor that is the glasses (by name) among `monitors`.
pub fn find_glasses_monitor<I>(monitors: I, name_hint: &str) -> Option<winit::monitor::MonitorHandle>
where
    I: Iterator<Item = winit::monitor::MonitorHandle>,
{
    monitors.into_iter().find(|m| {
        m.name()
            .is_some_and(|n| n.to_lowercase().contains(&name_hint.to_lowercase()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Point2;

    #[test]
    fn vertices_cover_panel_in_ndc() {
        let q = ScreenQuad {
            id: uuid::Uuid::nil(),
            corners: [
                Point2::new(0.0, 0.0),
                Point2::new(1920.0, 0.0),
                Point2::new(1920.0, 1080.0),
                Point2::new(0.0, 1080.0),
            ],
            depth: 2.0,
            z: 0,
            opacity: 1.0,
            on_screen: true,
        };
        let h = Projector::homography(&q);
        let v = quad_to_vertices(&q, &h, 1920.0, 1080.0, (0.0, 1920.0));
        // Axis-aligned rectangle: w == 1 everywhere, so pos_w == NDC.
        assert_eq!(v[0].pos_w, [-1.0, 1.0, 1.0]);
        assert_eq!(v[1].pos_w, [1.0, 1.0, 1.0]);
        assert_eq!(v[2].pos_w, [1.0, -1.0, 1.0]);
        assert_eq!(v[5].pos_w, [-1.0, -1.0, 1.0]);
        assert_eq!(v[0].uv, [0.0, 0.0]);
        assert_eq!(v[2].uv, [1.0, 1.0]);
    }

    #[test]
    fn trapezoid_gets_nonuniform_w() {
        // A window turned away: right edge shorter than left edge.
        let q = ScreenQuad {
            id: uuid::Uuid::nil(),
            corners: [
                Point2::new(100.0, 100.0),
                Point2::new(900.0, 300.0),
                Point2::new(900.0, 780.0),
                Point2::new(100.0, 980.0),
            ],
            depth: 2.0,
            z: 0,
            opacity: 1.0,
            on_screen: true,
        };
        let h = Projector::homography(&q);
        let v = quad_to_vertices(&q, &h, 1920.0, 1080.0, (0.0, 1920.0));
        let w_left = v[0].pos_w[2];
        let w_right = v[1].pos_w[2];
        assert!(w_left > 0.0 && w_right > 0.0);
        assert!(
            (w_left - w_right).abs() > 1e-3,
            "projective weights must differ for a trapezoid"
        );
        // Dividing by w must give back the pixel corners in NDC.
        let x_ndc = v[1].pos_w[0] / w_right;
        assert!((x_ndc - (900.0 / 1920.0 * 2.0 - 1.0)).abs() < 1e-4);
    }

    #[test]
    fn stereo_viewport_maps_to_half() {
        let q = ScreenQuad {
            id: uuid::Uuid::nil(),
            corners: [
                Point2::new(1920.0, 0.0),
                Point2::new(3840.0, 0.0),
                Point2::new(3840.0, 1080.0),
                Point2::new(1920.0, 1080.0),
            ],
            depth: 2.0,
            z: 0,
            opacity: 1.0,
            on_screen: true,
        };
        let h = Projector::homography(&q);
        let v = quad_to_vertices(&q, &h, 3840.0, 1080.0, (1920.0, 1920.0));
        assert_eq!(v[0].pos_w[0], -1.0);
        assert_eq!(v[1].pos_w[0], 1.0);
    }
}
