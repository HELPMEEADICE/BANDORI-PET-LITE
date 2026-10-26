# Vendored Neocari — local changes

Source: <https://github.com/HELPMEEADICE/Neocari> at revision `ba9de37` ("feat: implement full Cubism V2 model runtime"),
license GPL-3.0-only (same as this project). Copied from the upstream checkout on the development machine; the GitHub
remote is unreachable from this sandbox (TLS), so the source is vendored to make the build reproducible offline.

The library code carries **one functional fix** (MOC3 drawable flags → blend mode); everything else is packaging and test
selection:

| File | Change | Reason |
|---|---|---|
| `Cargo.toml` | Dropped `[dev-dependencies]` (`ab_glyph`, `pollster`, `winit`), the three `[[example]]` targets and the `[[test]] wgpu_renderer` target; added a header comment with the upstream revision. | The examples and the GPU integration test are not vendored, so the targets would fail to resolve. The library target is untouched. |
| `src/moc3/drawable.rs` | Added `Moc3DrawableBlendMode::from_moc3_flags()` and an explicit `blend_mode` field on `Moc3DrawableMesh`; `build_moc3_drawable_mesh` resolves the blend with the MOC3 decoder. Added two unit tests pinning both decoders. | Cubism 3/4/5 exporters set art-mesh constant bit 2 on virtually every drawable, while the shared flag byte reserves bit 2 for the Cubism 2 `Screen` blend. Decoding the raw MOC3 bit as `Screen` composited every drawable with `One/OneMinusSrc`, bleaching moc3 models to white (face line art lost, joint highlights dragged over the clothes). |
| `src/moc3/mesh_build.rs` | `build_moc3_drawable_mesh_for_pose` carries the base mesh's resolved blend mode into the rebuilt mesh. | Same fix; the per-frame rebuild path must not fall back to the Cubism 2 flag decode. |
| `src/moc3/keyform_bindings.rs` | `hiyori_binding_parameter_map_matches_layout` marked `#[ignore]`. | Needs `assets/models/Hiyori`, which upstream excludes from the crate (`exclude = ["assets/"]`). |
| `src/moc3/parts.rs` | `parses_hiyori_parts_with_default_opacity` marked `#[ignore]`. | Same missing sample asset. |
| `src/runtime.rs` | `part_keyform_opacity_does_not_drive_drawable_visibility` marked `#[ignore]`. | Same missing sample asset. |

The vendored crate's own tests (including the flag regressions) run with:

```powershell
cargo test --manifest-path vendor/neocari/Cargo.toml --lib --offline
```

See `docs/ACCEPTANCE.md` 修复记录 for the before/after render numbers.

Everything BANDORI-PET-LITE uses is public upstream API:

* `assets::{load_moc2_runtime, load_model_runtime, DecodedTexture}`
* `moc2::{Moc2RuntimeModel, Moc2Runtime}`
* `motion::{load_motion, MotionPlayer}`, `expression::{load_expression, ExpressionManager}`
* `render::wgpu::{WgpuLive2dRenderer, WgpuMeshBuffers, WgpuClippingPlan, WgpuMaskRenderTarget, preferred_surface_format}`
* `core::Matrix44`

If a future refactor needs an entry point that does not exist yet (for example an in-memory `Moc2Model::from_parts`
assembler to avoid extracting to disk, or a nearest-neighbour sampler with clipping), that addition belongs in this
vendored copy and must be listed in the table above.
