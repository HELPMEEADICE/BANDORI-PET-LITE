//! Opt-in regression using the real settings executable and native Windows UI.
#![cfg(windows)]

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use pet_app::ipc_host::{IpcEvent, IpcHost};
use pet_ipc::{Message, SettingsPage};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
};

struct UiChild(Child);

impl UiChild {
    fn assert_running(&mut self) {
        assert!(
            self.0.try_wait().unwrap().is_none(),
            "UI exited unexpectedly"
        );
    }
}

impl Drop for UiChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Default)]
struct VisibleWindows {
    pid: u32,
    windows: Vec<(HWND, String)>,
}

unsafe extern "system" fn collect_windows(hwnd: HWND, param: LPARAM) -> BOOL {
    let state = unsafe { &mut *(param.0 as *mut VisibleWindows) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if (state.pid == 0 || pid == state.pid) && unsafe { IsWindowVisible(hwnd) }.as_bool() {
        let mut title = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, &mut title) };
        // The event loop can own untitled system helper windows (e.g. IME).
        if len > 0 {
            state
                .windows
                .push((hwnd, String::from_utf16_lossy(&title[..len as usize])));
        }
    }
    BOOL(1)
}

fn visible_windows(pid: u32) -> Vec<(HWND, String)> {
    let mut state = VisibleWindows {
        pid,
        ..Default::default()
    };
    unsafe {
        EnumWindows(Some(collect_windows), LPARAM(&mut state as *mut _ as isize)).unwrap();
    }
    state.windows
}

fn wait_until(description: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out: {description}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn assert_buttons_are_painted(hwnd: HWND) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Dwm::DwmFlush;
    use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut bounds = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &mut bounds).unwrap();
    }
    let size = bounds.right - bounds.left;
    let scale = size as f32 / 360.;
    let center = size / 2;
    let offset = (110. * scale).round() as i32;
    let inset = (30. * scale).round() as i32;
    unsafe {
        DwmFlush().unwrap();
    }
    let screen = unsafe { GetDC(None) };
    assert!(!screen.is_invalid());
    let samples: Vec<_> = [
        (center, center - offset),
        (center + offset, center),
        (center, center + offset),
        (center - offset, center),
        (center, center),
    ]
    .into_iter()
    .map(|(x, y)| {
        let color = unsafe { GetPixel(screen, bounds.left + x + inset, bounds.top + y) }.0;
        [color & 255, (color >> 8) & 255, (color >> 16) & 255]
    })
    .collect();
    unsafe {
        ReleaseDC(None, screen);
    }
    for rgb in samples {
        assert!(
            rgb[0] >= 20
                && rgb[0] < 100
                && rgb[1] >= 20
                && rgb[1] < 100
                && rgb[2] >= 20
                && rgb[2] < 120,
            "popup must paint gray buttons on the desktop, even when its hit region works: {rgb:?}"
        );
    }
}

#[test]
#[ignore = "opens native settings and radial menu windows"]
fn ui_starts_hidden_reopens_after_close_and_exits_with_all_windows_visible() {
    let root = std::env::temp_dir().join(format!("bandori-ui-lifecycle-{}", std::process::id()));
    std::fs::create_dir_all(root.join("models")).unwrap();
    std::fs::create_dir_all(root.join("assets")).unwrap();
    let host = IpcHost::start(pet_ipc::pipe_name(&format!(
        "ui-lifecycle-{}",
        std::process::id()
    )));
    let mut child = UiChild(
        Command::new(
            std::env::var_os("BANDORI_UI_TEST_EXE")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_bandori-ui").into()),
        )
        .args(["--pipe", host.name()])
        .env("BANDORI_PET_ROOT", &root)
        .env("SLINT_BACKEND", "winit-software")
        .spawn()
        .unwrap(),
    );
    let pid = child.0.id();
    wait_until("UI requests its initial snapshot", || {
        child.assert_running();
        matches!(
            host.poll(),
            Some(IpcEvent::Message(Message::RequestSnapshot))
        )
    });
    host.send(&Message::ConfigSnapshot {
        config: serde_json::to_value(pet_config::Config::default()).unwrap(),
        model: Default::default(),
        first_run: false,
    });
    // Observe multiple IPC pump ticks: even snapshots must not show settings.
    for _ in 0..20 {
        child.assert_running();
        let windows = visible_windows(pid);
        assert!(windows.is_empty(), "UI popped up at startup: {windows:?}");
        std::thread::sleep(Duration::from_millis(50));
    }

    let settings_title = "Bandori 桌宠 LITE - 设置";
    host.send(&Message::OpenSettings {
        page: SettingsPage::Display,
    });
    wait_until("settings opens", || {
        visible_windows(pid)
            .iter()
            .any(|(_, title)| title == settings_title)
    });
    let settings = visible_windows(pid)
        .into_iter()
        .find(|(_, title)| title == settings_title)
        .unwrap()
        .0;
    unsafe { PostMessageW(Some(settings), WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap() };
    wait_until("settings close notification", || {
        matches!(
            host.poll(),
            Some(IpcEvent::Message(Message::SettingsClosed))
        )
    });
    wait_until("settings hides after close", || {
        visible_windows(pid).is_empty()
    });
    child.assert_running();

    host.send(&Message::OpenSettings {
        page: SettingsPage::Model,
    });
    host.send(&Message::MenuShow {
        x: 400,
        y: 400,
        items: pet_ipc::MenuItem::lite_items("zh_CN"),
        locked: false,
    });
    wait_until("both UI windows are visible after reopening", || {
        visible_windows(pid).len() == 2
    });
    host.send(&Message::Quit);
    wait_until("UI exits on Quit", || {
        child
            .0
            .try_wait()
            .unwrap()
            .map(|status| {
                assert!(status.success(), "UI exited with {status}");
                true
            })
            .unwrap_or(false)
    });
    assert!(visible_windows(pid).is_empty());
}

#[test]
#[ignore = "opens a native popup and a test backdrop; briefly moves the cursor"]
fn radial_popup_is_frameless_passes_gaps_through_animates_and_reopens() {
    use windows::core::w;
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::{ClientToScreen, GetWindowRgnBox};
    use windows::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEINPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetClientRect, GetForegroundWindow, GetWindowLongPtrW,
        GetWindowRect, SetCursorPos, WindowFromPoint, GWL_EXSTYLE, GWL_STYLE, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MOUSEMOVE, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
    };

    struct DpiContext(DPI_AWARENESS_CONTEXT);
    impl Drop for DpiContext {
        fn drop(&mut self) {
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
    let _dpi = DpiContext(unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
    });
    struct Backdrop(HWND, Option<(f32, f32)>);
    impl Drop for Backdrop {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
            if let Some((x, y)) = self.1 {
                pet_window::win32::set_cursor_position(x, y);
            }
        }
    }
    fn rect(hwnd: HWND) -> RECT {
        let mut rect = RECT::default();
        unsafe {
            GetWindowRect(hwnd, &mut rect).unwrap();
        }
        rect
    }
    fn post_click(hwnd: HWND, x: i32, y: i32) {
        let position = LPARAM(((y << 16) | (x & 0xffff)) as isize);
        unsafe {
            PostMessageW(Some(hwnd), WM_MOUSEMOVE, WPARAM(0), position).unwrap();
            PostMessageW(Some(hwnd), WM_LBUTTONDOWN, WPARAM(1), position).unwrap();
            PostMessageW(Some(hwnd), WM_LBUTTONUP, WPARAM(0), position).unwrap();
        }
    }
    fn mouse_button(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        };
        assert_eq!(
            unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) },
            1
        );
    }

    let foreground = unsafe { GetForegroundWindow() };
    let backdrop = Backdrop(
        unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                w!("BUTTON"),
                w!("Radial menu test backdrop"),
                WS_POPUP | WS_VISIBLE,
                100,
                100,
                1000,
                1000,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        },
        pet_window::cursor_position(),
    );
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("models")).unwrap();
    std::fs::create_dir_all(root.path().join("assets")).unwrap();
    let host = IpcHost::start(pet_ipc::pipe_name(&format!(
        "radial-popup-{}",
        std::process::id()
    )));
    let mut child = UiChild(
        Command::new(
            std::env::var_os("BANDORI_UI_TEST_EXE")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_bandori-ui").into()),
        )
        .args(["--pipe", host.name()])
        .env("BANDORI_PET_ROOT", root.path())
        .env(
            "SLINT_BACKEND",
            std::env::var("BANDORI_UI_TEST_BACKEND").unwrap_or_else(|_| "winit-software".into()),
        )
        .spawn()
        .unwrap(),
    );
    let pid = child.0.id();
    wait_until("radial UI connects", || {
        child.assert_running();
        matches!(
            host.poll(),
            Some(IpcEvent::Message(Message::RequestSnapshot))
        )
    });
    host.send(&Message::ConfigSnapshot {
        config: serde_json::to_value(pet_config::Config::default()).unwrap(),
        model: Default::default(),
        first_run: false,
    });
    if std::env::var_os("BANDORI_UI_TEST_AFTER_SETTINGS").is_some() {
        host.send(&Message::OpenSettings {
            page: SettingsPage::Display,
        });
        wait_until("settings opens before the menu", || {
            visible_windows(pid)
                .iter()
                .any(|(_, title)| title == "Bandori 桌宠 LITE - 设置")
        });
        std::thread::sleep(Duration::from_millis(300));
        let settings = visible_windows(pid)
            .into_iter()
            .find(|(_, title)| title == "Bandori 桌宠 LITE - 设置")
            .unwrap()
            .0;
        unsafe {
            PostMessageW(Some(settings), WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
        }
        wait_until("settings hides before the menu", || {
            visible_windows(pid).is_empty()
        });
    }
    let show = || {
        host.send(&Message::MenuShow {
            x: 600,
            y: 550,
            items: pet_ipc::MenuItem::lite_items("zh_CN"),
            locked: true,
        })
    };
    let find_menu = || {
        visible_windows(pid)
            .into_iter()
            .find(|(_, title)| title == "BANDORI-PET-LITE 菜单")
            .map(|(hwnd, _)| hwnd)
    };
    show();
    wait_until("radial popup opens", || find_menu().is_some());
    let menu = find_menu().unwrap();
    let bounds = rect(menu);
    let size = bounds.right - bounds.left;
    let scale = size as f32 / 360.;
    let center = size / 2;
    let offset = (110. * scale).round() as i32;
    let mut spans = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut region = RECT::default();
        unsafe {
            GetWindowRgnBox(menu, &mut region);
        }
        let span = region.right - region.left;
        spans.push(span);
        if span >= (306. * scale) as i32 {
            break;
        }
        assert!(Instant::now() < deadline, "popup did not expand: {spans:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        spans.iter().any(|span| *span < (300. * scale) as i32),
        "expected an intermediate animation frame: {spans:?}"
    );
    std::thread::sleep(Duration::from_millis(200));
    assert_buttons_are_painted(menu);
    let style = unsafe { GetWindowLongPtrW(menu, GWL_STYLE) } as u32;
    let extended = unsafe { GetWindowLongPtrW(menu, GWL_EXSTYLE) } as u32;
    assert_eq!(style & WS_THICKFRAME.0, 0);
    // winit deliberately keeps WS_CAPTION for snap support and removes its
    // non-client area in WM_NCCALCSIZE. Verify the actual frame geometry.
    let mut client = RECT::default();
    let mut client_origin = POINT::default();
    unsafe {
        GetClientRect(menu, &mut client).unwrap();
        assert!(ClientToScreen(menu, &mut client_origin).as_bool());
    }
    assert_eq!(
        (client.right, client.bottom),
        (bounds.right - bounds.left, bounds.bottom - bounds.top)
    );
    assert_eq!(
        (client_origin.x, client_origin.y),
        (bounds.left, bounds.top)
    );
    for flag in [WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE, WS_EX_TOPMOST] {
        assert_ne!(extended & flag.0, 0);
    }
    assert_eq!(extended & WS_EX_APPWINDOW.0, 0);
    assert_eq!(
        extended & WS_EX_LAYERED.0,
        0,
        "Slint popup must use its renderer's native presentation path"
    );
    assert_eq!(
        unsafe { GetForegroundWindow() },
        foreground,
        "popup stole focus"
    );
    for (x, y) in [
        (center, center),
        (center, center - offset),
        (center + offset, center),
    ] {
        assert_eq!(
            unsafe {
                WindowFromPoint(POINT {
                    x: bounds.left + x,
                    y: bounds.top + y,
                })
            },
            menu,
            "buttons must receive input"
        );
    }
    for (x, y) in [
        (10, 10),
        ((80. * scale) as i32, (80. * scale) as i32),
        (center, (129. * scale) as i32),
        ((147. * scale) as i32, (147. * scale) as i32),
    ] {
        assert_eq!(
            unsafe {
                WindowFromPoint(POINT {
                    x: bounds.left + x,
                    y: bounds.top + y,
                })
            },
            backdrop.0,
            "transparent gap ({x}, {y}) must hit the backdrop; popup={menu:?}"
        );
    }
    // Keep the popup open while alternating lock state twice, without waiting
    // for a configuration snapshot between the two native clicks.
    for locked in [false, true] {
        post_click(menu, center, center);
        wait_until(
            "hub reports its latest lock state",
            || matches!(host.poll(), Some(IpcEvent::Message(Message::MenuLocked { locked: value })) if value == locked),
        );
        assert!(find_menu().is_some());
    }

    // The underlying window is ours: an actual outside click can safely verify
    // both passthrough and the global-button dismissal path.
    unsafe {
        SetCursorPos(bounds.left + 10, bounds.top + 10).unwrap();
    }
    mouse_button(MOUSEEVENTF_LEFTDOWN);
    std::thread::sleep(Duration::from_millis(60));
    mouse_button(MOUSEEVENTF_LEFTUP);
    wait_until("outside click dismisses popup", || find_menu().is_none());
    show();
    wait_until("popup reopens", || find_menu().is_some());
    std::thread::sleep(Duration::from_millis(250));
    let menu = find_menu().unwrap();
    assert_buttons_are_painted(menu);
    post_click(menu, center + offset, center);
    wait_until(
        "motion button dispatches its action",
        || matches!(host.poll(), Some(IpcEvent::Message(Message::MenuAct { action })) if action == "motion"),
    );
    wait_until("action collapses and hides popup", || find_menu().is_none());
    show();
    wait_until("popup can open again after action", || {
        find_menu().is_some()
    });
    host.send(&Message::MenuClose);
    wait_until("MenuClose hides popup", || find_menu().is_none());
    host.send(&Message::Quit);
    wait_until("radial UI exits", || child.0.try_wait().unwrap().is_some());
    println!("native radial popup passed at {scale}x DPI; animation spans: {spans:?}");
}

#[test]
#[ignore = "requires a local installation, model and GPU; launches both executables and moves the cursor"]
fn right_click_model_paints_menu_in_the_real_two_process_runtime() {
    use std::path::PathBuf;
    use windows::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP};

    struct CursorRestore(Option<(f32, f32)>);
    impl Drop for CursorRestore {
        fn drop(&mut self) {
            if let Some((x, y)) = self.0 {
                pet_window::win32::set_cursor_position(x, y);
            }
        }
    }

    struct PetChild(Child, PathBuf);
    impl Drop for PetChild {
        fn drop(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(15);
            while self.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(25));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
            if std::thread::panicking() {
                for file in std::fs::read_dir(self.1.join("logs"))
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                {
                    if let Ok(log) = std::fs::read_to_string(file.path()) {
                        eprintln!(
                            "test process log:\n{}",
                            log.lines()
                                .rev()
                                .take(30)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                    }
                }
            }
        }
    }
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let _cursor = CursorRestore(pet_window::cursor_position());
    let source = PathBuf::from(
        std::env::var_os("BANDORI_PET_TEST_ROOT").expect("set BANDORI_PET_TEST_ROOT"),
    );
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("models")).unwrap();
    let mut config = pet_config::load_config(&source.join("config.json")).config;
    config.auto_start = false;
    config.emotion_behavior_enabled = false;
    for entry in &config.models {
        let name = format!("{}.zst", entry.character);
        let target = root.path().join("models").join(&name);
        if !target.exists() {
            std::fs::hard_link(source.join("models").join(name), target).unwrap();
        }
    }
    std::fs::hard_link(
        std::env::var_os("BANDORI_UI_TEST_EXE")
            .map(PathBuf::from)
            .unwrap_or_else(|| source.join("bandori-ui.exe")),
        root.path().join("bandori-ui.exe"),
    )
    .unwrap();
    pet_config::save_config(&root.path().join("config.json"), &config).unwrap();
    let mut child = PetChild(
        Command::new(source.join("bandori-pet.exe"))
            .env("BANDORI_PET_ROOT", root.path())
            .env("BANDORI_PET_EXIT_AFTER_SECS", "15")
            .env("RUST_LOG", "warn,pet_app::app=info")
            .env(
                "SLINT_BACKEND",
                std::env::var("BANDORI_UI_TEST_BACKEND").unwrap_or_else(|_| "winit".into()),
            )
            .spawn()
            .unwrap(),
        root.path().to_path_buf(),
    );
    let pid = child.0.id();
    wait_until("real model window appears", || {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "pet exited before presenting a model"
        );
        !visible_windows(pid).is_empty()
    });
    let model = visible_windows(pid)[0].0;
    // Wait for the actual connection, including a cold model-catalog scan.
    wait_until("real UI connects", || {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "pet exited before UI connected"
        );
        std::fs::read_dir(root.path().join("logs"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|file| {
                std::fs::read_to_string(file.path())
                    .is_ok_and(|log| log.contains("settings process connected"))
            })
    });
    for (column, row) in [(1, 1), (2, 2), (1, 2)] {
        let (x, y, width, height) = pet_window::win32::window_rect(model).unwrap();
        let local = (width as i32 * column / 3, height as i32 * row / 3);
        let click = (x + local.0, y + local.1);
        assert!(pet_window::win32::set_cursor_position(
            click.0 as f32,
            click.1 as f32,
        ));
        let position = LPARAM(((local.1 << 16) | (local.0 & 0xffff)) as isize);
        unsafe {
            PostMessageW(Some(model), WM_MOUSEMOVE, WPARAM(0), position).unwrap();
            PostMessageW(Some(model), WM_RBUTTONDOWN, WPARAM(2), position).unwrap();
            PostMessageW(Some(model), WM_RBUTTONUP, WPARAM(0), position).unwrap();
        }
        let find_menu = || {
            visible_windows(0)
                .into_iter()
                .find(|(_, title)| title == "BANDORI-PET-LITE 菜单")
                .map(|(hwnd, _)| hwnd)
        };
        wait_until("right-click opens the real menu", || find_menu().is_some());
        std::thread::sleep(Duration::from_millis(350));
        let menu = find_menu().unwrap();
        let (left, top, width, height) = pet_window::win32::window_rect(menu).unwrap();
        let monitors = pet_window::placement::monitors_for_scale(width as f64 / 360.);
        let monitor = pet_window::placement::monitor_at(&monitors, click).unwrap();
        let [work_x, work_y, work_width, work_height] = monitor.work;
        let expected = (
            (click.0 - width as i32 / 2).clamp(work_x, work_x + (work_width - width as i32).max(0)),
            (click.1 - height as i32 / 2)
                .clamp(work_y, work_y + (work_height - height as i32).max(0)),
        );
        assert_eq!(
            (left, top),
            expected,
            "menu must follow the right-click at {click:?}, bounded by the work area"
        );
        assert_buttons_are_painted(menu);
    }
}
