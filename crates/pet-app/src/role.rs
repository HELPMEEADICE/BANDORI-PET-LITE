//! One desktop-pet role: window, renderer, model, behavior, input.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use pet_assets::catalog::CatalogCostume;
use pet_assets::ModelCache;
use pet_behavior::{
    BehaviorContext, Command, RoleBehavior, RoleBehaviorConfig, WindowMotionKind,
};
use pet_config::{Config, ModelEntry, Placement};
use pet_input::hit::{FrameSnapshot, HitStability};

use pet_live2d::{ModelHandle, ModelInfo};
use pet_render::{ModelGpu, RenderError, SceneRenderer};
use pet_window::sizing::{content_size, fit_matrix, window_size_for_model};
use pet_window::{placement, win32};
use tracing::{error, info, warn};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

/// Result alias for role construction.
pub type RoleResult<T> = Result<T, RoleError>;

/// Errors raised while building a role window.
#[derive(Debug, thiserror::Error)]
pub enum RoleError {
    /// Model extraction failed.
    #[error("model cache error: {0}")]
    Cache(#[from] pet_assets::CacheError),
    /// Neocari could not load the model.
    #[error("model load error: {0}")]
    Model(#[from] pet_live2d::Live2dError),
    /// Window creation failed.
    #[error("window error: {0}")]
    Window(String),
    /// Rendering could not start.
    #[error("render error: {0}")]
    Render(#[from] RenderError),
    /// The model had no drawable geometry.
    #[error("model has no drawable bounds")]
    NoBounds,
}

struct DragState {
    /// Global (screen) cursor position when the drag started.
    press_cursor_global: (f32, f32),
    /// Window position when the drag started.
    press_window: (i32, i32),
    moved: bool,
    total: (f32, f32),
}

struct LastClick {
    at: Instant,
    position: (f32, f32),
}

/// One role window and everything it owns.
pub struct Role {
    /// The winit window.
    pub window: Arc<Window>,
    /// Swapchain + renderer for this window.
    pub renderer: SceneRenderer,
    /// Loaded model (moc or moc3).
    pub model: ModelHandle,
    /// GPU copies of meshes, textures and clipping.
    pub model_gpu: ModelGpu,
    /// Behavior state machine.
    pub behavior: RoleBehavior,
    /// Model-space content bounds.
    pub bounds: pet_window::sizing::Bounds,
    /// Model → clip matrix.
    pub matrix: neocari::core::Matrix44,
    /// Persisted placement record.
    pub placement: Placement,
    /// Config entry this role was created from.
    pub entry: ModelEntry,
    /// Cached decoded textures for hit testing.
    pub textures: Arc<Vec<neocari::assets::DecodedTexture>>,
    /// Window position before any emotion displacement.
    pub base_position: (i32, i32),
    /// Last known cursor position in local physical pixels.
    cursor: Option<(f32, f32)>,
    /// Active drag gesture.
    dragging: Option<DragState>,
    /// Hit stability compensation state.
    stability: HitStability,
    /// Last confirmed hit timestamp (ms since start).
    last_hit_ms: u64,
    /// Whether click-through is currently enabled.
    pub click_through: bool,
    /// Last passthrough poll timestamp.
    last_passthrough_poll: Instant,
    /// Last click, for manual double-click detection.
    last_click: Option<LastClick>,
    /// Bubble text and its expiry.
    bubble: Option<(String, Instant)>,
    /// Last time the model was advanced.
    last_update: Instant,
    /// Window hidden by the user (tray / radial menu).
    pub hidden: bool,
    /// Whether the first frame has been presented.
    first_frame_done: bool,
    /// Cached model info for the settings UI.
    info: ModelInfo,
    /// Timestamp of the last topmost re-assert.
    last_topmost: Instant,
    /// Model selection bookkeeping for `MODEL_DATA` messages.
    pub character: String,
    pub costume: String,
}



impl Role {
    /// Builds a role window for `entry`.
    ///
    /// `gpu` is reused when a second role is added so the process keeps a single
    /// device and queue.
    pub fn create(
        event_loop: &ActiveEventLoop,
        entry: &ModelEntry,
        config: &Config,
        _paths: &pet_config::Paths,
        cache: &ModelCache,
        gpu: Option<Arc<pet_render::Gpu>>,
    ) -> RoleResult<Self> {
        let started = Instant::now();
        let resolved = cache.resolve(&entry.path)?;
        let mut model = ModelHandle::load(&resolved.model_path)?;
        let info = model.info().clone();
        let textures = Arc::new(model.textures().to_vec());

        let bounds = content_size(model.meshes()).ok_or(RoleError::NoBounds)?;
        let monitors = placement::monitors();
        let dpr = event_loop
            .primary_monitor()
            .or_else(|| event_loop.available_monitors().next())
            .map(|monitor| monitor.scale_factor())
            .unwrap_or(1.0);
        let scale = config.effective_scale_percent(dpr);
        let (width, height) = window_size_for_model(bounds, scale, dpr);
        let primary = monitors.iter().find(|monitor| monitor.is_primary).cloned();
        let mut stored = entry.window_placement.clone();
        stored.width = width;
        stored.height = height;
        let decision = placement::resolve_position(&stored, (width, height), &monitors, primary.as_ref());
        let mut stored = stored;
        stored.x = decision.x;
        stored.y = decision.y;
        if let Some(monitor) = decision.monitor.as_ref() {
            let recorded = placement::record_placement(
                (decision.x, decision.y, width, height),
                dpr,
                monitor,
            );
            stored.relative_x = recorded.relative_x;
            stored.relative_y = recorded.relative_y;
            stored.right_offset = recorded.right_offset;
            stored.bottom_offset = recorded.bottom_offset;
            stored.screen_name = recorded.screen_name;
            stored.screen_serial = recorded.screen_serial;
            stored.screen_manufacturer = recorded.screen_manufacturer;
            stored.screen_model = recorded.screen_model;
            stored.screen_geometry = recorded.screen_geometry;
            stored.screen_available_geometry = recorded.screen_available_geometry;
            stored.device_pixel_ratio = recorded.device_pixel_ratio;
        }
        info!(
            character = %entry.character,
            costume = %entry.costume,
            ?decision.decision,
            x = decision.x,
            y = decision.y,
            width,
            height,
            "creating role window"
        );

        let attributes = Window::default_attributes()
            .with_title(format!("{}-{}", entry.character, entry.costume))
            .with_window_icon(crate::brand::icon_rgba(48).ok().and_then(|image| {
                winit::window::Icon::from_rgba(image.into_raw(), 48, 48).ok()
            }))
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(false)
            .with_visible(false)
            .with_inner_size(PhysicalSize::new(width, height))
            .with_position(PhysicalPosition::new(decision.x, decision.y));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| RoleError::Window(error.to_string()))?,
        );

        #[cfg(windows)]
        {
            if let Some(hwnd) = win32::hwnd_of(&window) {
                win32::apply_pet_style(hwnd, config.obs_window_capture_compatible);
                win32::set_window_rect(hwnd, decision.x, decision.y, width, height);
            }
        }

        let quality = config.live2d_quality.resolution_scale();
        let renderer = match gpu {
            Some(gpu) => SceneRenderer::with_gpu(
                gpu,
                Arc::clone(&window),
                config.vsync,
                quality,
                config.opacity,
            )?,
            None => SceneRenderer::new(
                Arc::clone(&window),
                config.vsync,
                quality,
                config.opacity,
                config.gpu_acceleration,
            )?,
        };

        let matrix = fit_matrix(bounds, 1.0, model.flips_y());
        let model_gpu = renderer.upload_model(model.meshes(), &textures, matrix)?;

        let mut behavior = RoleBehavior::new(pet_util::Rng::from_entropy().next_u64());
        let mut behavior_config = RoleBehaviorConfig::from(config);
        if let Some(actions) = config.model_action_settings.get(&entry.action_key()) {
            behavior_config.click_actions = actions.clone();
        }
        behavior.set_config(behavior_config);


        model.set_auto_blink(true);
        model.set_auto_breath(true);
        behavior.start(0.0);

        info!(
            ms = started.elapsed().as_millis(),
            format = ?info.format,
            motions = info.motion_groups.len(),
            expressions = info.expression_names.len(),
            "role ready"
        );

        let mut role = Self {
            window,
            renderer,
            model,
            model_gpu,
            behavior,
            bounds,
            matrix,
            placement: stored,
            entry: entry.clone(),
            textures,
            base_position: (decision.x, decision.y),
            cursor: None,
            dragging: None,
            stability: HitStability::default(),
            last_hit_ms: 0,
            click_through: false,
            last_passthrough_poll: Instant::now(),
            last_click: None,
            bubble: None,
            last_update: Instant::now(),
            hidden: false,
            first_frame_done: false,
            info,
            last_topmost: Instant::now(),
            character: entry.character.clone(),
            costume: entry.costume.clone(),
        };
        role.set_hit_alpha_threshold(config.live2d_hit_alpha_threshold);
        role.apply_display_settings(config);

        // Present the first frame *before* showing the window: a hidden winit
        // window receives no WM_PAINT, so waiting for `RedrawRequested` would
        // deadlock the very first frame (spec: first frame before everything else).
        let presented = role.render();
        role.window.set_visible(true);
        #[cfg(windows)]
        if let Some(hwnd) = win32::hwnd_of(&role.window) {
            win32::show_no_activate(hwnd);
            win32::reassert_topmost(hwnd);
        }
        if !presented {
            warn!("first frame was not presented; the window may need a redraw");
            role.window.request_redraw();
        }
        Ok(role)
    }

    fn set_hit_alpha_threshold(&mut self, _threshold: u8) {
        // The threshold is passed to `hit_test` at call time; kept as a method so
        // the config wiring is obvious.
    }

    /// Model capabilities for the settings window.
    pub fn info(&self) -> &ModelInfo {
        &self.info
    }

    /// Current motion group names.
    pub fn motion_names(&self) -> &[String] {
        &self.info.motion_groups
    }

    /// Current expression names.
    pub fn expression_names(&self) -> &[String] {
        &self.info.expression_names
    }

    /// Replaces the behavior config after a settings change.
    pub fn set_behavior_config(&mut self, config: &Config) {
        let mut behavior_config = RoleBehaviorConfig::from(config);
        if let Some(actions) = config.model_action_settings.get(&self.entry.action_key()) {
            behavior_config.click_actions = actions.clone();
        }
        self.behavior.set_config(behavior_config);
    }

    /// Applies instant-level settings (fps handled by the loop, opacity/quality here).
    pub fn apply_display_settings(&mut self, config: &Config) {
        self.renderer.set_opacity(config.opacity);
        let quality = config.live2d_quality.resolution_scale();
        if (self.renderer.quality() - quality).abs() > f32::EPSILON {
            self.renderer.set_quality(quality);
        }
        if self.renderer.vsync() != config.vsync {
            self.renderer.set_vsync(config.vsync);
        }
        self.window.set_visible(!config.hide_live2d_model && !self.hidden);
    }

    /// Rebuilds the window for a scale change (RebuildWindow apply level).
    pub fn apply_scale(&mut self, config: &Config, dpr: f64) -> RoleResult<()> {
        let scale = config.effective_scale_percent(dpr);
        let (width, height) = window_size_for_model(self.bounds, scale, dpr);
        let (x, y, current_width, current_height) = self
            .window_rect()
            .unwrap_or((self.base_position.0, self.base_position.1, width, height));
        if current_width == width && current_height == height {
            return Ok(());
        }
        // Keep the bottom-right corner anchored so the pet does not jump off-screen.
        let new_x = x + (current_width as i32 - width as i32);
        let new_y = y + (current_height as i32 - height as i32);
        let _ = self.window.request_inner_size(PhysicalSize::new(width, height));
        self.base_position = (new_x, new_y);
        self.set_position(new_x, new_y);
        let matrix = fit_matrix(self.bounds, 1.0, self.model.flips_y());
        self.matrix = matrix;
        self.model_gpu
            .set_matrix(self.renderer.gpu(), self.renderer.renderer(), matrix);
        self.renderer.resize(PhysicalSize::new(width, height), dpr);
        Ok(())
    }

    /// Window rect `(x, y, width, height)` in physical pixels.
    pub fn window_rect(&self) -> Option<(i32, i32, u32, u32)> {
        #[cfg(windows)]
        if let Some(hwnd) = win32::hwnd_of(&self.window) {
            return win32::window_rect(hwnd);
        }
        None
    }

    /// Moves the window without activating it.
    pub fn set_position(&mut self, x: i32, y: i32) {
        #[cfg(windows)]
        if let Some(hwnd) = win32::hwnd_of(&self.window) {
            let (_, _, width, height) = self
                .window_rect()
                .unwrap_or((x, y, self.window.inner_size().width, self.window.inner_size().height));
            win32::set_window_rect(hwnd, x, y, width, height);
            return;
        }
        let _ = self
            .window
            .request_inner_size(PhysicalSize::new(self.window.inner_size().width.max(1), self.window.inner_size().height.max(1)));
    }

    /// Records the current window rect into the placement record.
    pub fn record_placement(&mut self) {
        let Some((x, y, width, height)) = self.window_rect() else {
            return;
        };
        let monitors = placement::monitors();
        let monitor = placement::monitor_at(&monitors, (x + width as i32 / 2, y + height as i32 / 2));
        let Some(monitor) = monitor else {
            return;
        };
        let recorded = placement::record_placement((x, y, width, height), self.window.scale_factor(), &monitor);
        self.placement = recorded;
        self.entry.window_placement = self.placement.clone();
        self.entry.sync_placement_mirror();
        self.base_position = (x, y);
    }

    /// Advances the model by `dt` and uploads the resulting meshes.
    pub fn advance_model(&mut self, dt: f32) {
        if !self.model.update(dt) {
            warn!("model update failed");
            return;
        }
        if let Err(error) = self.model_gpu
            .update_meshes(self.renderer.gpu(), self.renderer.renderer(), self.model.meshes()) {
            warn!(%error, "mesh upload failed");
        }
    }

    /// Renders one frame.
    pub fn render(&mut self) -> bool {
        if self.hidden || self.renderer.is_fused() {
            return false;
        }
        match self.renderer.render(&self.model_gpu) {
            Ok(()) => {
                if !self.first_frame_done {
                    self.first_frame_done = true;
                    self.window.set_visible(true);
                    #[cfg(windows)]
                    if let Some(hwnd) = win32::hwnd_of(&self.window) {
                        win32::show_no_activate(hwnd);
                    }
                }
                true
            }
            Err(error) => {
                error!(%error, "render failed");
                false
            }
        }
    }

    /// Cursor position in local physical pixels.
    pub fn cursor_local(&self) -> Option<(f32, f32)> {
        self.cursor
    }

    /// Remembers that the user interacted (resets idle timers).
    pub fn note_interaction(&mut self, now: f64) {
        self.behavior.note_interaction(now);
    }

    /// Handles a click on `region`, returning the emitted commands.
    pub fn on_click(
        &mut self,
        region: pet_config::ClickRegion,
        context: &BehaviorContext<'_>,
    ) -> Vec<Command> {
        self.behavior.on_click(region, context.now, context)
    }

    /// Handles a double click (poke), returning the emitted commands.
    pub fn on_double_click(
        &mut self,
        region: pet_config::ClickRegion,
        context: &BehaviorContext<'_>,
    ) -> Vec<Command> {
        self.behavior.on_double_click(region, context.now, context)
    }

    /// Applies a command produced outside `tick` (click/poke feedback).
    pub fn apply_external_command(&mut self, command: &Command) {
        self.apply_command(command);
    }

    /// Passthrough duty cycle: 16 ms while the cursor is near, 125 ms when idle.
    pub fn poll_passthrough(&mut self, config: &Config, now_ms: u64, near: bool) {
        let interval = if near { 16 } else { 125 };
        if self.last_passthrough_poll.elapsed().as_millis() < interval as u128 {
            return;
        }
        self.last_passthrough_poll = Instant::now();
        let should = self.should_be_click_through(config, now_ms);
        self.set_click_through(should);
    }

    /// Updates the cached cursor position; `None` when the cursor left the window.
    pub fn set_cursor(&mut self, position: Option<(f32, f32)>) {
        self.cursor = position;
    }

    /// Builds a hit-test snapshot of the current frame.
    pub fn snapshot(&self) -> FrameSnapshot {
        FrameSnapshot::new(self.model.meshes(), Arc::clone(&self.textures), self.matrix)
    }

    /// Tests a local physical-pixel cursor position against the model.
    pub fn hit_test(&mut self, cursor: (f32, f32), alpha_threshold: u8, now_ms: u64) -> bool {
        let snapshot = self.snapshot();
        let size = self.window.inner_size();
        let result = snapshot.hit_test(cursor, (size.width, size.height), alpha_threshold);
        let hit = self
            .stability
            .confirm(result.hit, cursor, now_ms);
        if hit {
            self.last_hit_ms = now_ms;
        }
        hit
    }

    /// Region for a local cursor position.
    ///
    /// The 7-region split is computed from **clip space**, not model space: this
    /// keeps the y ratio correct for both the y-up (moc3) and y-down (moc) model
    /// spaces without a per-format branch.
    pub fn region_at(&mut self, cursor: (f32, f32)) -> pet_config::ClickRegion {
        let size = self.window.inner_size();
        let snapshot = self.snapshot();
        let Some(point) = snapshot.cursor_to_model(cursor, (size.width, size.height)) else {
            return pet_config::ClickRegion::UpperBodyCenter;
        };
        let area = self.model.hit_test_area(point.0, point.1).unwrap_or_default();
        // Model space → clip space with the very same matrix the renderer uses.
        let clip_x = self.matrix.transform_x(point.0);
        let clip_y = self.matrix.transform_y(point.1);
        let x_ratio = ((clip_x + 1.0) * 0.5).clamp(0.0, 1.0);
        let y_ratio = ((1.0 - clip_y) * 0.5).clamp(0.0, 1.0);
        let region = pet_input::classify_region(x_ratio, y_ratio, &area);
        tracing::debug!(
            region = region.key(),
            x_ratio = format_args!("{x_ratio:.2}"),
            y_ratio = format_args!("{y_ratio:.2}"),
            area = area.as_str(),
            "click region"
        );
        region
    }

    /// Handles a left-button press.
    pub fn on_left_press(&mut self, cursor: (f32, f32), config: &Config, now_ms: u64) -> bool {
        let hit = self.hit_test(cursor, config.live2d_hit_alpha_threshold, now_ms);
        if !hit || config.drag_locked {
            return hit;
        }
        let Some((x, y, ..)) = self.window_rect() else {
            return hit;
        };
        // Drag math uses the *global* cursor: moving the window changes the local
        // cursor by the same amount, which would otherwise create a feedback loop
        // that makes the window lag behind the pointer.
        let press_cursor_global = global_cursor().unwrap_or_else(|| {
            let (wx, wy) = self.base_position;
            (wx as f32 + cursor.0, wy as f32 + cursor.1)
        });
        self.dragging = Some(DragState {
            press_cursor_global,
            press_window: (x, y),
            moved: false,
            total: (0.0, 0.0),
        });
        true
    }

    /// Handles cursor movement while dragging; returns whether the window moved.
    pub fn on_cursor_moved_drag(&mut self, cursor: (f32, f32)) -> bool {
        let Some(global) = global_cursor() else {
            return false;
        };
        let Some(drag) = self.dragging.as_mut() else {
            return false;
        };
        let dx = global.0 - drag.press_cursor_global.0;
        let dy = global.1 - drag.press_cursor_global.1;
        drag.total = (dx, dy);
        if dx * dx + dy * dy >= 16.0 {
            drag.moved = true;
        }
        if !drag.moved {
            let _ = cursor;
            return false;
        }
        let new_x = drag.press_window.0 + dx.round() as i32;
        let new_y = drag.press_window.1 + dy.round() as i32;
        self.set_position(new_x, new_y);
        true
    }

    /// Finishes a drag; returns `(moved, total_delta)`.
    pub fn on_left_release(&mut self) -> (bool, (f32, f32)) {
        match self.dragging.take() {
            Some(drag) => {
                if drag.moved {
                    self.record_placement();
                }
                (drag.moved, drag.total)
            }
            None => (false, (0.0, 0.0)),
        }
    }

    /// Detects a double click from two presses close in time and space.
    pub fn register_click(&mut self, cursor: (f32, f32)) -> bool {
        let now = Instant::now();
        let double = self.last_click.as_ref().is_some_and(|last| {
            let dx = cursor.0 - last.position.0;
            let dy = cursor.1 - last.position.1;
            now.duration_since(last.at).as_millis() < 420 && dx * dx + dy * dy <= 64.0
        });
        self.last_click = if double {
            None
        } else {
            Some(LastClick {
                at: now,
                position: cursor,
            })
        };
        double
    }

    /// Runs one behavior tick and applies its commands.
    pub fn tick_behavior(
        &mut self,
        now: f64,
        dt: f32,
        idle_seconds: f64,
        peer_center: Option<(f32, f32)>,
        own_center: (f32, f32),
    ) -> Vec<Command> {
        let size = self.window.inner_size();
        let hour = pet_util::time_of_day::local_hour();
        let motion_names = self.info.motion_groups.clone();
        let expression_names = self.info.expression_names.clone();
        let cursor_local = self.cursor;
        let context = BehaviorContext {
            now,
            dt,
            cursor_local,
            surface: (size.width, size.height),
            idle_seconds,
            hour,
            peer_center,
            own_center,
            motion_names: &motion_names,
            expression_names: &expression_names,
            character: &self.character,
        };
        let commands = self.behavior.tick(&context, self.base_position);
        for command in &commands {
            self.apply_command(command);
        }
        commands
    }

    /// Screen position above the model's head, used to anchor the poke bubble.
    pub fn bubble_anchor(&self) -> (i32, i32) {
        match self.window_rect() {
            Some((x, y, width, height)) => {
                (x + width as i32 / 2, y + (height as f32 * 0.18) as i32)
            }
            None => (0, 0),
        }
    }

    fn apply_command(&mut self, command: &Command) {
        match command {
            Command::PlayMotion(group) => {
                tracing::debug!(motion = %group, "play motion");
                if group.is_empty() {
                    if let Some(default) = self.info.default_motion_group() {
                        let default = default.to_owned();
                        self.model.play_motion(&default);
                    }
                } else if !self.model.play_motion(group) {
                    warn!(group, "motion not available on this model");
                }
            }
            Command::PlayExpression(name) => {
                tracing::debug!(expression = %name, "play expression");
                if name.is_empty() {
                    self.model.play_expression("default");
                } else if !self.model.play_expression(name) {
                    warn!(name, "expression not available on this model");
                }
            }
            Command::PlayRandomMotion => {
                let mut rng = pet_util::Rng::from_entropy();
                let names: Vec<String> = self
                    .info
                    .motion_groups
                    .iter()
                    .filter(|name| {
                        let lower = name.to_ascii_lowercase();
                        !lower.starts_with("idle") && !lower.starts_with("sys-")
                    })
                    .cloned()
                    .collect();
                if let Some(name) = rng.pick(&names).cloned() {
                    self.model.play_motion(&name);
                }
            }
            Command::SetDrag(x, y) => self.model.set_drag(*x, *y),
            Command::MoveWindow(x, y) => {
                if !self.dragging.as_ref().is_some_and(|drag| drag.moved) {
                    self.set_position(*x, *y);
                }
            }
            Command::Bubble(text) => {
                self.bubble = Some((text.clone(), Instant::now()));
            }
        }
    }

    /// Current bubble text, if it is still visible.
    pub fn bubble_text(&self) -> Option<&str> {
        self.bubble.as_ref().and_then(|(text, at)| {
            (at.elapsed().as_millis() < 1980).then_some(text.as_str())
        })
    }

    /// Should the window be click-through right now?
    pub fn should_be_click_through(&mut self, config: &Config, now_ms: u64) -> bool {
        if config.drag_locked && self.dragging.is_some() {
            return false;
        }
        let Some(global) = global_cursor() else {
            return false;
        };
        let Some((x, y, width, height)) = self.window_rect() else {
            return false;
        };
        let halfway_outside = global.0 < x as f32 - 96.0
            || global.1 < y as f32 - 96.0
            || global.0 > (x as f32) + width as f32 + 96.0
            || global.1 > (y as f32) + height as f32 + 96.0;
        if halfway_outside {
            return true;
        }
        let local = (global.0 - x as f32, global.1 - y as f32);
        let hit = self.hit_test(local, config.live2d_hit_alpha_threshold, now_ms);
        !hit
    }

    /// Sets click-through if the state changed.
    pub fn set_click_through(&mut self, enabled: bool) {
        if self.click_through == enabled {
            return;
        }
        self.click_through = enabled;
        #[cfg(windows)]
        if let Some(hwnd) = win32::hwnd_of(&self.window) {
            win32::set_click_through(hwnd, enabled);
        }
    }

    /// Re-asserts topmost z-order (game compatibility guard).
    pub fn guard_topmost(&mut self, enabled: bool) {
        if !enabled {
            return;
        }
        if self.last_topmost.elapsed().as_millis() < 1000 {
            return;
        }
        self.last_topmost = Instant::now();
        #[cfg(windows)]
        if let Some(hwnd) = win32::hwnd_of(&self.window) {
            win32::reassert_topmost(hwnd);
        }
    }

    /// Window center in screen coordinates.
    pub fn center(&self) -> (f32, f32) {
        match self.window_rect() {
            Some((x, y, width, height)) => (
                x as f32 + width as f32 * 0.5,
                y as f32 + height as f32 * 0.5,
            ),
            None => (0.0, 0.0),
        }
    }

    /// Hides or shows the role window.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        self.window.set_visible(!hidden);
    }

    /// Plays a random motion from the radial menu.
    pub fn play_menu_motion(&mut self, now: f64) {
        let mut rng = pet_util::Rng::from_entropy();
        let names: Vec<String> = self
            .info
            .motion_groups
            .iter()
            .filter(|name| {
                let lower = name.to_ascii_lowercase();
                !lower.starts_with("idle") && !lower.starts_with("sys-")
            })
            .cloned()
            .collect();
        if let Some(name) = rng.pick(&names).cloned() {
            self.model.play_motion(&name);
        }
        let peer = None;
        let _ = self.tick_behavior(now, 0.0, 0.0, peer, self.center());
    }

    /// Triggers an emotion displacement.
    pub fn trigger_window_motion(&mut self, kind: WindowMotionKind, intensity: u8, now: f64) {
        self.behavior.trigger_window_motion(kind, intensity, now);
    }

    /// Model path of this role for the About page.
    pub fn model_path(&self) -> &Path {
        Path::new(&self.entry.path)
    }

    /// Current FPS estimate helper: last update timestamp.
    pub fn mark_updated(&mut self) {
        self.last_update = Instant::now();
    }

    /// Milliseconds since the last model advance.
    pub fn since_update_ms(&self) -> u128 {
        self.last_update.elapsed().as_millis()
    }

    /// Model entry snapshot with the latest placement.
    pub fn entry(&self) -> ModelEntry {
        let mut entry = self.entry.clone();
        entry.window_placement = self.placement.clone();
        entry.sync_placement_mirror();
        entry
    }

    /// Applies a new model to this role without recreating the window.
    pub fn swap_model(
        &mut self,
        entry: &ModelEntry,
        cache: &ModelCache,
        config: &Config,
    ) -> RoleResult<()> {
        let resolved = cache.resolve(&entry.path)?;
        let model = ModelHandle::load(&resolved.model_path)?;
        let info = model.info().clone();
        let textures = Arc::new(model.textures().to_vec());
        let bounds = content_size(model.meshes()).ok_or(RoleError::NoBounds)?;
        let dpr = self.window.scale_factor();
        let scale = config.effective_scale_percent(dpr);
        let (width, height) = window_size_for_model(bounds, scale, dpr);
        let matrix = fit_matrix(bounds, 1.0, model.flips_y());
        let model_gpu = self.renderer.upload_model(model.meshes(), &textures, matrix)?;

        self.model = model;
        self.model_gpu = model_gpu;
        self.textures = textures;
        self.bounds = bounds;
        self.matrix = matrix;
        self.info = info;
        self.entry = entry.clone();
        self.character = entry.character.clone();
        self.costume = entry.costume.clone();
        self.stability.reset();

        let _ = self
            .window
            .request_inner_size(PhysicalSize::new(width, height));
        self.model.set_auto_blink(true);
        self.model.set_auto_breath(true);
        self.behavior.set_config(RoleBehaviorConfig::from(config));
        self.behavior.start(0.0);
        info!(character = %entry.character, costume = %entry.costume, "model hot-swapped");
        Ok(())
    }

    /// Applies the OBS capture style without recreating the window.
    pub fn apply_obs_capture(&self, enabled: bool) {
        let _ = enabled; // taskbar presence is controlled by WS_EX_TOOLWINDOW / WS_EX_APPWINDOW
        #[cfg(windows)]
        if let Some(hwnd) = win32::hwnd_of(&self.window) {
            win32::apply_pet_style(hwnd, enabled);
        }
    }

    /// Current drawable count for diagnostics.
    pub fn drawable_count(&self) -> usize {
        self.model_gpu.drawable_count()
    }

    /// Radius used by the head-tracking / approach checks.
    pub fn size(&self) -> (u32, u32) {
        let size = self.window.inner_size();
        (size.width, size.height)
    }
}

fn global_cursor() -> Option<(f32, f32)> {
    #[cfg(windows)]
    {
        win32::cursor_position()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Convenience for the app: builds an entry from a catalog selection.
pub fn entry_for_selection(character: &str, costume: &CatalogCostume) -> ModelEntry {
    let mut entry = ModelEntry::new(character, &costume.id, &costume.path, costume.format);
    entry.sync_placement_mirror();
    entry
}

/// Applies a mouse-button state, returning whether the press was on the model.
pub fn is_left(state: ElementState, button: MouseButton) -> bool {
    state == ElementState::Pressed && button == MouseButton::Left
}












