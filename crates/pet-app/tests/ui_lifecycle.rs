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
        Command::new(env!("CARGO_BIN_EXE_bandori-ui"))
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
