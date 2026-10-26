//! Named-pipe protocol between `bandori-pet` and `bandori-ui` (spec §5).
//!
//! One long-lived connection carrying length-prefixed JSON. `bandori-pet` is the
//! only writer of `config.json`; the settings window sends [`Message::SettingsPatch`]
//! and receives [`Message::PatchApplied`] plus a fresh [`Message::ConfigSnapshot`].

use std::io::{Read, Write};
use std::sync::Arc;

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions, Stream};
use pet_config::SettingsPatch;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Default pipe base name; the session id is appended.
pub const PIPE_BASE: &str = "bandori-pet-lite";

/// One radial-menu entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    /// Stable action id (`costume`, `motion`, `settings`, `hide`).
    pub action: String,
    /// Localized label.
    pub label: String,
    /// Emoji glyph used by the radial menu.
    pub glyph: String,
    /// `[r, g, b]`.
    pub color: [u8; 3],
    /// Whether the item is clickable.
    pub enabled: bool,
}

impl MenuItem {
    /// Builds the four LITE radial-menu items in the original order
    /// (chat and pixel are out of scope for v0).
    pub fn lite_items(language: &str) -> Vec<Self> {
        let zh = !language.starts_with("en");
        vec![
            MenuItem {
                action: "costume".to_owned(),
                label: if zh { "换装" } else { "Outfit" }.to_owned(),
                glyph: "\u{1F457}".to_owned(),
                color: [220, 50, 120],
                enabled: true,
            },
            MenuItem {
                action: "motion".to_owned(),
                label: if zh { "动作" } else { "Motion" }.to_owned(),
                glyph: "\u{1F3AC}".to_owned(),
                color: [30, 144, 255],
                enabled: true,
            },
            MenuItem {
                action: "settings".to_owned(),
                label: if zh { "设置" } else { "Settings" }.to_owned(),
                glyph: "\u{2699}".to_owned(),
                color: [138, 43, 226],
                enabled: true,
            },
            MenuItem {
                action: "hide".to_owned(),
                label: if zh { "隐藏" } else { "Hide" }.to_owned(),
                glyph: "\u{1F441}".to_owned(),
                color: [91, 91, 116],
                enabled: true,
            },
        ]
    }
}

/// Settings page identifiers used by `OPEN_SETTINGS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsPage {
    /// Characters and models.
    Model,
    /// Display and performance.
    Display,
    /// Interaction and actions.
    Behavior,
    /// About.
    About,
}

/// Messages exchanged over the pipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// pet → ui: the pipe is up.
    Ready,
    /// pet → ui: full document (settings window opening).
    ConfigSnapshot {
        /// Raw `config.json` value, so unknown keys survive.
        config: serde_json::Value,
        /// Model names for the dropdowns of the current model.
        model: ModelData,
        /// False on first run / when no model is configured.
        first_run: bool,
    },
    /// pet → ui: current model capabilities.
    ModelData(ModelData),
    /// pet → ui: show the settings window on `page`.
    OpenSettings {
        /// Page to focus.
        page: SettingsPage,
    },
    /// pet → ui: show the radial menu at a screen position.
    MenuShow {
        /// Screen x in physical pixels.
        x: i32,
        /// Screen y in physical pixels.
        y: i32,
        /// Menu entries.
        items: Vec<MenuItem>,
        /// Current drag-lock state.
        locked: bool,
    },
    /// pet → ui: close the radial menu.
    MenuClose,
    /// pet → ui: result of applying a patch.
    PatchApplied {
        /// Whether the document changed on disk.
        ok: bool,
        /// Keys the schema refused.
        rejected: Vec<String>,
        /// New document.
        config: serde_json::Value,
    },
    /// pet → ui: show the poke bubble above the pet for ~2 s.
    Bubble {
        /// Screen x of the bubble anchor (physical pixels).
        x: i32,
        /// Screen y of the bubble anchor (physical pixels).
        y: i32,
        /// Bubble text.
        text: String,
        /// Lifetime in milliseconds.
        duration_ms: u32,
    },
    /// pet → ui: non-fatal problem for the InfoBar.
    Error {
        /// Short category.
        kind: String,
        /// Human readable message.
        message: String,
    },
    /// pet → ui: the pet process is shutting down.
    Quit,

    /// ui → pet: request a snapshot.
    RequestSnapshot,
    /// ui → pet: save a batch of settings.
    SettingsPatch {
        /// Patch payload.
        patch: SettingsPatch,
    },
    /// ui → pet: radial-menu action.
    MenuAct {
        /// Action id from [`MenuItem`].
        action: String,
    },
    /// ui → pet: drag-lock toggled from the radial-menu hub.
    MenuLocked {
        /// New lock state.
        locked: bool,
    },
    /// ui → pet: reset every role to the primary screen center.
    ResetPositions,
    /// ui → pet: add a character or change its costume, keeping other roles.
    ModelSelect {
        /// Character key.
        character: String,
        /// Costume id.
        costume: String,
    },
    /// ui → pet: open the model folder in Explorer.
    OpenModelFolder,
    /// ui → pet: the settings window was hidden.
    SettingsClosed,
    /// ui → pet: quit.
    QuitRequest,
}

/// Model capabilities for the settings dropdowns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelData {
    /// Character key.
    pub character: String,
    /// Costume id.
    pub costume: String,
    /// `moc` or `moc3`.
    pub format: String,
    /// Motion group names.
    pub motions: Vec<String>,
    /// Expression names.
    pub expressions: Vec<String>,
    /// Drawable count (diagnostics).
    pub drawables: usize,
    /// Texture count.
    pub textures: usize,
}

/// Builds the pipe name for this session.
pub fn pipe_name(session: &str) -> String {
    format!("{PIPE_BASE}-{session}")
}

/// Framing errors.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Transport failure.
    #[error("ipc io error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON failure.
    #[error("ipc codec error: {0}")]
    Codec(#[from] serde_json::Error),
    /// Named pipe creation failed.
    #[error("failed to bind {name}: {source}")]
    Bind {
        /// Pipe name.
        name: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Writes one length-prefixed JSON message.
pub fn write_message<W: Write>(writer: &mut W, message: &Message) -> Result<(), IpcError> {
    let payload = serde_json::to_vec(message)?;
    let length = (payload.len() as u32).to_le_bytes();
    writer.write_all(&length)?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Reads one length-prefixed JSON message; `Ok(None)` on a clean EOF.
pub fn read_message<R: Read>(reader: &mut R) -> Result<Option<Message>, IpcError> {
    let mut length_bytes = [0u8; 4];
    match reader.read_exact(&mut length_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(IpcError::Io(error)),
    }
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length == 0 || length > 32 * 1024 * 1024 {
        warn!(length, "refusing oversized ipc frame");
        return Ok(None);
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    Ok(Some(serde_json::from_slice(&payload)?))
}

/// Server side (owned by `bandori-pet`).
pub struct Server {
    name: String,
    listener: LocalSocketListener,
}

impl Server {
    /// Binds the pipe. A stale pipe from a previous run is replaced.
    pub fn bind(name: &str) -> Result<Self, IpcError> {
        let ns_name = name
            .to_ns_name::<GenericNamespaced>()
            .map_err(|error| IpcError::Bind {
                name: name.to_owned(),
                source: std::io::Error::other(error.to_string()),
            })?;
        let listener = ListenerOptions::new()
            .name(ns_name)
            .create_sync()
            .map_err(|error| IpcError::Bind {
                name: name.to_owned(),
                source: error,
            })?;
        debug!(name, "ipc server listening");
        Ok(Self {
            name: name.to_owned(),
            listener,
        })
    }

    /// Blocks until the settings process connects.
    pub fn accept(&self) -> Result<Connection, IpcError> {
        let stream = self.listener.accept()?;
        debug!(name = %self.name, "ipc client connected");
        Ok(Connection {
            stream: Arc::new(stream),
        })
    }
}

/// Client side (owned by `bandori-ui`), or an accepted server-side stream.
///
/// The stream sits behind an `Arc` so a reader and a writer can share it: `&Stream`
/// implements both `Read` and `Write`, which is exactly what `interprocess` asks
/// for instead of a `try_clone` split.
#[derive(Clone)]
pub struct Connection {
    stream: Arc<Stream>,
}

impl Connection {
    /// Connects to a running pet process.
    pub fn connect(name: &str) -> Result<Self, IpcError> {
        let ns_name = name
            .to_ns_name::<GenericNamespaced>()
            .map_err(|error| IpcError::Bind {
                name: name.to_owned(),
                source: std::io::Error::other(error.to_string()),
            })?;
        let stream = Stream::connect(ns_name)?;
        Ok(Self {
            stream: Arc::new(stream),
        })
    }

    /// Shares the same stream (cheap `Arc` clone).
    pub fn try_clone(&self) -> Result<Self, IpcError> {
        Ok(Self {
            stream: Arc::clone(&self.stream),
        })
    }

    /// Sends one message.
    pub fn send(&mut self, message: &Message) -> Result<(), IpcError> {
        let mut stream: &Stream = &self.stream;
        write_message(&mut stream, message)
    }

    /// Receives one message, blocking. `None` means the peer closed the pipe.
    pub fn recv(&mut self) -> Result<Option<Message>, IpcError> {
        let mut stream: &Stream = &self.stream;
        read_message(&mut stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trips() {
        let mut buffer = Vec::new();
        let message = Message::OpenSettings {
            page: SettingsPage::Behavior,
        };
        write_message(&mut buffer, &message).unwrap();
        let mut cursor = std::io::Cursor::new(buffer);
        assert_eq!(read_message(&mut cursor).unwrap(), Some(message));
        assert_eq!(read_message(&mut cursor).unwrap(), None);
    }

    #[test]
    fn radial_items_match_the_lite_scope() {
        let items = MenuItem::lite_items("zh_CN");
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].action, "costume");
        assert_eq!(items[0].label, "换装");
        assert_eq!(items[3].action, "hide");
        let english = MenuItem::lite_items("en_US");
        assert_eq!(english[0].label, "Outfit");
    }

    #[test]
    fn pipe_name_is_stable() {
        assert_eq!(pipe_name("abc"), "bandori-pet-lite-abc");
    }

    #[test]
    fn server_and_client_exchange_messages() {
        let name = pipe_name(&format!("test-{}", std::process::id()));
        let server = Server::bind(&name).expect("bind");
        let handle = std::thread::spawn(move || {
            let mut client = Connection::connect(&name).expect("connect");
            client.send(&Message::RequestSnapshot).expect("send");
            let reply = client.recv().expect("recv").expect("message");
            assert_eq!(reply, Message::MenuClose);
        });
        let mut connection = server.accept().expect("accept");
        let request = connection.recv().expect("recv").expect("message");
        assert_eq!(request, Message::RequestSnapshot);
        connection.send(&Message::MenuClose).expect("send");
        handle.join().unwrap();
    }
}
