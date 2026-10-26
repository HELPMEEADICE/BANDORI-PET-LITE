//! Orientation regression test (Cubism 2 y-down vs Cubism 3+ y-up).
//!
//! Neocari documents the two conventions explicitly:
//! `moc2::layout` — *"Cubism 2 model coordinates are canvas pixels with the origin at
//! the top-left corner and the y axis pointing down"* — and its `render_moc2` example
//! compensates with `matrix.scale(scale_x, -scale_y)`, while the moc3 viewer uses a
//! positive scale. Getting this wrong draws the character upside down and drags the
//! seven click regions along with it.
//!
//! The test measures the coordinate space of a real model instead of trusting a flag:
//! * `.moc` bounds must live in canvas-pixel space (non-negative, inside the canvas),
//! * `.moc3` bounds must be centred around the origin (y-up model space).
//!
//! Run with real models:
//! ```powershell
//! $env:BANDORI_PET_TEST_MODELS = "C:\oss\BANDORI-PET-REV\models"
//! cargo test -p pet-app --test orientation -- --nocapture
//! ```

use pet_assets::ModelCache;
use pet_live2d::ModelHandle;
use pet_window::sizing::{content_size, fit_matrix, Bounds};

fn load(spec: &str) -> Option<(ModelHandle, Bounds)> {
    let models = std::path::PathBuf::from(std::env::var("BANDORI_PET_TEST_MODELS").ok()?);
    if !models.is_dir() {
        return None;
    }
    let cache = ModelCache::new(models.join(".cache-orientation"), &models);
    let resolved = cache.resolve(spec).ok()?;
    let model = ModelHandle::load(&resolved.model_path).ok()?;
    let bounds = content_size(model.meshes())?;
    Some((model, bounds))
}

#[test]
fn moc_lives_in_a_y_down_canvas_and_needs_a_flip() {
    let Some((model, bounds)) = load("models/kasumi.zst::live_default/model.json") else {
        eprintln!("skipped: set BANDORI_PET_TEST_MODELS to a directory with kasumi.zst");
        return;
    };
    let canvas_height = model.info().canvas_height;
    eprintln!(
        "moc bounds=({:.1},{:.1})..({:.1},{:.1}) canvas={}x{}",
        bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y,
        model.info().canvas_width, canvas_height
    );

    assert!(model.flips_y(), "Cubism 2 must request a y flip");
    // Canvas-pixel space: everything is positive and inside the canvas.
    assert!(bounds.min_y >= 0.0, "moc y should start at 0 (top-left origin)");
    assert!(
        bounds.max_y <= canvas_height as f32 * 1.2,
        "moc y should stay inside the canvas ({} > {})",
        bounds.max_y,
        canvas_height
    );
    assert!(bounds.min_x >= 0.0);

    // With the flip, the *small* model y (the top of the artwork, i.e. the head)
    // lands at the top of the window: clip y ≈ +1.
    let matrix = fit_matrix(bounds, 1.0, true);
    let head_side = matrix.transform_y(bounds.min_y);
    let feet_side = matrix.transform_y(bounds.max_y);
    eprintln!("flipped: y={:.1} -> clip {head_side:.3}; y={:.1} -> clip {feet_side:.3}", bounds.min_y, bounds.max_y);
    assert!(
        head_side > 0.9 && feet_side < -0.9,
        "flip must put the top of the model at the top of the window"
    );

    // Without the flip the model is upside down — this is the bug being guarded.
    let wrong = fit_matrix(bounds, 1.0, false);
    assert!(wrong.transform_y(bounds.min_y) < 0.0);
}

#[test]
fn moc3_is_centred_and_y_up() {
    let Some((model, bounds)) = load(
        "models/anon.zst::casual_spring_01/adv_live2d_anon_002_casual_spring_01.model3.json",
    ) else {
        eprintln!("skipped: set BANDORI_PET_TEST_MODELS");
        return;
    };
    eprintln!(
        "moc3 bounds=({:.2},{:.2})..({:.2},{:.2})",
        bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
    );
    assert!(!model.flips_y(), "Cubism 3+ is y-up, no flip");
    assert!(
        bounds.min_x < 0.0 && bounds.min_y < 0.0,
        "moc3 model space is centred around the origin"
    );
    let matrix = fit_matrix(bounds, 1.0, false);
    assert!(matrix.transform_y(bounds.max_y) > 0.0);
    assert!(matrix.transform_y(bounds.min_y) < 0.0);
}
