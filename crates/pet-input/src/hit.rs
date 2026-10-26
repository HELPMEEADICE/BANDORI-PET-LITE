//! Pure-CPU hit testing (spec §2.4).
//!
//! The wgpu surface texture cannot be read back, so LITE classifies the cursor the
//! same way the original did for its alpha mask, but entirely on the CPU:
//!
//! 1. invert the fit transform to get a model-space point,
//! 2. test the point against each drawable's AABB, then its triangles,
//! 3. interpolate the UV with barycentric weights and sample the decoded PNG
//!    alpha — a hit requires `alpha > threshold`,
//! 4. keep a 120 ms / 12 px stability window so animation frames do not make the
//!    window flicker between "hit" and "click-through".

use std::sync::Arc;

use neocari::assets::DecodedTexture;
use neocari::core::Matrix44;
use neocari::moc3::Moc3DrawableMesh;

use crate::region::Bounds;

/// Original-resolution, lossless alpha used by CPU hit testing. RGB pixels are
/// needed only for GPU upload and are not retained by the interaction code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlphaTexture {
    width: u32,
    height: u32,
    alpha: Vec<u8>,
}

impl AlphaTexture {
    /// Copies every original alpha byte, without resampling or thresholding.
    pub fn from_texture(texture: &DecodedTexture) -> Self {
        Self {
            width: texture.width(),
            height: texture.height(),
            alpha: texture
                .rgba()
                .chunks_exact(4)
                .map(|pixel| pixel[3])
                .collect(),
        }
    }

    /// Bytes retained for hit testing (one byte per original pixel).
    pub fn byte_len(&self) -> usize {
        self.alpha.len()
    }
}

impl From<DecodedTexture> for AlphaTexture {
    fn from(texture: DecodedTexture) -> Self {
        Self::from_texture(&texture)
    }
}

/// One vertex of a hit-testable triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitVertex {
    /// Model-space position.
    pub position: [f32; 2],
    /// Texture coordinates.
    pub uv: [f32; 2],
}

/// One drawable prepared for CPU hit testing.
#[derive(Debug, Clone)]
pub struct HitDrawable {
    /// Index into the texture array.
    pub texture_index: usize,
    /// Drawable opacity (`0.0` when hidden by a part fade).
    pub opacity: f32,
    /// Model-space bounding box.
    pub bounds: Bounds,
    /// Vertices.
    pub vertices: Vec<HitVertex>,
    /// Triangle indices.
    pub indices: Vec<u16>,
}

/// Immutable per-frame snapshot shared between the render and interaction threads.
#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    /// Model-space → clip-space matrix of the last frame.
    pub matrix: Matrix44,
    /// Model-space bounding box of all vertices.
    pub content_bounds: Bounds,
    /// Drawables in model order.
    pub drawables: Vec<HitDrawable>,
    /// Original alpha channels (shared, never mutated).
    pub textures: Arc<Vec<AlphaTexture>>,
}

/// Result of one hit test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitResult {
    /// Whether the cursor landed on opaque pixels.
    pub hit: bool,
    /// Model-space point under the cursor.
    pub model_point: (f32, f32),
    /// Drawable that produced the hit, if any.
    pub drawable_index: Option<usize>,
}

impl FrameSnapshot {
    /// Builds a snapshot from the meshes of the last rendered frame.
    pub fn new(
        meshes: &[Moc3DrawableMesh],
        textures: Arc<Vec<AlphaTexture>>,
        matrix: Matrix44,
    ) -> Self {
        let mut drawables = Vec::with_capacity(meshes.len());
        let mut content_bounds: Option<Bounds> = None;
        for mesh in meshes {
            if mesh.vertices().is_empty() || mesh.indices().len() < 3 {
                continue;
            }
            let mut bounds: Option<Bounds> = None;
            let vertices: Vec<HitVertex> = mesh
                .vertices()
                .iter()
                .map(|vertex| {
                    let position = vertex.position();
                    bounds = Some(match bounds {
                        Some(bounds) => Bounds {
                            min_x: bounds.min_x.min(position[0]),
                            min_y: bounds.min_y.min(position[1]),
                            max_x: bounds.max_x.max(position[0]),
                            max_y: bounds.max_y.max(position[1]),
                        },
                        None => Bounds::from_corners(
                            (position[0], position[1]),
                            (position[0], position[1]),
                        ),
                    });
                    HitVertex {
                        position,
                        uv: vertex.uv(),
                    }
                })
                .collect();
            let Some(bounds) = bounds else {
                continue;
            };
            content_bounds = Some(match content_bounds {
                Some(existing) => Bounds {
                    min_x: existing.min_x.min(bounds.min_x),
                    min_y: existing.min_y.min(bounds.min_y),
                    max_x: existing.max_x.max(bounds.max_x),
                    max_y: existing.max_y.max(bounds.max_y),
                },
                None => bounds,
            });
            if mesh.opacity() <= 0.01 {
                continue;
            }
            drawables.push(HitDrawable {
                texture_index: usize::try_from(mesh.texture_index()).unwrap_or(0),
                opacity: mesh.opacity(),
                bounds,
                vertices,
                indices: mesh.indices().to_vec(),
            });
        }

        Self {
            matrix,
            content_bounds: content_bounds.unwrap_or(Bounds::from_corners((0.0, 0.0), (1.0, 1.0))),
            drawables,
            textures,
        }
    }

    /// Maps a physical-pixel cursor position to a model-space point.
    pub fn cursor_to_model(&self, cursor: (f32, f32), surface: (u32, u32)) -> Option<(f32, f32)> {
        if surface.0 == 0 || surface.1 == 0 {
            return None;
        }
        let clip_x = (cursor.0 / surface.0 as f32) * 2.0 - 1.0;
        let clip_y = 1.0 - (cursor.1 / surface.1 as f32) * 2.0;
        Some((
            self.matrix.invert_transform_x(clip_x),
            self.matrix.invert_transform_y(clip_y),
        ))
    }

    /// Tests one physical-pixel cursor position.
    pub fn hit_test(
        &self,
        cursor: (f32, f32),
        surface: (u32, u32),
        alpha_threshold: u8,
    ) -> HitResult {
        let Some(point) = self.cursor_to_model(cursor, surface) else {
            return HitResult {
                hit: false,
                model_point: (0.0, 0.0),
                drawable_index: None,
            };
        };
        let threshold = alpha_threshold as u16;
        for (index, drawable) in self.drawables.iter().enumerate() {
            if point.0 < drawable.bounds.min_x
                || point.0 > drawable.bounds.max_x
                || point.1 < drawable.bounds.min_y
                || point.1 > drawable.bounds.max_y
            {
                continue;
            }
            if let Some((uv, coverage)) = self.triangle_uv(drawable, point) {
                let texture = self.textures.get(drawable.texture_index);
                let alpha = texture.map_or(255, |texture| sample_alpha(texture, uv));
                let scaled = alpha as f32 / 255.0;
                if (scaled * drawable.opacity * coverage) * 255.0 > threshold as f32 {
                    return HitResult {
                        hit: true,
                        model_point: point,
                        drawable_index: Some(index),
                    };
                }
            }
        }
        HitResult {
            hit: false,
            model_point: point,
            drawable_index: None,
        }
    }

    fn triangle_uv(&self, drawable: &HitDrawable, point: (f32, f32)) -> Option<([f32; 2], f32)> {
        for triangle in drawable.indices.chunks_exact(3) {
            let a = drawable.vertices.get(triangle[0] as usize)?;
            let b = drawable.vertices.get(triangle[1] as usize)?;
            let c = drawable.vertices.get(triangle[2] as usize)?;
            if let Some((u, v, w)) = barycentric(point, a.position, b.position, c.position) {
                let uv = [
                    a.uv[0] * u + b.uv[0] * v + c.uv[0] * w,
                    a.uv[1] * u + b.uv[1] * v + c.uv[1] * w,
                ];
                return Some((uv, 1.0));
            }
        }
        None
    }
}

/// Barycentric coordinates of `point` inside triangle `a,b,c` (any winding).
fn barycentric(
    point: (f32, f32),
    a: [f32; 2],
    b: [f32; 2],
    c: [f32; 2],
) -> Option<(f32, f32, f32)> {
    let v0 = (c[0] - a[0], c[1] - a[1]);
    let v1 = (b[0] - a[0], b[1] - a[1]);
    let v2 = (point.0 - a[0], point.1 - a[1]);
    let den = v0.0 * v1.1 - v1.0 * v0.1;
    if den.abs() < 1e-9 {
        return None;
    }
    let v = (v2.0 * v1.1 - v1.0 * v2.1) / den;
    let w = (v0.0 * v2.1 - v2.0 * v0.1) / den;
    let u = 1.0 - v - w;
    const EPSILON: f32 = -1e-4;
    if u < EPSILON || v < EPSILON || w < EPSILON {
        return None;
    }
    Some((u, v, w))
}

/// Nearest-neighbour alpha sample at normalized UV coordinates.
fn sample_alpha(texture: &AlphaTexture, uv: [f32; 2]) -> u8 {
    let width = texture.width;
    let height = texture.height;
    if width == 0 || height == 0 {
        return 255;
    }
    let x = (uv[0].clamp(0.0, 1.0) * (width - 1) as f32).round() as u32;
    let y = (uv[1].clamp(0.0, 1.0) * (height - 1) as f32).round() as u32;
    let index = (y * width + x) as usize;
    texture.alpha.get(index).copied().unwrap_or(255)
}

/// Stability compensation for repeated hit tests.
#[derive(Debug, Clone, Default)]
pub struct HitStability {
    last_hit: Option<((f32, f32), u64)>,
}

impl HitStability {
    /// Grace window in milliseconds (spec: 120 ms for hit, 80 ms for passthrough).
    pub const GRACE_MS: u64 = 120;
    /// Maximum drift in pixels that still counts as the same spot.
    pub const GRACE_DISTANCE: f32 = 12.0;

    /// Applies the same grace rule as the original: a recent confirmed hit within
    /// 12 px keeps the verdict "hit".
    pub fn confirm(&mut self, hit: bool, cursor: (f32, f32), now_ms: u64) -> bool {
        self.confirm_with(hit, cursor, now_ms, Self::GRACE_MS)
    }

    /// Grace rule with an explicit window (the passthrough poller uses 80 ms).
    pub fn confirm_with(
        &mut self,
        hit: bool,
        cursor: (f32, f32),
        now_ms: u64,
        grace_ms: u64,
    ) -> bool {
        if hit {
            self.last_hit = Some((cursor, now_ms));
            return true;
        }
        if let Some(((last_x, last_y), last_ms)) = self.last_hit {
            if now_ms.saturating_sub(last_ms) < grace_ms {
                let dx = cursor.0 - last_x;
                let dy = cursor.1 - last_y;
                if dx * dx + dy * dy <= Self::GRACE_DISTANCE * Self::GRACE_DISTANCE {
                    return true;
                }
            }
        }
        false
    }

    /// Forgets the last confirmed hit (for example when the model reloads).
    pub fn reset(&mut self) {
        self.last_hit = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neocari::moc3::Moc3DrawableMesh;

    fn quad_mesh(half: f32) -> Moc3DrawableMesh {
        // A unit quad in model space spanning -half..half on both axes.
        Moc3DrawableMesh::from_parts(
            0,
            0,
            1.0,
            0.0,
            vec![
                neocari::moc3::Moc3DrawableVertex::new([-half, half], [0.0, 0.0]),
                neocari::moc3::Moc3DrawableVertex::new([half, half], [1.0, 0.0]),
                neocari::moc3::Moc3DrawableVertex::new([half, -half], [1.0, 1.0]),
                neocari::moc3::Moc3DrawableVertex::new([-half, -half], [0.0, 1.0]),
            ],
            vec![0, 1, 2, 0, 2, 3],
            Vec::new(),
        )
    }

    fn opaque_texture() -> Arc<Vec<AlphaTexture>> {
        Arc::new(vec![DecodedTexture::new(2, 2, vec![255; 16]).into()])
    }

    #[test]
    fn centre_of_the_quad_hits() {
        let meshes = vec![quad_mesh(1.0)];
        let snapshot = FrameSnapshot::new(&meshes, opaque_texture(), Matrix44::identity());
        let hit = snapshot.hit_test((50.0, 50.0), (100, 100), 8);
        assert!(hit.hit, "centre must hit: {hit:?}");
    }

    #[test]
    fn outside_the_quad_misses() {
        let meshes = vec![quad_mesh(0.4)];
        let snapshot = FrameSnapshot::new(&meshes, opaque_texture(), Matrix44::identity());
        let hit = snapshot.hit_test((2.0, 2.0), (100, 100), 8);
        assert!(!hit.hit, "corner is outside the mesh");
    }

    #[test]
    fn transparent_pixels_do_not_hit() {
        let meshes = vec![quad_mesh(1.0)];
        let transparent = Arc::new(vec![DecodedTexture::new(2, 2, vec![0; 16]).into()]);
        let snapshot = FrameSnapshot::new(&meshes, transparent, Matrix44::identity());
        assert!(!snapshot.hit_test((50.0, 50.0), (100, 100), 8).hit);
        // Lowering the threshold below 0 is impossible, so alpha 0 never hits.
        assert!(!snapshot.hit_test((50.0, 50.0), (100, 100), 0).hit);
    }

    #[test]
    fn alpha_storage_preserves_every_byte_and_uv_sample() {
        let rgba: Vec<u8> = (0..=255u8)
            .flat_map(|alpha| [alpha.wrapping_add(1), 123, 45, alpha])
            .collect();
        let source = DecodedTexture::new(16, 16, rgba);
        let texture = AlphaTexture::from_texture(&source);
        assert_eq!(texture.byte_len(), source.rgba().len() / 4);
        assert_eq!(texture.alpha, (0..=255u8).collect::<Vec<_>>());
        // Cover pixel centers, edges, clamping and intermediate UVs using the
        // previous RGBA sampler as the reference.
        for y in -10..=110 {
            for x in -10..=110 {
                let uv = [x as f32 / 100.0, y as f32 / 100.0];
                let px = (uv[0].clamp(0.0, 1.0) * 15.0).round() as usize;
                let py = (uv[1].clamp(0.0, 1.0) * 15.0).round() as usize;
                assert_eq!(
                    sample_alpha(&texture, uv),
                    source.rgba()[(py * 16 + px) * 4 + 3]
                );
            }
        }
    }

    #[test]
    fn all_alpha_thresholds_and_opacities_keep_the_same_hit_result() {
        let mut snapshot =
            FrameSnapshot::new(&[quad_mesh(1.0)], opaque_texture(), Matrix44::identity());
        for alpha in 0..=255u8 {
            snapshot.textures = Arc::new(vec![
                DecodedTexture::new(1, 1, vec![19, 37, 61, alpha]).into()
            ]);
            for opacity in [0.0, 0.01, 0.25, 0.5, 0.75, 1.0] {
                snapshot.drawables[0].opacity = opacity;
                for threshold in 0..=255u8 {
                    let expected = (alpha as f32 / 255.0 * opacity) * 255.0 > threshold as f32;
                    assert_eq!(
                        snapshot.hit_test((50.0, 50.0), (100, 100), threshold).hit,
                        expected,
                        "alpha={alpha} opacity={opacity} threshold={threshold}"
                    );
                }
            }
        }
    }

    #[test]
    fn snapshots_share_alpha_and_empty_textures_keep_the_fallback() {
        let textures = opaque_texture();
        let snapshot = FrameSnapshot::new(
            &[quad_mesh(1.0)],
            Arc::clone(&textures),
            Matrix44::identity(),
        );
        assert!(Arc::ptr_eq(&snapshot.textures, &textures));
        assert_eq!(
            sample_alpha(
                &AlphaTexture::from(DecodedTexture::new(0, 0, vec![])),
                [0.5, 0.5]
            ),
            255
        );
    }

    #[test]
    fn stability_keeps_a_recent_hit() {
        let mut stability = HitStability::default();
        assert!(stability.confirm(true, (10.0, 10.0), 1_000));
        assert!(stability.confirm(false, (14.0, 12.0), 1_050));
        assert!(!stability.confirm(false, (40.0, 40.0), 1_050));
        assert!(!stability.confirm(false, (10.0, 10.0), 1_500));
    }

    #[test]
    fn content_bounds_cover_all_drawables() {
        let meshes = vec![quad_mesh(1.0), quad_mesh(2.0)];
        let snapshot = FrameSnapshot::new(&meshes, opaque_texture(), Matrix44::identity());
        assert_eq!(snapshot.content_bounds.max_x, 2.0);
        assert_eq!(snapshot.content_bounds.min_y, -2.0);
    }
}
