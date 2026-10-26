//! BANDORI-PET-LITE: a Live2D desktop pet plus a settings window.
//!
//! `bandori-pet` owns the transparent wgpu windows, the tray icon and
//! `config.json`; `bandori-ui` is a Slint front end that only sends patches over a
//! named pipe. See `docs/BANDORI-PET-LITE.md` for the product contract.

pub mod app;
pub mod ipc_host;
pub mod logging;
pub mod role;
pub mod single_instance;
pub mod tray;
pub mod ui_process;

pub use app::{PetApp, AppResult};
pub use role::{Role, RoleError};
