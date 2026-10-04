use crate::game::SavedGame;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Message {
    Hello {
        version: u32,
    },
    Game(SavedGame),
    Move {
        ply: usize,
        before: String,
        text: String,
    },
    Request(String),
    Reply {
        request: String,
        accepted: bool,
    },
    Result(String),
    Chat(String),
}
#[derive(Debug)]
pub enum Event {
    Listening(String),
    Connected,
    Message(Box<Message>),
    Error(String),
    Disconnected,
}
pub struct Peer {
    pub events: Receiver<Event>,
    out: Sender<Message>,
    stop: Arc<AtomicBool>,
    pub host: bool,
    pub connected: bool,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl Peer {
    pub fn start(address: String, host: bool) -> Self {
        let (tx, events) = mpsc::channel();
        let (out, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let stream = if host {
                    let listener = TcpListener::bind(&address).map_err(|e| e.to_string())?;
                    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
                    let _ = tx.send(Event::Listening(
                        listener
                            .local_addr()
                            .map_err(|e| e.to_string())?
                            .to_string(),
                    ));
                    loop {
                        if flag.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        match listener.accept() {
                            Ok((s, _)) => break s,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(25))
                            }
                            Err(e) => return Err(e.to_string()),
                        }
                    }
                } else {
                    let addresses: Vec<_> = address
                        .to_socket_addrs()
                        .map_err(|e| e.to_string())?
                        .collect();
                    let mut connection = None;
                    for address in addresses {
                        if let Ok(stream) =
                            TcpStream::connect_timeout(&address, Duration::from_secs(5))
                        {
                            connection = Some(stream);
                            break;
                        }
                    }
                    connection.ok_or("Could not connect to the host")?
                };
                run(stream, &rx, &tx, &flag)
            })();
            if let Err(e) = result {
                let _ = tx.send(Event::Error(e));
            }
            let _ = tx.send(Event::Disconnected);
        });
        Self {
            events,
            out,
            stop,
            host,
            connected: false,
        }
    }
    pub fn send(&self, message: Message) -> Result<(), String> {
        self.out.send(message).map_err(|e| e.to_string())
    }
}
fn run(
    mut stream: TcpStream,
    out: &Receiver<Message>,
    events: &Sender<Event>,
    stop: &AtomicBool,
) -> Result<(), String> {
    stream.set_nonblocking(true).map_err(|e| e.to_string())?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    let mut input = Vec::new();
    let mut pending = Vec::new();
    let mut offset = 0usize;
    let mut buf = [0u8; 8192];
    let mut hello = false;
    pending.extend(serde_json::to_vec(&Message::Hello { version: 1 }).map_err(|e| e.to_string())?);
    pending.push(b'\n');
    while !stop.load(Ordering::Relaxed) {
        while let Ok(m) = out.try_recv() {
            if pending.len() > 4 * 1024 * 1024 {
                return Err("Network send queue exceeded its limit".into());
            }
            pending.extend(serde_json::to_vec(&m).map_err(|e| e.to_string())?);
            pending.push(b'\n');
        }
        if offset < pending.len() {
            match stream.write(&pending[offset..]) {
                Ok(0) => return Ok(()),
                Ok(n) => offset += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e.to_string()),
            }
            if offset == pending.len() {
                pending.clear();
                offset = 0;
            }
        }
        loop {
            match stream.read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    input.extend_from_slice(&buf[..n]);
                    if input.len() > 1024 * 1024 {
                        return Err("Network message exceeded its limit".into());
                    }
                    while let Some(end) = input.iter().position(|&b| b == b'\n') {
                        let message: Message =
                            serde_json::from_slice(&input[..end]).map_err(|e| e.to_string())?;
                        input.drain(..=end);
                        if !hello {
                            if !matches!(message, Message::Hello { version: 1 }) {
                                return Err("Incompatible network protocol".into());
                            }
                            hello = true;
                            let _ = events.send(Event::Connected);
                        } else {
                            if matches!(&message,Message::Game(g)if g.moves.len()>10000) {
                                return Err("Network game exceeds the move limit".into());
                            }
                            let _ = events.send(Event::Message(Box::new(message)));
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.to_string()),
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peers_exchange_moves() {
        let mut host = Peer::start("127.0.0.1:0".into(), true);
        let address = match host.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            Event::Listening(a) => a,
            e => panic!("{e:?}"),
        };
        let guest = Peer::start(address, false);
        assert!(matches!(
            host.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Connected
        ));
        assert!(matches!(
            guest.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Connected
        ));
        host.connected = true;
        host.send(Message::Move {
            ply: 0,
            before: "test".into(),
            text: "e2e4".into(),
        })
        .unwrap();
        match guest.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            Event::Message(m) => assert!(matches!(*m, Message::Move { ply: 0, .. })),
            e => panic!("{e:?}"),
        };
    }
}
