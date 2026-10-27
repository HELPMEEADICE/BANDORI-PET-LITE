//! System tray icon and menu (spec §2.5).

use std::sync::mpsc::Sender;

use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use pet_i18n::{Key, Lang};
use tracing::{debug, warn};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

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

/// Menu plus the action table that maps each item id to its event.
type LocalizedMenu = (Menu, Vec<(muda::MenuId, TrayEvent)>);

/// Tray icon owner. Dropping it removes the icon.
pub struct Tray {
    _icon: TrayIcon,
    menu: Menu,
    ids: Vec<(muda::MenuId, TrayEvent)>,
    sender: Sender<TrayEvent>,
    language: Lang,
}

impl Tray {
    /// Builds the tray icon and menu in `language`.
    pub fn new(
        sender: Sender<TrayEvent>,
        language: Lang,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // winit does not initialize or dispatch GTK. AppIndicator and muda must
        // be created and pumped on this same event-loop thread.
        #[cfg(target_os = "linux")]
        gtk::init()?;
        let (menu, ids) = menu_for(language)?;
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
            language,
        })
    }

    /// Rebuilds the menu when the UI language changed.
    pub fn set_language(&mut self, language: Lang) {
        if language == self.language {
            return;
        }
        match menu_for(language) {
            Ok((menu, ids)) => {
                self._icon.set_menu(Some(Box::new(menu.clone())));
                self.menu = menu;
                self.ids = ids;
                self.language = language;
            }
            Err(error) => warn!(%error, "could not relabel the tray menu"),
        }
    }

    /// Polls the global muda menu event queue and forwards mapped actions.
    pub fn poll(&mut self) {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if event.id() == self._icon.id()
                && matches!(
                    event,
                    TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    }
                )
            {
                let _ = self.sender.send(TrayEvent::OpenSettings);
            }
        }
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

    /// Access the current menu.
    pub fn menu(&self) -> &Menu {
        &self.menu
    }
}

/// Builds the localized menu plus the action table for each item.
fn menu_for(language: Lang) -> Result<LocalizedMenu, Box<dyn std::error::Error>> {
    let menu = Menu::new();
    let mut ids = Vec::new();

    let show_hide = MenuItem::new(language.text(Key::TrayShowHide), true, None);
    let settings = MenuItem::new(language.text(Key::TraySettings), true, None);
    let reset = MenuItem::new(language.text(Key::TrayResetPositions), true, None);
    let quit = MenuItem::new(language.text(Key::TrayQuit), true, None);

    let opacity_menu = Submenu::new(language.text(Key::TrayOpacity), true);
    let mut opacity_items = Vec::new();
    for percent in [100, 80, 60, 40, 20] {
        let item = MenuItem::new(format!("{percent}%"), true, None);
        opacity_menu.append(&item)?;
        opacity_items.push((
            item.id().clone(),
            TrayEvent::Opacity(percent as f32 / 100.0),
        ));
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

    Ok((menu, ids))
}

/// Embeds the same logo as the settings window, including its alpha channel.
fn build_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    const SIZE: u32 = 32;
    let image = crate::brand::icon_rgba(SIZE)?;
    Icon::from_rgba(image.into_raw(), SIZE, SIZE).map_err(|error| {
        warn!(%error, "failed to build tray icon");
        Box::new(error) as Box<dyn std::error::Error>
    })
}
