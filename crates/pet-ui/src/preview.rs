//! Cached static costume previews, rendered entirely offscreen.
use pet_config::Paths;
use std::path::PathBuf;

pub(super) fn render(paths: &Paths, virtual_path: &str) -> Result<PathBuf, String> {
    let cache =
        pet_assets::cache::ModelCache::new(paths.cache().join("picker-models"), paths.models());
    let extracted = cache
        .resolve(virtual_path)
        .map_err(|error| error.to_string())?;
    let output = extracted.costume_dir.join(".bandori-picker-preview-v1.png");
    if output.exists() {
        return Ok(output);
    }
    let mut model =
        pet_live2d::ModelHandle::load(&extracted.model_path).map_err(|error| error.to_string())?;
    model.update(1.0 / 60.0);
    let bounds = pet_window::sizing::content_size(model.meshes()).ok_or("模型没有可预览的网格")?;
    let width = 384;
    let height = (width as f32 / bounds.aspect()).round().clamp(128.0, 768.0) as u32;
    let matrix = pet_window::sizing::fit_matrix(bounds, 0.95, model.flips_y());
    let pixels =
        pet_render::render_preview(model.meshes(), model.textures(), matrix, width, height)
            .map_err(|error| error.to_string())?;
    image::save_buffer(&output, &pixels, width, height, image::ColorType::Rgba8)
        .map_err(|error| error.to_string())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a local model archive and GPU; creates no native windows"]
    fn renders_local_costume_offscreen() {
        let paths = Paths::with_root(
            std::env::var("BANDORI_UI_PREVIEW_ROOT").expect("set BANDORI_UI_PREVIEW_ROOT"),
        );
        let model =
            std::env::var("BANDORI_UI_PREVIEW_MODEL").expect("set BANDORI_UI_PREVIEW_MODEL");
        let output = render(&paths, &model).unwrap();
        let image = image::open(&output).unwrap().to_rgba8();
        assert_eq!(image.width(), 384);
        let visible = image.pixels().filter(|pixel| pixel[3] > 16).count();
        assert!(visible > 1000, "preview must contain the model");
        assert!(
            visible < (image.width() * image.height()) as usize,
            "preview must retain transparency"
        );
        if let Ok(directory) = std::env::var("BANDORI_UI_QA_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            image
                .save(PathBuf::from(directory).join("costume-preview.png"))
                .unwrap();
        }
    }
}
