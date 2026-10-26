//! Window plumbing for the pet: Win32 styles, click-through, DWM decorations,
//! opacity, DPI, position persistence and z-order.
//!
//! The transparent wgpu surface lives in `pet-render`; this crate only owns the
//! native window behaviour (spec §2.1, §2.4, §2.8).

pub mod placement;
pub mod sizing;

#[cfg(windows)]
pub mod win32;

pub use placement::{
    auto_start_enabled, resolve_position, set_auto_start, MonitorInfo, PositionDecision,
};
pub use sizing::{content_size, fit_matrix, Bounds, ModelLayout};

#[cfg(windows)]
pub use win32::{
    apply_pet_style, cursor_position, hwnd_of, is_windows_11, reassert_topmost, set_window_rect,
    window_rect,
};
