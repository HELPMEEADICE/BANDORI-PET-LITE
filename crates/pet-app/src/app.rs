//! Application wiring: winit event loop, roles, tray, IPC, config persistence.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use pet_assets::catalog::Catalog;
use pet_assets::{index, ModelCache};
use pet_config::{ApplyLevel, Config, LoadOutcome, Paths};
use pet_ipc::{pipe_name, Message, ModelData, SettingsPage};
use pet_window::placement;
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::ipc_host::{IpcEvent, IpcHost};
use crate::role::Role;
use crate::tray::{Tray, TrayEvent};
use crate::ui_process::UiSupervisor;

/// Result alias for startup work.
pub type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Frame budget floor so a 240 fps request cannot spin at 100 %.
const MIN_FRAME_MS: u64 = 1000 / 240;

type DragPeer = (usize, (i32, i32), (i32, i32));

/// Everything the pet process owns.
pub struct PetApp {
    paths: Paths,
    config: Config,
    load: LoadOutcome,
    cache: ModelCache,
    /// Loaded lazily: scanning 64 archives must not delay the first frame.
    catalog: Option<Catalog>,
    roles: Vec<Role>,
    /// Window creation must run inside the active winit event loop.
    roles_dirty: bool,
    tray: Option<Tray>,
    ipc: IpcHost,
    ui: UiSupervisor,
    /// Open requests made before the settings process connects.
    pending_settings_page: Option<SettingsPage>,
    started: Instant,
    last_frame: Instant,
    last_config_save: Instant,
    config_dirty: bool,
    /// Roles that were deliberately hidden via tray/radial menu.
    hidden: bool,
    gpu: Option<std::sync::Arc<pet_render::Gpu>>,
    tray_events: Receiver<TrayEvent>,
    quit: bool,
    /// `BANDORI_PET_EXIT_AFTER_SECS`: quit automatically (smoke tests / CI).
    exit_after: Option<f64>,
    /// `BANDORI_PET_EXIT_HIDDEN`: never create a role window (config smoke tests).
    headless: bool,
    /// Deferred tray creation (icon is built after the first frame).
    tray_sender: Option<std::sync::mpsc::Sender<TrayEvent>>,
    create_tray: bool,
    /// Role currently being dragged, and its rect at the previous tick.
    drag_peer: Option<DragPeer>,
    /// Last applied `auto_start` value, so the registry is written only on change.
    auto_start_applied: bool,
}

impl PetApp {
    /// Builds the app, loading config, the model index and the catalog.
    pub fn new(paths: Paths) -> AppResult<Self> {
        Self::new_with(paths, "default", true)
    }

    /// Builds the app with an explicit IPC session name (tests use a unique one).
    pub fn new_with(paths: Paths, session: &str, enable_tray: bool) -> AppResult<Self> {
        let load = pet_config::load_config(&paths.config());
        for warning in &load.warnings {
            warn!(%warning, "config warning");
        }
        let mut config = load.config.clone();
        config.normalize_models();

        // Scan on first run or without configured roles. Existing configured
        // installations keep the lazy scan so the first frame appears promptly.
        let catalog = if load.is_first_run() || config.models.is_empty() {
            Some(Self::scan_catalog(&paths))
        } else {
            None
        };

        let cache = ModelCache::new(paths.cache(), paths.models());
        let auto_start = config.auto_start;
        let pipe = pipe_name(session);
        let ipc = IpcHost::start(pipe.clone());
        let (tray_tx, tray_events) = std::sync::mpsc::channel();
        let ui = UiSupervisor::new(paths.ui_executable(), pipe);
        let tray = None;
        let tray_sender = Some(tray_tx);
        let create_tray = enable_tray;
        let config_dirty = load.is_first_run() || !load.warnings.is_empty();

        Ok(Self {
            paths,
            config,
            load,
            cache,
            catalog,
            roles: Vec::new(),
            roles_dirty: true,
            tray,
            ipc,
            ui,
            pending_settings_page: None,
            started: Instant::now(),
            last_frame: Instant::now(),
            last_config_save: Instant::now(),
            config_dirty,
            hidden: false,
            gpu: None,
            tray_events,
            tray_sender,
            create_tray,
            quit: false,
            exit_after: std::env::var("BANDORI_PET_EXIT_AFTER_SECS")
                .ok()
                .and_then(|value| value.parse::<f64>().ok()),
            headless: std::env::var("BANDORI_PET_HEADLESS").is_ok(),
            drag_peer: None,
            auto_start_applied: auto_start,
        })
    }

    /// Scans `models/*.zst` and joins it with `band.json` / `outfit.json`.
    fn scan_catalog(paths: &Paths) -> Catalog {
        let index = index::load_or_scan(&paths.models());
        let catalog = Catalog::build(&paths.assets(), &index);
        info!(
            characters = catalog.len(),
            costumes = index.costume_count(),
            "catalog ready"
        );
        if catalog.is_empty() {
            warn!(
                models = %paths.models().display(),
                "no models found; use the settings window to download models"
            );
        }
        catalog
    }

    /// Returns the catalog, scanning on first use.
    fn ensure_catalog(&mut self) -> &Catalog {
        if self.catalog.is_none() {
            self.catalog = Some(Self::scan_catalog(&self.paths));
        }
        self.catalog.as_ref().expect("catalog just built")
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Ensures at least one usable `models[]` entry, falling back to the catalog.
    fn ensure_role_entries(&mut self) -> bool {
        if self.config.needs_model_selection() {
            let preferred_character = self.config.character.clone();
            let preferred_costume = self.config.costume.clone();
            let catalog = self.ensure_catalog();
            let default_entry = catalog
                .character(&preferred_character)
                .or_else(|| catalog.characters().first())
                .and_then(|character| {
                    character
                        .costumes
                        .iter()
                        .find(|costume| costume.id == preferred_costume)
                        .or_else(|| character.costumes.first())
                        .map(|costume| {
                            (
                                character.key.clone(),
                                costume.id.clone(),
                                crate::role::entry_for_selection(&character.key, costume),
                            )
                        })
                });
            let Some((character, costume, entry)) = default_entry else {
                return false;
            };
            self.config.models.push(entry);
            self.config.character = character;
            self.config.costume = costume;
            self.config_dirty = true;
        }
        !self.config.models.is_empty()
    }

    fn create_roles(&mut self, event_loop: &ActiveEventLoop) {
        if !self.roles.is_empty() || self.headless {
            return;
        }
        if !self.ensure_role_entries() {
            warn!("no model available to display");
            self.open_settings(SettingsPage::Model);
            return;
        }
        self.reconcile_roles(event_loop);
        if self.roles.is_empty() || self.load.is_first_run() {
            self.open_settings(SettingsPage::Model);
        }
    }

    fn role_index(&self, id: WindowId) -> Option<usize> {
        self.roles.iter().position(|role| role.window.id() == id)
    }

    fn open_settings(&mut self, page: SettingsPage) {
        if self.ipc.is_connected() {
            self.pending_settings_page = None;
            self.ipc.send(&Message::OpenSettings { page });
        } else {
            self.pending_settings_page = Some(page);
        }
        self.ui.ensure_running();
    }

    fn send_snapshot(&self) {
        let model = self
            .active_role()
            .map(|role| ModelData {
                character: role.character.clone(),
                costume: role.costume.clone(),
                format: format!("{:?}", role.info().format).to_lowercase(),
                motions: role.info().motion_groups.clone(),
                expressions: role.info().expression_names.clone(),
                drawables: role.drawable_count(),
                textures: role.info().texture_count,
            })
            .unwrap_or_default();
        self.ipc.send(&Message::ConfigSnapshot {
            config: serde_json::to_value(&self.config).unwrap_or_default(),
            model,
            // A missing config or an empty model list is the real "first run"; an
            // empty role list right after start-up is not.
            first_run: self.load.is_first_run() || self.config.models.is_empty(),
        });
    }

    fn handle_patch(&mut self, mut patch: pet_config::SettingsPatch) {
        self.sync_models_from_roles();
        // A UI snapshot can predate a drag. The host owns live placement, so
        // preserve it when a model-list patch keeps or changes a role's costume.
        if let Some(value) = patch.values.get_mut("models") {
            if let Ok(mut entries) =
                serde_json::from_value::<Vec<pet_config::ModelEntry>>(value.clone())
            {
                for entry in &mut entries {
                    if let Some(current) = self
                        .config
                        .models
                        .iter()
                        .find(|current| current.character == entry.character)
                    {
                        entry.window_placement = current.window_placement.clone();
                        entry.sync_placement_mirror();
                    }
                }
                *value = serde_json::json!(entries);
            }
        }
        self.expand_preset(&mut patch);
        let level = patch.highest_apply_level();
        let path = self.paths.config();
        let outcome =
            pet_config::apply_patch(&path, &mut self.config, &patch).and_then(|outcome| {
                if self.config_dirty && !outcome.written {
                    pet_config::save_config(&path, &self.config)?;
                }
                Ok(outcome)
            });
        match outcome {
            Ok(outcome) => {
                self.config_dirty = false;
                self.last_config_save = Instant::now();
                info!(?level, written = outcome.written, "settings applied");
                // Model lists also need reconciling when another key in the
                // same patch has a higher apply level (e.g. scale or vsync).
                if patch.values.keys().any(|key| {
                    matches!(
                        key.split('.').next().unwrap_or(key),
                        "models" | "character" | "costume"
                    )
                }) {
                    self.roles_dirty = true;
                }
                self.apply_level(level);
                self.sync_models_from_roles();
                if self.config_dirty {
                    self.persist_now();
                }
                self.ipc.send(&Message::PatchApplied {
                    ok: true,
                    rejected: outcome.rejected,
                    config: serde_json::to_value(&self.config).unwrap_or_default(),
                });
            }
            Err(error) => {
                error!(%error, "failed to save settings");
                self.ipc.send(&Message::PatchApplied {
                    ok: false,
                    rejected: vec!["*".to_owned()],
                    config: serde_json::to_value(&self.config).unwrap_or_default(),
                });
                self.ipc.send(&Message::Error {
                    kind: "config".to_owned(),
                    message: format!("保存设置失败：{error}"),
                });
            }
        }
        self.send_snapshot();
        self.broadcast_model_data();
    }

    /// Resolves a built-in preset name into literal `click_motion_actions` entries.
    ///
    /// The original resolves presets once, at settings-apply time, against the live
    /// model, and stores the resulting names (spec §2.6). The runtime never
    /// re-resolves a preset tag.
    fn expand_preset(&mut self, patch: &mut pet_config::SettingsPatch) {
        let Some(serde_json::Value::String(profile)) =
            patch.values.get("click_motion_active_profile").cloned()
        else {
            return;
        };
        if !pet_config::is_builtin_name(&profile) {
            return;
        }
        let Some(role) = self.active_role() else {
            return;
        };
        let motions = role.motion_names().to_vec();
        let expressions = role.expression_names().to_vec();
        let character = role.character.clone();
        let mut rng = pet_util::Rng::from_entropy();
        let actions =
            pet_input::resolve_preset(&profile, &motions, &expressions, &character, &mut rng);
        for (region, action) in actions {
            patch.values.insert(
                format!("click_motion_actions.{region}.motion"),
                serde_json::json!(action.motion),
            );
            patch.values.insert(
                format!("click_motion_actions.{region}.expression"),
                serde_json::json!(action.expression),
            );
        }
        info!(profile = %profile, "preset expanded against the live model");
    }

    fn broadcast_model_data(&self) {
        if let Some(role) = self.active_role() {
            self.ipc.send(&Message::ModelData(ModelData {
                character: role.character.clone(),
                costume: role.costume.clone(),
                format: format!("{:?}", role.info().format).to_lowercase(),
                motions: role.info().motion_groups.clone(),
                expressions: role.info().expression_names.clone(),
                drawables: role.drawable_count(),
                textures: role.info().texture_count,
            }));
        }
    }

    fn apply_level(&mut self, level: ApplyLevel) {
        for role in &mut self.roles {
            role.set_behavior_config(&self.config);
            role.apply_display_settings(&self.config);
        }
        if self.config.auto_start != self.auto_start_applied {
            match pet_window::set_auto_start(&self.paths.pet_executable(), self.config.auto_start) {
                Ok(()) => {
                    self.auto_start_applied = self.config.auto_start;
                    info!(enabled = self.config.auto_start, "autostart updated");
                }
                Err(error) => warn!(%error, "failed to update autostart"),
            }
        }
        match level {
            ApplyLevel::Instant => {}
            ApplyLevel::ReloadModel => {
                self.roles_dirty = true;
            }
            ApplyLevel::RebuildWindow => {
                for role in &mut self.roles {
                    let dpr = role.window.scale_factor();
                    if let Err(error) = role.apply_scale(&self.config, dpr) {
                        error!(%error, "failed to rebuild role window");
                    }
                    role.apply_display_settings(&self.config);
                    role.apply_obs_capture(self.config.obs_window_capture_compatible);
                }
            }
            ApplyLevel::RestartApp => {
                self.ipc.send(&Message::Error {
                    kind: "restart".to_owned(),
                    message: "该设置将在下次启动时生效".to_owned(),
                });
            }
        }
    }

    fn active_role(&self) -> Option<&Role> {
        self.roles.iter().find(|role| {
            role.character == self.config.character && role.costume == self.config.costume
        })
    }

    fn reconcile_roles(&mut self, event_loop: &ActiveEventLoop) {
        self.roles_dirty = false;
        if self.headless {
            return;
        }
        let entries: Vec<_> = self.config.models.clone();
        let mut new_roles: Vec<Role> = Vec::new();
        let mut reusable: Vec<Role> = std::mem::take(&mut self.roles);
        for entry in entries {
            // Reuse the window when the character/costume pair is unchanged.
            if let Some(position) = reusable.iter().position(|role| {
                role.character == entry.character
                    && role.costume == entry.costume
                    && role.entry.path == entry.path
            }) {
                let mut role = reusable.remove(position);
                role.set_behavior_config(&self.config);
                role.apply_display_settings(&self.config);
                new_roles.push(role);
                continue;
            }
            if let Some(position) = reusable
                .iter()
                .position(|role| role.character == entry.character)
            {
                let mut role = reusable.remove(position);
                match role.swap_model(&entry, &self.cache, &self.config) {
                    Ok(()) => {
                        role.set_behavior_config(&self.config);
                        role.apply_display_settings(&self.config);
                        new_roles.push(role);
                    }
                    Err(error) => {
                        error!(%error, "failed to swap model");
                        self.ipc.send(&Message::Error {
                            kind: "model".to_owned(),
                            message: format!(
                                "无法加载角色 {} 的服装 {}：{error}",
                                entry.character, entry.costume
                            ),
                        });
                        new_roles.push(role);
                    }
                }
            } else {
                match Role::create(
                    event_loop,
                    &entry,
                    &self.config,
                    &self.paths,
                    &self.cache,
                    self.gpu.clone(),
                ) {
                    Ok(mut role) => {
                        self.gpu = Some(role.renderer.gpu().clone());
                        if entry.window_placement.is_unset() {
                            let occupied: Vec<_> = new_roles
                                .iter()
                                .chain(reusable.iter())
                                .filter_map(Role::window_rect)
                                .collect();
                            role.place_apart(&occupied);
                        }
                        role.set_hidden(self.hidden);
                        role.apply_display_settings(&self.config);
                        role.record_placement();
                        new_roles.push(role);
                    }
                    Err(error) => {
                        error!(character = %entry.character, %error, "failed to create role window");
                        self.ipc.send(&Message::Error {
                            kind: "model".to_owned(),
                            message: format!("无法加载角色 {}：{error}", entry.character),
                        });
                    }
                }
            }
        }
        self.drag_peer = None;
        self.roles = new_roles;
        if self.roles.is_empty() {
            warn!("no roles left after reload");
        }
        self.persist_now();
        self.send_snapshot();
        self.broadcast_model_data();
    }

    /// Applies this tick's drag delta to every other role window (spec §2.8).
    fn broadcast_peer_drag(&mut self, index: usize) {
        let Some((x, y, ..)) = self.roles.get(index).and_then(Role::window_rect) else {
            return;
        };
        let previous = self
            .drag_peer
            .filter(|(dragged, _, _)| *dragged == index)
            .map(|(_, position, _)| position);
        if let Some((old_x, old_y)) = previous {
            let dx = x - old_x;
            let dy = y - old_y;
            if dx != 0 || dy != 0 {
                for other_index in 0..self.roles.len() {
                    if other_index == index {
                        continue;
                    }
                    let role = &mut self.roles[other_index];
                    if let Some((other_x, other_y, ..)) = role.window_rect() {
                        role.set_position(other_x + dx, other_y + dy);
                        role.base_position = (other_x + dx, other_y + dy);
                    }
                }
            }
        }
        self.drag_peer = Some((index, (x, y), (0, 0)));
    }

    fn reset_positions(&mut self) {
        let mut occupied = Vec::new();
        for role in &mut self.roles {
            let monitors = placement::monitors_for_scale(role.window.scale_factor());
            let primary = monitors.iter().find(|monitor| monitor.is_primary).cloned();
            let size = role.size();
            let resolved = placement::resolve_position(
                &pet_config::Placement::default(),
                size,
                &monitors,
                primary.as_ref(),
            );
            role.set_position(resolved.x, resolved.y);
            role.place_apart(&occupied);
            role.record_placement();
            if let Some(rect) = role.window_rect() {
                occupied.push(rect);
            }
        }
        self.persist_now();
        self.send_snapshot();
        info!("all role positions reset");
    }

    fn drain_ipc(&mut self) {
        while let Some(event) = self.ipc.poll() {
            match event {
                IpcEvent::Connected => {
                    info!("settings process connected");
                    self.ui.note_connected();
                    self.send_snapshot();
                    if let Some(page) = self.pending_settings_page.take() {
                        self.ipc.send(&Message::OpenSettings { page });
                    }
                }
                IpcEvent::Disconnected => {
                    info!("settings process disconnected");
                    self.ui.note_disconnected();
                }
                IpcEvent::Message(message) => self.handle_ipc_message(message),
            }
        }
    }

    fn handle_ipc_message(&mut self, message: Message) {
        match message {
            Message::RequestSnapshot => self.send_snapshot(),
            Message::SettingsPatch { patch } => self.handle_patch(patch),
            Message::MenuAct { action } => {
                self.handle_menu_action(&action);
            }
            Message::MenuLocked { locked } => {
                self.config.drag_locked = locked;
                self.config_dirty = true;
                for role in &mut self.roles {
                    role.set_behavior_config(&self.config);
                }
                self.send_snapshot();
            }
            Message::ResetPositions => self.reset_positions(),
            Message::ModelSelect { character, costume } => {
                let found = self.ensure_catalog().find(&character, &costume).cloned();
                let Some(entry) = found
                    .as_ref()
                    .map(|entry| crate::role::entry_for_selection(&character, entry))
                else {
                    warn!(%character, %costume, "unknown selection");
                    return;
                };
                let mut models = self.config.models.clone();
                if let Some(existing) = models.iter_mut().find(|model| model.character == character)
                {
                    let placement = existing.window_placement.clone();
                    *existing = entry;
                    existing.window_placement = placement;
                    existing.sync_placement_mirror();
                } else {
                    models.push(entry);
                }
                let mut picker = self.config.model_picker_state.clone();
                picker.remember(&character, &costume);
                self.handle_patch(pet_config::SettingsPatch::from_pairs([
                    ("models", serde_json::json!(models)),
                    ("character", serde_json::json!(character)),
                    ("costume", serde_json::json!(costume)),
                    ("model_picker_state", serde_json::json!(picker)),
                ]));
            }
            Message::OpenModelFolder => {
                let folder = self.paths.models();
                let _ = std::fs::create_dir_all(&folder);
                let _ = std::process::Command::new("explorer").arg(folder).spawn();
            }
            Message::ModelsChanged { characters } => {
                self.catalog = None;
                // ModelCache compares archive size/mtime and re-extracts the new
                // costume. Keep the existing window and placement on updates.
                for role in &mut self.roles {
                    if !characters.contains(&role.character) {
                        continue;
                    }
                    let entry = role.entry.clone();
                    if let Err(error) = role.swap_model(&entry, &self.cache, &self.config) {
                        self.ipc.send(&Message::Error {
                            kind: "model".into(),
                            message: format!(
                                "模型已更新，但重新加载 {} 失败：{error}",
                                role.character
                            ),
                        });
                    }
                }
                // Configured roles whose packages were missing can now load.
                self.roles_dirty = true;
                self.send_snapshot();
            }
            Message::SettingsClosed => {
                // Nothing to do: the settings window lives in another process.
            }
            Message::QuitRequest => {
                self.quit = true;
            }
            Message::Quit => {}
            other => {
                warn!(?other, "unexpected ipc message from ui");
            }
        }
    }

    fn handle_menu_action(&mut self, action: &str) {
        match action {
            "costume" => self.open_settings(SettingsPage::Model),
            "motion" => {
                let now = self.now();
                if let Some(role) = self
                    .roles
                    .iter_mut()
                    .find(|role| role.character == self.config.character)
                {
                    role.play_menu_motion(now);
                }
            }
            "settings" => self.open_settings(SettingsPage::Model),
            "hide" => {
                self.hidden = true;
                for role in &mut self.roles {
                    role.set_hidden(true);
                }
            }
            other => warn!(action = other, "unknown menu action"),
        }
    }

    /// Copies live role state (placement, character/costume) back into the config.
    fn sync_models_from_roles(&mut self) {
        // Keep the configured list authoritative, including models that failed
        // to load. A stale/failed window must never replace a requested model.
        for entry in &mut self.config.models {
            if let Some(role) = self.roles.iter().find(|role| {
                role.character == entry.character
                    && role.costume == entry.costume
                    && role.entry.path == entry.path
            }) {
                if entry.window_placement != role.placement {
                    entry.window_placement = role.placement.clone();
                    self.config_dirty = true;
                }
                entry.sync_placement_mirror();
            }
        }
    }

    fn persist_now(&mut self) {
        self.sync_models_from_roles();
        let path = self.paths.config();
        self.last_config_save = Instant::now();
        if let Err(error) = pet_config::save_config(&path, &self.config) {
            self.config_dirty = true;
            error!(%error, "failed to write config");
            self.ipc.send(&Message::Error {
                kind: "config".to_owned(),
                message: format!("保存设置失败：{error}"),
            });
        } else {
            self.config_dirty = false;
        }
    }

    fn drain_tray(&mut self) {
        while let Ok(event) = self.tray_events.try_recv() {
            match event {
                TrayEvent::ToggleVisible => {
                    self.hidden = !self.hidden;
                    let hidden = self.hidden;
                    for role in &mut self.roles {
                        role.set_hidden(hidden);
                    }
                }
                TrayEvent::Opacity(value) => {
                    self.config.opacity = value;
                    for role in &mut self.roles {
                        role.apply_display_settings(&self.config);
                    }
                    self.persist_now();
                }
                TrayEvent::ResetPositions => self.reset_positions(),
                TrayEvent::OpenSettings => self.open_settings(SettingsPage::Model),
                TrayEvent::Quit => self.quit = true,
            }
        }
        if let Some(tray) = self.tray.as_mut() {
            tray.poll();
        }
    }

    fn tick_roles(&mut self) {
        let dt = self.last_frame.elapsed().as_secs_f32().clamp(0.001, 0.1);
        self.last_frame = Instant::now();
        let now = self.now();
        let now_ms = self.now_ms();
        let fps = self.config.fps.max(30);
        let frame_budget_ms = (1000 / fps) as u64;

        // Peer centers for mutual gaze (recomputed every tick, cheap for ≤8 roles).
        let centers: Vec<(f32, f32)> = self.roles.iter().map(Role::center).collect();

        for index in 0..self.roles.len() {
            let peers: Vec<(f32, f32)> = centers
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, center)| *center)
                .collect();
            let own = centers[index];
            let nearest_peer = peers
                .iter()
                .min_by(|left, right| {
                    let left_distance = (left.0 - own.0).powi(2) + (left.1 - own.1).powi(2);
                    let right_distance = (right.0 - own.0).powi(2) + (right.1 - own.1).powi(2);
                    left_distance
                        .partial_cmp(&right_distance)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .copied();
            let role = &mut self.roles[index];
            role.advance_model(dt);
            let idle_seconds = now; // the behavior keeps its own interaction clock
            role.tick_behavior(now, dt, idle_seconds, nearest_peer, own);
            role.guard_topmost(self.config.game_topmost);
            role.poll_passthrough(&self.config, now_ms);
        }
        let _ = frame_budget_ms;

        // Persist while dragging-induced changes settle.
        if self.config_dirty && self.last_config_save.elapsed() > Duration::from_secs(2) {
            self.persist_now();
        }
    }

    fn request_frames(&self, event_loop: &ActiveEventLoop) {
        for role in &self.roles {
            role.window.request_redraw();
        }
        let fps = self.config.fps.clamp(30, 240);
        let budget = Duration::from_millis(((1000 / fps) as u64).max(MIN_FRAME_MS));
        event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
            Instant::now() + budget,
        ));
    }

    fn show_radial_menu(&mut self, role_index: usize) {
        let Some(role) = self.roles.get(role_index) else {
            return;
        };
        let (x, y, width, height) =
            role.window_rect()
                .unwrap_or((role.base_position.0, role.base_position.1, 0, 0));
        let center = (x + width as i32 / 2, y + height as i32 / 2);
        // Cross-process coordinates on macOS use logical desktop points so the
        // popup can select its own backing scale on mixed Retina displays.
        #[cfg(target_os = "macos")]
        let center = (
            (center.0 as f64 / role.window.scale_factor()).round() as i32,
            (center.1 as f64 / role.window.scale_factor()).round() as i32,
        );
        self.config.character = role.character.clone();
        self.config.costume = role.costume.clone();
        self.config_dirty = true;
        self.send_snapshot();
        self.ipc.send(&Message::MenuShow {
            x: center.0,
            y: center.1,
            items: pet_ipc::MenuItem::lite_items(&self.config.language),
            locked: self.config.drag_locked,
        });
        self.ui.ensure_running();
    }

    fn save_and_exit(&mut self) {
        for role in &mut self.roles {
            role.record_placement();
        }
        self.persist_now();
        self.ipc.send(&Message::Quit);
        self.ui.shutdown();
    }
}

impl ApplicationHandler for PetApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.create_roles(event_loop);
        self.finish_startup();
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(index) = self.role_index(window_id) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                // The pet window never closes; hide it instead.
                if let Some(role) = self.roles.get_mut(index) {
                    role.set_hidden(true);
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(role) = self.roles.get_mut(index) {
                    role.renderer.resize(size, role.window.scale_factor());
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(role) = self.roles.get_mut(index) {
                    let size = role.window.inner_size();
                    role.renderer.resize(size, role.window.scale_factor());
                    #[cfg(target_os = "macos")]
                    role.record_placement();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor = (position.x as f32, position.y as f32);
                let moved = if let Some(role) = self.roles.get_mut(index) {
                    role.set_cursor(Some(cursor));
                    role.on_cursor_moved_drag(cursor)
                } else {
                    false
                };
                if moved && self.config.move_all_roles_together {
                    self.broadcast_peer_drag(index);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(role) = self.roles.get_mut(index) {
                    role.set_cursor(None);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let now_ms = self.now_ms();
                let interaction_time = self.now();
                let Some(role) = self.roles.get_mut(index) else {
                    return;
                };
                match state {
                    ElementState::Pressed => {
                        let cursor = role.cursor_local().unwrap_or((0.0, 0.0));
                        let on_model = role.on_left_press(cursor, &self.config, now_ms);
                        if on_model {
                            role.note_interaction(interaction_time);
                            self.drag_peer =
                                role.window_rect().map(|(x, y, ..)| (index, (x, y), (0, 0)));
                        }
                    }
                    ElementState::Released => {
                        let cursor = role.cursor_local().unwrap_or((0.0, 0.0));
                        let (moved, delta) = role.on_left_release();
                        if !moved {
                            let now = interaction_time;
                            let region = role.region_at(cursor);
                            let size = role.window.inner_size();
                            let motion_names = role.info().motion_groups.clone();
                            let expression_names = role.info().expression_names.clone();
                            let character = role.character.clone();
                            let context = pet_behavior::BehaviorContext {
                                now,
                                dt: 0.0,
                                cursor_local: Some(cursor),
                                surface: (size.width, size.height),
                                idle_seconds: 0.0,
                                hour: pet_util::time_of_day::local_hour(),
                                peer_center: None,
                                own_center: role.center(),
                                motion_names: &motion_names,
                                expression_names: &expression_names,
                                character: &character,
                                motion_finished: role.model.is_motion_finished(),
                            };
                            for command in role.on_click(region, &context) {
                                role.apply_external_command(&command);
                            }
                            self.config_dirty = true;
                        } else if delta.0.abs() + delta.1.abs() > 0.0 {
                            self.config_dirty = true;
                            if self.config.move_all_roles_together {
                                for peer in &mut self.roles {
                                    peer.record_placement();
                                }
                            }
                        }
                        self.drag_peer = None;
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                self.show_radial_menu(index);
            }
            WindowEvent::RedrawRequested => {
                if let Some(role) = self.roles.get_mut(index) {
                    role.mark_updated();
                    role.render();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(limit) = self.exit_after {
            if self.now() >= limit {
                info!(limit, "smoke run finished, exiting");
                self.quit = true;
            }
        }
        if self.quit {
            event_loop.exit();
            return;
        }
        self.drain_ipc();
        if self.roles_dirty {
            self.reconcile_roles(event_loop);
        }
        self.drain_tray();
        self.ui.tick();
        self.tick_roles();
        self.request_frames(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.save_and_exit();
    }
}

impl PetApp {
    fn finish_startup(&mut self) {
        // The tray is built only after the first frame so it cannot delay it.
        if self.create_tray && self.tray.is_none() {
            if let Some(sender) = self.tray_sender.take() {
                self.tray = Tray::new(sender)
                    .map_err(|error| {
                        warn!(%error, "tray icon unavailable");
                        error
                    })
                    .ok();
            }
        }
        if let Some(role) = self.roles.first() {
            let info = role.renderer.adapter_info();
            info!(
                adapter = %info.name,
                alpha = ?role.renderer.alpha_mode(),
                size = ?role.renderer.size(),
                render_size = ?role.renderer.render_size(),
                "first role ready"
            );
        }
        self.ui.ensure_running();
        // Start the UI hidden so the pet appears first (spec: first frame before
        // background work).
        self.send_snapshot();
    }

    /// Exposes the config for diagnostics/tests.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Paths of this installation.
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// Number of live role windows.
    pub fn role_count(&self) -> usize {
        self.roles.len()
    }

    /// Requests a clean shutdown (used by tests).
    pub fn request_quit(&mut self) {
        self.quit = true;
    }

    /// Feeds one IPC message through the normal handler (tests only).
    pub fn handle_message_for_test(&mut self, message: Message) {
        self.handle_ipc_message(message);
    }

    /// Access to the catalog (tests only; scans on first use).
    pub fn catalog_for_test(&mut self) -> &Catalog {
        self.ensure_catalog()
    }
}

/// Entry point used by both binaries and integration tests.
pub fn run(paths: Paths) -> AppResult<()> {
    let app = PetApp::new(paths)?;
    let mut builder = winit::event_loop::EventLoop::builder();
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_activate_ignoring_other_apps(false)
            .with_default_menu(false);
    }
    let event_loop = builder.build()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut app = app;
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Convenience for the About page (delegates to `pet-config`).
pub fn about_lines(paths: &Paths, config: &Config) -> Vec<(String, String)> {
    pet_config::paths::about_lines(paths, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pet_ipc::Connection;
    use std::sync::mpsc::channel;

    #[test]
    fn missing_or_invalid_models_are_not_added_as_roles() {
        let root = tempfile::tempdir().unwrap();
        let session = format!("model-startup-{}", std::process::id());
        let mut app = PetApp::new_with(Paths::with_root(root.path()), &session, false).unwrap();
        assert!(app.load.is_first_run());
        assert!(!app.ensure_role_entries());

        std::fs::create_dir_all(root.path().join("models")).unwrap();
        std::fs::write(root.path().join("models/kasumi.zst"), b"broken model").unwrap();
        app.handle_ipc_message(Message::ModelsChanged {
            characters: vec!["kasumi".into()],
        });
        assert!(!app.ensure_role_entries());

        let file = std::fs::File::create(root.path().join("models/kasumi.zst")).unwrap();
        let encoder = zstd::stream::write::Encoder::new(file, 1).unwrap();
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(2);
        header.set_cksum();
        tar.append_data(&mut header, "casual/model.json", b"{}".as_slice())
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        app.handle_ipc_message(Message::ModelsChanged {
            characters: vec!["kasumi".into()],
        });
        assert!(app.ensure_role_entries());
        assert_eq!(app.config.models[0].character, "kasumi");
    }

    #[test]
    fn settings_menu_and_tray_open_the_role_list() {
        let root = tempfile::tempdir().unwrap();
        let session = format!("settings-entry-{}", std::process::id());
        let mut app = PetApp::new_with(Paths::with_root(root.path()), &session, false).unwrap();
        app.handle_menu_action("settings");
        assert_eq!(app.pending_settings_page, Some(SettingsPage::Model));
        app.pending_settings_page = None;
        app.tray_sender
            .as_ref()
            .unwrap()
            .send(TrayEvent::OpenSettings)
            .unwrap();
        app.drain_tray();
        assert_eq!(app.pending_settings_page, Some(SettingsPage::Model));
    }

    #[test]
    fn settings_open_request_survives_a_delayed_ui_connection() {
        let session = format!("pending-settings-{}", std::process::id());
        let root = std::env::temp_dir().join(&session);
        std::fs::create_dir_all(&root).unwrap();
        let mut app = PetApp::new_with(Paths::with_root(root), &session, false).unwrap();
        // Startup requests downloads; a later user request wins.
        app.open_settings(SettingsPage::Downloads);
        app.open_settings(SettingsPage::Display);

        let pipe = pipe_name(&session);
        let (tx, rx) = channel();
        let client = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut connection = loop {
                match Connection::connect(&pipe) {
                    Ok(connection) => break connection,
                    Err(_) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("connect: {error}"),
                }
            };
            for _ in 0..3 {
                tx.send(connection.recv().unwrap().unwrap()).unwrap();
            }
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        while app.pending_settings_page.is_some() && Instant::now() < deadline {
            app.drain_ipc();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(app.pending_settings_page, None);
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Message::ConfigSnapshot { .. }
        ));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Message::OpenSettings {
                page: SettingsPage::Display
            }
        );
        app.open_settings(SettingsPage::Behavior);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Message::OpenSettings {
                page: SettingsPage::Behavior
            }
        );
        client.join().unwrap();
    }
}
