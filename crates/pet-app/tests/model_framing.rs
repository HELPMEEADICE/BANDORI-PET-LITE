//! Optional real-asset regression. Set BANDORI_PET_TEST_MODELS to models/.

use pet_assets::ModelCache;
use pet_live2d::ModelHandle;
use pet_window::sizing::{content_size, ModelLayout, BASE_FACE_HEIGHT};

#[test]
fn real_models_share_face_scale_and_have_room_for_motions() {
    let Some(models) = std::env::var_os("BANDORI_PET_TEST_MODELS").map(std::path::PathBuf::from)
    else {
        eprintln!("skipped: set BANDORI_PET_TEST_MODELS to exercise real models");
        return;
    };
    let cache = ModelCache::new(models.join(".cache-framing"), &models);
    for spec in [
        "models/kasumi.zst::live_default/model.json",
        "models/anon.zst::casual_spring_01/adv_live2d_anon_002_casual_spring_01.model3.json",
    ] {
        let resolved = cache.resolve(spec).unwrap();
        let mut model = ModelHandle::load(&resolved.model_path).unwrap();
        let face = model
            .face_height()
            .expect("Bandori face geometry must be identified");
        let layout = ModelLayout::new(
            content_size(model.meshes()).unwrap(),
            Some(face),
            model.format(),
        );
        let matrix = layout.matrix(model.flips_y());
        let (width, height) = layout.window_size(100.0, 1.0);
        let face_pixels =
            (matrix.transform_y(face) - matrix.transform_y(0.0)).abs() * height as f32 / 2.0;
        assert!((face_pixels - BASE_FACE_HEIGHT).abs() < 0.5);
        eprintln!("{spec}: window={width}x{height}, face={face_pixels:.2}px");
        if let Some(output) = std::env::var_os("BANDORI_PET_FRAMING_SHOTS") {
            let output = std::path::PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            let pixels =
                pet_render::render_preview(model.meshes(), model.textures(), matrix, width, height)
                    .unwrap();
            let name = if model.flips_y() { "moc" } else { "moc3" };
            image::save_buffer(
                output.join(format!("{name}.png")),
                &pixels,
                width,
                height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        // A desktop role uploads the original pixels, then retains only alpha.
        // Both runtimes must keep animating after their CPU textures are gone.
        let texture_count = model.info().texture_count;
        let textures = model.take_textures();
        assert_eq!(textures.len(), texture_count);
        let rgba_bytes: usize = textures.iter().map(|texture| texture.rgba().len()).sum();
        let alpha_bytes: usize = textures
            .into_iter()
            .map(pet_input::hit::AlphaTexture::from)
            .map(|texture| texture.byte_len())
            .sum();
        assert!(alpha_bytes <= rgba_bytes / 4);
        assert!(model.textures().is_empty());
        assert!(model.take_textures().is_empty());
        assert_eq!(model.info().texture_count, texture_count);
        eprintln!("{spec}: CPU texture storage {rgba_bytes} RGBA bytes -> {alpha_bytes} alpha bytes (previous desktop code also held a full RGBA clone)");
        let groups = model.info().motion_groups.clone();
        for group in groups {
            model.stop_motions();
            assert!(model.play_motion(&group));
            // Exercise six seconds per motion, including start/end transitions,
            // physics and extreme cursor tracking, without creating a window.
            for frame in 0..120 {
                model.set_drag(
                    if frame % 40 < 20 { -1.0 } else { 1.0 },
                    if frame % 60 < 30 { -1.0 } else { 1.0 },
                );
                assert!(model.update(0.05));
                for mesh in model.meshes().iter().filter(|mesh| mesh.opacity() > 0.01) {
                    for vertex in mesh.vertices() {
                        let [x, y] = vertex.position();
                        assert!(matrix.transform_x(x).abs() <= 1.001 && matrix.transform_y(y).abs() <= 1.001,
                            "{spec} motion={group} frame={frame}: vertex ({x}, {y}) outside animation viewport");
                    }
                }
            }
        }
    }
}
