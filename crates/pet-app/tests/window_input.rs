//! Real-window regression: BANDORI_PET_TEST_ROOT must contain models/.
//! Opt-in because it creates pet windows and briefly moves the system cursor.
#![cfg(windows)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pet_app::role::Role;
use pet_assets::{catalog::Catalog, index, ModelCache};
use pet_config::{Config, Paths};
use pet_window::win32;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, SendMessageW, WindowFromPoint, GWL_EXSTYLE,
    MA_NOACTIVATE, WM_MOUSEACTIVATE, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{WindowId, WindowLevel};

struct CursorRestore(Option<(f32, f32)>);

impl Drop for CursorRestore {
    fn drop(&mut self) {
        if let Some((x, y)) = self.0 {
            win32::set_cursor_position(x, y);
        }
    }
}

#[derive(Default)]
struct Scenario {
    completed: usize,
}

impl ApplicationHandler for Scenario {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let source = PathBuf::from(std::env::var_os("BANDORI_PET_TEST_ROOT").unwrap());
        let paths = Paths::with_root(&source);
        let index = index::load_or_scan(&paths.models());
        let catalog = Catalog::build(&paths.assets(), &index);
        let cache = ModelCache::new(
            std::env::temp_dir().join(format!("bandori-input-cache-{}", std::process::id())),
            paths.models(),
        );
        let _restore = CursorRestore(win32::cursor_position());
        let foreground = unsafe { GetForegroundWindow() };
        let mut gpu = None;
        for (character, costume) in [("kasumi", "live_default"), ("anon", "casual_spring_01")] {
            let mut config = Config::default();
            config.live2d_scale = 50;
            config.drag_locked = false;
            config.emotion_behavior_enabled = false;
            config.hide_live2d_model = character == "anon";
            let entry = pet_app::role::entry_for_selection(
                character,
                catalog.find(character, costume).unwrap(),
            );
            let mut role =
                Role::create(event_loop, &entry, &config, &paths, &cache, gpu.clone()).unwrap();
            gpu = Some(Arc::clone(role.renderer.gpu()));
            assert_eq!(
                unsafe { GetForegroundWindow() },
                foreground,
                "startup stole focus"
            );
            assert_eq!(role.window.is_visible(), Some(!config.hide_live2d_model));
            config.hide_live2d_model = false;
            role.apply_display_settings(&config);

            // Visibility and other winit flag changes used to erase the native
            // NOACTIVATE/TOOLWINDOW/LAYERED styles and cached click-through state.
            for passthrough in [true, false, true] {
                role.set_click_through(passthrough);
                role.set_hidden(true);
                assert_eq!(role.window.is_visible(), Some(false));
                role.set_hidden(false);
                assert_eq!(role.window.is_visible(), Some(true));
                role.window.set_window_level(WindowLevel::Normal);
                role.window.set_window_level(WindowLevel::AlwaysOnTop);
                let hwnd = win32::hwnd_of(&role.window).unwrap();
                let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
                assert_ne!(style & WS_EX_NOACTIVATE.0, 0);
                assert_ne!(style & WS_EX_LAYERED.0, 0);
                assert_ne!(style & WS_EX_TOOLWINDOW.0, 0);
                assert_eq!(style & WS_EX_APPWINDOW.0, 0);
                assert_eq!(win32::click_through(hwnd), passthrough);
                assert_eq!(
                    unsafe { SendMessageW(hwnd, WM_MOUSEACTIVATE, None, None) }.0,
                    MA_NOACTIVATE as isize
                );
                assert_eq!(
                    unsafe { GetForegroundWindow() },
                    foreground,
                    "reshow stole focus"
                );
            }

            let (x, y, width, height) = role.window_rect().unwrap();
            let size = role.window.inner_size();
            let snapshot = role.snapshot();
            let mut hit = None;
            let mut miss = None;
            for cy in (4..height.saturating_sub(4)).step_by(8) {
                for cx in (4..width.saturating_sub(4)).step_by(8) {
                    let local = (cx as f32, cy as f32);
                    if snapshot.hit_test(local, (size.width, size.height), 8).hit {
                        hit.get_or_insert(local);
                    } else {
                        miss.get_or_insert(local);
                    }
                }
            }
            let hwnd = win32::hwnd_of(&role.window).unwrap();
            // Query actual OS hit targeting, beyond checking a cached boolean.
            for (step, (local, expected)) in [
                (miss.unwrap(), true),
                (hit.unwrap(), false),
                (miss.unwrap(), true),
            ]
            .into_iter()
            .enumerate()
            {
                assert!(win32::set_cursor_position(
                    x as f32 + local.0,
                    y as f32 + local.1
                ));
                // No CursorMoved event is delivered while transparent. Re-entry
                // must switch back to the fast poll using the global cursor.
                std::thread::sleep(Duration::from_millis(20));
                role.poll_passthrough(&config, 1_000 + step as u64 * 200);
                assert_eq!(role.click_through, expected);
                let target = unsafe {
                    WindowFromPoint(POINT {
                        x: x + local.0 as i32,
                        y: y + local.1 as i32,
                    })
                };
                assert_eq!(
                    target == hwnd,
                    !expected,
                    "wrong OS mouse target for {character}"
                );
            }

            let hit = hit.unwrap();
            let miss = miss.unwrap();
            assert!(win32::set_cursor_position(
                x as f32 + hit.0,
                y as f32 + hit.1
            ));
            assert!(role.on_left_press(hit, &config, 2_000));
            assert!(win32::set_cursor_position(
                x as f32 + miss.0,
                y as f32 + miss.1
            ));
            assert!(
                !role.should_be_click_through(&config, 2_200),
                "drag lost its grip over transparent pixels"
            );
            role.on_left_release();
            assert!(role.should_be_click_through(&config, 2_400));
            config.drag_locked = true;
            assert!(win32::set_cursor_position(
                x as f32 + hit.0,
                y as f32 + hit.1
            ));
            assert!(role.on_left_press(hit, &config, 2_600));
            assert!(win32::set_cursor_position(
                x as f32 + miss.0,
                y as f32 + miss.1
            ));
            assert!(
                role.should_be_click_through(&config, 2_800),
                "drag lock blocked transparent pixels"
            );

            role.apply_obs_capture(true);
            role.set_click_through(false);
            role.set_click_through(true);
            assert!(win32::click_through(hwnd));
            role.apply_obs_capture(false);
            config.hide_live2d_model = true;
            role.apply_display_settings(&config);
            role.set_hidden(true);
            role.set_hidden(false);
            assert_eq!(role.window.is_visible(), Some(false));
            config.hide_live2d_model = false;
            role.apply_display_settings(&config);
            config.live2d_scale = 75;
            let dpr = role.window.scale_factor();
            role.apply_scale(&config, dpr).unwrap();
            assert!(win32::click_through(hwnd));
            assert_eq!(unsafe { GetForegroundWindow() }, foreground);
            self.completed += 1;
        }
        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

#[test]
#[ignore = "requires local kasumi/anon archives and a GPU; creates windows and moves the cursor"]
fn transparent_pixels_pass_through_and_pet_windows_never_activate() {
    let mut scenario = Scenario::default();
    let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
    event_loop.run_app(&mut scenario).unwrap();
    assert_eq!(scenario.completed, 2);
}
