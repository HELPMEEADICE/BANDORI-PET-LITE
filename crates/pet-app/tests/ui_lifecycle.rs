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
    if pid == state.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
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

#[test]
#[ignore = "opens native settings, radial menu and bubble windows"]
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
    host.send(&Message::Bubble {
        x: 800,
        y: 400,
        text: "退出回归".into(),
        duration_ms: 60_000,
    });
    wait_until("all UI windows are visible after reopening", || {
        visible_windows(pid).len() == 3
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
    for flag in [
        WS_EX_TOOLWINDOW,
        WS_EX_NOACTIVATE,
        WS_EX_LAYERED,
        WS_EX_TOPMOST,
    ] {
        assert_ne!(extended & flag.0, 0);
    }
    assert_eq!(extended & WS_EX_APPWINDOW.0, 0);
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
