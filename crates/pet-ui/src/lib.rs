//! Slint settings window and radial menu, wired to `bandori-pet` over the pipe.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pet_assets::catalog::Catalog;
use pet_config::{ClickAction, Config, Paths, SettingsPatch, CLICK_REGIONS};
use pet_ipc::{Connection, Message, ModelData, SettingsPage};
use slint::{ModelRc, Timer, TimerMode, VecModel};
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
}

/// Message sender shared by every callback.
type Sender = Rc<dyn Fn(&Message) -> bool>;

/// Runs the settings UI until the window is closed.
pub fn run(pipe: &str, paths: &Paths) -> Result<(), UiError> {
    let window = SettingsWindow::new()?;
    let radial = RadialMenu::new()?;
    let bubble = PokeBubble::new()?;
    let _ = bubble.hide();

    let index = pet_assets::index::load_or_scan(&paths.models());
    let catalog = Rc::new(Catalog::build(&paths.assets(), &index));
    info!(
        characters = catalog.len(),
        costumes = index.costume_count(),
        "settings catalog loaded"
    );

    let shared = Arc::new(Shared {
        config: Mutex::new(None),
        model: Mutex::new(ModelData::default()),
        connected: AtomicBool::new(false),
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

    // ── Character / costume lists ────────────────────────────────────────────
    let characters: Vec<CharacterItem> = catalog
        .characters()
        .iter()
        .map(|character| CharacterItem {
            key: character.key.clone().into(),
            display: character.display.clone().into(),
            band: character.band_display.clone().into(),
            costume_count: character.costumes.len() as i32,
            selected: false,
        })
        .collect();
    window.set_characters(ModelRc::new(VecModel::from(characters)));
    window.set_about_lines(ModelRc::new(VecModel::from(
        pet_config::about_lines(paths, &Config::default())
            .into_iter()
            .map(|(label, value)| AboutLine {
                label: label.into(),
                value: value.into(),
            })
            .collect::<Vec<_>>(),
    )));

    let show_costumes = {
        let window_weak = window.as_weak();
        let catalog = Rc::clone(&catalog);
        Rc::new(move |character: &str| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let costumes = catalog.costumes_of(character);
            let labels: Vec<slint::SharedString> = costumes
                .iter()
                .map(|costume| format!("{} ({})", costume.display, costume.id).into())
                .collect();
            let ids: Vec<slint::SharedString> = costumes
                .iter()
                .map(|costume| costume.id.clone().into())
                .collect();
            window.set_costume_labels(ModelRc::new(VecModel::from(labels)));
            window.set_costume_ids(ModelRc::new(VecModel::from(ids)));
            if let Some(first) = costumes.first() {
                window.set_selected_costume(first.id.clone().into());
            }
            window.set_selected_character_label(
                catalog
                    .character(character)
                    .map(|entry| entry.display.clone())
                    .unwrap_or_else(|| character.to_owned())
                    .into(),
            );
        })
    };

    {
        let show_costumes = Rc::clone(&show_costumes);
        window.on_select_character(move |character| show_costumes(&character));
    }

    {
        let send = Rc::clone(&send);
        let window_weak = window.as_weak();
        window.on_apply_selection(move |character, costume| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let request = Message::ModelSelect {
                character: character.to_string(),
                costume: costume.to_string(),
            };
            if send(&request) {
                window.set_status("正在切换模型…".into());
            } else {
                window.set_status("桌宠进程未连接".into());
            }
        });
    }

    // ── Patch plumbing ───────────────────────────────────────────────────────
    wire_patches(&window, &send);

    // ── Radial menu ──────────────────────────────────────────────────────────
    {
        let send = Rc::clone(&send);
        radial.on_action(move |action| {
            send(&Message::MenuAct {
                action: action.to_string(),
            });
        });
    }
    {
        let send = Rc::clone(&send);
        radial.on_locked_toggled(move |locked| {
            send(&Message::MenuLocked { locked });
        });
    }

    // ── Message pump ─────────────────────────────────────────────────────────
    let pump_window = window.as_weak();
    let pump_radial = radial.as_weak();
    let pump_bubble = bubble.as_weak();
    let bubble_timer = Timer::default();
    let bubble_deadline: Rc<std::cell::Cell<std::time::Instant>> =
        Rc::new(std::cell::Cell::new(std::time::Instant::now()));
    {
        let pump_bubble_tick = pump_bubble.clone();
        let bubble_deadline_tick = Rc::clone(&bubble_deadline);
        bubble_timer.start(TimerMode::Repeated, Duration::from_millis(120), move || {
            if let Some(bubble) = pump_bubble_tick.upgrade() {
                if std::time::Instant::now() >= bubble_deadline_tick.get() {
                    let _ = bubble.hide();
                }
            }
        });
    }
    let pump_shared = Arc::clone(&shared);
    let pump_send = Rc::clone(&send);
    let pump_paths = paths.clone();
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
                    let mut parsed: Config =
                        serde_json::from_value(config).unwrap_or_default();
                    parsed.normalize_models();
                    if let Ok(mut guard) = pump_shared.config.lock() {
                        *guard = Some(parsed.clone());
                    }
                    if let Ok(mut guard) = pump_shared.model.lock() {
                        *guard = model.clone();
                    }
                    pump_shared.connected.store(true, Ordering::Relaxed);
                    window.set_first_run(first_run);
                    window.set_status("已连接桌宠进程".into());
                    apply_config_to_window(&window, &parsed, &model, &pump_paths);
                }
                Message::PatchApplied {
                    ok,
                    rejected,
                    config,
                } => {
                    if ok {
                        window.set_status("已保存".into());
                    } else {
                        window.set_status(format!("保存被拒绝：{}", rejected.join(", ")).into());
                    }
                    let mut parsed: Config =
                        serde_json::from_value(config).unwrap_or_default();
                    parsed.normalize_models();
                    if let Ok(mut guard) = pump_shared.config.lock() {
                        *guard = Some(parsed.clone());
                    }
                    let model = pump_shared
                        .model
                        .lock()
                        .map(|guard| guard.clone())
                        .unwrap_or_default();
                    apply_config_to_window(&window, &parsed, &model, &pump_paths);
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
                    apply_config_to_window(&window, &config, &model, &pump_paths);
                }
                Message::MenuShow { items, locked, x, y } => {
                    if let Some(radial) = pump_radial.upgrade() {
                        radial.set_items(ModelRc::new(VecModel::from(items_to_slint(&items))));
                        radial.set_locked(locked);
                        // 380x380 window (radius 110 + 80 px item box) centred on the pet.
                        set_window_position(&radial.window(), x - 190, y - 190);
                        let _ = radial.show();
                    }
                }
                Message::Bubble {
                    x,
                    y,
                    text,
                    duration_ms,
                } => {
                    if let Some(bubble) = pump_bubble.upgrade() {
                        bubble.set_text(text.into());
                        set_window_position(&bubble.window(), x - 110, y - 60);
                        bubble_deadline.set(
                            std::time::Instant::now() + Duration::from_millis(duration_ms as u64),
                        );
                        let _ = bubble.show();
                    }
                }
                Message::MenuClose => {
                    if let Some(radial) = pump_radial.upgrade() {
                        let _ = radial.hide();
                    }
                }
                Message::OpenSettings { page } => {
                    window.set_page(page_index(page));
                    let _ = window.show();
                }
                Message::Error { kind, message } => {
                    warn!(%kind, %message, "pet reported an error");
                    window.set_status(message.into());
                }
                Message::Quit => {
                    if let Some(radial) = pump_radial.upgrade() {
                        let _ = radial.hide();
                    }
                }
                Message::Ready | Message::RequestSnapshot => {}
                other => {
                    let _ = other;
                }
            }
        }
        // The settings window requests a snapshot on open.
        if !pump_shared.connected.load(Ordering::Relaxed) {
            let _ = pump_send(&Message::RequestSnapshot);
        }
    });

    let result = window.run();
    let _ = radial.hide();
    result.map_err(UiError::Slint)
}

/// Places a Slint window at physical screen coordinates.
fn set_window_position(window: &slint::Window, x: i32, y: i32) {
    use slint::PhysicalPosition;
    window.set_position(PhysicalPosition::new(x.max(0), y.max(0)));
}

fn page_index(page: SettingsPage) -> i32 {
    match page {
        SettingsPage::Model => 0,
        SettingsPage::Display => 1,
        SettingsPage::Behavior => 2,
        SettingsPage::About => 3,
    }
}

fn items_to_slint(items: &[pet_ipc::MenuItem]) -> Vec<RadialItem> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| RadialItem {
            action: item.action.clone().into(),
            label: item.label.clone().into(),
            glyph: item.glyph.clone().into(),
            angle: index as f32,
        })
        .collect()
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
            loop {
                match connection.recv() {
                    Ok(Some(message)) => {
                        let quit = matches!(message, Message::Quit);
                        if messages.send(message).is_err() {
                            return;
                        }
                        if quit {
                            break;
                        }
                    }
                    Ok(None) | Err(_) => break,
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
            patch.values.insert(key.to_string(), serde_json::json!(value));
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
        window.on_edit_region(move |index, field, option| {
            let Some(region) = CLICK_REGIONS.get(index as usize).copied() else {
                return;
            };
            let value = match option.as_str() {
                "自动匹配" => ClickAction::AUTO.to_owned(),
                "随机动作" => ClickAction::RANDOM.to_owned(),
                "不触发" => ClickAction::NONE.to_owned(),
                other => other.to_owned(),
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

fn apply_config_to_window(window: &SettingsWindow, config: &Config, model: &ModelData, paths: &Paths) {
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
    window.set_obs(config.obs_window_capture_compatible);
    window.set_topmost(config.game_topmost);
    window.set_autostart(config.auto_start);
    window.set_poke_motion(config.poke_motion.clone().into());
    window.set_poke_expression(config.poke_expression.clone().into());

    if !model.character.is_empty() {
        window.set_selected_character(model.character.clone().into());
    }
    if !model.costume.is_empty() {
        window.set_selected_costume(model.costume.clone().into());
    }
    window.set_model_info(
        format!(
            "格式 {} · 动作 {} · 表情 {} · 绘制 {} · 纹理 {}",
            model.format,
            model.motions.len(),
            model.expressions.len(),
            model.drawables,
            model.textures
        )
        .into(),
    );

    let motion_options = build_options(&model.motions);
    let expression_options = build_options(&model.expressions);
    window.set_motion_options(ModelRc::new(VecModel::from(motion_options.clone())));
    window.set_expression_options(ModelRc::new(VecModel::from(expression_options.clone())));

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
                title: region.display_name(&config.language).into(),
                motion_index: option_index(&action.motion, &motion_options),
                expression_index: option_index(&action.expression, &expression_options),
            }
        })
        .collect();
    window.set_regions(ModelRc::new(VecModel::from(regions)));

    window.set_about_lines(ModelRc::new(VecModel::from(
        pet_config::about_lines(paths, config)
            .into_iter()
            .map(|(label, value)| AboutLine {
                label: label.into(),
                value: value.into(),
            })
            .collect::<Vec<_>>(),
    )));
}

fn build_options(names: &[String]) -> Vec<slint::SharedString> {
    let mut options: Vec<slint::SharedString> =
        vec!["自动匹配".into(), "随机动作".into(), "不触发".into()];
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
    use std::collections::HashMap;

    #[test]
    fn option_lists_put_specials_first() {
        let names = vec!["smile01".to_owned()];
        let options = build_options(&names);
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
    }

    #[test]
    fn about_rows_include_version() {
        let lines = pet_config::about_lines(&Paths::with_root("C:/app"), &Config::default());
        assert!(lines.iter().any(|(label, _)| label == "版本"));
    }

    #[test]
    fn menu_items_convert_to_slint_rows() {
        let items = pet_ipc::MenuItem::lite_items("zh_CN");
        let rows = items_to_slint(&items);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1].action.as_str(), "motion");
    }

    #[test]
    fn unused_hash_map_marker() {
        let map: HashMap<u8, u8> = HashMap::new();
        assert!(map.is_empty());
    }
}






