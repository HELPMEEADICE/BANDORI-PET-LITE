//! Animated popup and its native circular input region. Transparent gaps are
//! excluded from input, so clicks reach windows in other processes too.

use std::cell::Cell;
#[cfg(any(windows, test))]
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};

use crate::{RadialItem, RadialMenu};

const SIZE: f32 = 360.;
const RADIUS: f32 = 110.;
const ITEM_RADIUS: f32 = 44.;
const HUB_RADIUS: f32 = 36.;
const SHOW_MS: f32 = 180.;
const HIDE_MS: f32 = 120.;

/// The attributes hook runs when the component's adapter is constructed, before
/// its native window can be shown. Settings keep their normal window attributes.
pub(crate) fn initialize_backend() -> Result<Rc<Cell<bool>>, slint::PlatformError> {
    let creating_popup = Rc::new(Cell::new(false));
    let popup = Rc::clone(&creating_popup);
    let backend = slint::BackendSelector::new();
    #[cfg(target_os = "linux")]
    let backend = {
        use slint::winit_030::winit::platform::x11::EventLoopBuilderExtX11;
        let mut builder = slint::winit_030::winit::event_loop::EventLoop::<
            slint::winit_030::SlintEvent,
        >::with_user_event();
        builder.with_x11();
        backend
            .backend_name("winit".into())
            .with_winit_event_loop_builder(builder)
    };
    #[cfg(target_os = "macos")]
    let backend = {
        use slint::winit_030::winit::platform::macos::{
            ActivationPolicy, EventLoopBuilderExtMacOS,
        };
        let mut builder = slint::winit_030::winit::event_loop::EventLoop::<
            slint::winit_030::SlintEvent,
        >::with_user_event();
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_activate_ignoring_other_apps(false);
        backend.with_winit_event_loop_builder(builder)
    };
    backend
        .with_winit_window_attributes_hook(move |attributes| {
            if !popup.get() {
                return attributes;
            }
            let attributes = attributes
                .with_decorations(false)
                .with_resizable(false)
                .with_active(false);
            #[cfg(windows)]
            let attributes = {
                use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
                attributes
                    .with_skip_taskbar(true)
                    .with_undecorated_shadow(false)
            };
            #[cfg(target_os = "macos")]
            let attributes = {
                use slint::winit_030::winit::platform::macos::WindowAttributesExtMacOS;
                attributes
                    .with_has_shadow(false)
                    .with_accepts_first_mouse(true)
            };
            #[cfg(target_os = "linux")]
            let attributes = {
                use slint::winit_030::winit::platform::x11::{WindowAttributesExtX11, WindowType};
                attributes
                    .with_name("bandori-ui", "BANDORI-PET-LITE")
                    .with_x11_window_type(vec![WindowType::Utility])
            };
            attributes
        })
        .select()?;
    Ok(creating_popup)
}

pub(crate) fn items_to_slint(items: &[pet_ipc::MenuItem]) -> Vec<RadialItem> {
    let count = items.len().max(1) as f32;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let angle = std::f32::consts::TAU * index as f32 / count;
            RadialItem {
                action: item.action.clone().into(),
                label: item.label.clone().into(),
                glyph: item.glyph.clone().into(),
                offset_x: angle.sin(),
                offset_y: -angle.cos(),
                enabled: item.enabled,
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
struct Transition {
    started: Instant,
    opening: bool,
    from_spread: f32,
    from_reveal: f32,
}

impl Transition {
    fn sample(self, now: Instant) -> (f32, f32, bool) {
        let duration = if self.opening { SHOW_MS } else { HIDE_MS };
        let t = (now.duration_since(self.started).as_secs_f32() * 1000. / duration).min(1.);
        // OutBack on reveal, reversed on dismissal, matching the reference.
        let out_back = |t: f32| {
            let t = t - 1.;
            1. + 2.70158 * t * t * t + 1.70158 * t * t
        };
        if self.opening {
            (out_back(t), (t * SHOW_MS / 130.).min(1.), t >= 1.)
        } else {
            (
                self.from_spread * out_back(1. - t),
                self.from_reveal * (1. - t * HIDE_MS / 80.).max(0.),
                t >= 1.,
            )
        }
    }
}

pub(crate) struct Controller {
    menu: slint::Weak<RadialMenu>,
    timer: Timer,
    transition: Cell<Option<Transition>>,
    pending_lock: Cell<Option<bool>>,
    mouse_was_down: Cell<bool>,
    #[cfg(windows)]
    native: native::Popup,
}

impl Controller {
    pub(crate) fn new(menu: &RadialMenu) -> Rc<Self> {
        Rc::new(Self {
            menu: menu.as_weak(),
            timer: Timer::default(),
            transition: Cell::new(None),
            pending_lock: Cell::new(None),
            mouse_was_down: Cell::new(false),
            #[cfg(windows)]
            native: native::Popup::default(),
        })
    }

    pub(crate) fn show(self: &Rc<Self>, x: i32, y: i32, items: &[pet_ipc::MenuItem], locked: bool) {
        let Some(menu) = self.menu.upgrade() else {
            return;
        };
        self.timer.stop();
        self.pending_lock.set(None);
        menu.set_items(ModelRc::new(VecModel::from(items_to_slint(items))));
        menu.set_locked(locked);
        menu.set_interactive(false);
        menu.set_spread(0.);
        menu.set_reveal(0.);
        #[cfg(windows)]
        self.native.reset();
        // A hidden winit window can be recreated on each show. Configure both
        // before and after mapping; the creation attributes prevent focus theft.
        self.update_native(&menu);
        if let Err(error) = menu.show() {
            tracing::warn!(%error, "could not show radial menu");
            return;
        }
        place(&menu, x, y);
        self.update_native(&menu);
        self.mouse_was_down.set(mouse_down());
        self.transition.set(Some(Transition {
            started: Instant::now(),
            opening: true,
            from_spread: 0.,
            from_reveal: 0.,
        }));
        let weak = Rc::downgrade(self);
        self.timer
            .start(TimerMode::Repeated, Duration::from_millis(16), move || {
                if let Some(controller) = weak.upgrade() {
                    controller.tick();
                }
            });
    }

    pub(crate) fn dismiss(&self) {
        let Some(menu) = self.menu.upgrade() else {
            return;
        };
        if !menu.window().is_visible() || self.transition.get().is_some_and(|t| !t.opening) {
            return;
        }
        menu.set_interactive(false);
        self.transition.set(Some(Transition {
            started: Instant::now(),
            opening: false,
            from_spread: menu.get_spread(),
            from_reveal: menu.get_reveal(),
        }));
    }

    pub(crate) fn hide(&self) {
        self.timer.stop();
        self.transition.set(None);
        if let Some(menu) = self.menu.upgrade() {
            menu.set_interactive(false);
            let _ = menu.hide();
        }
    }

    pub(crate) fn lock_requested(&self, locked: bool, sent: bool) {
        self.pending_lock.set(sent.then_some(locked));
        if !sent {
            if let Some(menu) = self.menu.upgrade() {
                menu.set_locked(!locked);
            }
        }
    }

    pub(crate) fn sync_locked(&self, locked: bool) {
        // Do not let an older snapshot undo a click while its acknowledgement
        // is in flight. The latest requested value is applied in Slint at once.
        if self
            .pending_lock
            .get()
            .is_some_and(|pending| pending != locked)
        {
            return;
        }
        self.pending_lock.set(None);
        if let Some(menu) = self.menu.upgrade() {
            menu.set_locked(locked);
        }
    }

    fn tick(&self) {
        let Some(menu) = self.menu.upgrade() else {
            self.timer.stop();
            return;
        };
        if let Some(transition) = self.transition.get() {
            let (spread, reveal, finished) = transition.sample(Instant::now());
            menu.set_spread(spread);
            menu.set_reveal(reveal);
            if finished {
                self.transition.set(None);
                if !transition.opening {
                    self.hide();
                    return;
                }
                menu.set_interactive(true);
            }
        }
        self.update_native(&menu);
        let down = mouse_down();
        let pressed = down && !self.mouse_was_down.get();
        self.mouse_was_down.set(down);
        if pressed && self.outside(&menu) {
            self.dismiss();
        }
    }

    fn update_native(&self, menu: &RadialMenu) {
        #[cfg(windows)]
        self.native.update(menu);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        menu.window().with_winit_window(|window| {
            #[cfg(target_os = "macos")]
            pet_window::macos::apply_overlay_style(window);
            let outside = desktop_outside(menu, window);
            let _ = window.set_cursor_hittest(!outside);
        });
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        let _ = menu;
    }

    fn outside(&self, menu: &RadialMenu) -> bool {
        #[cfg(windows)]
        return self.native.outside(menu);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        return menu
            .window()
            .with_winit_window(|window| desktop_outside(menu, window))
            .unwrap_or(false);
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            let _ = menu;
            false
        }
    }
}

fn mouse_down() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
        };
        [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
            .iter()
            .any(|key| unsafe { GetAsyncKeyState(key.0 as i32) < 0 })
    }
    #[cfg(target_os = "macos")]
    {
        pet_window::macos::mouse_down()
    }
    #[cfg(target_os = "linux")]
    {
        pet_window::x11::mouse_down()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

fn bounded_position(center: (i32, i32), size: i32, work: [i32; 4]) -> (i32, i32) {
    (
        (center.0 - size / 2).clamp(work[0], work[0] + (work[2] - size).max(0)),
        (center.1 - size / 2).clamp(work[1], work[1] + (work[3] - size).max(0)),
    )
}

fn place(menu: &RadialMenu, x: i32, y: i32) {
    #[cfg(not(target_os = "macos"))]
    let scale = menu
        .window()
        .with_winit_window(|window| {
            window
                .available_monitors()
                .find(|monitor| {
                    let origin = monitor.position();
                    let size = monitor.size();
                    x >= origin.x
                        && y >= origin.y
                        && x < origin.x + size.width as i32
                        && y < origin.y + size.height as i32
                })
                .map(|monitor| monitor.scale_factor() as f32)
        })
        .flatten()
        .unwrap_or_else(|| menu.window().scale_factor());
    #[cfg(target_os = "macos")]
    let scale = menu
        .window()
        .with_winit_window(|window| {
            window
                .available_monitors()
                .find(|monitor| {
                    let scale = monitor.scale_factor();
                    let origin = monitor.position().to_logical::<f64>(scale);
                    let size = monitor.size().to_logical::<f64>(scale);
                    x as f64 >= origin.x
                        && y as f64 >= origin.y
                        && (x as f64) < origin.x + size.width
                        && (y as f64) < origin.y + size.height
                })
                .map(|monitor| monitor.scale_factor() as f32)
        })
        .flatten()
        .unwrap_or_else(|| menu.window().scale_factor());
    #[cfg(target_os = "macos")]
    let (x, y) = (
        (x as f32 * scale).round() as i32,
        (y as f32 * scale).round() as i32,
    );
    let size = (SIZE * scale).round() as i32;
    let monitors = pet_window::placement::monitors_for_scale(scale as f64);
    let monitor = monitors
        .iter()
        .find(|monitor| {
            let [left, top, width, height] = monitor.geometry;
            x >= left && y >= top && x < left + width && y < top + height
        })
        .or_else(|| monitors.iter().find(|monitor| monitor.is_primary));
    let position = monitor
        .map(|monitor| bounded_position((x, y), size, monitor.work))
        .unwrap_or((x - size / 2, y - size / 2));
    #[cfg(not(target_os = "macos"))]
    {
        menu.window()
            .set_position(slint::PhysicalPosition::new(position.0, position.1));
        menu.window()
            .set_size(slint::PhysicalSize::new(size as u32, size as u32));
    }
    #[cfg(target_os = "macos")]
    {
        menu.window().set_position(slint::LogicalPosition::new(
            position.0 as f32 / scale,
            position.1 as f32 / scale,
        ));
        menu.window().set_size(slint::LogicalSize::new(SIZE, SIZE));
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn desktop_outside(menu: &RadialMenu, window: &slint::winit_030::winit::window::Window) -> bool {
    #[cfg(target_os = "macos")]
    let cursor = pet_window::macos::cursor_position(window);
    #[cfg(target_os = "linux")]
    let cursor = pet_window::x11::cursor_position();
    let Some((x, y)) = cursor else {
        return false;
    };
    let Ok(position) = window.outer_position() else {
        return false;
    };
    let size = window.inner_size();
    let scale = window.scale_factor() as f32;
    let x = (x - position.x as f32 - size.width as f32 / 2.) / scale;
    let y = (y - position.y as f32 - size.height as f32 / 2.) / scale;
    !inside_circles(
        x,
        y,
        menu.get_reveal(),
        menu.get_spread(),
        menu.get_items()
            .iter()
            .map(|item| (item.offset_x, item.offset_y)),
    )
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn inside_circles(
    x: f32,
    y: f32,
    reveal: f32,
    spread: f32,
    items: impl Iterator<Item = (f32, f32)>,
) -> bool {
    reveal > 0.001
        && (x * x + y * y <= (HUB_RADIUS * (0.72 + 0.28 * reveal)).powi(2)
            || items.into_iter().any(|(offset_x, offset_y)| {
                (x - offset_x * RADIUS * spread).powi(2) + (y - offset_y * RADIUS * spread).powi(2)
                    <= ITEM_RADIUS.powi(2)
            }))
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::Win32::Graphics::Gdi::{
        CombineRgn, CreateEllipticRgn, CreateRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
    };

    #[derive(Default)]
    pub(super) struct Popup {
        hwnd: Cell<isize>,
        bounds: RefCell<Option<Vec<[i32; 4]>>>,
    }

    impl Popup {
        pub(super) fn reset(&self) {
            self.hwnd.set(0);
            self.bounds.borrow_mut().take();
        }

        pub(super) fn update(&self, menu: &RadialMenu) {
            menu.window().with_winit_window(|window| {
                let Some(hwnd) = pet_window::hwnd_of(window) else {
                    return;
                };
                if self.hwnd.replace(hwnd.0 as isize) != hwnd.0 as isize {
                    use slint::winit_030::winit::platform::windows::WindowExtWindows;
                    window.set_decorations(false);
                    window.set_resizable(false);
                    window.set_skip_taskbar(true);
                    window.set_undecorated_shadow(false);
                    if let Err(error) = pet_window::win32::apply_popup_style(hwnd) {
                        tracing::warn!(%error, "could not configure radial popup");
                    }
                    self.bounds.borrow_mut().take();
                }
                let scale = menu.window().scale_factor();
                let size = menu.window().size();
                let center = (size.width as f32 / 2., size.height as f32 / 2.);
                let mut circles = Vec::new();
                if menu.get_reveal() > 0.001 {
                    circles.push((
                        center.0,
                        center.1,
                        HUB_RADIUS * (0.72 + 0.28 * menu.get_reveal()) * scale,
                    ));
                    for item in menu.get_items().iter() {
                        circles.push((
                            center.0 + item.offset_x * RADIUS * menu.get_spread() * scale,
                            center.1 + item.offset_y * RADIUS * menu.get_spread() * scale,
                            ITEM_RADIUS * scale,
                        ));
                    }
                }
                let bounds: Vec<_> = circles
                    .into_iter()
                    .map(|(x, y, radius)| {
                        [
                            (x - radius).floor() as i32,
                            (y - radius).floor() as i32,
                            (x + radius).ceil() as i32,
                            (y + radius).ceil() as i32,
                        ]
                    })
                    .collect();
                if self.bounds.borrow().as_ref() == Some(&bounds) {
                    return;
                }
                unsafe {
                    let region = CreateRectRgn(0, 0, 0, 0);
                    if region.is_invalid() {
                        return;
                    }
                    for [left, top, right, bottom] in &bounds {
                        let circle = CreateEllipticRgn(*left, *top, *right, *bottom);
                        if circle.is_invalid() {
                            let _ = DeleteObject(region.into());
                            return;
                        }
                        CombineRgn(Some(region), Some(region), Some(circle), RGN_OR);
                        let _ = DeleteObject(circle.into());
                    }
                    // Windows owns the region only after a successful call.
                    if SetWindowRgn(hwnd, Some(region), true) == 0 {
                        let _ = DeleteObject(region.into());
                        return;
                    }
                }
                *self.bounds.borrow_mut() = Some(bounds);
                // Expanding a region can expose pixels without generating a
                // WM_PAINT (the first frame may have had an empty region).
                window.request_redraw();
                if self.hwnd.get() != 0 {
                    pet_window::reassert_topmost(hwnd);
                }
            });
        }

        pub(super) fn outside(&self, menu: &RadialMenu) -> bool {
            let Some((x, y)) = pet_window::cursor_position() else {
                return false;
            };
            let position = menu.window().position();
            let (x, y) = (x - position.x as f32, y - position.y as f32);
            self.bounds.borrow().as_ref().is_some_and(|bounds| {
                !bounds.iter().any(|[left, top, right, bottom]| {
                    let dx = (x - (*left + *right) as f32 / 2.) / ((*right - *left) as f32 / 2.);
                    let dy = (y - (*top + *bottom) as f32 / 2.) / ((*bottom - *top) as f32 / 2.);
                    dx * dx + dy * dy <= 1.
                })
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn popup_transparent_gaps_do_not_intercept_clicks() {
        let items = [(0., -1.), (1., 0.), (0., 1.), (-1., 0.)];
        let inside = |x, y, reveal| super::inside_circles(x, y, reveal, 1., items.into_iter());
        assert!(inside(0., 0., 1.));
        assert!(inside(0., -110., 1.));
        assert!(!inside(70., 70., 1.));
        assert!(!inside(180., 180., 1.));
        assert!(!inside(0., 0., 0.));
    }
    use super::*;

    #[test]
    fn animation_spreads_from_center_and_collapses_on_dismissal() {
        let started = Instant::now();
        let opening = Transition {
            started,
            opening: true,
            from_spread: 0.,
            from_reveal: 0.,
        };
        assert_eq!(opening.sample(started), (0., 0., false));
        let (spread, reveal, done) = opening.sample(started + Duration::from_millis(90));
        assert!(spread > 0.5 && spread < 1.15);
        assert!(reveal > 0.5 && reveal < 1. && !done);
        assert_eq!(
            opening.sample(started + Duration::from_millis(180)),
            (1., 1., true)
        );
        let closing = Transition {
            opening: false,
            from_spread: 1.,
            from_reveal: 1.,
            ..opening
        };
        assert_eq!(closing.sample(started), (1., 1., false));
        assert_eq!(
            closing.sample(started + Duration::from_millis(120)),
            (0., 0., true)
        );
    }

    #[test]
    fn popup_clamps_to_work_area_including_negative_monitor_coordinates() {
        assert_eq!(
            bounded_position((-1910, 10), 540, [-1920, 0, 1920, 1040]),
            (-1920, 0)
        );
        assert_eq!(
            bounded_position((-10, 1030), 540, [-1920, 0, 1920, 1040]),
            (-540, 500)
        );
        assert_eq!(bounded_position((5, 5), 540, [0, 0, 400, 300]), (0, 0));
    }

    #[test]
    fn menu_has_transparent_gaps_and_lock_icon_changes_on_each_click() {
        use i_slint_backend_testing::{ElementHandle, TestingBackend, TestingBackendOptions};
        slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        })))
        .unwrap();
        let menu = RadialMenu::new().unwrap();
        crate::texts::apply_radial(&menu, pet_i18n::Lang::ZhHans);
        menu.set_items(ModelRc::new(VecModel::from(items_to_slint(
            &pet_ipc::MenuItem::lite_items("zh_CN"),
        ))));
        menu.set_spread(1.);
        menu.set_reveal(1.);
        menu.set_interactive(true);
        menu.show().unwrap();
        let toggles = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&toggles);
        menu.on_locked_toggled(move |locked| recorded.borrow_mut().push(locked));
        for scale in [1., 1.5, 2.] {
            menu.window()
                .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
                    scale_factor: scale,
                });
            menu.window().set_size(slint::PhysicalSize::new(
                (SIZE * scale) as u32,
                (SIZE * scale) as u32,
            ));
            menu.window()
                .dispatch_event(slint::platform::WindowEvent::Resized {
                    size: slint::LogicalSize::new(SIZE, SIZE),
                });
            let locked = menu.window().take_snapshot().unwrap();
            let pixel = |x: f32, y: f32| {
                locked.as_slice()
                    [((y * scale) as u32 * locked.width() + (x * scale) as u32) as usize]
            };
            for (x, y) in [
                (0., 0.),
                (80., 80.),
                (180., 129.),
                (147., 147.),
                (359., 359.),
            ] {
                assert_eq!(
                    pixel(x, y).a,
                    0,
                    "background must be transparent at ({x}, {y}), scale {scale}"
                );
            }
            let button = pixel(180., 35.);
            assert_eq!(button.a, 255);
            assert!(
                (button.r as i16 - button.g as i16).abs() < 5 && button.b < 80,
                "LITE buttons must stay gray-black"
            );
            ElementHandle::find_by_accessible_label(&menu, "解除拖拽锁定")
                .next()
                .unwrap()
                .mock_single_click(slint::platform::PointerEventButton::Left);
            assert!(
                !menu.get_locked(),
                "unlock must update without an IPC round trip"
            );
            let unlocked = menu.window().take_snapshot().unwrap();
            let changed = locked
                .as_slice()
                .iter()
                .zip(unlocked.as_slice())
                .filter(|(a, b)| a != b)
                .count();
            assert!(changed > 20, "lock icon must repaint");
            if let Ok(output) = std::env::var("BANDORI_UI_QA_DIR") {
                std::fs::create_dir_all(&output).unwrap();
                image::save_buffer(
                    std::path::Path::new(&output).join(format!("radial-{scale}.png")),
                    unlocked.as_bytes(),
                    unlocked.width(),
                    unlocked.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            ElementHandle::find_by_accessible_label(&menu, "锁定拖拽")
                .next()
                .unwrap()
                .mock_single_click(slint::platform::PointerEventButton::Left);
            assert!(menu.get_locked());
        }
        assert_eq!(
            *toggles.borrow(),
            vec![false, true, false, true, false, true]
        );

        let controller = Controller::new(&menu);
        menu.set_locked(false);
        controller.lock_requested(false, true);
        controller.sync_locked(true);
        assert!(
            !menu.get_locked(),
            "a stale snapshot must not undo the immediate toggle"
        );
        controller.sync_locked(false);
        controller.sync_locked(true);
        assert!(
            menu.get_locked(),
            "settings changes must stay synchronized after acknowledgement"
        );
    }
}
