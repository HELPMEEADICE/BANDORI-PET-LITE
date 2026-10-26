//! Window sizing from model content bounds (spec §2.1, §2.6).
//!
//! The original used a fixed 400×500 base and then patched moc3 with an 800 px
//! virtual canvas plus leg cropping. LITE derives the window from the actual
//! content AABB instead, so those two options disappear.

use neocari::core::Matrix44;
use neocari::moc3::Moc3DrawableMesh;

/// Model-space axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Minimum x.
    pub min_x: f32,
    /// Minimum y.
    pub min_y: f32,
    /// Maximum x.
    pub max_x: f32,
    /// Maximum y.
    pub max_y: f32,
}

impl Bounds {
    /// Width in model units.
    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    /// Height in model units.
    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    /// Center x.
    pub fn center_x(&self) -> f32 {
        (self.min_x + self.max_x) * 0.5
    }

    /// Center y.
    pub fn center_y(&self) -> f32 {
        (self.min_y + self.max_y) * 0.5
    }

    /// Aspect ratio (width / height).
    pub fn aspect(&self) -> f32 {
        if self.height().abs() < 1e-6 {
            1.0
        } else {
            self.width() / self.height()
        }
    }
}

/// Logical window height at 100% scale (model is fitted to this).
pub const BASE_LOGICAL_HEIGHT: f32 = 500.0;

/// Computes the content AABB of a model's meshes.
pub fn content_size(meshes: &[Moc3DrawableMesh]) -> Option<Bounds> {
    let mut bounds: Option<Bounds> = None;
    for vertex in meshes.iter().flat_map(|mesh| mesh.vertices()) {
        let [x, y] = vertex.position();
        bounds = Some(match bounds {
            Some(current) => Bounds {
                min_x: current.min_x.min(x),
                min_y: current.min_y.min(y),
                max_x: current.max_x.max(x),
                max_y: current.max_y.max(y),
            },
            None => Bounds {
                min_x: x,
                min_y: y,
                max_x: x,
                max_y: y,
            },
        });
    }
    bounds.filter(|bounds| bounds.width() > 1e-4 && bounds.height() > 1e-4)
}

/// Window size (physical pixels) for a model at `scale_percent` and `dpr`.
pub fn window_size_for_model(bounds: Bounds, scale_percent: f32, dpr: f64) -> (u32, u32) {
    let scale = (scale_percent / 100.0).clamp(0.25, 5.0) as f64;
    let logical_height = BASE_LOGICAL_HEIGHT as f64 * scale;
    let logical_width = (logical_height * bounds.aspect() as f64).max(64.0);
    let width = (logical_width * dpr).round().clamp(64.0, 8192.0) as u32;
    let height = (logical_height * dpr).round().clamp(64.0, 8192.0) as u32;
    (width, height)
}

/// Builds the model → clip transform that fits `bounds` exactly into the window.
///
/// Unlike the Neocari example this has no view fill factor: the window is already
/// sized to the content, so `1.0` fills it.
///
/// `flip_y` is **not** a preference: Cubism 2 model coordinates put the origin at
/// the top-left corner with y pointing down, while Cubism 3+ points y up. Neocari's
/// own `render_moc2` example compensates with `matrix.scale(scale_x, -scale_y)`,
/// and `moc2::layout` documents the same thing. Passing the wrong value renders the
/// character upside down (and drags its click regions along with it).
pub fn fit_matrix(bounds: Bounds, view_fill: f32, flip_y: bool) -> Matrix44 {
    let view_fill = view_fill.clamp(0.1, 4.0);
    let scale_x = 2.0 * view_fill / bounds.width();
    let scale_y = 2.0 * view_fill / bounds.height();
    let signed_scale_y = if flip_y { -scale_y } else { scale_y };
    let mut matrix = Matrix44::identity();
    matrix.scale(scale_x, signed_scale_y);
    matrix.translate(
        -bounds.center_x() * scale_x,
        -bounds.center_y() * signed_scale_y,
    );
    matrix
}

#[cfg(test)]
mod tests {
    use super::*;
    use neocari::moc3::Moc3DrawableVertex;

    fn quad(min: (f32, f32), max: (f32, f32)) -> Moc3DrawableMesh {
        Moc3DrawableMesh::from_parts(
            0,
            0,
            1.0,
            0.0,
            vec![
                Moc3DrawableVertex::new([min.0, max.1], [0.0, 0.0]),
                Moc3DrawableVertex::new([max.0, max.1], [1.0, 0.0]),
                Moc3DrawableVertex::new([max.0, min.1], [1.0, 1.0]),
                Moc3DrawableVertex::new([min.0, min.1], [0.0, 1.0]),
            ],
            vec![0, 1, 2, 0, 2, 3],
            Vec::new(),
        )
    }

    #[test]
    fn bounds_cover_every_vertex() {
        let meshes = vec![quad((-1.0, -1.5), (1.0, 1.5)), quad((0.0, 0.0), (0.5, 0.5))];
        let bounds = content_size(&meshes).unwrap();
        assert_eq!(bounds.min_y, -1.5);
        assert_eq!(bounds.max_x, 1.0);
    }

    #[test]
    fn window_size_tracks_aspect_and_dpr() {
        let bounds = Bounds {
            min_x: -0.5,
            min_y: -0.75,
            max_x: 0.5,
            max_y: 0.75,
        };
        let (width, height) = window_size_for_model(bounds, 100.0, 1.0);
        assert_eq!(height, 500);
        assert!((width as f32 - 500.0 * bounds.aspect()).abs() < 1.0);
        let (scaled_width, scaled_height) = window_size_for_model(bounds, 200.0, 1.5);
        assert_eq!(scaled_height, 1500);
        assert!((scaled_width as f32 - 1500.0 * bounds.aspect()).abs() < 1.0);
    }

    #[test]
    fn flip_matrix_puts_model_down_y_at_the_bottom() {
        // Cubism 2 space: head at y = 0.2 (near the top-left origin), feet at y = 1.8.
        let bounds = Bounds {
            min_x: -1.0,
            min_y: 0.2,
            max_x: 1.0,
            max_y: 1.8,
        };
        let flipped = fit_matrix(bounds, 1.0, true);
        // Larger model y must map to a *lower* clip y when flipping.
        assert!(flipped.transform_y(1.8) < flipped.transform_y(0.2));
        assert!((flipped.transform_y(1.8) + 1.0).abs() < 1e-6);
        assert!((flipped.transform_y(0.2) - 1.0).abs() < 1e-6);
        let straight = fit_matrix(bounds, 1.0, false);
        assert!(straight.transform_y(1.8) > straight.transform_y(0.2));
    }

    #[test]
    fn fit_matrix_maps_content_to_full_clip_space() {
        let bounds = Bounds {
            min_x: -1.0,
            min_y: -2.0,
            max_x: 1.0,
            max_y: 2.0,
        };
        let matrix = fit_matrix(bounds, 1.0, false);
        // Corners map to the clip-space corners.
        assert!((matrix.transform_x(-1.0) + 1.0).abs() < 1e-6);
        assert!((matrix.transform_x(1.0) - 1.0).abs() < 1e-6);
        assert!((matrix.transform_y(-2.0) + 1.0).abs() < 1e-6);
        assert!((matrix.transform_y(2.0) - 1.0).abs() < 1e-6);
        // Inverse round trip.
        let x = matrix.invert_transform_x(0.25);
        assert!((matrix.transform_x(x) - 0.25).abs() < 1e-6);
    }
}

