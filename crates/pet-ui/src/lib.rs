//! Slint settings window and radial menu, wired to `bandori-pet` over the pipe.

use std::cell::RefCell;
use std::rc::Rc;

mod downloads;
mod picker;
mod preview;
mod radial;
mod texts;
#[cfg(test)]
use radial::items_to_slint;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pet_assets::catalog::Catalog;
use pet_config::{ClickAction, Config, Paths, SettingsPatch, CLICK_REGIONS};
use pet_i18n::{Key, Lang};
use pet_ipc::{Connection, Message, ModelData, SettingsPage};
use slint::winit_030::WinitWindowAccessor;
use slint::{Model, ModelRc, Timer, TimerMode, VecModel};
use tracing::{info, warn};

slint::include_modules!();

/// Errors surfaced by the settings front end.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    /// Slint could not create a window.
    #[error("slint error: {0}")]
    Slint(#[from] slint::PlatformError),
    /// Filesystem problem.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Shared view state updated by the IPC pump.
struct Shared {
    config: Mutex<Option<Config>>,
    model: Mutex<ModelData>,
    connected: AtomicBool,
    /// Language currently rendered by the window and the radial menu.
    language: Mutex<Option<Lang>>,
}

/// Message sender shared by every callback.
type Sender = Rc<dyn Fn(&Message) -> bool>;

/// Runs the hidden UI service until the pet process asks it to quit.
pub fn run(pipe: &str, paths: &Paths) -> Result<(), UiError> {
    let creating_popup = radial::initialize_backend()?;
    let window = SettingsWindow::new()?;
    window.set_obs_supported(cfg!(windows));
    texts::apply(&window, Lang::default());
    window.set_status(window.get_t().status_connecting.clone());
    creating_popup.set(true);
    let radial = RadialMenu::new()?;
    texts::apply_radial(&radial, Lang::default());
    // Slint creates its window adapter lazily. Capture the popup attributes
    // while this component, rather than the settings window, is being built.
    let _ = radial.window().has_winit_window();
    creating_popup.set(false);
    let radial_controller = radial::Controller::new(&radial);
    let radial_weak = radial.as_weak();
    let index = pet_assets::index::load_or_scan(&paths.models());
    let catalog = Rc::new(RefCell::new(Catalog::build(&paths.assets(), &index)));
    info!(
        characters = catalog.borrow().len(),
        costumes = index.costume_count(),
        "settings catalog loaded"
    );

    let shared = Arc::new(Shared {
        config: Mutex::new(None),
        model: Mutex::new(ModelData::default()),
        connected: AtomicBool::new(false),
        language: Mutex::new(Some(Lang::default())),
    });

    // ── IPC client thread ────────────────────────────────────────────────────
    let (message_tx, message_rx) = channel::<Message>();
    let writer: Arc<Mutex<Option<Connection>>> = Arc::new(Mutex::new(None));
    spawn_client(pipe.to_owned(), message_tx, Arc::clone(&writer));
    let send: Sender = Rc::new({
        let writer = Arc::clone(&writer);
        move |message: &Message| {
            let Ok(mut guard) = writer.lock() else {
                return false;
            };
            let Some(connection) = guard.as_mut() else {
                return false;
            };
            connection.send(message).is_ok()
        }
    });
    {
        let send = Rc::clone(&send);
        window.window().on_close_requested(move || {
            send(&Message::SettingsClosed);
            slint::CloseRequestResponse::HideWindow
        });
    }

    // ── Character / costume lists ────────────────────────────────────────────
    let picker = picker::Picker::new(Rc::clone(&catalog), paths, Rc::clone(&send))?;
    picker.wire(&window);
    picker.refresh(&window);
    let downloads = downloads::Downloads::new(
        paths,
        &index,
        Rc::clone(&catalog),
        Rc::clone(&picker),
        Rc::clone(&send),
    )?;
    downloads.wire(&window);
    downloads.refresh(&window);
    window.set_about_lines(ModelRc::new(VecModel::from(
        pet_config::about_lines(paths, &Config::default(), Lang::default())
            .into_iter()
            .map(|(label, value)| AboutLine {
                label: label.into(),
                value: value.into(),
            })
            .collect::<Vec<_>>(),
    )));

    {
        let send = Rc::clone(&send);
        let window_weak = window.as_weak();
        let picker = Rc::clone(&picker);
        let shared = Arc::clone(&shared);
        let catalog = Rc::clone(&catalog);
        window.on_apply_selection(move |character, costume| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let catalog = catalog.borrow();
            let Some(selected) = catalog.find(&character, &costume) else {
                return;
            };
            let mut config = shared
                .config
                .lock()
                .ok()
                .and_then(|value| value.clone())
                .unwrap_or_default();
            let entry =
                pet_config::ModelEntry::new(&character, &costume, &selected.path, selected.format);
            if let Some(existing) = config
                .models
                .iter_mut()
                .find(|entry| entry.character == character.as_str())
            {
                existing.costume = entry.costume;
                existing.path = entry.path;
                existing.format = entry.format;
            } else if !window.get_adding_model() && !config.models.is_empty() {
                let index = config
                    .models
                    .iter()
                    .position(|entry| entry.character == config.character)
                    .unwrap_or(0);
                let mut entry = entry;
                entry.window_placement = config.models[index].window_placement.clone();
                entry.sync_placement_mirror();
                config.models[index] = entry;
            } else {
                config.models.push(entry);
            }
            config.model_picker_state = picker.remember_selection(&character, &costume);
            let request = Message::SettingsPatch {
                patch: SettingsPatch::from_pairs([
                    ("models", serde_json::json!(config.models)),
                    ("character", serde_json::json!(character.as_str())),
                    ("costume", serde_json::json!(costume.as_str())),
                    (
                        "model_picker_state",
                        serde_json::json!(config.model_picker_state),
                    ),
                ]),
            };
            if send(&request) {
                picker.close_preview(&window);
                window.set_status(
                    texts::lang_of(&window)
                        .text(Key::StatusSwitchingModel)
                        .into(),
                );
                window.set_picking_model(false);
            } else {
                window.set_status(texts::lang_of(&window).text(Key::StatusNotConnected).into());
            }
        });
    }

    {
        let send = Rc::clone(&send);
        let weak = window.as_weak();
        window.on_apply_options(move || {
            if let Some(window) = weak.upgrade() {
                let patch = SettingsPatch::from_pairs([
                    ("game_topmost", serde_json::json!(window.get_topmost())),
                    (
                        "obs_window_capture_compatible",
                        serde_json::json!(window.get_obs()),
                    ),
                    (
                        "hide_live2d_model",
                        serde_json::json!(window.get_hide_model()),
                    ),
                    ("auto_start", serde_json::json!(window.get_autostart())),
                    ("language", serde_json::json!(texts::lang_of(&window).tag())),
                ]);
                if send(&Message::SettingsPatch { patch }) {
                    window.set_options_dirty(false);
                } else {
                    window.set_status(texts::lang_of(&window).text(Key::StatusNotConnected).into());
                }
            }
        });
    }
    {
        let send = Rc::clone(&send);
        let shared = Arc::clone(&shared);
        let weak = window.as_weak();
        window.on_select_model(move |index| {
            let Some(config) = shared.config.lock().ok().and_then(|value| value.clone()) else {
                return;
            };
            let Some(entry) = usize::try_from(index)
                .ok()
                .and_then(|index| config.models.get(index))
            else {
                return;
            };
            let patch = SettingsPatch::from_pairs([
                ("character", serde_json::json!(entry.character)),
                ("costume", serde_json::json!(entry.costume)),
            ]);
            if let Some(window) = weak.upgrade() {
                if send(&Message::SettingsPatch { patch }) {
                    window.set_picking_model(false);
                    window.set_adding_model(false);
                } else {
                    window.set_status(texts::lang_of(&window).text(Key::StatusNotConnected).into());
                }
            }
        });
    }
    {
        let send = Rc::clone(&send);
        let shared = Arc::clone(&shared);
        let weak = window.as_weak();
        window.on_remove_model(move |index| {
            let Some(mut config) = shared.config.lock().ok().and_then(|value| value.clone()) else {
                return;
            };
            if index < 0 || index as usize >= config.models.len() {
                return;
            }
            if config.models.len() <= 1 {
                if let Some(window) = weak.upgrade() {
                    window.set_status(texts::lang_of(&window).text(Key::StatusKeepOneRole).into());
                }
                return;
            }
            config.models.remove(index as usize);
            let active = config.active_entry();
            let patch = SettingsPatch::from_pairs([
                ("models", serde_json::json!(config.models)),
                (
                    "character",
                    serde_json::json!(active.map(|entry| entry.character.as_str()).unwrap_or("")),
                ),
                (
                    "costume",
                    serde_json::json!(active.map(|entry| entry.costume.as_str()).unwrap_or("")),
                ),
            ]);
            if !send(&Message::SettingsPatch { patch }) {
                if let Some(window) = weak.upgrade() {
                    window.set_status(texts::lang_of(&window).text(Key::StatusNotConnected).into());
                }
            }
        });
    }

    // ── Patch plumbing ───────────────────────────────────────────────────────
    wire_patches(&window, &send);

    // ── Radial menu ──────────────────────────────────────────────────────────
    {
        let send = Rc::clone(&send);
        let controller = Rc::clone(&radial_controller);
        radial.on_action(move |action| {
            send(&Message::MenuAct {
                action: action.to_string(),
            });
            controller.dismiss();
        });
    }
    {
        let send = Rc::clone(&send);
        let controller = Rc::clone(&radial_controller);
        radial.on_locked_toggled(move |locked| {
            let sent = send(&Message::MenuLocked { locked });
            controller.lock_requested(locked, sent);
        });
    }

    // ── Message pump ─────────────────────────────────────────────────────────
    let pump_window = window.as_weak();
    let pump_radial = Rc::clone(&radial_controller);
    let pump_shared = Arc::clone(&shared);
    let pump_send = Rc::clone(&send);
    let pump_paths = paths.clone();
    let pump_catalog = Rc::clone(&catalog);
    let pump_picker = Rc::clone(&picker);
    let pump_downloads = Rc::clone(&downloads);
    let pump_radial_weak = radial_weak.clone();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(window) = pump_window.upgrade() else {
            return;
        };
        while let Ok(message) = message_rx.try_recv() {
            match message {
                Message::ConfigSnapshot {
                    config,
                    model,
                    first_run,
                } => {
                    let mut parsed: Config = serde_json::from_value(config).unwrap_or_default();
                    parsed.normalize_models();
                    pump_radial.sync_locked(parsed.drag_locked);
                    if let Ok(mut guard) = pump_shared.config.lock() {
                        *guard = Some(parsed.clone());
                    }
                    if let Ok(mut guard) = pump_shared.model.lock() {
                        *guard = model.clone();
                    }
                    let was_connected = pump_shared.connected.swap(true, Ordering::Relaxed);
                    window.set_first_run(first_run);
                    if !was_connected {
                        window.set_status("".into());
                    }
                    sync_language(
                        &window,
                        &parsed,
                        &pump_shared,
                        &pump_radial_weak,
                        &pump_picker,
                        &pump_downloads,
                    );
                    pump_picker.update_config(&window, &parsed);
                    apply_config_to_window(
                        &window,
                        &parsed,
                        &model,
                        &pump_paths,
                        &pump_catalog.borrow(),
                    );
                }
                Message::PatchApplied {
                    ok,
                    rejected,
                    config,
                } => {
                    let mut parsed: Config = serde_json::from_value(config).unwrap_or_default();
                    parsed.normalize_models();
                    pump_radial.sync_locked(parsed.drag_locked);
                    if let Ok(mut guard) = pump_shared.config.lock() {
                        *guard = Some(parsed.clone());
                    }
                    let model = pump_shared
                        .model
                        .lock()
                        .map(|guard| guard.clone())
                        .unwrap_or_default();
                    // The status line follows the language the patch just selected.
                    sync_language(
                        &window,
                        &parsed,
                        &pump_shared,
                        &pump_radial_weak,
                        &pump_picker,
                        &pump_downloads,
                    );
                    let lang = texts::lang_of(&window);
                    if ok {
                        window.set_status(lang.text(Key::StatusSaved).into());
                    } else {
                        window.set_status(
                            lang.format(Key::StatusSaveRejected, &[&rejected.join(", ")])
                                .into(),
                        );
                    }
                    pump_picker.update_config(&window, &parsed);
                    apply_config_to_window(
                        &window,
                        &parsed,
                        &model,
                        &pump_paths,
                        &pump_catalog.borrow(),
                    );
                }
                Message::ModelData(model) => {
                    if let Ok(mut guard) = pump_shared.model.lock() {
                        *guard = model.clone();
                    }
                    let config = pump_shared
                        .config
                        .lock()
                        .ok()
                        .and_then(|guard| guard.clone())
                        .unwrap_or_default();
                    sync_language(
                        &window,
                        &config,
                        &pump_shared,
                        &pump_radial_weak,
                        &pump_picker,
                        &pump_downloads,
                    );
                    pump_picker.update_config(&window, &config);
                    apply_config_to_window(
                        &window,
                        &config,
                        &model,
                        &pump_paths,
                        &pump_catalog.borrow(),
                    );
                }
                Message::MenuShow {
                    items,
                    locked,
                    x,
                    y,
                } => {
                    pump_radial.show(x, y, &items, locked);
                }
                Message::MenuClose => {
                    pump_radial.dismiss();
                }
                Message::OpenSettings { page } => {
                    if page == SettingsPage::Model {
                        pump_picker.close_preview(&window);
                        reset_role_list(&window);
                    }
                    window.set_page(page_index(page));
                    let _ = window.show();
                }
                Message::Error { kind, message } => {
                    warn!(%kind, %message, "pet reported an error");
                    window.set_status(message.into());
                }
                Message::Quit => {
                    let _ = window.hide();
                    pump_radial.hide();
                    let _ = slint::quit_event_loop();
                    return;
                }
                Message::Ready | Message::RequestSnapshot => {}
                other => {
                    let _ = other;
                }
            }
        }
        let current_character = pump_shared
            .config
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|config| config.character.clone()))
            .unwrap_or_default();
        pump_picker.poll(&window, &current_character);
        downloads.poll(&window);
        // The settings window requests a snapshot on open.
        if !pump_shared.connected.load(Ordering::Relaxed) {
            let _ = pump_send(&Message::RequestSnapshot);
        }
    });

    // Showing settings is driven only by OpenSettings. Closing the last window
    // must keep the IPC pump alive for subsequent tray/menu requests.
    let result = slint::run_event_loop_until_quit();
    let _ = window.hide();
    radial_controller.hide();
    result.map_err(UiError::Slint)
}

fn reset_role_list(window: &SettingsWindow) {
    window.set_picking_model(false);
    window.set_adding_model(false);
    window.set_import_visible(false);
}

/// Re-renders every language-dependent surface when `config` selects a new
/// language. Rows and notices are rebuilt in the same pass, so switching
/// language never needs a restart.
fn sync_language(
    window: &SettingsWindow,
    config: &Config,
    shared: &Shared,
    radial: &slint::Weak<RadialMenu>,
    picker: &Rc<picker::Picker>,
    downloads: &Rc<downloads::Downloads>,
) {
    let lang = Lang::from_tag(&config.language);
    let changed = match shared.language.lock() {
        Ok(mut guard) => {
            if *guard == Some(lang) {
                false
            } else {
                *guard = Some(lang);
                true
            }
        }
        Err(_) => return,
    };
    if !changed {
        return;
    }
    texts::apply(window, lang);
    if let Some(radial) = radial.upgrade() {
        texts::apply_radial(&radial, lang);
    }
    picker.refresh(window);
    downloads.refresh(window);
}

fn page_index(page: SettingsPage) -> i32 {
    match page {
        SettingsPage::Model => 0,
        SettingsPage::Display => 1,
        SettingsPage::Behavior => 2,
        SettingsPage::About => 3,
        SettingsPage::Downloads => 4,
    }
}

fn spawn_client(
    pipe: String,
    messages: std::sync::mpsc::Sender<Message>,
    writer: Arc<Mutex<Option<Connection>>>,
) {
    std::thread::Builder::new()
        .name("bandori-ui-ipc".to_owned())
        .spawn(move || loop {
            let mut connection = None;
            for _ in 0..50 {
                match Connection::connect(&pipe) {
                    Ok(client) => {
                        connection = Some(client);
                        break;
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(200)),
                }
            }
            let Some(mut connection) = connection else {
                warn!(pipe = %pipe, "could not connect to the pet process");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            info!(pipe = %pipe, "connected to the pet process");
            if let Ok(clone) = connection.try_clone() {
                if let Ok(mut guard) = writer.lock() {
                    *guard = Some(clone);
                }
            }
            let _ = connection.send(&Message::RequestSnapshot);
            while let Ok(Some(message)) = connection.recv() {
                let quit = matches!(message, Message::Quit);
                if messages.send(message).is_err() {
                    return;
                }
                if quit {
                    if let Ok(mut guard) = writer.lock() {
                        *guard = None;
                    }
                    return;
                }
            }
            if let Ok(mut guard) = writer.lock() {
                *guard = None;
            }
            std::thread::sleep(Duration::from_millis(300));
        })
        .expect("spawn ipc client thread");
}

fn wire_patches(window: &SettingsWindow, send: &Sender) {
    {
        let send = Rc::clone(send);
        window.on_patch_number(move |key, value| {
            let mut patch = SettingsPatch::default();
            let json_value = match key.as_str() {
                "fps" | "live2d_scale" => serde_json::json!(value.round() as i64),
                _ => serde_json::json!(value),
            };
            patch.values.insert(key.to_string(), json_value);
            let _ = send(&Message::SettingsPatch { patch });
        });
    }
    {
        let send = Rc::clone(send);
        window.on_patch_bool(move |key, value| {
            let mut patch = SettingsPatch::default();
            patch
                .values
                .insert(key.to_string(), serde_json::json!(value));
            let _ = send(&Message::SettingsPatch { patch });
        });
    }
    {
        let send = Rc::clone(send);
        window.on_patch_string(move |key, value| {
            let mut patch = SettingsPatch::default();
            patch
                .values
                .insert(key.to_string(), serde_json::json!(value.to_string()));
            let _ = send(&Message::SettingsPatch { patch });
        });
    }
    {
        let send = Rc::clone(send);
        let weak = window.as_weak();
        window.on_edit_region(move |index, field, option| {
            let Some(region) = CLICK_REGIONS.get(index as usize).copied() else {
                return;
            };
            let Some(window) = weak.upgrade() else {
                return;
            };
            let options = if field.as_str() == "expression" {
                window.get_expression_options()
            } else {
                window.get_motion_options()
            };
            let value = match option {
                0 => ClickAction::AUTO.to_owned(),
                1 => ClickAction::RANDOM.to_owned(),
                2 => ClickAction::NONE.to_owned(),
                other => match options.row_data(other as usize) {
                    Some(name) => name.to_string(),
                    None => return,
                },
            };
            let mut patch = SettingsPatch::default();
            patch.values.insert(
                format!("click_motion_actions.{}.{}", region.key(), field),
                serde_json::json!(value),
            );
            let _ = send(&Message::SettingsPatch { patch });
        });
    }
    {
        // Picking a language applies it immediately; the apply button keeps the
        // other sidebar toggles explicit.
        let send = Rc::clone(send);
        let weak = window.as_weak();
        window.on_language_selected(move |index| {
            let lang = Lang::from_slint_index(index);
            let mut patch = SettingsPatch::default();
            patch
                .values
                .insert("language".to_owned(), serde_json::json!(lang.tag()));
            if !send(&Message::SettingsPatch { patch }) {
                if let Some(window) = weak.upgrade() {
                    window.set_language_index(texts::lang_of(&window).index() as i32);
                    window.set_status(texts::lang_of(&window).text(Key::StatusNotConnected).into());
                }
            }
        });
    }
    {
        let send = Rc::clone(send);
        window.on_apply_preset(move |name| {
            let mut patch = SettingsPatch::default();
            patch.values.insert(
                "click_motion_active_profile".to_owned(),
                serde_json::json!(name.to_string()),
            );
            let _ = send(&Message::SettingsPatch { patch });
        });
    }
    {
        let send = Rc::clone(send);
        window.on_reset_positions(move || {
            let _ = send(&Message::ResetPositions);
        });
    }
    {
        let send = Rc::clone(send);
        window.on_open_folder(move || {
            let _ = send(&Message::OpenModelFolder);
        });
    }
}

fn apply_config_to_window(
    window: &SettingsWindow,
    config: &Config,
    model: &ModelData,
    paths: &Paths,
    catalog: &Catalog,
) {
    let lang = Lang::from_tag(&config.language);
    window.set_fps(config.fps as i32);
    window.set_window_opacity(config.opacity * 100.0);
    window.set_scale(if config.live2d_scale == 0 {
        (window.window().scale_factor() * 100.0).round() as i32
    } else {
        config.live2d_scale as i32
    });
    window.set_quality(match config.live2d_quality {
        pet_config::Live2dQuality::Performance => 0,
        pet_config::Live2dQuality::Balanced => 1,
    });
    window.set_vsync(config.vsync);
    window.set_gpu(config.gpu_acceleration);
    window.set_theme(match config.dark_theme {
        pet_config::DarkTheme::Off => 0,
        pet_config::DarkTheme::On => 1,
        pet_config::DarkTheme::System => 2,
    });
    window.set_drag_locked(config.drag_locked);
    window.set_head_tracking(config.live2d_head_tracking_enabled);
    window.set_mutual_gaze(config.live2d_mutual_gaze_enabled);
    window.set_idle_actions(config.live2d_idle_actions_enabled);
    window.set_random_actions(config.live2d_random_actions_enabled);
    window.set_emotion(config.emotion_behavior_enabled);
    if !window.get_options_dirty() {
        window.set_obs(config.obs_window_capture_compatible);
        window.set_topmost(config.game_topmost);
        window.set_autostart(config.auto_start);
        window.set_hide_model(config.hide_live2d_model);
    }
    // The language is never gated by unsaved toggles: it switches at once.
    window.set_language_index(lang.index() as i32);
    window.set_move_together(config.move_all_roles_together);

    window.set_has_model(!config.models.is_empty());
    window.set_active_model_index(
        config
            .models
            .iter()
            .position(|entry| {
                entry.character == config.character && entry.costume == config.costume
            })
            .map(|index| index as i32)
            .unwrap_or(-1),
    );
    let character = catalog.character(&config.character);
    window.set_active_character_label(
        character
            .map(|entry| entry.display.as_str())
            .unwrap_or(if config.character.is_empty() {
                lang.text(Key::NeverSelected)
            } else {
                &config.character
            })
            .into(),
    );
    window.set_roles_heading(
        format!(
            "{}{}{}",
            lang.text(Key::RolesHeadingPrefix),
            config.models.len(),
            lang.text(Key::RolesHeadingSuffix)
        )
        .into(),
    );
    window.set_first_run_banner(
        lang.text(if catalog.is_empty() {
            Key::FirstRunWelcomeEmpty
        } else {
            Key::FirstRunWelcome
        })
        .into(),
    );
    window.set_active_band(
        character
            .map(|entry| entry.band_display.as_str())
            .unwrap_or("")
            .into(),
    );
    window.set_active_costume_label(
        costume_display(catalog, &config.character, &config.costume).into(),
    );
    window.set_active_models(ModelRc::new(VecModel::from(
        config
            .models
            .iter()
            .map(|entry| AboutLine {
                label: catalog
                    .character(&entry.character)
                    .map(|character| character.display.as_str())
                    .unwrap_or(&entry.character)
                    .into(),
                value: costume_display(catalog, &entry.character, &entry.costume).into(),
            })
            .collect::<Vec<_>>(),
    )));
    if !window.get_picking_model() && !config.character.is_empty() {
        populate_costumes(window, catalog, &config.character, Some(&config.costume));
    }
    window.set_model_info(
        lang.format(
            Key::ModelInfo,
            &[
                &model.format,
                &model.motions.len().to_string(),
                &model.expressions.len().to_string(),
                &model.drawables.to_string(),
                &model.textures.to_string(),
            ],
        )
        .into(),
    );

    let motion_options = build_options(lang, &model.motions);
    let expression_options = build_options(lang, &model.expressions);
    window.set_motion_options(ModelRc::new(VecModel::from(motion_options.clone())));
    window.set_expression_options(ModelRc::new(VecModel::from(expression_options.clone())));
    window.set_preset_index(
        [
            "auto",
            "genki",
            "tsundere",
            "shy",
            "cool",
            "surprised",
            "random",
        ]
        .iter()
        .position(|name| *name == config.click_motion_active_profile)
        .unwrap_or(0) as i32,
    );

    let actions = config
        .active_entry()
        .map(|entry| config.actions_for(entry))
        .unwrap_or(&config.click_motion_actions);
    let regions: Vec<RegionRow> = CLICK_REGIONS
        .iter()
        .map(|region| {
            let action = actions.get(region.key()).cloned().unwrap_or_default();
            RegionRow {
                region: region.key().into(),
                title: region.display_name(lang).into(),
                motion_index: option_index(&action.motion, &motion_options),
                expression_index: option_index(&action.expression, &expression_options),
            }
        })
        .collect();
    window.set_regions(ModelRc::new(VecModel::from(regions)));

    window.set_about_lines(ModelRc::new(VecModel::from(
        pet_config::about_lines(paths, config, lang)
            .into_iter()
            .map(|(label, value)| AboutLine {
                label: label.into(),
                value: value.into(),
            })
            .collect::<Vec<_>>(),
    )));
}

fn populate_costumes(
    window: &SettingsWindow,
    catalog: &Catalog,
    character: &str,
    preferred: Option<&str>,
) {
    let costumes = catalog.costumes_of(character);
    window.set_selected_character(character.into());
    window.set_selected_character_label(
        catalog
            .character(character)
            .map(|entry| entry.display.as_str())
            .unwrap_or(character)
            .into(),
    );
    window.set_selected_costume(
        preferred
            .filter(|id| costumes.iter().any(|entry| entry.id == *id))
            .or_else(|| costumes.first().map(|entry| entry.id.as_str()))
            .unwrap_or("")
            .into(),
    );
}

fn costume_display<'a>(catalog: &'a Catalog, character: &str, costume: &'a str) -> &'a str {
    catalog
        .find(character, costume)
        .map(|entry| entry.display.as_str())
        .unwrap_or(costume)
}

fn build_options(lang: Lang, names: &[String]) -> Vec<slint::SharedString> {
    let mut options: Vec<slint::SharedString> =
        [Key::OptionAuto, Key::OptionRandom, Key::OptionNone]
            .iter()
            .map(|key| slint::SharedString::from(lang.text(*key)))
            .collect();
    options.extend(names.iter().map(|name| name.clone().into()));
    options
}

fn option_index(value: &str, options: &[slint::SharedString]) -> i32 {
    if value.is_empty() {
        return 0;
    }
    if value == ClickAction::RANDOM {
        return 1;
    }
    if value == ClickAction::NONE {
        return 2;
    }
    options
        .iter()
        .position(|option| option.as_str() == value)
        .map(|index| index as i32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window with the default language applied, for tests that assert labels.
    pub(super) fn test_window() -> SettingsWindow {
        let window = SettingsWindow::new().unwrap();
        texts::apply(&window, Lang::ZhHans);
        window
    }

    pub(super) fn settle_animations(window: &SettingsWindow) {
        // Paint the initial state, fire entrance timers, then finish transitions.
        let _ = window.window().take_snapshot().unwrap();
        i_slint_backend_testing::testing_backend::mock_elapsed_time(20);
        let _ = window.window().take_snapshot().unwrap();
        i_slint_backend_testing::testing_backend::mock_elapsed_time(250);
    }

    #[test]
    fn option_lists_put_specials_first() {
        let names = vec!["smile01".to_owned()];
        let options = build_options(Lang::ZhHans, &names);
        assert_eq!(options.len(), 4);
        assert_eq!(options[0].as_str(), "自动匹配");
        assert_eq!(option_index("", &options), 0);
        assert_eq!(option_index("__random__", &options), 1);
        assert_eq!(option_index("__none__", &options), 2);
        assert_eq!(option_index("smile01", &options), 3);
        assert_eq!(option_index("missing", &options), 0);
    }

    #[test]
    fn page_ids_match_tabs() {
        assert_eq!(page_index(SettingsPage::Model), 0);
        assert_eq!(page_index(SettingsPage::Display), 1);
        assert_eq!(page_index(SettingsPage::Behavior), 2);
        assert_eq!(page_index(SettingsPage::About), 3);
        assert_eq!(page_index(SettingsPage::Downloads), 4);
    }

    #[test]
    fn about_rows_include_version() {
        let lines = pet_config::about_lines(
            &Paths::with_root("C:/app"),
            &Config::default(),
            Lang::ZhHans,
        );
        assert!(lines.iter().any(|(label, _)| label == "版本"));
        let english =
            pet_config::about_lines(&Paths::with_root("C:/app"), &Config::default(), Lang::En);
        assert!(english.iter().any(|(label, _)| label == "Version"));
    }

    #[test]
    fn menu_items_convert_to_slint_rows() {
        let items = pet_ipc::MenuItem::lite_items("zh_CN");
        let rows = items_to_slint(&items);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1].action.as_str(), "motion");
    }

    #[test]
    fn model_list_keeps_selection_and_targets_the_clicked_role() {
        use i_slint_backend_testing::{ElementHandle, TestingBackend, TestingBackendOptions};
        slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        })))
        .unwrap();
        let window = test_window();
        let config = Config {
            models: vec![
                pet_config::ModelEntry::new(
                    "kasumi",
                    "live",
                    "models/kasumi.zst::live/model.json",
                    pet_config::ModelFormat::Moc,
                ),
                pet_config::ModelEntry::new(
                    "anon",
                    "school",
                    "models/anon.zst::school/model3.json",
                    pet_config::ModelFormat::Moc3,
                ),
            ],
            character: "anon".into(),
            costume: "school".into(),
            ..Default::default()
        };
        apply_config_to_window(
            &window,
            &config,
            &ModelData::default(),
            &Paths::with_root("C:/app"),
            &Catalog::default(),
        );
        assert_eq!(window.get_active_model_index(), 1);
        let selected = Rc::new(std::cell::Cell::new(-1));
        let removed = Rc::new(std::cell::Cell::new(-1));
        let result = selected.clone();
        window.on_select_model(move |index| result.set(index));
        let result = removed.clone();
        window.on_remove_model(move |index| result.set(index));
        window.show().unwrap();
        settle_animations(&window);
        let _ = window.window().take_snapshot().unwrap();
        ElementHandle::find_by_accessible_label(&window, "选择角色：anon")
            .next()
            .unwrap()
            .invoke_accessible_default_action();
        ElementHandle::find_by_accessible_label(&window, "移除角色：kasumi")
            .next()
            .unwrap()
            .invoke_accessible_default_action();
        assert_eq!(selected.get(), 1);
        assert_eq!(removed.get(), 0);
        if let Ok(output) = std::env::var("BANDORI_UI_QA_DIR") {
            std::fs::create_dir_all(&output).unwrap();
            let snapshot = window.window().take_snapshot().unwrap();
            image::save_buffer(
                std::path::Path::new(&output).join("multi-role-list.png"),
                snapshot.as_bytes(),
                snapshot.width(),
                snapshot.height(),
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }

    #[test]
    fn large_portrait_stays_inside_its_card_at_different_sizes_and_dpi() {
        use i_slint_backend_testing::{ElementHandle, TestingBackend, TestingBackendOptions};
        slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        })))
        .unwrap();
        let window = test_window();
        window.set_theme(1);
        window.set_has_model(true);
        window.set_active_character_label("户山香澄".into());
        window.set_active_costume_label("295期 Star Trajectory".into());
        window.set_active_band("Poppin'Party".into());
        let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(1600, 2400);
        pixels
            .make_mut_slice()
            .fill(slint::Rgba8Pixel::new(220, 30, 30, 255));
        window.set_portrait(slint::Image::from_rgba8(pixels));
        window.set_has_portrait(true);
        window.show().unwrap();
        for (width, height, scale) in [(1080., 620., 1.), (1180., 680., 1.5), (1500., 900., 1.)] {
            window
                .window()
                .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
                    scale_factor: scale,
                });
            // The testing adapter stores physical size at scale 1, so deliver
            // the logical resize separately when simulating monitor DPI.
            window.window().set_size(slint::PhysicalSize::new(
                (width * scale) as u32,
                (height * scale) as u32,
            ));
            window
                .window()
                .dispatch_event(slint::platform::WindowEvent::Resized {
                    size: slint::LogicalSize::new(width, height),
                });
            settle_animations(&window);
            let snapshot = window.window().take_snapshot().unwrap();
            let element = |id: &str| {
                ElementHandle::find_by_element_id(&window, &format!("SettingsWindow::{id}"))
                    .next()
                    .unwrap_or_else(|| panic!("missing {id}"))
            };
            let frame = element("portrait-frame");
            let portrait = element("portrait-image");
            let action = element("action-card");
            assert!(frame.size().width > 100. && frame.size().height > 100.);
            assert_eq!(portrait.size(), frame.size());
            assert!(
                action.size().width >= 245.,
                "action panel must remain visible"
            );
            assert!(action.absolute_position().x + action.size().width < width - 260.);
            // A large opaque image must never paint over the navigation or header.
            let physical_width = snapshot.width();
            let mut red_pixels = 0;
            for (index, pixel) in snapshot.as_slice().iter().enumerate() {
                if pixel.r == 220 && pixel.g == 30 && pixel.b == 30 {
                    let x = (index as u32 % physical_width) as f32 / scale;
                    let y = (index as u32 / physical_width) as f32 / scale;
                    assert!(
                        x >= frame.absolute_position().x - 1.
                            && x <= frame.absolute_position().x + frame.size().width + 1.
                    );
                    assert!(
                        y >= frame.absolute_position().y - 1.
                            && y <= frame.absolute_position().y + frame.size().height + 1.
                    );
                    red_pixels += 1;
                }
            }
            assert!(
                red_pixels > 1000,
                "portrait should be visible inside the card"
            );
            if let Ok(output) = std::env::var("BANDORI_UI_QA_DIR") {
                let output = std::path::Path::new(&output);
                std::fs::create_dir_all(output).unwrap();
                image::save_buffer(
                    output.join(format!("layout-{width}-{scale}.png")),
                    snapshot.as_bytes(),
                    snapshot.width(),
                    snapshot.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
        let switch = ElementHandle::find_by_accessible_label(&window, "切换角色/服装")
            .next()
            .unwrap();
        switch.invoke_accessible_default_action();
        assert!(window.get_picking_model());
        window.set_adding_model(true);
        window.set_import_visible(true);
        reset_role_list(&window);
        assert!(!window.get_picking_model());
        assert!(!window.get_adding_model());
        assert!(!window.get_import_visible());

        // Optional review artifacts use a locally supplied portrait. The
        // regression assertions above always use the oversized synthetic image.
        if let Ok(output) = std::env::var("BANDORI_UI_QA_DIR") {
            window
                .window()
                .set_size(slint::LogicalSize::new(1180., 680.));
            window.set_picking_model(false);
            window.set_motion_options(ModelRc::new(VecModel::from(build_options(
                Lang::ZhHans,
                &[],
            ))));
            window.set_expression_options(ModelRc::new(VecModel::from(build_options(
                Lang::ZhHans,
                &[],
            ))));
            window.set_active_models(ModelRc::new(VecModel::from(vec![AboutLine {
                label: "户山香澄".into(),
                value: "295期 Star Trajectory".into(),
            }])));
            if let Ok(path) = std::env::var("BANDORI_UI_QA_PORTRAIT") {
                window.set_portrait(
                    slint::Image::load_from_path(std::path::Path::new(&path)).unwrap(),
                );
            }
            for (label, page, theme) in [
                ("model-dark", 0, 1),
                ("model-light", 0, 0),
                ("behavior", 2, 1),
                ("display", 1, 1),
                ("about", 3, 1),
            ] {
                window.set_page(page);
                window.set_theme(theme);
                settle_animations(&window);
                let snapshot = window.window().take_snapshot().unwrap();
                image::save_buffer(
                    std::path::Path::new(&output).join(format!("{label}.png")),
                    snapshot.as_bytes(),
                    snapshot.width(),
                    snapshot.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
    }
}
