//! Integration tests for the config + IPC contract (spec §3.6, §5, §4.1).
//!
//! These tests never create a window: they exercise the pet process' single-writer
//! path, which is where data loss would otherwise happen.

use std::fs;
use std::path::PathBuf;

use pet_app::PetApp;
use pet_config::{Config, ModelEntry, ModelFormat, Paths, SettingsPatch};
use pet_ipc::Message;

fn temp_root(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("bandori-pet-lite-it-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("models")).unwrap();
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::create_dir_all(dir.join("logs")).unwrap();
    fs::write(
        dir.join("assets/band.json"),
        r#"{"bands":[{"id":"poppin_party","display":"Poppin'Party","characters":["kasumi"]}]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("assets/outfit.json"),
        r#"{"characters":{"kasumi":{"display":"户山香澄","costumes":{"live_default":"队服"}}}}"#,
    )
    .unwrap();
    dir
}

fn session(tag: &str) -> String {
    format!("test-{tag}-{}", std::process::id())
}

/// Writes a minimal `tar.zst` model archive with one costume.
fn write_minimal_archive(path: &std::path::Path, character: &str, costume: &str) {
    let file = fs::File::create(path).unwrap();
    let encoder = zstd::stream::write::Encoder::new(file, 3).unwrap();
    let mut builder = tar::Builder::new(encoder);
    let index = format!(r#"{{"files":["{costume}/model.json"]}}"#);
    append(&mut builder, ".bandori_zst_index.json", index.as_bytes());
    append(
        &mut builder,
        &format!("{costume}/model.json"),
        br#"{"model":"data/model.moc","textures":[]}"#,
    );
    let _ = character;
    builder.into_inner().unwrap().finish().unwrap();
}

fn append(
    builder: &mut tar::Builder<zstd::stream::write::Encoder<'static, fs::File>>,
    name: &str,
    data: &[u8],
) {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, name, data).unwrap();
}

#[test]
fn settings_patch_is_persisted_by_the_pet_process() {
    let root = temp_root("patch");
    let paths = Paths::with_root(&root);
    let config = Config {
        fps: 120,
        ..Default::default()
    };
    pet_config::save_config(&paths.config(), &config).unwrap();

    let mut app = PetApp::new_with(paths.clone(), &session("patch"), false).expect("pet app");
    let patch = SettingsPatch::from_pairs([
        ("fps", serde_json::json!(60)),
        (
            "click_motion_actions.head.motion",
            serde_json::json!("smile01"),
        ),
        ("live2d_quality", serde_json::json!("performance")),
    ]);
    app.handle_message_for_test(Message::SettingsPatch { patch });

    let reloaded = pet_config::load_config(&paths.config()).config;
    assert_eq!(reloaded.fps, 60);
    assert_eq!(
        reloaded.click_motion_actions["head"].motion,
        "smile01".to_owned()
    );
    assert_eq!(
        reloaded.live2d_quality,
        pet_config::Live2dQuality::Performance
    );
}

#[test]
fn unknown_keys_survive_a_settings_patch() {
    let root = temp_root("extras");
    let paths = Paths::with_root(&root);
    fs::write(
        paths.config(),
        r#"{"schema_version":1,"fps":120,"llm_api_key":"keep-me","future":{"a":[1,2]}}"#,
    )
    .unwrap();

    let mut app = PetApp::new_with(paths.clone(), &session("extras"), false).expect("pet app");
    app.handle_message_for_test(Message::SettingsPatch {
        patch: SettingsPatch::from_pairs([("fps", serde_json::json!(90))]),
    });

    let raw = fs::read_to_string(paths.config()).unwrap();
    assert!(raw.contains("llm_api_key"), "{raw}");
    assert!(raw.contains("keep-me"));
    assert!(raw.contains("\"future\""));
    assert!(raw.contains("90"));
}

#[test]
fn invalid_values_are_clamped_not_crashing() {
    let root = temp_root("clamp");
    let paths = Paths::with_root(&root);
    pet_config::save_config(&paths.config(), &Config::default()).unwrap();

    let mut app = PetApp::new_with(paths.clone(), &session("clamp"), false).expect("pet app");
    app.handle_message_for_test(Message::SettingsPatch {
        patch: SettingsPatch::from_pairs([
            ("fps", serde_json::json!(9999)),
            ("opacity", serde_json::json!(0.01)),
        ]),
    });

    let reloaded = pet_config::load_config(&paths.config()).config;
    assert_eq!(reloaded.fps, 240);
    assert!((reloaded.opacity - 0.2).abs() < f32::EPSILON);
}

#[test]
fn model_selection_without_models_reports_first_run() {
    let root = temp_root("firstrun");
    let paths = Paths::with_root(&root);
    let app = PetApp::new_with(paths.clone(), &session("firstrun"), false).expect("pet app");
    assert!(app.config().needs_model_selection());
}

#[test]
fn model_selection_switches_the_configured_entry() {
    let root = temp_root("select");
    let paths = Paths::with_root(&root);
    // The catalog is built from a real archive scan (the index is only a cache),
    // so write a minimal `.zst` tar with one costume.
    write_minimal_archive(&paths.models().join("kasumi.zst"), "kasumi", "live_default");

    let mut app = PetApp::new_with(paths.clone(), &session("select"), false).expect("pet app");
    assert_eq!(app.catalog_for_test().len(), 1, "catalog should see kasumi");
    app.handle_message_for_test(Message::ModelSelect {
        character: "kasumi".to_owned(),
        costume: "live_default".to_owned(),
    });
    let reloaded = pet_config::load_config(&paths.config()).config;
    assert_eq!(reloaded.character, "kasumi");
    assert_eq!(reloaded.costume, "live_default");
    assert!(!reloaded.models.is_empty());
}

#[test]
fn model_selection_adds_characters_and_keeps_existing_positions() {
    let root = temp_root("multi-select");
    let paths = Paths::with_root(&root);
    write_minimal_archive(&paths.models().join("kasumi.zst"), "kasumi", "live_default");
    write_minimal_archive(&paths.models().join("anon.zst"), "anon", "school");
    write_minimal_archive(
        &paths.models().join("anon-extra.zst"),
        "anon-extra",
        "other",
    );
    let mut kasumi = ModelEntry::new(
        "kasumi",
        "live_default",
        "models/kasumi.zst::live_default/model.json",
        ModelFormat::Moc,
    );
    kasumi.window_placement.x = 120;
    kasumi.window_placement.y = 200;
    kasumi.sync_placement_mirror();
    let mut config = Config::default();
    config.models.push(kasumi.clone());
    pet_config::save_config(&paths.config(), &config).unwrap();
    let mut app = PetApp::new_with(paths.clone(), &session("multi-select"), false).unwrap();
    app.handle_message_for_test(Message::ModelSelect {
        character: "anon".into(),
        costume: "school".into(),
    });
    let saved = pet_config::load_config(&paths.config()).config;
    assert_eq!(saved.models.len(), 2);
    assert_eq!(saved.models[0], kasumi);
    assert_eq!(saved.character, "anon");
    // Selecting the same character again must not add another window entry.
    app.handle_message_for_test(Message::ModelSelect {
        character: "anon".into(),
        costume: "school".into(),
    });
    assert_eq!(app.config().models.len(), 2);
    // An unrelated settings save must keep both entries and their selection.
    app.handle_message_for_test(Message::SettingsPatch {
        patch: SettingsPatch::from_pairs([("fps", serde_json::json!(90))]),
    });
    let saved = pet_config::load_config(&paths.config()).config;
    assert_eq!(saved.models.len(), 2);
    assert_eq!(saved.models[0].window_x, 120);
    assert_eq!(saved.character, "anon");
}

#[test]
fn model_list_can_be_removed_and_restored_with_other_settings() {
    let root = temp_root("multi-patch");
    let paths = Paths::with_root(&root);
    let first = ModelEntry::new(
        "kasumi",
        "live",
        "models/kasumi.zst::live/model.json",
        ModelFormat::Moc,
    );
    let mut second = ModelEntry::new(
        "anon",
        "school",
        "models/anon.zst::school/model3.json",
        ModelFormat::Moc3,
    );
    second.window_placement.x = 700;
    second.window_placement.y = 250;
    second.sync_placement_mirror();
    let mut app = PetApp::new_with(paths.clone(), &session("multi-patch"), false).unwrap();
    app.handle_message_for_test(Message::SettingsPatch {
        patch: SettingsPatch::from_pairs([
            ("models", serde_json::json!([first, second.clone()])),
            ("character", serde_json::json!("anon")),
            ("costume", serde_json::json!("school")),
            ("live2d_scale", serde_json::json!(75)),
        ]),
    });
    assert_eq!(app.config().models.len(), 2);
    app.handle_message_for_test(Message::SettingsPatch {
        patch: SettingsPatch::from_pairs([("models", serde_json::json!([second.clone()]))]),
    });
    let saved = pet_config::load_config(&paths.config()).config;
    assert_eq!(saved.models, vec![second]);
    assert_eq!(saved.character, "anon");
    assert_eq!(saved.live2d_scale, 75);
    let restored = PetApp::new_with(paths, &session("multi-patch-restored"), false).unwrap();
    assert_eq!(restored.config(), &saved);
}
