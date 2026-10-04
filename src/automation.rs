//! Linux scripting equivalent: a private Unix socket with JSON requests/replies.
use crate::{game::Rules, render::View};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};
#[derive(Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    Status,
    Move {
        text: String,
    },
    New {
        #[serde(default)]
        variant: Rules,
        #[serde(default)]
        computer: [bool; 2],
    },
    Undo,
    Seek {
        ply: usize,
    },
    Open {
        path: PathBuf,
    },
    Save {
        path: PathBuf,
    },
    SetFen {
        fen: String,
    },
    Hint,
    SetView {
        view: View,
    },
    Screenshot {
        path: PathBuf,
    },
    Pause {
        paused: bool,
    },
    Host {
        address: String,
    },
    Join {
        address: String,
    },
    Disconnect,
    Ask {
        request: String,
    },
    Respond {
        accepted: bool,
    },
    Resign,
    Quit,
}
pub struct Request {
    pub command: Command,
    pub reply: Sender<Value>,
}
pub struct Control {
    pub requests: Receiver<Request>,
    pub path: PathBuf,
    stop: Arc<AtomicBool>,
}
fn directory() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
                })
        })
        .join("chess-linux-control")
}
impl Control {
    pub fn start() -> Result<Self, String> {
        let dir = directory();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.sock", std::process::id()));
        let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let (tx, requests) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let tx = tx.clone();
                        std::thread::spawn(move || {
                            let _ = serve(stream, tx);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            requests,
            path,
            stop,
        })
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }
}
fn serve(mut stream: UnixStream, requests: Sender<Request>) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut text = String::new();
    BufReader::new(stream.try_clone().map_err(|e| e.to_string())?)
        .take(1024 * 1024)
        .read_line(&mut text)
        .map_err(|e| e.to_string())?;
    let response = match serde_json::from_str::<Command>(&text) {
        Ok(command) => {
            let (tx, rx) = mpsc::channel();
            requests
                .send(Request { command, reply: tx })
                .map_err(|e| e.to_string())?;
            rx.recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| json!({"ok":false,"error":"Application did not respond"}))
        }
        Err(e) => json!({"ok":false,"error":e.to_string()}),
    };
    writeln!(stream, "{response}").map_err(|e| e.to_string())
}
pub fn send(text: &str, path: Option<PathBuf>) -> Result<Value, String> {
    let mut paths = if let Some(path) = path {
        vec![path]
    } else {
        std::fs::read_dir(directory())
            .map_err(|e| format!("No running Chess application: {e}"))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|s| s == "sock"))
            .collect::<Vec<_>>()
    };
    paths.sort();
    paths.reverse();
    let mut connection = None;
    for path in paths {
        if let Ok(stream) = UnixStream::connect(path) {
            connection = Some(stream);
            break;
        }
    }
    let mut stream =
        connection.ok_or("No running Chess application; launch it before sending a command")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| e.to_string())?;
    writeln!(stream, "{text}").map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream)
        .take(1024 * 1024)
        .read_line(&mut reply)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&reply).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_control_roundtrip() {
        let control = Control::start().unwrap();
        let path = control.path.clone();
        let job = std::thread::spawn(move || send(r#"{"command":"status"}"#, Some(path)).unwrap());
        let request = control
            .requests
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert!(matches!(request.command, Command::Status));
        request
            .reply
            .send(json!({"ok":true,"fen":"example"}))
            .unwrap();
        assert_eq!(job.join().unwrap()["fen"], "example");
        assert_eq!(
            std::fs::metadata(&control.path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
