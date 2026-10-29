//! Frame-rate counter drawn into the model window.
//!
//! The readout is a small bitmap-font strip rasterised on the CPU, so the
//! counter needs neither a font file nor a text-shaping dependency: a 5×7 glyph
//! table covers `FPS`, the digits and a little punctuation. One strip is uploaded
//! per reading and composited over the swapchain image as a single quad.

use std::time::{Duration, Instant};

use crate::Gpu;

/// Glyph cell width in font pixels.
const GLYPH_WIDTH: u32 = 5;
/// Glyph cell height in font pixels.
const GLYPH_HEIGHT: u32 = 7;
/// Largest bitmap scale the rasteriser accepts (1 font pixel → 6 device pixels).
const MAX_SCALE: u32 = 6;

/// Averaging window of a steady-state reading.
const SAMPLE_WINDOW: Duration = Duration::from_millis(500);
/// The first reading uses half the window so the counter appears quickly.
const FIRST_WINDOW: Duration = Duration::from_millis(250);
/// A gap longer than this means the window stopped presenting; the reading is
/// restarted instead of reporting a bogus near-zero rate.
const STALL_TIMEOUT: Duration = Duration::from_secs(1);

/// Plate colour (sRGB, straight alpha).
const PLATE_RGB: [f32; 3] = [0.05, 0.07, 0.10];
/// Plate opacity, so the digits stay readable over bright hair and skin.
const PLATE_ALPHA: f32 = 0.62;
/// Glyph colour (sRGB, straight alpha).
const TEXT_RGB: [f32; 3] = [0.96, 0.98, 1.0];
/// Glyph opacity.
const TEXT_ALPHA: f32 = 0.96;

/// Bitmap rows of `c`, bit 4 being the leftmost column.
///
/// `None` means the font has no glyph, which lets callers skip unsupported
/// characters instead of drawing a placeholder box.
fn glyph(c: char) -> Option<[u8; GLYPH_HEIGHT as usize]> {
    let rows: [u8; GLYPH_HEIGHT as usize] = match c {
        '0' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        ':' => [
            0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        ' ' => [0; GLYPH_HEIGHT as usize],
        _ => return None,
    };
    Some(rows)
}

/// A rasterised readout, premultiplied and ready for `Queue::write_texture`.
pub(crate) struct TextStrip {
    /// Strip width in device pixels.
    pub(crate) width: u32,
    /// Strip height in device pixels.
    pub(crate) height: u32,
    /// Premultiplied RGBA8 texels, `width * height * 4` bytes.
    pub(crate) pixels: Vec<u8>,
}

/// Rasterises `text` into a rounded plate, `scale` device pixels per font pixel.
///
/// Returns `None` for empty text and for text the bitmap font cannot draw.
pub(crate) fn rasterize(text: &str, scale: u32) -> Option<TextStrip> {
    let glyphs: Vec<[u8; GLYPH_HEIGHT as usize]> = text.chars().filter_map(glyph).collect();
    if glyphs.is_empty() {
        return None;
    }
    let scale = scale.clamp(1, MAX_SCALE);
    let advance = GLYPH_WIDTH * scale + scale;
    let pad_x = 3 * scale;
    let pad_y = 2 * scale;
    let width = pad_x * 2 + glyphs.len() as u32 * advance - scale;
    let height = pad_y * 2 + GLYPH_HEIGHT * scale;
    let radius = 3.0 * scale as f32;

    // Ink mask first: a glyph pixel covers a `scale`×`scale` block.
    let mut ink = vec![0.0f32; (width * height) as usize];
    for (index, rows) in glyphs.iter().enumerate() {
        let origin_x = pad_x + index as u32 * advance;
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..GLYPH_WIDTH {
                if (*bits & (1 << (GLYPH_WIDTH - 1 - column))) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    let y = pad_y + row as u32 * scale + dy;
                    for dx in 0..scale {
                        let x = origin_x + column * scale + dx;
                        ink[(y * width + x) as usize] = 1.0;
                    }
                }
            }
        }
    }

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (y, row) in pixels.chunks_exact_mut((width * 4) as usize).enumerate() {
        let row_ink = &ink[y * width as usize..(y + 1) * width as usize];
        for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
            let coverage = rounded_rect_coverage(
                x as f32 + 0.5,
                y as f32 + 0.5,
                width as f32,
                height as f32,
                radius,
            );
            let plate = coverage * PLATE_ALPHA;
            let glyph_alpha = row_ink[x] * TEXT_ALPHA;
            // Text over plate, premultiplied.
            let alpha = glyph_alpha + plate * (1.0 - glyph_alpha);
            if alpha <= 0.0 {
                continue;
            }
            for (channel, value) in pixel[..3].iter_mut().enumerate() {
                let colour = TEXT_RGB[channel] * glyph_alpha
                    + PLATE_RGB[channel] * plate * (1.0 - glyph_alpha);
                *value = (colour.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            pixel[3] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }

    Some(TextStrip {
        width,
        height,
        pixels,
    })
}

/// Analytic coverage of a rounded rectangle, sampled at a pixel centre.
fn rounded_rect_coverage(x: f32, y: f32, width: f32, height: f32, radius: f32) -> f32 {
    let inner_x = (x - width * 0.5).abs() - (width * 0.5 - radius);
    let inner_y = (y - height * 0.5).abs() - (height * 0.5 - radius);
    let corner = (inner_x.max(0.0).powi(2) + inner_y.max(0.0).powi(2)).sqrt();
    let distance = corner + inner_x.max(inner_y).min(0.0) - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

/// Rolling frame-rate estimate of one role window.
pub(crate) struct FpsMeter {
    window_start: Instant,
    frames: u32,
    reading: Option<u32>,
}

impl Default for FpsMeter {
    fn default() -> Self {
        Self {
            window_start: Instant::now(),
            frames: 0,
            reading: None,
        }
    }
}

impl FpsMeter {
    /// Restarts the averaging window, e.g. after the counter was switched on.
    pub(crate) fn restart(&mut self, now: Instant) {
        self.window_start = now;
        self.frames = 0;
        self.reading = None;
    }

    /// Counts one presented frame; returns a new reading once the window elapsed.
    pub(crate) fn record(&mut self, now: Instant) -> Option<u32> {
        self.frames = self.frames.saturating_add(1);
        let elapsed = now.saturating_duration_since(self.window_start);
        if elapsed > STALL_TIMEOUT {
            self.restart(now);
            return None;
        }
        let window = if self.reading.is_some() {
            SAMPLE_WINDOW
        } else {
            FIRST_WINDOW
        };
        if elapsed < window {
            return None;
        }
        let seconds = elapsed.as_secs_f64();
        if seconds <= 0.0 {
            return None;
        }
        let reading = ((f64::from(self.frames) / seconds).round() as u32).clamp(1, 999);
        self.window_start = now;
        self.frames = 0;
        self.reading = Some(reading);
        Some(reading)
    }
}

/// GPU state of the counter: pipeline, placement uniform and the uploaded strip.
pub(crate) struct FpsOverlay {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    texture_format: wgpu::TextureFormat,
    texture: Option<wgpu::Texture>,
    bind_group: Option<wgpu::BindGroup>,
    texture_size: (u32, u32),
    strip_size: (u32, u32),
}

impl FpsOverlay {
    /// Builds the overlay pipeline for `format` surfaces.
    pub(crate) fn create(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bandori-pet.fps_overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/fps_overlay.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bandori-pet.fps_overlay.layout"),
            entries: &[
                // Strip texels.
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
                // Nearest sampling keeps the bitmap font crisp.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Pixel rect, viewport size and window opacity.
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
            label: Some("bandori-pet.fps_overlay.pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bandori-pet.fps_overlay.pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_overlay"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_overlay"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bandori-pet.fps_overlay.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bandori-pet.fps_overlay.params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            bind_group_layout,
            sampler,
            params,
            texture_format: overlay_texture_format(format),
            texture: None,
            bind_group: None,
            texture_size: (0, 0),
            strip_size: (0, 0),
        }
    }

    /// `true` when a strip is uploaded and ready to draw.
    pub(crate) fn is_ready(&self) -> bool {
        self.bind_group.is_some()
    }

    /// Uploads `text`; empty or undrawable text clears the counter.
    pub(crate) fn set_text(&mut self, gpu: &Gpu, text: &str, scale: u32) {
        let Some(strip) = rasterize(text, scale) else {
            self.clear();
            return;
        };
        if self.texture_size != (strip.width, strip.height) {
            self.create_texture(gpu, strip.width, strip.height);
        }
        let Some(texture) = self.texture.as_ref() else {
            return;
        };
        gpu.queue.write_texture(
            texture.as_image_copy(),
            &strip.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(strip.width * 4),
                rows_per_image: Some(strip.height),
            },
            wgpu::Extent3d {
                width: strip.width,
                height: strip.height,
                depth_or_array_layers: 1,
            },
        );
        self.strip_size = (strip.width, strip.height);
    }

    /// Drops the uploaded strip so nothing is drawn.
    pub(crate) fn clear(&mut self) {
        self.texture = None;
        self.bind_group = None;
        self.texture_size = (0, 0);
        self.strip_size = (0, 0);
    }

    /// Places the strip at `origin` inside a `viewport`, both in device pixels.
    ///
    /// `opacity` is the window opacity, so the counter fades with the pet.
    pub(crate) fn prepare(
        &self,
        queue: &wgpu::Queue,
        viewport: (u32, u32),
        origin: (u32, u32),
        opacity: f32,
    ) {
        let params = [
            origin.0 as f32,
            origin.1 as f32,
            self.strip_size.0 as f32,
            self.strip_size.1 as f32,
            viewport.0 as f32,
            viewport.1 as f32,
            opacity.clamp(0.0, 1.0),
            0.0,
        ];
        queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&params));
    }

    /// Draws the strip; the pass must load the already rendered swapchain image.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(bind_group) = self.bind_group.as_ref() else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..6, 0..1);
    }

    fn create_texture(&mut self, gpu: &Gpu, width: u32, height: u32) {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bandori-pet.fps_overlay.strip"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.texture_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bandori-pet.fps_overlay.bind_group"),
            layout: &self.bind_group_layout,
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
                    resource: self.params.as_entire_binding(),
                },
            ],
        });
        self.texture = Some(texture);
        self.bind_group = Some(bind_group);
        self.texture_size = (width, height);
    }
}

/// sRGB surfaces are written in linear space, so the strip has to be sRGB too.
fn overlay_texture_format(surface: wgpu::TextureFormat) -> wgpu::TextureFormat {
    if surface.is_srgb() {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_has_a_plate_and_a_rounded_edge() {
        let strip = rasterize("FPS 60", 2).expect("strip");
        // Two font pixels of plate padding above and below, at scale 2.
        assert_eq!(strip.height, 4 * 2 + GLYPH_HEIGHT * 2);
        assert_eq!(
            strip.pixels.len(),
            (strip.width * strip.height * 4) as usize
        );
        let alpha = |x: u32, y: u32| strip.pixels[((y * strip.width + x) * 4 + 3) as usize];
        assert!(alpha(0, 0) < 40, "the corner is rounded away");
        assert!(alpha(strip.width / 2, strip.height / 2) > 120);
    }

    #[test]
    fn undrawable_text_produces_no_strip() {
        assert!(rasterize("", 2).is_none());
        assert!(rasterize("帧率", 2).is_none());
        assert!(rasterize("FPS 240", 2).is_some());
    }

    #[test]
    fn rounded_coverage_is_one_inside_and_zero_outside() {
        assert!(rounded_rect_coverage(50.0, 50.0, 100.0, 100.0, 8.0) > 0.99);
        assert!(rounded_rect_coverage(1.0, 1.0, 100.0, 100.0, 8.0) < 0.01);
    }

    #[test]
    fn the_meter_reports_one_reading_per_window() {
        let start = Instant::now();
        let mut meter = FpsMeter::default();
        meter.restart(start);
        for _ in 0..9 {
            assert_eq!(meter.record(start + Duration::from_millis(24)), None);
        }
        // Ten frames in the short first window.
        assert_eq!(meter.record(start + Duration::from_millis(250)), Some(40));
        let after = start + Duration::from_millis(250);
        for frame in 0..29u64 {
            assert_eq!(
                meter.record(after + Duration::from_millis(frame * 16)),
                None
            );
        }
        assert_eq!(meter.record(after + Duration::from_millis(500)), Some(60));
    }

    #[test]
    fn a_stalled_window_restarts_instead_of_reporting_a_bogus_rate() {
        let start = Instant::now();
        let mut meter = FpsMeter::default();
        meter.restart(start);
        assert_eq!(meter.record(start + Duration::from_millis(20)), None);
        // Occluded for two seconds: the next frame must not read as `1 FPS`.
        assert_eq!(meter.record(start + Duration::from_secs(2)), None);
        for _ in 0..19 {
            assert_eq!(meter.record(start + Duration::from_millis(2010)), None);
        }
        assert_eq!(meter.record(start + Duration::from_millis(2250)), Some(80));
    }
}
