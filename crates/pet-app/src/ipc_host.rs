//! IPC server hosted by the pet process.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use pet_ipc::{Connection, IpcError, Message, Server};
use tracing::{debug, warn};

/// Events surfaced to the event loop.
#[derive(Debug, Clone, PartialEq)]
pub enum IpcEvent {
    /// The settings process connected.
    Connected,
    /// The settings process went away.
    Disconnected,
    /// One message from the settings process.
    Message(Message),
}

/// Owns the IPC listener thread and the active connection.
pub struct IpcHost {
    name: String,
    events: Receiver<IpcEvent>,
    writer: Arc<Mutex<Option<Connection>>>,
    _listener: JoinHandle<()>,
}

impl IpcHost {
    /// Starts the listener thread for `name` (blocking accept happens there).
    pub fn start(name: String) -> Self {
        let (tx, rx) = channel();
        let writer: Arc<Mutex<Option<Connection>>> = Arc::new(Mutex::new(None));
        let thread_writer = Arc::clone(&writer);
        let thread_name = name.clone();
        let listener = std::thread::Builder::new()
            .name("pet-ipc".to_owned())
            .spawn(move || listen_loop(thread_name, tx, thread_writer))
            .expect("spawn ipc thread");
        Self {
            name,
            events: rx,
            writer,
            _listener: listener,
        }
    }

    /// Pipe name in use.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `true` when a settings process is attached.
    pub fn is_connected(&self) -> bool {
        self.writer.lock().map(|guard| guard.is_some()).unwrap_or(false)
    }

    /// Sends one message to the settings process (dropped when disconnected).
    pub fn send(&self, message: &Message) {
        let Ok(mut guard) = self.writer.lock() else {
            return;
        };
        let Some(connection) = guard.as_mut() else {
            return;
        };
        if let Err(error) = connection.send(message) {
            warn!(%error, "ipc send failed");
            *guard = None;
        }
    }

    /// Drains pending events.
    pub fn poll(&self) -> Option<IpcEvent> {
        self.events.try_recv().ok()
    }
}

fn listen_loop(name: String, tx: Sender<IpcEvent>, writer: Arc<Mutex<Option<Connection>>>) {
    let server = loop {
        match Server::bind(&name) {
            Ok(server) => break server,
            Err(error) => {
                warn!(%error, "failed to bind ipc pipe, retrying");
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    };
    loop {
        debug!(pipe = %name, "waiting for settings process");
        let mut reader = match server.accept() {
            Ok(connection) => connection,
            Err(error) => {
                warn!(%error, "ipc accept failed");
                continue;
            }
        };
        {
            if let Ok(mut guard) = writer.lock() {
                if let Ok(clone) = reader.try_clone() {
                    *guard = Some(clone);
                }
            }
        }
        let _ = tx.send(IpcEvent::Connected);
        loop {
            match reader.recv() {
                Ok(Some(message)) => {
                    if tx.send(IpcEvent::Message(message)).is_err() {
                        return;
                    }
                }
                Ok(None) => break,
                Err(IpcError::Io(error)) => {
                    debug!(%error, "ipc read ended");
                    break;
                }
                Err(error) => {
                    warn!(%error, "ipc read error");
                    break;
                }
            }
        }
        if let Ok(mut guard) = writer.lock() {
            *guard = None;
        }
        let _ = tx.send(IpcEvent::Disconnected);
    }
}



