//! Headless GPU regression coverage; CI uses Mesa's software Vulkan adapter.

use neocari::moc3::Moc3DrawableVertex;

use super::*;

const SIZE: u32 = 128;

fn quad(
    texture: i32,
    flags: u8,
    opacity: f32,
    order: f32,
    x: f32,
    masks: Vec<i32>,
) -> Moc3DrawableMesh {
    Moc3DrawableMesh::from_parts(
        texture,
        flags,
        opacity,
        order,
        vec![
            Moc3DrawableVertex::new([-0.75 + x, -0.7], [0.0, 1.0]),
            Moc3DrawableVertex::new([0.65 + x, -0.7], [1.0, 1.0]),
            Moc3DrawableVertex::new([0.65 + x, 0.8], [1.0, 0.0]),
            Moc3DrawableVertex::new([-0.75 + x, 0.8], [0.0, 0.0]),
        ],
        vec![0, 1, 2, 0, 2, 3],
        masks,
    )
}

fn meshes(frame: usize, moc2: bool) -> Vec<Moc3DrawableMesh> {
    let mut meshes = vec![
        quad(0, 0, 0.5, 0.0, 0.0, vec![]),
        quad(
            1,
            0,
            if frame == 2 { 0.0 } else { 0.8 },
            1.0,
            frame as f32 * 0.02,
            vec![0],
        ),
        // Screen is specific to moc; moc3 uses normal/additive/multiplicative.
        quad(1, if moc2 { 4 } else { 0 }, 0.3, 2.0, -0.1, vec![]),
        quad(
            1,
            8,
            0.7,
            3.0,
            0.1,
            if frame >= 4 { vec![0, 2] } else { vec![0] },
        ),
        quad(0, 1, 0.2, 4.0, -0.2, vec![]),
        quad(1, 2, 0.6, 5.0, 0.15, vec![]),
    ];
    if frame >= 5 {
        meshes[1] = Moc3DrawableMesh::from_parts(
            1,
            0,
            0.8,
            1.0,
            meshes[1].vertices().to_vec(),
            vec![0, 1, 2],
            vec![0],
        );
    }
    if frame >= 6 {
        meshes.push(quad(0, 0, 0.2, -1.0, 0.0, vec![]));
    }
    meshes
}

fn pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("render-regression"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

struct Harness<'a> {
    gpu: &'a Gpu,
    renderer: WgpuLive2dRenderer,
    format: wgpu::TextureFormat,
    blit: Blit,
    overlay: overlay::FpsOverlay,
}

impl<'a> Harness<'a> {
    fn new(gpu: &'a Gpu, format: wgpu::TextureFormat) -> Self {
        let mut overlay = overlay::FpsOverlay::create(&gpu.device, format);
        overlay.set_text(gpu, "FPS 60", 2);
        Self {
            gpu,
            renderer: WgpuLive2dRenderer::new(&gpu.device, format),
            format,
            blit: Blit::new(&gpu.device, format),
            overlay,
        }
    }

    fn pixels(
        &self,
        model: &ModelGpu,
        quality: f32,
        opacity: f32,
        show_fps: bool,
        reference: bool,
    ) -> Vec<u8> {
        let gpu = self.gpu;
        let size = wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        };
        let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("regression.output"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        self.blit.set_opacity(&gpu.queue, opacity);
        self.overlay
            .prepare(&gpu.queue, (SIZE, SIZE), (4, 4), opacity);
        let intermediate = (reference
            || needs_intermediate(quality, opacity, wgpu::CompositeAlphaMode::PreMultiplied))
        .then(|| {
            Intermediate::new(
                &gpu.device,
                scale_dim(SIZE, quality),
                scale_dim(SIZE, quality),
                self.format,
                &self.blit,
            )
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        if reference {
            // Original path: fresh clipping resources, unconditional mask pass,
            // intermediate/blit, then an independent FPS pass.
            {
                let mut pass = pass(
                    &mut encoder,
                    model.mask_target.view(),
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
                self.renderer
                    .draw_masks_with_textures(
                        &mut pass,
                        &model.mesh_buffers,
                        &model.clipping,
                        &model.textures,
                    )
                    .unwrap();
            }
            let target = intermediate.as_ref().unwrap();
            {
                let mut pass = pass(
                    &mut encoder,
                    &target.view,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
                self.renderer
                    .draw_with_textures_clipping_and_transform(
                        &mut pass,
                        &model.mesh_buffers,
                        &model.textures,
                        &model.clipping,
                        &model.mask_target,
                        &model.transform,
                    )
                    .unwrap();
            }
            {
                let mut pass = pass(
                    &mut encoder,
                    &view,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
                pass.set_pipeline(&self.blit.pipeline);
                pass.set_bind_group(0, &target.blit_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if show_fps {
                self.overlay
                    .draw(&mut pass(&mut encoder, &view, wgpu::LoadOp::Load));
            }
        } else {
            encode_frame(
                &mut encoder,
                &self.renderer,
                model,
                &view,
                intermediate.as_ref(),
                &self.blit,
                show_fps.then_some(&self.overlay),
            )
            .unwrap();
        }
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("regression.readback"),
            size: u64::from(SIZE * SIZE * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: Some(SIZE),
                },
            },
            size,
        );
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        mapped.to_vec()
    }
}

#[test]
#[ignore = "requires a Vulkan software adapter; run by Linux CI"]
fn gpu_render_regressions() {
    let (gpu, _) = Gpu::create_on_backends(false, None, wgpu::Backends::VULKAN).unwrap();
    let textures = [
        DecodedTexture::new(
            2,
            2,
            vec![
                240, 20, 60, 255, 80, 230, 40, 0, 20, 70, 240, 128, 220, 150, 30, 220,
            ],
        ),
        DecodedTexture::new(
            2,
            2,
            vec![
                40, 170, 250, 190, 220, 60, 90, 255, 190, 220, 30, 80, 70, 240, 160, 230,
            ],
        ),
    ];
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        let harness = Harness::new(&gpu, format);
        for moc2 in [true, false] {
            let mut matrix = Matrix44::identity();
            if moc2 {
                matrix.scale(1.0, -1.0);
            }
            let mut model =
                ModelGpu::new(&gpu, &harness.renderer, &meshes(0, moc2), &textures, matrix)
                    .unwrap();
            for frame in 0..7 {
                let meshes = meshes(frame, moc2);
                let context_storage = model.clipping_plan.contexts().as_ptr();
                let membership_storage = model.clipping_plan.contexts()[0]
                    .drawable_indices()
                    .as_ptr();
                model
                    .update_meshes(&gpu, &harness.renderer, &meshes)
                    .unwrap();
                let reference =
                    ModelGpu::new(&gpu, &harness.renderer, &meshes, &textures, matrix).unwrap();
                assert_eq!(
                    model.clipping_plan, reference.clipping_plan,
                    "frame {frame}"
                );
                if frame == 1 {
                    assert_eq!(context_storage, model.clipping_plan.contexts().as_ptr());
                    assert_eq!(
                        membership_storage,
                        model.clipping_plan.contexts()[0]
                            .drawable_indices()
                            .as_ptr()
                    );
                }
                for (quality, opacity, fps) in [
                    (1.0, 1.0, false),
                    (1.0, 1.0, true),
                    (0.75, 0.6, true),
                    (1.0, 0.4, false),
                ] {
                    let expected = harness.pixels(&reference, quality, opacity, fps, true);
                    let actual = harness.pixels(&model, quality, opacity, fps, false);
                    assert!(actual.chunks_exact(4).any(|p| p[3] > 0));
                    assert!(actual.chunks_exact(4).any(|p| p[3] == 0));
                    let max_error = actual
                        .iter()
                        .zip(&expected)
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap();
                    assert!(max_error <= 1, "{format:?}, moc2={moc2}, frame={frame}, quality={quality}, opacity={opacity}, fps={fps}: max error {max_error}");
                }
            }
            // Removing all masks, hiding the only clipped mesh, and showing it
            // again must switch paths without sampling an old mask atlas.
            for (masks, opacity) in [(vec![], 0.8), (vec![0], 0.0), (vec![0], 0.8)] {
                let meshes = vec![
                    quad(0, 0, 0.5, 0.0, 0.0, vec![]),
                    quad(1, 0, opacity, 1.0, 0.0, masks),
                ];
                model
                    .update_meshes(&gpu, &harness.renderer, &meshes)
                    .unwrap();
                let reference =
                    ModelGpu::new(&gpu, &harness.renderer, &meshes, &textures, matrix).unwrap();
                assert_eq!(model.clipping_plan, reference.clipping_plan);
                let actual = harness.pixels(&model, 1.0, 1.0, true, false);
                let expected = harness.pixels(&reference, 1.0, 1.0, true, true);
                assert!(actual
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.abs_diff(*b) <= 1));
            }
        }
    }
}
