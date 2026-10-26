//! `bandori-ui` — the Slint settings window and radial menu.
//!
//! Connects to the running `bandori-pet` over the named pipe given by `--pipe`
//! (or the default pipe), then serves the settings pages and the radial menu.

use std::error::Error;

use pet_app::logging;
use pet_config::Paths;

fn main() -> Result<(), Box<dyn Error>> {
    let paths = Paths::discover();
    let _guard = logging::init(&paths.logs(), "info");

    let mut pipe = pet_ipc::pipe_name("default");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pipe" => {
                if let Some(value) = args.next() {
                    pipe = value;
                }
            }
            other => tracing::warn!(argument = other, "unknown argument"),
        }
    }

    tracing::info!(%pipe, "bandori-ui starting");
    pet_ui::run(&pipe, &paths)?;
    tracing::info!("bandori-ui exited");
    Ok(())
}
