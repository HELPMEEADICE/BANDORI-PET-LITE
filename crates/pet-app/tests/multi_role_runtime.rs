//! Opt-in regression test using real Live2D archives and native GPU windows.
//! Run with BANDORI_PET_TEST_ROOT pointing at an installation containing models/.
#![cfg(windows)]

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use pet_app::PetApp;
use pet_assets::{catalog::Catalog, index};
use pet_config::{Config, ModelEntry, ModelFormat, Paths, SettingsPatch};
use pet_ipc::Message;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::WindowId;

struct Scenario {
    app: PetApp,
    paths: Paths,
    second: ModelEntry,
    alternate: ModelEntry,
    stage: u8,
    started: Instant,
    restored: Option<Config>,
}

impl Scenario {
    fn patch(&mut self, pairs: impl IntoIterator<Item = (&'static str, serde_json::Value)>) {
        self.app.handle_message_for_test(Message::SettingsPatch {
            patch: SettingsPatch::from_pairs(pairs),
        });
    }
}

impl ApplicationHandler for Scenario {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.app.resumed(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.app.window_event(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.stage >= 7 {
            event_loop.exit();
            return;
        }
        assert!(
            self.started.elapsed().as_secs() < 60,
            "runtime scenario timed out"
        );
        self.app.about_to_wait(event_loop);
        match self.stage {
            0 => {
                assert_eq!(self.app.role_count(), 1);
                self.app.handle_message_for_test(Message::ModelSelect {
                    character: self.second.character.clone(),
                    costume: self.second.costume.clone(),
                });
            }
            1 => {
                assert_eq!(self.app.role_count(), 2, "adding must create a window");
                let config = self.app.config();
                let a = &config.models[0].window_placement;
                let b = &config.models[1].window_placement;
                assert_ne!((a.x, a.y), (b.x, b.y), "new roles must be placed apart");
                let mut stale = config.models.clone();
                stale[0].window_placement.x = 0;
                self.patch([
                    ("models", serde_json::json!(stale)),
                    ("live2d_scale", serde_json::json!(75)),
                    ("fps", serde_json::json!(90)),
                ]);
            }
            2 => {
                assert_eq!(
                    self.app.role_count(),
                    2,
                    "combined model/scale patches keep all windows"
                );
                assert_eq!(self.app.config().fps, 90);
                assert_ne!(
                    self.app.config().models[0].window_x,
                    0,
                    "stale UI placement must not win"
                );
                self.second = self.app.config().models[1].clone();
                self.patch([("models", serde_json::json!([self.second]))]);
            }
            3 => {
                assert_eq!(self.app.role_count(), 1, "removing must drop the window");
                assert_eq!(
                    self.app.config().models[0].window_placement,
                    self.second.window_placement
                );
                self.app.handle_message_for_test(Message::ModelSelect {
                    character: self.alternate.character.clone(),
                    costume: self.alternate.costume.clone(),
                });
            }
            4 => {
                assert_eq!(
                    self.app.role_count(),
                    1,
                    "costume changes must reuse a window"
                );
                assert_eq!(self.app.config().models[0].costume, self.alternate.costume);
                let missing = ModelEntry::new(
                    "missing",
                    "missing",
                    "models/missing.zst::missing/model.json",
                    ModelFormat::Moc,
                );
                self.patch([(
                    "models",
                    serde_json::json!([self.app.config().models[0], missing]),
                )]);
            }
            5 => {
                assert_eq!(self.app.role_count(), 1);
                assert_eq!(
                    self.app.config().models.len(),
                    2,
                    "failed models remain configured"
                );
                self.patch([("opacity", serde_json::json!(0.8))]);
                let saved = pet_config::load_config(&self.paths.config()).config;
                assert_eq!(
                    saved.models.len(),
                    2,
                    "runtime saves must not drop failed models"
                );
                self.restored = Some(saved);
                self.app = PetApp::new_with(
                    self.paths.clone(),
                    &format!("runtime-restored-{}", std::process::id()),
                    false,
                )
                .unwrap();
                self.app.resumed(event_loop);
            }
            6 => {
                assert_eq!(
                    self.app.role_count(),
                    1,
                    "available roles are restored on startup"
                );
                let saved = self.restored.as_ref().unwrap();
                assert_eq!(self.app.config().models, saved.models);
                assert_eq!(self.app.config().character, saved.character);
                assert_eq!(self.app.config().opacity, 0.8);
                self.app.request_quit();
                self.app.about_to_wait(event_loop);
            }
            _ => unreachable!(),
        }
        self.stage += 1;
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.app.exiting(event_loop);
    }
}

#[test]
#[ignore = "requires local kasumi/anon archives and a GPU; creates native windows"]
fn add_remove_swap_and_restore_real_roles() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();
    let source = PathBuf::from(
        std::env::var_os("BANDORI_PET_TEST_ROOT").expect("set BANDORI_PET_TEST_ROOT"),
    );
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let root = std::env::temp_dir().join(format!(
        "bandori-multi-runtime-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("models")).unwrap();
    std::fs::create_dir_all(root.join("assets")).unwrap();
    for character in ["kasumi", "anon"] {
        std::fs::hard_link(
            source.join(format!("models/{character}.zst")),
            root.join(format!("models/{character}.zst")),
        )
        .unwrap();
    }
    let paths = Paths::with_root(&root);
    let index = index::load_or_scan(&paths.models());
    let catalog = Catalog::build(&paths.assets(), &index);
    let first = pet_app::role::entry_for_selection(
        "kasumi",
        catalog.find("kasumi", "live_default").unwrap(),
    );
    let second = pet_app::role::entry_for_selection(
        "anon",
        catalog.find("anon", "casual_spring_01").unwrap(),
    );
    let alternate = pet_app::role::entry_for_selection(
        "anon",
        catalog.find("anon", "school_winter_hs_1st").unwrap(),
    );
    let config = Config {
        models: vec![first],
        live2d_scale: 50,
        emotion_behavior_enabled: false,
        ..Default::default()
    };
    pet_config::save_config(&paths.config(), &config).unwrap();
    let app = PetApp::new_with(
        paths.clone(),
        &format!("runtime-{}", std::process::id()),
        false,
    )
    .unwrap();
    let mut scenario = Scenario {
        app,
        paths,
        second,
        alternate,
        stage: 0,
        started: Instant::now(),
        restored: None,
    };
    let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
    event_loop.run_app(&mut scenario).unwrap();
    assert_eq!(scenario.stage, 7);
}
