//! wgpu rendering for the desktop pet window (spec §2.3).
//!
//! Frame order follows the Neocari viewer example: a mask pass into a small
//! offscreen atlas, then the main pass with `LoadOp::Clear(alpha = 0)`. Neocari's
//! shader outputs premultiplied alpha and its `Normal` blend state is
//! `One/OneMinusSrcAlpha`, which matches `CompositeAlphaMode::PreMultiplied` on a
//! transparent surface.
//!
//! Full-resolution, fully opaque windows with premultiplied surfaces render
//! directly to the swapchain. Resolution scaling and window opacity use an
//! intermediate texture and a fullscreen blit. Both paths retain transparent
//! model pixels and composite the FPS counter in their final render pass.

mod overlay;

#[cfg(test)]
mod regression;

use std::sync::Arc;
use std::time::Instant;

use neocari::assets::DecodedTexture;
use neocari::core::Matrix44;
use neocari::moc3::Moc3DrawableMesh;
use neocari::render::wgpu::{
    preferred_surface_format, WgpuClippingPlan, WgpuClippingResources, WgpuLive2dRenderer,
    WgpuMaskRenderTarget, WgpuMeshBuffers, WgpuTexture, WgpuTransform,
};
use tracing::{info, warn};
use winit::dpi::PhysicalSize;
use winit::window::Window;

/// Size of the clipping mask atlas (matches the Neocari example).
pub const MASK_TEXTURE_SIZE: u32 = 256;
/// Frames rendered before the circuit breaker trips (spec §2.3).
pub const FAILURE_FUSE: u32 = 3;

/// Rendering errors that must not kill the process.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// No suitable GPU adapter.
    #[error("no suitable GPU adapter: {0}")]
    Adapter(String),
    /// Device creation failed.
    #[error("failed to create device: {0}")]
    Device(String),
    /// The surface cannot present.
    #[error("surface unsupported by adapter")]
    UnsupportedSurface,
    /// Surface texture acquisition failed.
    #[error("failed to acquire surface texture: {0}")]
    Acquire(String),
    /// Neocari draw call failed.
    #[error("draw failed: {0}")]
    Draw(String),
    /// Mesh upload failed.
    #[error("mesh upload failed")]
    MeshUpload,
    /// Clipping resources failed.
    #[error("clipping setup failed: {0}")]
    Clipping(String),
    /// Texture upload failed.
    #[error("texture upload failed: {0}")]
    Texture(String),
}

/// Shared GPU device, queue and Live2D pipelines.
pub struct Gpu {
    /// wgpu instance.
    pub instance: wgpu::Instance,
    /// Chosen adapter.
    pub adapter: wgpu::Adapter,
    /// Logical device.
    pub device: wgpu::Device,
    /// Queue.
    pub queue: wgpu::Queue,
    /// Adapter description for the About page.
    pub adapter_info: wgpu::AdapterInfo,
}

impl Gpu {
    /// Creates a device. `allow_hardware` mirrors `gpu_acceleration`.
    pub fn new(
        allow_hardware: bool,
        compatible_surface: Option<&wgpu::Surface<'static>>,
    ) -> Result<Self, RenderError> {
        if compatible_surface.is_none() {
            return Self::create_preferred(allow_hardware, None).map(|(gpu, _)| gpu);
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        Self::new_from(instance, allow_hardware, compatible_surface)
    }

    fn create_preferred(
        allow_hardware: bool,
        window: Option<Arc<Window>>,
    ) -> Result<(Self, Option<wgpu::Surface<'static>>), RenderError> {
        // wgpu sorts by device class, with Vulkan first for equal classes on
        // Windows. A Vulkan discrete GPU (or a requested CPU adapter) already
        // wins that search. Avoid initializing DX12/GL and their driver heaps
        // just to discard their adapters. Less preferred classes and failures
        // still use the full search, preserving cross-backend GPU selection.
        #[cfg(windows)]
        if let Ok(candidate) =
            Self::create_on_backends(allow_hardware, window.clone(), wgpu::Backends::VULKAN)
        {
            if preferred_device_class(allow_hardware, candidate.0.adapter_info.device_type) {
                return Ok(candidate);
            }
        }
        Self::create_on_backends(allow_hardware, window, wgpu::Backends::all())
    }

    fn create_on_backends(
        allow_hardware: bool,
        window: Option<Arc<Window>>,
        backends: wgpu::Backends,
    ) -> Result<(Self, Option<wgpu::Surface<'static>>), RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = window
            .map(|window| instance.create_surface(window))
            .transpose()
            .map_err(|error| RenderError::Adapter(error.to_string()))?;
        let gpu = Self::new_from(instance, allow_hardware, surface.as_ref())?;
        Ok((gpu, surface))
    }

    /// Creates a device on an existing instance (so the surface stays valid).
    pub fn new_from(
        instance: wgpu::Instance,
        allow_hardware: bool,
        compatible_surface: Option<&wgpu::Surface<'static>>,
    ) -> Result<Self, RenderError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: if allow_hardware {
                wgpu::PowerPreference::HighPerformance
            } else {
                wgpu::PowerPreference::LowPower
            },
            force_fallback_adapter: !allow_hardware,
            compatible_surface,
            apply_limit_buckets: false,
        }))
        .map_err(|error| RenderError::Adapter(error.to_string()))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("bandori-pet.device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            // Desktop pets use small, mostly persistent resources. Avoid the
            // large allocation blocks intended for games; texture dimensions,
            // render precision and frame scheduling stay unchanged.
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|error| RenderError::Device(error.to_string()))?;

        let adapter_info = adapter.get_info();
        info!(
            adapter = %adapter_info.name,
            backend = ?adapter_info.backend,
            driver = %adapter_info.driver,
            "renderer initialised"
        );
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            adapter_info,
        })
    }
}

#[cfg(windows)]
fn preferred_device_class(allow_hardware: bool, device_type: wgpu::DeviceType) -> bool {
    device_type
        == if allow_hardware {
            wgpu::DeviceType::DiscreteGpu
        } else {
            wgpu::DeviceType::Cpu
        }
}

/// The swapchain plus its configuration.
pub struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    alpha_mode: wgpu::CompositeAlphaMode,
    vsync: bool,
    dpr: f64,
}

impl Surface {
    /// Creates and configures a surface for `window`.
    pub fn new(gpu: &Gpu, window: Arc<Window>, vsync: bool) -> Result<Self, RenderError> {
        let surface = gpu
            .instance
            .create_surface(Arc::clone(&window))
            .map_err(|error| RenderError::Adapter(error.to_string()))?;
        Self::from_existing(gpu, surface, window, vsync)
    }

    /// Configures an already-created surface (the one the adapter was probed with).
    fn from_existing(
        gpu: &Gpu,
        surface: wgpu::Surface<'static>,
        window: Arc<Window>,
        vsync: bool,
    ) -> Result<Self, RenderError> {
        let capabilities = surface.get_capabilities(&gpu.adapter);
        if capabilities.formats.is_empty() {
            return Err(RenderError::UnsupportedSurface);
        }
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or(RenderError::UnsupportedSurface)?;
        config.format = preferred_surface_format(&capabilities.formats)
            .ok_or(RenderError::UnsupportedSurface)?;
        let alpha_mode = choose_alpha_mode(&capabilities.alpha_modes);
        config.alpha_mode = alpha_mode;
        config.present_mode = choose_present_mode(&capabilities.present_modes, vsync);
        config.desired_maximum_frame_latency = 3;
        surface.configure(&gpu.device, &config);
        info!(
            format = ?config.format,
            alpha = ?alpha_mode,
            present = ?config.present_mode,
            "surface configured"
        );
        Ok(Self {
            surface,
            config,
            alpha_mode,
            vsync,
            dpr: window.scale_factor(),
        })
    }

    /// Current size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Current device pixel ratio.
    pub fn dpr(&self) -> f64 {
        self.dpr
    }

    /// Surface format.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Alpha mode in use.
    pub fn alpha_mode(&self) -> wgpu::CompositeAlphaMode {
        self.alpha_mode
    }

    /// `true` when vsync is requested.
    pub fn vsync(&self) -> bool {
        self.vsync
    }

    /// Resizes the swapchain (no-op for zero sizes).
    pub fn resize(&mut self, gpu: &Gpu, size: PhysicalSize<u32>, dpr: f64) {
        self.dpr = dpr;
        if size.width == 0 || size.height == 0 {
            return;
        }
        if self.config.width == size.width && self.config.height == size.height {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Re-applies configuration after a vsync change.
    pub fn reconfigure(&mut self, gpu: &Gpu, vsync: bool) {
        self.vsync = vsync;
        self.config.present_mode = choose_present_mode(
            &self.surface.get_capabilities(&gpu.adapter).present_modes,
            vsync,
        );
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Recreates the swapchain after a lost/outdated frame.
    pub fn recreate(&mut self, gpu: &Gpu) {
        self.surface.configure(&gpu.device, &self.config);
    }

    fn acquire(&self) -> Result<wgpu::SurfaceTexture, RenderError> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Ok(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(frame),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Err(RenderError::Acquire("timed out".to_owned()))
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                Err(RenderError::Acquire("outdated".to_owned()))
            }
            wgpu::CurrentSurfaceTexture::Lost => Err(RenderError::Acquire("lost".to_owned())),
            wgpu::CurrentSurfaceTexture::Validation => {
                Err(RenderError::Acquire("validation".to_owned()))
            }
        }
    }
}

fn choose_alpha_mode(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    if modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
        wgpu::CompositeAlphaMode::PreMultiplied
    } else if modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied) {
        wgpu::CompositeAlphaMode::PostMultiplied
    } else {
        wgpu::CompositeAlphaMode::Auto
    }
}

fn choose_present_mode(modes: &[wgpu::PresentMode], vsync: bool) -> wgpu::PresentMode {
    if vsync {
        if modes.contains(&wgpu::PresentMode::Fifo) {
            wgpu::PresentMode::Fifo
        } else {
            wgpu::PresentMode::AutoVsync
        }
    } else if modes.contains(&wgpu::PresentMode::AutoNoVsync) {
        wgpu::PresentMode::AutoNoVsync
    } else if modes.contains(&wgpu::PresentMode::Immediate) {
        wgpu::PresentMode::Immediate
    } else {
        wgpu::PresentMode::AutoVsync
    }
}

/// GPU state for one loaded model.
pub struct ModelGpu {
    mesh_buffers: WgpuMeshBuffers,
    textures: Vec<WgpuTexture>,
    clipping_plan: WgpuClippingPlan,
    clipping: WgpuClippingResources,
    mask_target: WgpuMaskRenderTarget,
    transform: WgpuTransform,
    drawable_count: usize,
}

impl ModelGpu {
    /// Uploads meshes and textures and builds the clipping resources.
    pub fn new(
        gpu: &Gpu,
        renderer: &WgpuLive2dRenderer,
        meshes: &[Moc3DrawableMesh],
        textures: &[DecodedTexture],
        matrix: Matrix44,
    ) -> Result<Self, RenderError> {
        let mesh_buffers =
            WgpuMeshBuffers::from_drawables(&gpu.device, meshes).ok_or(RenderError::MeshUpload)?;
        let mut uploaded = Vec::with_capacity(textures.len());
        for texture in textures {
            let uploaded_texture = renderer
                .create_rgba8_texture(
                    &gpu.device,
                    &gpu.queue,
                    texture.width(),
                    texture.height(),
                    texture.rgba(),
                )
                .map_err(|error| RenderError::Texture(error.to_string()))?;
            uploaded.push(uploaded_texture);
        }
        let mut clipping_plan = WgpuClippingPlan::from_mesh_buffers(&mesh_buffers);
        clipping_plan
            .prepare_single_texture_masks(&mesh_buffers)
            .map_err(|error| RenderError::Clipping(format!("{error:?}")))?;
        let clipping = renderer
            .create_clipping_resources(&gpu.device, &clipping_plan)
            .map_err(|error| RenderError::Clipping(format!("{error:?}")))?;
        let mask_target = renderer
            .create_mask_render_target(&gpu.device, MASK_TEXTURE_SIZE)
            .map_err(|error| RenderError::Texture(error.to_string()))?;
        let transform = renderer.create_transform(&gpu.device, &matrix);
        let drawable_count = mesh_buffers.drawables().len();
        Ok(Self {
            mesh_buffers,
            textures: uploaded,
            clipping_plan,
            clipping,
            mask_target,
            transform,
            drawable_count,
        })
    }

    /// Re-uploads mesh vertices from a new frame.
    pub fn update_meshes(
        &mut self,
        gpu: &Gpu,
        renderer: &WgpuLive2dRenderer,
        meshes: &[Moc3DrawableMesh],
    ) -> Result<(), RenderError> {
        match self.mesh_buffers.update_drawables(&gpu.queue, meshes) {
            Ok(update) => {
                if update.visibility_changed() {
                    // Plans group visible meshes, so part fades can change the
                    // membership even when geometry and mask ids stay fixed.
                    self.clipping_plan = WgpuClippingPlan::from_mesh_buffers(&self.mesh_buffers);
                }
                if update.visibility_changed()
                    || (update.bounds_changed() && !self.clipping_plan.contexts().is_empty())
                {
                    self.refresh_clipping(gpu, renderer)?;
                }
                Ok(())
            }
            Err(_) => {
                self.mesh_buffers = WgpuMeshBuffers::from_drawables(&gpu.device, meshes)
                    .ok_or(RenderError::MeshUpload)?;
                self.drawable_count = self.mesh_buffers.drawables().len();
                self.clipping_plan = WgpuClippingPlan::from_mesh_buffers(&self.mesh_buffers);
                self.refresh_clipping(gpu, renderer)
            }
        }
    }

    /// Updates the model → clip transform.
    pub fn set_matrix(&mut self, gpu: &Gpu, renderer: &WgpuLive2dRenderer, matrix: Matrix44) {
        if !self.transform.update_matrix(&gpu.queue, &matrix) {
            self.transform = renderer.create_transform(&gpu.device, &matrix);
        }
    }

    fn refresh_clipping(
        &mut self,
        gpu: &Gpu,
        renderer: &WgpuLive2dRenderer,
    ) -> Result<(), RenderError> {
        self.clipping_plan
            .prepare_single_texture_masks(&self.mesh_buffers)
            .map_err(|error| RenderError::Clipping(format!("{error:?}")))?;
        if !renderer
            .update_clipping_resources(&gpu.queue, &mut self.clipping, &self.clipping_plan)
            .map_err(|error| RenderError::Clipping(format!("{error:?}")))?
        {
            self.clipping = renderer
                .create_clipping_resources(&gpu.device, &self.clipping_plan)
                .map_err(|error| RenderError::Clipping(format!("{error:?}")))?;
        }
        Ok(())
    }

    /// Number of drawables currently uploaded.
    pub fn drawable_count(&self) -> usize {
        self.drawable_count
    }

    /// Number of uploaded textures.
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    fn draw_masks(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        renderer: &WgpuLive2dRenderer,
    ) -> Result<(), RenderError> {
        if self.clipping.contexts().is_empty() {
            return Ok(());
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bandori-pet.mask_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.mask_target.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer
            .draw_masks_with_textures(
                &mut pass,
                &self.mesh_buffers,
                &self.clipping,
                &self.textures,
            )
            .map(|_| ())
            .map_err(|error| RenderError::Draw(error.to_string()))
    }

    fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        renderer: &WgpuLive2dRenderer,
    ) -> Result<(), RenderError> {
        let result = if self.clipping.contexts().is_empty() {
            renderer.draw_with_textures_and_transform(
                pass,
                &self.mesh_buffers,
                &self.textures,
                &self.transform,
            )
        } else {
            renderer.draw_with_textures_clipping_and_transform(
                pass,
                &self.mesh_buffers,
                &self.textures,
                &self.clipping,
                &self.mask_target,
                &self.transform,
            )
        };
        result
            .map(|_| ())
            .map_err(|error| RenderError::Draw(error.to_string()))
    }
}

/// Renders a single transparent preview without creating a native window.
/// The returned RGBA pixels use straight alpha, suitable for PNG and Slint.
pub fn render_preview(
    meshes: &[Moc3DrawableMesh],
    textures: &[DecodedTexture],
    matrix: Matrix44,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    let gpu = Gpu::new(true, None)?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let renderer = WgpuLive2dRenderer::new(&gpu.device, format);
    let model = ModelGpu::new(&gpu, &renderer, meshes, textures, matrix)?;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bandori-picker.preview"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let stride = (width * 4).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bandori-picker.readback"),
        size: stride as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    model.draw_masks(&mut encoder, &renderer)?;
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bandori-picker.preview-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        model.draw(&mut pass, &renderer)?;
    }
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| RenderError::Draw(error.to_string()))?;
    rx.recv()
        .map_err(|error| RenderError::Draw(error.to_string()))?
        .map_err(|error| RenderError::Draw(error.to_string()))?;
    let mapped = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|error| RenderError::Draw(error.to_string()))?;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks(stride as usize) {
        for pixel in row[..width as usize * 4].chunks_exact(4) {
            let alpha = pixel[3] as u32;
            for channel in &pixel[..3] {
                pixels.push(
                    (*channel as u32 * 255 + alpha / 2)
                        .checked_div(alpha)
                        .unwrap_or(0)
                        .min(255) as u8,
                );
            }
            pixels.push(pixel[3]);
        }
    }
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}

/// Blit pipeline used for the downscale and opacity pass.
struct Blit {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    opacity: wgpu::Buffer,
}

impl Blit {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bandori-pet.blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/blit.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bandori-pet.blit.layout"),
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
            label: Some("bandori-pet.blit.pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bandori-pet.blit.pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_blit"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_blit"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
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
            label: Some("bandori-pet.blit.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let opacity = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bandori-pet.blit.opacity"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            bind_group_layout,
            sampler,
            opacity,
        }
    }

    fn set_opacity(&self, queue: &wgpu::Queue, opacity: f32) {
        let value = opacity.clamp(0.0, 1.0);
        queue.write_buffer(
            &self.opacity,
            0,
            bytemuck::cast_slice(&[value, 0.0f32, 0.0, 0.0]),
        );
    }

    fn bind_group(&self, device: &wgpu::Device, view: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bandori-pet.blit.bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.opacity.as_entire_binding(),
                },
            ],
        })
    }
}

/// A render-resolution-scaled intermediate target.
struct Intermediate {
    view: wgpu::TextureView,
    blit_bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

impl Intermediate {
    fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        blit: &Blit,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bandori-pet.intermediate"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let blit_bind_group = blit.bind_group(device, &view);
        Self {
            view,
            blit_bind_group,
            width: width.max(1),
            height: height.max(1),
        }
    }
}

fn encode_frame(
    encoder: &mut wgpu::CommandEncoder,
    renderer: &WgpuLive2dRenderer,
    model: &ModelGpu,
    frame_view: &wgpu::TextureView,
    intermediate: Option<&Intermediate>,
    blit: &Blit,
    overlay: Option<&overlay::FpsOverlay>,
) -> Result<(), RenderError> {
    model.draw_masks(encoder, renderer)?;

    // Draw directly to the surface when no scaling or opacity blit is needed.
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bandori-pet.main_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: intermediate.map_or(frame_view, |target| &target.view),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        model.draw(&mut pass, renderer)?;
        if let (None, Some(overlay)) = (intermediate, overlay) {
            overlay.draw(&mut pass);
        }
    }

    // Scaling/opacity path: composite the counter in the same final pass.
    if let Some(target) = intermediate {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bandori-pet.blit_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&blit.pipeline);
        pass.set_bind_group(0, &target.blit_bind_group, &[]);
        pass.draw(0..3, 0..1);
        if let Some(overlay) = overlay {
            overlay.draw(&mut pass);
        }
    }
    Ok(())
}

/// One role window's renderer: swapchain, blit, intermediate target.
pub struct SceneRenderer {
    gpu: Arc<Gpu>,
    /// Neocari pipelines for this surface's format.
    renderer: WgpuLive2dRenderer,
    /// Surface format the pipelines were built for.
    format: wgpu::TextureFormat,
    surface: Surface,
    blit: Blit,
    intermediate: Option<Intermediate>,
    quality: f32,
    opacity: f32,
    consecutive_failures: u32,
    fused: bool,
    frames: u64,
    /// FPS counter: pipeline and uploaded strip.
    overlay: overlay::FpsOverlay,
    /// Frame-rate measurement for the counter.
    meter: overlay::FpsMeter,
    /// Whether the counter is drawn at all.
    show_fps: bool,
}

impl SceneRenderer {
    /// Creates the GPU (or shares `gpu`) and configures a surface for `window`.
    pub fn new(
        window: Arc<Window>,
        vsync: bool,
        quality: f32,
        opacity: f32,
        allow_hardware: bool,
    ) -> Result<Self, RenderError> {
        // The surface has to exist before the adapter request so the adapter is
        // guaranteed to be compatible with this window. The very same surface is
        // then configured: creating a throwaway probe surface and dropping it
        // leaves dangling `SurfaceId`s inside wgpu.
        let (gpu, probe_surface) =
            Gpu::create_preferred(allow_hardware, Some(Arc::clone(&window)))?;
        let gpu = Arc::new(gpu);
        let probe_surface = probe_surface.expect("window supplied for surface creation");
        let surface = Surface::from_existing(&gpu, probe_surface, window, vsync)?;
        let format = surface.format();
        let renderer = WgpuLive2dRenderer::new(&gpu.device, format);
        let blit = Blit::new(&gpu.device, surface.format());
        blit.set_opacity(&gpu.queue, opacity);
        let overlay = overlay::FpsOverlay::create(&gpu.device, format);
        let (width, height) = surface.size();
        let intermediate = needs_intermediate(quality, opacity, surface.alpha_mode()).then(|| {
            Intermediate::new(
                &gpu.device,
                scale_dim(width, quality),
                scale_dim(height, quality),
                surface.format(),
                &blit,
            )
        });
        Ok(Self {
            gpu,
            renderer,
            format,
            surface,
            blit,
            intermediate,
            quality,
            opacity,
            consecutive_failures: 0,
            fused: false,
            frames: 0,
            overlay,
            meter: overlay::FpsMeter::default(),
            show_fps: false,
        })
    }

    /// Reuses an existing device for an additional role window.
    pub fn with_gpu(
        gpu: Arc<Gpu>,
        window: Arc<Window>,
        vsync: bool,
        quality: f32,
        opacity: f32,
    ) -> Result<Self, RenderError> {
        let surface = Surface::new(&gpu, window, vsync)?;
        // Pipelines are format specific; every role builds its own for its surface.
        let format = surface.format();
        let renderer = WgpuLive2dRenderer::new(&gpu.device, format);
        let blit = Blit::new(&gpu.device, format);
        blit.set_opacity(&gpu.queue, opacity);
        let overlay = overlay::FpsOverlay::create(&gpu.device, format);
        let (width, height) = surface.size();
        let intermediate = needs_intermediate(quality, opacity, surface.alpha_mode()).then(|| {
            Intermediate::new(
                &gpu.device,
                scale_dim(width, quality),
                scale_dim(height, quality),
                surface.format(),
                &blit,
            )
        });
        Ok(Self {
            gpu,
            renderer,
            format,
            surface,
            blit,
            intermediate,
            quality,
            opacity,
            consecutive_failures: 0,
            fused: false,
            frames: 0,
            overlay,
            meter: overlay::FpsMeter::default(),
            show_fps: false,
        })
    }

    /// The shared device.
    pub fn gpu(&self) -> &Arc<Gpu> {
        &self.gpu
    }

    /// The Live2D pipelines used by this surface.
    pub fn renderer(&self) -> &WgpuLive2dRenderer {
        &self.renderer
    }

    /// GPU info for the About page.
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.gpu.adapter_info
    }

    /// Surface format the model pipelines were built for.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Surface size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.surface.size()
    }

    /// Device pixel ratio.
    pub fn dpr(&self) -> f64 {
        self.surface.dpr()
    }

    /// Alpha mode negotiated with the compositor.
    pub fn alpha_mode(&self) -> wgpu::CompositeAlphaMode {
        self.surface.alpha_mode()
    }

    /// `true` when vsync is active.
    pub fn vsync(&self) -> bool {
        self.surface.vsync()
    }

    /// Frames presented since start-up.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// `true` when the circuit breaker tripped (rendering stays stopped).
    pub fn is_fused(&self) -> bool {
        self.fused
    }

    /// Current render-resolution scale.
    pub fn quality(&self) -> f32 {
        self.quality
    }

    /// Switches the render-resolution scale.
    pub fn set_quality(&mut self, quality: f32) {
        self.quality = quality.clamp(0.25, 1.0);
        self.resize_intermediate();
    }

    fn resize_intermediate(&mut self) {
        if !needs_intermediate(self.quality, self.opacity, self.surface.alpha_mode()) {
            self.intermediate = None;
            return;
        }
        let (width, height) = self.surface.size();
        let width = scale_dim(width, self.quality);
        let height = scale_dim(height, self.quality);
        if self
            .intermediate
            .as_ref()
            .is_some_and(|target| (target.width, target.height) == (width, height))
        {
            return;
        }
        self.intermediate = Some(Intermediate::new(
            &self.gpu.device,
            width,
            height,
            self.surface.format(),
            &self.blit,
        ));
    }

    /// Applies window opacity (multiplied into the blit).
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
        self.blit.set_opacity(&self.gpu.queue, self.opacity);
        self.resize_intermediate();
    }

    /// Shows or hides the FPS counter in the window's top-left corner.
    pub fn set_show_fps(&mut self, enabled: bool) {
        if self.show_fps == enabled {
            return;
        }
        self.show_fps = enabled;
        if enabled {
            // Restart the window so the first reading covers frames drawn with
            // the counter enabled instead of the ones before it.
            self.meter.restart(Instant::now());
        } else {
            self.overlay.clear();
        }
    }

    /// Counts a presented frame and re-uploads the counter at most twice a second.
    fn tick_show_fps(&mut self) {
        if !self.show_fps {
            return;
        }
        let Some(fps) = self.meter.record(Instant::now()) else {
            return;
        };
        let scale = overlay_scale(self.surface.dpr());
        self.overlay
            .set_text(&self.gpu, &format!("FPS {fps}"), scale);
    }

    /// Resizes the swapchain and the intermediate target.
    pub fn resize(&mut self, size: PhysicalSize<u32>, dpr: f64) {
        self.surface.resize(&self.gpu, size, dpr);
        self.resize_intermediate();
    }

    /// Re-applies the vsync preference.
    pub fn set_vsync(&mut self, vsync: bool) {
        self.surface.reconfigure(&self.gpu, vsync);
    }

    /// Uploads a freshly loaded model.
    pub fn upload_model(
        &self,
        meshes: &[Moc3DrawableMesh],
        textures: &[DecodedTexture],
        matrix: Matrix44,
    ) -> Result<ModelGpu, RenderError> {
        ModelGpu::new(&self.gpu, &self.renderer, meshes, textures, matrix)
    }

    /// Renders one frame of `model` into the swapchain.
    pub fn render(&mut self, model: &ModelGpu) -> Result<(), RenderError> {
        if self.fused {
            return Ok(());
        }
        let result = self.render_inner(model);
        match &result {
            Ok(()) => {
                self.consecutive_failures = 0;
                self.frames += 1;
                self.tick_show_fps();
            }
            Err(error) => {
                self.consecutive_failures += 1;
                warn!(%error, failures = self.consecutive_failures, "frame failed");
                if let RenderError::Acquire(reason) = error {
                    if reason == "outdated" || reason == "lost" {
                        self.surface.recreate(&self.gpu);
                    }
                }
                if self.consecutive_failures >= FAILURE_FUSE {
                    self.fused = true;
                    warn!("renderer fused after repeated failures");
                }
            }
        }
        result
    }

    fn render_inner(&mut self, model: &ModelGpu) -> Result<(), RenderError> {
        let frame = self.surface.acquire()?;
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("bandori-pet.encoder"),
            });

        let show_overlay = self.show_fps && self.overlay.is_ready();
        if show_overlay {
            let margin = overlay_margin(self.surface.dpr());
            self.overlay.prepare(
                &self.gpu.queue,
                self.surface.size(),
                (margin, margin),
                self.opacity,
            );
        }
        encode_frame(
            &mut encoder,
            &self.renderer,
            model,
            &frame_view,
            self.intermediate.as_ref(),
            &self.blit,
            show_overlay.then_some(&self.overlay),
        )?;

        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(frame);
        Ok(())
    }

    /// Intermediate target size (for the quality indicator).
    pub fn render_size(&self) -> (u32, u32) {
        let (width, height) = self.surface.size();
        (
            scale_dim(width, self.quality),
            scale_dim(height, self.quality),
        )
    }
}

fn needs_intermediate(quality: f32, opacity: f32, alpha_mode: wgpu::CompositeAlphaMode) -> bool {
    quality != 1.0 || opacity != 1.0 || alpha_mode != wgpu::CompositeAlphaMode::PreMultiplied
}

fn scale_dim(value: u32, quality: f32) -> u32 {
    ((value as f32) * quality).round().max(1.0) as u32
}

/// Bitmap scale that keeps the counter legible at this device pixel ratio.
fn overlay_scale(dpr: f64) -> u32 {
    (dpr * 2.0).round().clamp(2.0, 6.0) as u32
}

/// Distance between the window corner and the counter, in device pixels.
fn overlay_margin(dpr: f64) -> u32 {
    (dpr * 8.0).round().clamp(4.0, 24.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_mode_prefers_premultiplied() {
        assert_eq!(
            choose_alpha_mode(&[
                wgpu::CompositeAlphaMode::Opaque,
                wgpu::CompositeAlphaMode::PreMultiplied
            ]),
            wgpu::CompositeAlphaMode::PreMultiplied
        );
        assert_eq!(
            choose_alpha_mode(&[wgpu::CompositeAlphaMode::PostMultiplied]),
            wgpu::CompositeAlphaMode::PostMultiplied
        );
        assert_eq!(
            choose_alpha_mode(&[wgpu::CompositeAlphaMode::Opaque]),
            wgpu::CompositeAlphaMode::Auto
        );
    }

    #[test]
    fn present_mode_follows_vsync() {
        let modes = [wgpu::PresentMode::Fifo, wgpu::PresentMode::AutoNoVsync];
        assert_eq!(choose_present_mode(&modes, true), wgpu::PresentMode::Fifo);
        assert_eq!(
            choose_present_mode(&modes, false),
            wgpu::PresentMode::AutoNoVsync
        );
    }

    #[test]
    fn quality_scales_the_render_target() {
        assert_eq!(scale_dim(800, 0.75), 600);
        assert_eq!(scale_dim(801, 1.0), 801);
        assert_eq!(scale_dim(2, 0.25), 1);
    }

    #[test]
    fn direct_render_requires_full_resolution_opacity_and_premultiplied_surface() {
        use wgpu::CompositeAlphaMode::*;
        assert!(!needs_intermediate(1.0, 1.0, PreMultiplied));
        assert!(needs_intermediate(0.75, 1.0, PreMultiplied));
        assert!(needs_intermediate(1.0, 0.5, PreMultiplied));
        assert!(needs_intermediate(0.75, 0.5, PreMultiplied));
        for alpha in [Auto, Opaque, PostMultiplied, Inherit] {
            assert!(needs_intermediate(1.0, 1.0, alpha));
        }
    }
}
