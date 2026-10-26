//! System tray icon and menu (spec §2.5).

use std::sync::mpsc::Sender;

use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use tracing::{debug, warn};

/// Menu actions surfaced to the app.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrayEvent {
    /// Show / hide the pet windows.
    ToggleVisible,
    /// Set window opacity (`0.2..1.0`).
    Opacity(f32),
    /// Reset every role to the primary screen center.
    ResetPositions,
    /// Open the settings window.
    OpenSettings,
    /// Quit the pet process.
    Quit,
}

/// Tray icon owner. Dropping it removes the icon.
pub struct Tray {
    _icon: TrayIcon,
    menu: Menu,
    ids: Vec<(muda::MenuId, TrayEvent)>,
    sender: Sender<TrayEvent>,
}

impl Tray {
    /// Builds the tray icon and menu.
    pub fn new(sender: Sender<TrayEvent>) -> Result<Self, Box<dyn std::error::Error>> {
        let menu = Menu::new();
        let mut ids = Vec::new();

        let show_hide = MenuItem::new("显示 / 隐藏", true, None);
        let settings = MenuItem::new("设置…", true, None);
        let reset = MenuItem::new("重置位置", true, None);
        let quit = MenuItem::new("退出", true, None);

        let opacity_menu = Submenu::new("不透明度", true);
        let mut opacity_items = Vec::new();
        for percent in [100, 80, 60, 40, 20] {
            let item = MenuItem::new(format!("{percent}%"), true, None);
            opacity_menu.append(&item)?;
            opacity_items.push((item.id().clone(), TrayEvent::Opacity(percent as f32 / 100.0)));
        }

        menu.append(&show_hide)?;
        menu.append(&settings)?;
        menu.append(&reset)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&opacity_menu)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit)?;

        ids.push((show_hide.id().clone(), TrayEvent::ToggleVisible));
        ids.push((settings.id().clone(), TrayEvent::OpenSettings));
        ids.push((reset.id().clone(), TrayEvent::ResetPositions));
        ids.push((quit.id().clone(), TrayEvent::Quit));
        ids.extend(opacity_items);

        let icon = build_icon()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip("BANDORI-PET-LITE")
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _icon: tray,
            menu,
            ids,
            sender,
        })
    }

    /// Polls the global muda menu event queue and forwards mapped actions.
    pub fn poll(&mut self) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some((_, action)) = self
                .ids
                .iter()
                .find(|(id, _)| id == &event.id)
                .map(|(id, action)| (id.clone(), *action))
            {
                debug!(?action, "tray menu action");
                let _ = self.sender.send(action);
            }
        }
    }

    /// Replaces the menu (used when the language or role list changes).
    pub fn set_menu(&mut self, menu: Menu) {
        self._icon.set_menu(Some(Box::new(menu.clone())));
        self.menu = menu;
    }

    /// Access the current menu.
    pub fn menu(&self) -> &Menu {
        &self.menu
    }
}

/// Builds a 32×32 brand-coloured tray icon.
fn build_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = SIZE as f32 / 2.0;
    let radius = center - 1.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let index = ((y * SIZE + x) * 4) as usize;
            if distance <= radius {
                // Brand pink #e4004f with a soft inner highlight.
                let shade = 1.0 - (distance / radius) * 0.25;
                rgba[index] = (0xe4 as f32 * shade) as u8;
                rgba[index + 1] = (0x00 as f32 * shade) as u8;
                rgba[index + 2] = (0x4f as f32 * shade) as u8;
                rgba[index + 3] = 255;
            } else if distance <= radius + 1.0 {
                let alpha = ((radius + 1.0 - distance) * 255.0).clamp(0.0, 255.0) as u8;
                rgba[index] = 0xe4;
                rgba[index + 1] = 0x00;
                rgba[index + 2] = 0x4f;
                rgba[index + 3] = alpha;
            }
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).map_err(|error| {
        warn!(%error, "failed to build tray icon");
        Box::new(error) as Box<dyn std::error::Error>
    })
}

