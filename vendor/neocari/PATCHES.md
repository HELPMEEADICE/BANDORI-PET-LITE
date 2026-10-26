# Vendored Neocari — local changes

Source: <https://github.com/HELPMEEADICE/Neocari> at revision `ba9de37` ("feat: implement full Cubism V2 model runtime"),
license GPL-3.0-only (same as this project). Copied from the upstream checkout on the development machine; the GitHub
remote is unreachable from this sandbox (TLS), so the source is vendored to make the build reproducible offline.

The library code is **unmodified**. Only packaging and test selection changed:

| File | Change | Reason |
|---|---|---|
| `Cargo.toml` | Dropped `[dev-dependencies]` (`ab_glyph`, `pollster`, `winit`), the three `[[example]]` targets and the `[[test]] wgpu_renderer` target; added a header comment with the upstream revision. | The examples and the GPU integration test are not vendored, so the targets would fail to resolve. The library target is untouched. |
| `src/moc3/keyform_bindings.rs` | `hiyori_binding_parameter_map_matches_layout` marked `#[ignore]`. | Needs `assets/models/Hiyori`, which upstream excludes from the crate (`exclude = ["assets/"]`). |
| `src/moc3/parts.rs` | `parses_hiyori_parts_with_default_opacity` marked `#[ignore]`. | Same missing sample asset. |
| `src/runtime.rs` | `part_keyform_opacity_does_not_drive_drawable_visibility` marked `#[ignore]`. | Same missing sample asset. |

Everything BANDORI-PET-LITE uses is public upstream API:

* `assets::{load_moc2_runtime, load_model_runtime, DecodedTexture}`
* `moc2::{Moc2RuntimeModel, Moc2Runtime}`
* `motion::{load_motion, MotionPlayer}`, `expression::{load_expression, ExpressionManager}`
* `render::wgpu::{WgpuLive2dRenderer, WgpuMeshBuffers, WgpuClippingPlan, WgpuMaskRenderTarget, preferred_surface_format}`
* `core::Matrix44`

If a future refactor needs an entry point that does not exist yet (for example an in-memory `Moc2Model::from_parts`
assembler to avoid extracting to disk, or a nearest-neighbour sampler with clipping), that addition belongs in this
vendored copy and must be listed in the table above.
