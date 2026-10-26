//! `bandori-pet` — the resident desktop-pet process.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::error::Error;

use pet_app::{logging, single_instance};
use pet_config::Paths;

fn main() -> Result<(), Box<dyn Error>> {
    let paths = Paths::discover();
    let _guard = logging::init(&paths.logs(), "info");

    let (instance, _mutex) = single_instance::acquire();
    if instance == single_instance::Instance::AlreadyRunning {
        tracing::info!("another bandori-pet is already running; exiting");
        return Ok(());
    }

    tracing::info!(
        root = %paths.root().display(),
        config = %paths.config().display(),
        "bandori-pet starting"
    );

    pet_app::app::run(paths)?;
    tracing::info!("bandori-pet exited");
    Ok(())
}
