// FPS counter overlay: one textured quad placed at a pixel rect of the
// swapchain image. The strip already carries premultiplied alpha, so the
// pipeline blends with `One / OneMinusSrcAlpha` and leaves the model pass
// untouched.

struct OverlayParams {
  // x, y, width, height of the strip in physical pixels.
  rect: vec4<f32>,
  // Swapchain size in physical pixels (xy) and the window opacity (z).
  viewport: vec4<f32>,
};

@group(0) @binding(0) var overlay_texture: texture_2d<f32>;
@group(0) @binding(1) var overlay_sampler: sampler;
@group(0) @binding(2) var<uniform> params: OverlayParams;

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs_overlay(@builtin(vertex_index) index: u32) -> VertexOutput {
  // Two triangles, placed in pixel space before the clip-space conversion.
  var corners = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0),
  );
  let corner = corners[index];
  let pixel = params.rect.xy + corner * params.rect.zw;
  let clip = vec2<f32>(
    pixel.x / params.viewport.x * 2.0 - 1.0,
    1.0 - pixel.y / params.viewport.y * 2.0,
  );
  var output: VertexOutput;
  output.position = vec4<f32>(clip, 0.0, 1.0);
  output.uv = corner;
  return output;
}

@fragment
fn fs_overlay(input: VertexOutput) -> @location(0) vec4<f32> {
  // Premultiplied: scaling all four channels by the window opacity keeps it valid.
  return textureSample(overlay_texture, overlay_sampler, input.uv) * params.viewport.z;
}
