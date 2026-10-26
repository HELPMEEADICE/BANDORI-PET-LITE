//! Opt-in comparison of GPU allocation policies at unchanged render resolution.
#![cfg(windows)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use pet_app::role::Role;
use pet_assets::ModelCache;
use pet_config::{Config, ModelEntry, ModelFormat, Paths};
use pet_render::Gpu;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::WindowId;

struct Comparison;

impl ApplicationHandler for Comparison {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let root = std::env::var_os("BANDORI_PET_TEST_ROOT").expect("set BANDORI_PET_TEST_ROOT");
        let paths = Paths::with_root(root);
        let cache = ModelCache::new(paths.cache(), paths.models());
        let entry = ModelEntry::new("anon", "school_winter_hs_1st",
            "models/anon.zst::school_winter_hs_1st/adv_live2d_anon_002_school_winter_hs_1st.model3.json",
            ModelFormat::Moc3);
        let mut config = Config::default();
        // Uncap both runs to expose any throughput loss; no config is saved.
        config.vsync = false;
        config.emotion_behavior_enabled = false;
        for memory_hints in [
            wgpu::MemoryHints::Performance,
            wgpu::MemoryHints::MemoryUsage,
        ] {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    ..Default::default()
                }))
                .unwrap();
            let adapter_info = adapter.get_info();
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    memory_hints: memory_hints.clone(),
                    ..Default::default()
                }))
                .unwrap();
            let gpu = Arc::new(Gpu {
                instance,
                adapter,
                device,
                queue,
                adapter_info,
            });
            let mut role =
                Role::create(event_loop, &entry, &config, &paths, &cache, Some(gpu)).unwrap();
            assert!(role.model.textures().is_empty());
            assert_eq!(
                role.model_gpu.texture_count(),
                role.model.info().texture_count
            );
            assert_eq!(role.renderer.render_size(), role.renderer.size());
            let group = role.model.info().default_motion_group().unwrap().to_owned();
            assert!(role.model.play_motion(&group));
            for _ in 0..30 {
                role.advance_model(1.0 / 120.0);
                assert!(role.render());
            }
            let first = role.renderer.frames();
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(4) {
                role.advance_model(1.0 / 120.0);
                assert!(role.render());
            }
            let frames = role.renderer.frames() - first;
            eprintln!(
                "{memory_hints:?}: {}x{}, {frames} frames, {:.1} FPS (uncapped)",
                role.renderer.render_size().0,
                role.renderer.render_size().1,
                frames as f64 / started.elapsed().as_secs_f64()
            );
        }
        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

#[test]
#[ignore = "requires anon.zst and a GPU; briefly creates native windows"]
fn compare_gpu_allocator_throughput() {
    let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
    event_loop.run_app(&mut Comparison).unwrap();
}
