//! Rust application controller for the Kirigami/Qt Quick shell.
use crate::{
    app::{Preferences, Recovery, Session, config_dir, notify_end, state_dir},
    automation::{Command, Control},
    document,
    engine::{self, Analysis},
    game::{Game, Rules},
    network::{self, Message, Peer},
    recording::Recording,
    render::{self, BoardRenderer, Motion},
    speech::{self, Listener, VoiceAction},
};
use serde_json::{Value, json};
use shakmaty::{CastlingMode, Color, Move, Position, Role, Square, uci::UciMove};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

struct Search {
    rx: Receiver<Result<Analysis, String>>,
    cancel: Arc<AtomicBool>,
    key: String,
    ply: usize,
    hint: bool,
}
impl Drop for Search {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
struct Controller {
    game: Game,
    path: Option<PathBuf>,
    dirty: bool,
    tabs: Vec<Session>,
    active: usize,
    prefs: Preferences,
    renderer: BoardRenderer,
    selected: Option<Square>,
    drop_role: Option<Role>,
    promotion: Vec<Move>,
    hint: Option<Move>,
    show_last: bool,
    motion: Option<Motion>,
    motion_start: Instant,
    search: Option<Search>,
    analysis: Option<Analysis>,
    paused: bool,
    message: String,
    peer: Option<Peer>,
    address: String,
    pending_request: Option<String>,
    remote_request: Option<String>,
    chat_log: Vec<String>,
    listener: Option<Listener>,
    recording: Option<Recording>,
    finishing: Vec<Receiver<Result<(), String>>>,
    control: Control,
    revision: u64,
    last_persist: Instant,
    close_index: Option<usize>,
    quitting: bool,
    close_allowed: bool,
    ui_events: Vec<Value>,
}
impl Controller {
    fn new(path: Option<PathBuf>, fresh: bool) -> Result<Self, String> {
        let prefs: Preferences = std::fs::read(config_dir().join("preferences.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut tabs = Vec::new();
        let mut active = 0;
        if !fresh
            && path.is_none()
            && let Ok(bytes) = std::fs::read(state_dir().join("recovery.json"))
            && let Ok(recovery) = serde_json::from_slice::<Recovery>(&bytes)
        {
            for (i, data) in recovery.games.into_iter().enumerate() {
                if let Ok(game) = Game::load(data) {
                    tabs.push(Session {
                        game,
                        path: recovery.paths.get(i).cloned().flatten(),
                        dirty: recovery.dirty.get(i).copied().unwrap_or(true),
                    });
                }
            }
            active = recovery.active.min(tabs.len().saturating_sub(1));
        }
        let mut message = String::new();
        if tabs.is_empty() {
            let (game, path) = if let Some(path) = path {
                match document::read(&path) {
                    Ok(g) => (g, Some(path)),
                    Err(e) => {
                        message = e;
                        (Game::new(Rules::Standard), None)
                    }
                }
            } else {
                (Game::new(Rules::Standard), None)
            };
            tabs.push(Session {
                game,
                path,
                dirty: false,
            });
        }
        let session = tabs[active].clone();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| e.to_string())?;
        let info = adapter.get_info();
        if info.backend != wgpu::Backend::Vulkan {
            return Err("A Vulkan adapter is required".into());
        }
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let renderer = BoardRenderer::new(device, queue, info.name);
        let mut app = Self {
            game: session.game,
            path: session.path,
            dirty: session.dirty,
            tabs,
            active,
            prefs,
            renderer,
            selected: None,
            drop_role: None,
            promotion: vec![],
            hint: None,
            show_last: true,
            motion: None,
            motion_start: Instant::now(),
            search: None,
            analysis: None,
            paused: false,
            message,
            peer: None,
            address: "127.0.0.1:7878".into(),
            pending_request: None,
            remote_request: None,
            chat_log: vec![],
            listener: None,
            recording: None,
            finishing: vec![],
            control: Control::start()?,
            revision: 1,
            last_persist: Instant::now(),
            close_index: None,
            quitting: false,
            close_allowed: false,
            ui_events: vec![],
        };
        app.clamp_preferences();
        Ok(app)
    }
    fn snapshot(&self) -> Session {
        Session {
            game: self.game.clone(),
            path: self.path.clone(),
            dirty: self.dirty,
        }
    }
    fn reset(&mut self) {
        self.search = None;
        self.selected = None;
        self.drop_role = None;
        self.promotion.clear();
        self.hint = None;
        self.analysis = None;
        self.motion = None;
        self.paused = true;
        self.revision += 1;
    }
    fn switch(&mut self, index: usize) -> Result<(), String> {
        if index >= self.tabs.len() {
            return Err("Unknown document".into());
        }
        self.tabs[self.active] = self.snapshot();
        let s = self.tabs[index].clone();
        self.active = index;
        self.game = s.game;
        self.path = s.path;
        self.dirty = s.dirty;
        self.reset();
        Ok(())
    }
    fn open_game(&mut self, game: Game, path: Option<PathBuf>, dirty: bool) {
        self.tabs[self.active] = self.snapshot();
        self.game = game;
        self.path = path;
        self.dirty = dirty;
        self.tabs.push(self.snapshot());
        self.active = self.tabs.len() - 1;
        self.reset();
    }
    fn recent(&mut self, path: PathBuf) {
        self.prefs.recent.retain(|p| *p != path);
        self.prefs.recent.insert(0, path);
        self.prefs.recent.truncate(12);
    }
    fn local_turn(&self) -> bool {
        if self.remote_request.is_some() || self.pending_request.is_some() {
            return false;
        }
        self.peer.as_ref().map_or_else(
            || !self.game.data.computer[usize::from(self.game.board.pos.turn() == Color::Black)],
            |p| p.connected && (self.game.board.pos.turn() == Color::White) == p.host,
        )
    }
    fn play(&mut self, m: Move, remote: bool) -> Result<(), String> {
        if !remote && self.peer.is_some() && !self.local_turn() {
            return Err("Waiting for your opponent".into());
        }
        let before = self.game.board.key();
        let ply = self.game.data.cursor;
        let previous = self.game.clone();
        self.game.push(m)?;
        self.dirty = true;
        self.search = None;
        self.hint = None;
        self.selected = None;
        self.drop_role = None;
        self.promotion.clear();
        self.analysis = None;
        self.motion_start = Instant::now();
        self.motion = self.prefs.view.animations.then_some(Motion {
            m,
            previous,
            progress: 0.0,
        });
        self.revision += 1;
        if !remote && let Some(peer) = &self.peer {
            peer.send(Message::Move {
                ply,
                before,
                text: UciMove::from_move(m, CastlingMode::Standard).to_string(),
            })?;
        }
        let side = usize::from(self.game.board.pos.turn() == Color::White);
        let speak = if remote || self.game.data.computer[side] {
            self.prefs.speak_computer
        } else {
            self.prefs.speak_human
        };
        if speak && let Err(e) = speech::speak(&self.game, m, &self.prefs.voices[side]) {
            self.message = e;
        }
        if self.game.result() != "*" {
            notify_end(&self.game);
        }
        Ok(())
    }
    fn square(&mut self, sq: Square) -> Result<(), String> {
        if !self.local_turn() || self.game.result() != "*" {
            return Ok(());
        }
        let candidates: Vec<_> = self
            .game
            .board
            .moves()
            .into_iter()
            .filter(|m| {
                render::destination(*m) == sq
                    && if let Some(role) = self.drop_role {
                        m.from().is_none() && m.role() == role
                    } else {
                        m.from() == self.selected && self.selected.is_some()
                    }
            })
            .collect();
        if candidates.len() == 1 {
            self.play(candidates[0], false)?;
        } else if candidates.len() > 1 {
            self.promotion = candidates;
        } else if self
            .game
            .at(sq)
            .is_some_and(|p| p.color == self.game.board.pos.turn())
        {
            self.selected = Some(sq);
            self.drop_role = None;
            self.hint = None;
        } else {
            self.selected = None;
        }
        self.revision += 1;
        Ok(())
    }
    fn seek(&mut self, ply: usize) -> Result<(), String> {
        if self.peer.is_some() {
            return Err("History is locked during network play".into());
        }
        if ply > self.game.data.moves.len() {
            return Err("Ply exceeds move history".into());
        }
        self.game.seek(ply);
        self.reset();
        self.dirty = true;
        Ok(())
    }
    fn undo(&mut self) -> Result<(), String> {
        if self.peer.is_some() {
            return self.ask("takeback");
        }
        let step = if self.game.data.computer.iter().filter(|&&v| v).count() == 1
            && self.game.data.cursor >= 2
        {
            2
        } else {
            1
        };
        self.seek(self.game.data.cursor.saturating_sub(step))
    }
    fn analyze(&mut self, hint: bool) {
        if self.game.result() != "*" {
            return;
        }
        self.search = None;
        let game = self.game.clone();
        let key = game.board.key();
        let ply = game.data.cursor;
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let path = self.prefs.sjeng_path.clone();
        let seconds = self.prefs.seconds;
        let depth = self.prefs.depth;
        std::thread::spawn(move || {
            let r = if path.is_empty() {
                Ok(engine::analyze(game.board, seconds, depth, flag))
            } else {
                crate::legacy_engine::analyze(
                    game,
                    std::path::Path::new(&path),
                    seconds,
                    depth,
                    flag,
                )
            };
            let _ = tx.send(r);
        });
        self.search = Some(Search {
            rx,
            cancel,
            key,
            ply,
            hint,
        });
    }
    fn ask(&mut self, request: &str) -> Result<(), String> {
        if !matches!(request, "draw" | "takeback") {
            return Err("Unknown offer".into());
        }
        if let Some(peer) = &self.peer {
            if !peer.connected {
                return Err("No connected opponent".into());
            }
            if self.pending_request.is_some() || self.remote_request.is_some() {
                return Err("Another offer is pending".into());
            }
            peer.send(Message::Request(request.into()))?;
            self.pending_request = Some(request.into());
        } else if request == "draw" {
            self.apply_offer("draw");
        }
        Ok(())
    }
    fn apply_offer(&mut self, request: &str) {
        if request == "draw" {
            self.game.data.result = "1/2-1/2".into();
            self.search = None;
            notify_end(&self.game);
        } else if request == "takeback" {
            self.game.seek(self.game.data.cursor.saturating_sub(1));
            self.game.data.moves.truncate(self.game.data.cursor);
            self.game.sans.truncate(self.game.data.cursor);
            self.game.states.truncate(self.game.data.cursor + 1);
            self.game
                .data
                .comments
                .retain(|&i, _| i <= self.game.data.cursor);
            self.game.data.result = "*".into();
            self.reset();
        }
        self.dirty = true;
        self.revision += 1;
    }
    fn respond(&mut self, accepted: bool) -> Result<(), String> {
        let request = self
            .remote_request
            .as_ref()
            .ok_or("No opponent offer is pending")?
            .clone();
        self.peer
            .as_ref()
            .ok_or("No network session")?
            .send(Message::Reply {
                request: request.clone(),
                accepted,
            })?;
        self.remote_request = None;
        if accepted {
            self.apply_offer(&request);
        }
        Ok(())
    }
    fn disconnect(&mut self) {
        self.peer = None;
        self.pending_request = None;
        self.remote_request = None;
    }
    fn resign(&mut self) {
        if self.game.result() != "*" {
            return;
        }
        let white = self
            .peer
            .as_ref()
            .map_or(self.game.board.pos.turn() == Color::White, |p| p.host);
        let result = if white { "0-1" } else { "1-0" };
        self.game.data.result = result.into();
        self.search = None;
        self.dirty = true;
        if let Some(p) = &self.peer {
            let _ = p.send(Message::Result(result.into()));
        }
        notify_end(&self.game);
        self.revision += 1;
    }
    fn network_message(&mut self, message: Message) {
        let host = self.peer.as_ref().is_some_and(|p| p.host);
        let result = (|| -> Result<(), String> {
            match message {
                Message::Game(mut data) if !host => {
                    data.computer = [false, false];
                    self.tabs[self.active] = self.snapshot();
                    self.game = Game::load(data)?;
                    self.path = None;
                    self.dirty = true;
                    self.reset();
                }
                Message::Move { ply, before, text } => {
                    if ply != self.game.data.cursor
                        || before != self.game.board.key()
                        || (self.game.board.pos.turn() == Color::White) == host
                    {
                        return Err("Rejected an out-of-sync network move".into());
                    }
                    self.play(self.game.board.parse_move(&text)?, true)?;
                }
                Message::Request(r) if matches!(r.as_str(), "draw" | "takeback") => {
                    if self.pending_request.is_some() || self.remote_request.is_some() {
                        if let Some(p) = &self.peer {
                            p.send(Message::Reply {
                                request: r,
                                accepted: false,
                            })?;
                        }
                    } else {
                        self.remote_request = Some(r);
                    }
                }
                Message::Reply { request, accepted }
                    if self.pending_request.as_deref() == Some(&request) =>
                {
                    self.pending_request = None;
                    if accepted {
                        self.apply_offer(&request);
                    }
                }
                Message::Result(r) if r == if host { "1-0" } else { "0-1" } => {
                    self.game.data.result = r;
                    self.search = None;
                    self.dirty = true;
                    notify_end(&self.game);
                }
                Message::Chat(text) => self.chat_log.push(format!(
                    "Opponent: {}",
                    text.chars().take(1000).collect::<String>()
                )),
                _ => (),
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.message = e;
        }
    }
    fn poll(&mut self) {
        let requests: Vec<_> = self.control.requests.try_iter().collect();
        for request in requests {
            let response = self.execute(request.command);
            let _ = request.reply.send(response);
        }
        if let Some(result) = self.search.as_ref().and_then(|s| s.rx.try_recv().ok()) {
            let search = self.search.take().unwrap();
            match result {
                Ok(a)
                    if search.key == self.game.board.key()
                        && search.ply == self.game.data.cursor =>
                {
                    if self.prefs.engine_log {
                        eprintln!(
                            "Search depth {} · score {} · nodes {}",
                            a.depth, a.score, a.nodes
                        );
                    }
                    self.analysis = Some(a.clone());
                    if search.hint {
                        self.hint = a.best;
                        self.revision += 1;
                    } else if let Some(m) = a.best
                        && let Err(e) = self.play(m, false)
                    {
                        self.message = e;
                    }
                }
                Err(e) => {
                    self.message = e;
                    self.paused = true;
                }
                _ => (),
            }
        }
        let events: Vec<_> = self
            .peer
            .as_ref()
            .map(|p| p.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                network::Event::Listening(address) => {
                    self.address = address;
                    self.message = format!("Waiting on {}", self.address);
                }
                network::Event::Connected => {
                    let p = self.peer.as_mut().unwrap();
                    p.connected = true;
                    self.search = None;
                    self.game.data.computer = [false, false];
                    if p.host {
                        self.game.seek(self.game.data.moves.len());
                        let _ = p.send(Message::Game(self.game.data.clone()));
                    } else {
                        self.prefs.view.yaw = 180.0;
                    }
                    self.message = "Connected · Host plays White, guest plays Black".into();
                }
                network::Event::Message(m) => self.network_message(*m),
                network::Event::Error(e) => self.message = e,
                network::Event::Disconnected => {
                    self.disconnect();
                    self.message = "Network session ended".into();
                }
            }
        }
        let voice: Vec<_> = self
            .listener
            .as_ref()
            .map(|l| l.events.try_iter().collect())
            .unwrap_or_default();
        for event in voice {
            match event {
                Ok(text) => {
                    if let Err(e) = self.voice(&text) {
                        self.message = e;
                    }
                }
                Err(e) => {
                    self.message = e;
                    self.listener = None;
                }
            }
        }
        if let Some(m) = &mut self.motion {
            m.progress = (self.motion_start.elapsed().as_secs_f32() / 0.28).min(1.0);
            self.revision += 1;
            if m.progress >= 1.0 {
                self.motion = None;
            }
        }
        let done: Vec<_> = self
            .finishing
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.try_recv().ok().map(|v| (i, v)))
            .collect();
        for (i, result) in done.into_iter().rev() {
            self.finishing.remove(i);
            self.message = result.err().unwrap_or("Recording saved".into());
        }
        if let Some(result) = self.recording.as_ref().and_then(|r| r.done.try_recv().ok()) {
            self.recording = None;
            self.message = result.err().unwrap_or("Recording saved".into());
        }
        if self.search.is_none()
            && !self.paused
            && self.peer.is_none()
            && self.game.data.cursor == self.game.data.moves.len()
            && self.game.result() == "*"
            && self.game.data.computer[usize::from(self.game.board.pos.turn() == Color::Black)]
            && self.motion.is_none()
        {
            self.analyze(false);
        }
        if self.last_persist.elapsed() > Duration::from_secs(2) {
            self.persist();
            self.last_persist = Instant::now();
        }
    }
    fn voice(&mut self, text: &str) -> Result<(), String> {
        match speech::parse(text, &self.game)? {
            VoiceAction::Move(m) => {
                if !self.local_turn() {
                    return Err("Waiting for the other player".into());
                }
                self.play(m, false)?;
            }
            VoiceAction::Hint => self.analyze(true),
            VoiceAction::Undo => self.undo()?,
            VoiceAction::LastMove => {
                self.show_last = true;
                self.revision += 1;
            }
        }
        Ok(())
    }
    fn last_move(&self) -> Option<Move> {
        if self.show_last && self.game.data.cursor > 0 {
            self.game.states[self.game.data.cursor - 1]
                .parse_move(&self.game.data.moves[self.game.data.cursor - 1])
                .ok()
        } else {
            None
        }
    }
    fn render(&mut self, size: [u32; 2]) -> Result<Vec<u8>, String> {
        self.renderer.resize_offscreen(size);
        self.renderer.render(
            &self.game,
            &self.prefs.view,
            self.selected,
            self.hint,
            self.last_move(),
            self.motion.as_ref(),
        );
        self.renderer.pixels()
    }
    fn save(&mut self, path: PathBuf) -> Result<(), String> {
        document::write(&path, &self.game)?;
        self.path = Some(path.clone());
        self.dirty = false;
        self.recent(path);
        self.message = "Game saved".into();
        Ok(())
    }
    fn clamp_preferences(&mut self) {
        let p = &mut self.prefs;
        p.seconds = p.seconds.clamp(0.05, 30.0);
        p.depth = p.depth.clamp(1, 16);
        p.view.board_style = p.view.board_style.min(3);
        p.view.piece_style = p.view.piece_style.min(3);
        p.view.distance = p.view.distance.clamp(9.0, 22.0);
        p.view.elevation = p.view.elevation.clamp(20.0, 89.0);
        p.view.ambient = p.view.ambient.clamp(0.0, 1.0);
        p.view.reflectivity = p.view.reflectivity.clamp(0.0, 1.0);
        p.view.label_intensity = p.view.label_intensity.clamp(0.0, 1.0);
        for m in &mut p.view.materials {
            m.diffuse = m.diffuse.clamp(0.0, 2.0);
            m.specular = m.specular.clamp(0.0, 2.0);
            m.shininess = m.shininess.clamp(1.0, 200.0);
            m.alpha = m.alpha.clamp(0.0, 1.0);
        }
    }
    fn close(&mut self, index: usize) -> Result<(), String> {
        if self.peer.is_some() {
            return Err("Disconnect before closing a network game".into());
        }
        if index >= self.tabs.len() {
            return Err("Unknown document".into());
        }
        self.tabs[self.active] = self.snapshot();
        if self.tabs[index].dirty {
            self.close_index = Some(index);
            return Ok(());
        }
        self.close_now(index);
        Ok(())
    }
    fn close_now(&mut self, index: usize) {
        self.close_index = None;
        self.tabs[self.active] = self.snapshot();
        if self.tabs.len() == 1 {
            self.game = Game::new(Rules::Standard);
            self.path = None;
            self.dirty = false;
            self.tabs[0] = self.snapshot();
            self.reset();
        } else {
            self.tabs.remove(index);
            self.active = if self.active > index {
                self.active - 1
            } else {
                self.active.min(self.tabs.len() - 1)
            };
            let s = self.tabs[self.active].clone();
            self.game = s.game;
            self.path = s.path;
            self.dirty = s.dirty;
            self.reset();
        }
        if self.quitting {
            self.prepare_quit();
        }
    }
    fn prepare_quit(&mut self) {
        self.tabs[self.active] = self.snapshot();
        self.quitting = true;
        self.close_index = self.tabs.iter().position(|s| s.dirty);
        self.close_allowed = self.close_index.is_none();
    }
    fn action(&mut self, name: &str, data: Value) -> Result<(), String> {
        let index = || {
            data.get("index")
                .and_then(Value::as_u64)
                .map(|v| v as usize)
                .ok_or("Missing index".to_string())
        };
        let text = |key: &str| {
            data.get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(format!("Missing {key}"))
        };
        if self.peer.is_some()
            && matches!(
                name,
                "switch" | "close" | "duplicate" | "variation" | "computer"
            )
        {
            return Err("Disconnect before changing documents or players".into());
        }
        match name {
            "switch" => self.switch(index()?)?,
            "close" => self.close(index()?)?,
            "close_response" => {
                let choice = text("choice")?;
                if choice == "cancel" {
                    self.close_index = None;
                    self.quitting = false;
                } else if let Some(index) = self.close_index {
                    self.switch(index)?;
                    if choice == "save" {
                        let path = data
                            .get("path")
                            .and_then(Value::as_str)
                            .map(PathBuf::from)
                            .or_else(|| self.path.clone())
                            .ok_or("Choose a save path")?;
                        self.save(path)?;
                    } else if choice == "discard" {
                        self.dirty = false;
                    } else {
                        return Err("Unknown close choice".into());
                    }
                    self.tabs[index] = self.snapshot();
                    self.close_now(index);
                }
            }
            "duplicate" => self.open_game(self.game.clone(), None, true),
            "square" => self.square(text("square")?.parse().map_err(|_| "Invalid square")?)?,
            "click" => {
                let x = data["x"].as_f64().ok_or("Missing x")? as f32;
                let y = data["y"].as_f64().ok_or("Missing y")? as f32;
                let rect = eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(self.renderer.size[0] as f32, self.renderer.size[1] as f32),
                );
                if let Some(sq) = self
                    .prefs
                    .view
                    .pick(eframe::egui::pos2(x, y), rect, &self.game)
                {
                    self.square(sq)?;
                }
            }
            "clear_selection" => {
                self.selected = None;
                self.drop_role = None;
                self.promotion.clear();
                self.revision += 1;
            }
            "drop" => {
                self.drop_role = Some(role(&text("role")?)?);
                self.selected = None;
                self.revision += 1;
            }
            "promote" => {
                let role = role(&text("role")?)?;
                let m = self
                    .promotion
                    .iter()
                    .copied()
                    .find(|m| m.promotion() == Some(role))
                    .ok_or("Invalid promotion choice")?;
                self.play(m, false)?;
            }
            "voice" => self.voice(&text("text")?)?,
            "flip" => {
                self.prefs.view.yaw += 180.0;
                self.revision += 1;
            }
            "orbit" => {
                self.prefs.view.yaw += data["dx"].as_f64().unwrap_or(0.0) as f32 * 0.45;
                self.prefs.view.elevation = (self.prefs.view.elevation
                    + data["dy"].as_f64().unwrap_or(0.0) as f32 * 0.25)
                    .clamp(20.0, 89.0);
                self.revision += 1;
            }
            "zoom" => {
                self.prefs.view.distance = (self.prefs.view.distance
                    - data["delta"].as_f64().unwrap_or(0.0) as f32 * 0.008)
                    .clamp(9.0, 22.0);
                self.revision += 1;
            }
            "preferences" => {
                let mut value = serde_json::to_value(&self.prefs).map_err(|e| e.to_string())?;
                merge(&mut value, &data);
                self.prefs = serde_json::from_value(value).map_err(|e| e.to_string())?;
                self.clamp_preferences();
                self.search = None;
                self.revision += 1;
            }
            "computer" => {
                self.game.data.computer =
                    serde_json::from_value(data["computer"].clone()).map_err(|e| e.to_string())?;
                self.search = None;
                self.dirty = true;
            }
            "metadata" => {
                let headers =
                    serde_json::from_value::<std::collections::BTreeMap<String, String>>(data)
                        .map_err(|e| e.to_string())?;
                self.game.data.headers.extend(headers);
                self.dirty = true;
            }
            "comment" => {
                let t = text("text")?;
                if t.is_empty() {
                    self.game.data.comments.remove(&self.game.data.cursor);
                } else {
                    self.game.data.comments.insert(self.game.data.cursor, t);
                }
                self.dirty = true;
            }
            "variation" => {
                let i = index()?;
                let mut saved = self.game.data.clone();
                let line = saved.variations.get(i).ok_or("Unknown variation")?.clone();
                saved.variations[i] = saved.moves.clone();
                saved.moves = line;
                saved.cursor = saved.moves.len();
                saved.result = "*".into();
                saved.comments.clear();
                self.game = Game::load(saved)?;
                self.dirty = true;
                self.reset();
            }
            "copy_pgn" => self
                .ui_events
                .push(json!({"name":"clipboard","data":{"text":document::to_pgn(&self.game)}})),
            "show_last" => {
                self.show_last = data["enabled"].as_bool().unwrap_or(true);
                self.revision += 1;
            }
            "clear_message" => self.message.clear(),
            "chat" => {
                let text = text("text")?.chars().take(1000).collect::<String>();
                self.peer
                    .as_ref()
                    .filter(|p| p.connected)
                    .ok_or("No connected opponent")?
                    .send(Message::Chat(text.clone()))?;
                self.chat_log.push(format!("You: {text}"));
            }
            "listen" => {
                if data["enabled"].as_bool().unwrap_or(false) {
                    self.listener = Some(Listener::start(&PathBuf::from(&self.prefs.model))?);
                } else {
                    self.listener = None;
                }
            }
            "record" => {
                if self.recording.is_some() {
                    return Err("Recording is already running".into());
                }
                let size =
                    serde_json::from_value(data["size"].clone()).map_err(|e| e.to_string())?;
                self.recording = Some(Recording::start(&PathBuf::from(text("path")?), size)?);
            }
            "stop_record" => self.stop_recording(),
            "gui" | "gui_screenshot" | "ui_resize" => {
                self.ui_events.push(json!({"name":name,"data":data}))
            }
            _ => return Err(format!("Unknown action: {name}")),
        }
        Ok(())
    }
    fn execute(&mut self, command: Command) -> Value {
        let result = self.command(command);
        match result {
            Ok(()) => json!({"ok":true,"data":self.state(false)}),
            Err(error) => {
                self.message = error.clone();
                json!({"ok":false,"error":error})
            }
        }
    }
    fn command(&mut self, command: Command) -> Result<(), String> {
        if self.peer.is_some()
            && matches!(
                command,
                Command::New { .. }
                    | Command::Open { .. }
                    | Command::SetFen { .. }
                    | Command::Seek { .. }
                    | Command::Pause { .. }
            )
        {
            return Err("Disconnect before changing the document".into());
        }
        match command {
            Command::Status => (),
            Command::Move { text } => {
                if !self.local_turn() {
                    return Err("Waiting for the computer, opponent or a pending offer".into());
                }
                self.play(self.game.board.parse_move(&text)?, false)?;
            }
            Command::New { variant, computer } => {
                let mut g = Game::new(variant);
                g.data.computer = computer;
                self.open_game(g, None, false);
                self.paused = false;
            }
            Command::Undo => self.undo()?,
            Command::Seek { ply } => self.seek(ply)?,
            Command::Open { path } => {
                let game = document::read(&path)?;
                self.open_game(game, Some(path.clone()), false);
                self.recent(path);
            }
            Command::Save { path } => self.save(path)?,
            Command::SetFen { fen } => {
                let mut g = Game::new(self.game.data.rules);
                g.set_fen(&fen)?;
                g.data.computer = [false, false];
                self.open_game(g, None, true);
            }
            Command::Hint => self.analyze(true),
            Command::Pause { paused } => {
                self.paused = paused;
                if paused {
                    self.search = None;
                }
            }
            Command::SetView { view } => {
                self.prefs.view = view;
                self.clamp_preferences();
                self.revision += 1;
            }
            Command::Screenshot { path } => {
                self.renderer.render(
                    &self.game,
                    &self.prefs.view,
                    self.selected,
                    self.hint,
                    self.last_move(),
                    None,
                );
                self.renderer.save_png(&path)?;
            }
            Command::Host { address } => {
                self.reset();
                self.address = address.clone();
                self.peer = Some(Peer::start(address, true));
            }
            Command::Join { address } => {
                self.reset();
                self.address = address.clone();
                self.peer = Some(Peer::start(address, false));
            }
            Command::Disconnect => self.disconnect(),
            Command::Ask { request } => self.ask(&request)?,
            Command::Respond { accepted } => self.respond(accepted)?,
            Command::Resign => self.resign(),
            Command::Action { name, data } => self.action(&name, data)?,
            Command::Quit => self.prepare_quit(),
        }
        Ok(())
    }
    fn state(&mut self, events: bool) -> Value {
        let side = self.game.board.pos.turn();
        let selected = self.selected.map(|s| s.to_string());
        let targets: Vec<_> = self
            .game
            .board
            .moves()
            .into_iter()
            .filter(|m| {
                if let Some(role) = self.drop_role {
                    m.from().is_none() && m.role() == role
                } else {
                    self.selected.is_some() && m.from() == self.selected
                }
            })
            .map(|m| render::destination(m).to_string())
            .collect();
        let squares:Vec<_>=(0..64).map(|i|{let sq=Square::new(i);let piece=self.game.at(sq);json!({"square":sq.to_string(),"file":i%8,"rank":i/8,"white":piece.is_some_and(|p|p.color==Color::White),"role":piece.map(|p|format!("{:?}",p.role)),"glyph":piece.map(|p|glyph(p.role,p.color)).unwrap_or(""),"label":piece.map(|p|format!("{sq}, {:?} {:?}",p.color,p.role)).unwrap_or_else(||format!("{sq}, empty"))})}).collect();
        let pockets: Vec<_> = [Color::White, Color::Black]
            .into_iter()
            .map(|color| {
                let pieces: Vec<_> = [
                    Role::Pawn,
                    Role::Knight,
                    Role::Bishop,
                    Role::Rook,
                    Role::Queen,
                ]
                .into_iter()
                .filter_map(|r| {
                    let count = self.game.board.pos.pockets().map_or(0, |p| p[color][r]);
                    (count > 0).then(
                        || json!({"role":format!("{r:?}"),"glyph":glyph(r,color),"count":count}),
                    )
                })
                .collect();
                json!({"white":color==Color::White,"pieces":pieces})
            })
            .collect();
        let tabs:Vec<_>=self.tabs.iter().enumerate().map(|(i,t)|{let(path,dirty)=if i==self.active{(self.path.as_ref(),self.dirty)}else{(t.path.as_ref(),t.dirty)};json!({"title":path.and_then(|p|p.file_name()).map(|s|s.to_string_lossy().into_owned()).unwrap_or_else(||format!("Game {}",i+1)),"dirty":dirty})}).collect();
        let offset = usize::from(self.game.states[0].pos.turn() == Color::Black);
        let base = u32::from(self.game.states[0].pos.fullmoves());
        let rows:Vec<_>=(0..(self.game.sans.len()+offset).div_ceil(2)).map(|row|{let cell=|half: usize|{let i=(row*2+half).checked_sub(offset);i.filter(|&i|i<self.game.sans.len()).map(|i|json!({"san":self.game.sans[i],"ply":i+1,"comment":self.game.data.comments.get(&(i+1))})).unwrap_or(Value::Null)};json!({"number":base+row as u32,"white":cell(0),"black":cell(1)})}).collect();
        let coordinates = if self.prefs.view.coordinates {
            let rect = eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(self.renderer.size[0] as f32, self.renderer.size[1] as f32),
            );
            let mut labels = vec![];
            for i in 0..8 {
                for (point, text) in [
                    (
                        glam::Vec3::new(i as f32 - 3.5, 0.03, 4.24),
                        ((b'a' + i) as char).to_string(),
                    ),
                    (
                        glam::Vec3::new(i as f32 - 3.5, 0.03, -4.24),
                        ((b'a' + i) as char).to_string(),
                    ),
                    (
                        glam::Vec3::new(4.25, 0.03, 3.5 - i as f32),
                        (i + 1).to_string(),
                    ),
                    (
                        glam::Vec3::new(-4.25, 0.03, 3.5 - i as f32),
                        (i + 1).to_string(),
                    ),
                ] {
                    if let Some(p) = self.prefs.view.project(point, rect) {
                        labels
                            .push(json!({"x":p.x/rect.width(),"y":p.y/rect.height(),"text":text}));
                    }
                }
            }
            labels
        } else {
            vec![]
        };
        let ui_events = if events {
            std::mem::take(&mut self.ui_events)
        } else {
            vec![]
        };
        json!({"fen":self.game.board.fen(),"status":self.game.status(),"variant":self.game.data.rules,"variant_name":self.game.data.rules.name(),"ply":self.game.data.cursor,"total":self.game.data.moves.len(),"result":self.game.result(),"white_turn":side==Color::White,"thinking":self.search.is_some(),"paused":self.paused,"local_turn":self.local_turn(),"network_active":self.peer.is_some(),"connected":self.peer.as_ref().is_some_and(|p|p.connected),"host":self.peer.as_ref().is_some_and(|p|p.host),"network_address":self.address,"pending_request":self.pending_request,"remote_request":self.remote_request,"message":self.message,"preferences":self.prefs,"computer":self.game.data.computer,"headers":self.game.data.headers,"path":self.path,"dirty":self.dirty,"tabs":tabs,"active":self.active,"squares":squares,"selected":selected,"targets":targets,"history":rows,"comment":self.game.data.comments.get(&self.game.data.cursor).cloned().unwrap_or_default(),"variations":self.game.data.variations.iter().map(Vec::len).collect::<Vec<_>>(),"pockets":pockets,"promotion":self.promotion.iter().filter_map(|m|m.promotion()).map(|r|format!("{r:?}")).collect::<Vec<_>>(),"board_revision":self.revision,"coordinates":coordinates,"show_last":self.show_last,"hint":self.hint.map(|m|shakmaty::san::SanPlus::from_move(self.game.board.pos.clone(),m).to_string()),"analysis":self.analysis.as_ref().map(|a|json!({"depth":a.depth,"score":a.score as f64/100.0,"nodes":a.nodes})),"chat":self.chat_log,"listening":self.listener.is_some(),"recording":self.recording.is_some(),"finishing_recording":!self.finishing.is_empty(),"close_index":self.close_index,"close_allowed":self.close_allowed,"quitting":self.quitting,"ui_events":ui_events,"adapter":self.renderer.adapter_name})
    }
    fn stop_recording(&mut self) {
        if let Some(r) = self.recording.take() {
            self.finishing.push(r.finish());
            self.message = "Finishing recording…".into();
        }
    }
    fn persist(&mut self) {
        self.tabs[self.active] = self.snapshot();
        let recovery = Recovery {
            games: self.tabs.iter().map(|s| s.game.data.clone()).collect(),
            paths: self.tabs.iter().map(|s| s.path.clone()).collect(),
            dirty: self.tabs.iter().map(|s| s.dirty).collect(),
            active: self.active,
        };
        for (dir, file, bytes) in [
            (
                config_dir(),
                "preferences.json",
                serde_json::to_vec_pretty(&self.prefs),
            ),
            (state_dir(), "recovery.json", serde_json::to_vec(&recovery)),
        ] {
            if let Ok(bytes) = bytes
                && std::fs::create_dir_all(&dir).is_ok()
            {
                let tmp = dir.join(format!("{file}.tmp"));
                if std::fs::write(&tmp, bytes).is_ok() {
                    let _ = std::fs::rename(tmp, dir.join(file));
                }
            }
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.search = None;
        self.listener = None;
        self.disconnect();
        self.stop_recording();
        self.persist();
        for done in &self.finishing {
            let _ = done.recv_timeout(Duration::from_secs(10));
        }
    }
}
fn merge(target: &mut Value, patch: &Value) {
    if let (Some(a), Some(b)) = (target.as_object_mut(), patch.as_object()) {
        for (k, v) in b {
            if let Some(current) = a.get_mut(k)
                && v.is_object()
            {
                merge(current, v);
            } else {
                a.insert(k.clone(), v.clone());
            }
        }
    }
}
fn role(text: &str) -> Result<Role, String> {
    match text.to_lowercase().as_str() {
        "pawn" => Ok(Role::Pawn),
        "knight" => Ok(Role::Knight),
        "bishop" => Ok(Role::Bishop),
        "rook" => Ok(Role::Rook),
        "queen" => Ok(Role::Queen),
        "king" => Ok(Role::King),
        _ => Err("Unknown piece".into()),
    }
}
fn glyph(role: Role, color: Color) -> &'static str {
    match (role, color) {
        (Role::King, Color::White) => "♔",
        (Role::Queen, Color::White) => "♕",
        (Role::Rook, Color::White) => "♖",
        (Role::Bishop, Color::White) => "♗",
        (Role::Knight, Color::White) => "♘",
        (Role::Pawn, Color::White) => "♙",
        (Role::King, Color::Black) => "♚",
        (Role::Queen, Color::Black) => "♛",
        (Role::Rook, Color::Black) => "♜",
        (Role::Bishop, Color::Black) => "♝",
        (Role::Knight, Color::Black) => "♞",
        (Role::Pawn, Color::Black) => "♟",
    }
}

#[repr(C)]
struct Frame {
    data: *mut u8,
    len: usize,
    width: u32,
    height: u32,
}
unsafe extern "C" {
    fn chess_qt_run(
        context: *mut c_void,
        dispatch: extern "C" fn(*mut c_void, *const c_char) -> *mut c_char,
        free_text: extern "C" fn(*mut c_char),
        render: extern "C" fn(*mut c_void, u32, u32) -> Frame,
        free_frame: extern "C" fn(Frame),
        record: extern "C" fn(*mut c_void, *const u8, u32, u32),
    ) -> i32;
}
extern "C" fn dispatch(context: *mut c_void, request: *const c_char) -> *mut c_char {
    // All callbacks use an owned controller protected against concurrent image-provider requests.
    let result = std::panic::catch_unwind(|| {
        let state = unsafe { &*(context as *const Mutex<Controller>) };
        let mut app = state.lock().map_err(|e| e.to_string())?;
        let text = unsafe { CStr::from_ptr(request) }
            .to_str()
            .map_err(|e| e.to_string())?;
        let request: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let response = if request["command"] == "poll" {
            app.poll();
            json!({"ok":true,"data":app.state(true)})
        } else {
            let command = serde_json::from_value(request).map_err(|e| e.to_string())?;
            app.execute(command)
        };
        Ok::<_, String>(response)
    });
    let response = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({"ok":false,"error":e}),
        Err(_) => json!({"ok":false,"error":"Rust controller callback failed"}),
    };
    CString::new(response.to_string()).unwrap().into_raw()
}
extern "C" fn free_text(text: *mut c_char) {
    if !text.is_null() {
        unsafe {
            drop(CString::from_raw(text));
        }
    }
}
extern "C" fn frame(context: *mut c_void, width: u32, height: u32) -> Frame {
    let result = std::panic::catch_unwind(|| {
        let state = unsafe { &*(context as *const Mutex<Controller>) };
        let mut app = state.lock().map_err(|e| e.to_string())?;
        let bytes = app.render([width, height])?;
        Ok::<_, String>((bytes, app.renderer.size))
    });
    if let Ok(Ok((bytes, size))) = result {
        let mut data = bytes.into_boxed_slice();
        let frame = Frame {
            data: data.as_mut_ptr(),
            len: data.len(),
            width: size[0],
            height: size[1],
        };
        std::mem::forget(data);
        frame
    } else {
        Frame {
            data: std::ptr::null_mut(),
            len: 0,
            width: 0,
            height: 0,
        }
    }
}
extern "C" fn free_frame(frame: Frame) {
    if !frame.data.is_null() {
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                frame.data, frame.len,
            )));
        }
    }
}
extern "C" fn record(context: *mut c_void, data: *const u8, width: u32, height: u32) {
    let _ = std::panic::catch_unwind(|| {
        let state = unsafe { &*(context as *const Mutex<Controller>) };
        if let Ok(mut app) = state.lock()
            && let Some(recording) = &mut app.recording
        {
            let bytes =
                unsafe { std::slice::from_raw_parts(data, width as usize * height as usize * 4) };
            recording.frame(&eframe::egui::ColorImage::from_rgba_unmultiplied(
                [width as usize, height as usize],
                bytes,
            ));
        }
    });
}
pub fn run(path: Option<PathBuf>, fresh: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = Box::new(Mutex::new(Controller::new(path, fresh)?));
    let result = unsafe {
        chess_qt_run(
            (&mut *app as *mut Mutex<Controller>).cast(),
            dispatch,
            free_text,
            frame,
            free_frame,
            record,
        )
    };
    if result != 0 {
        return Err(format!("Kirigami application exited with status {result}").into());
    }
    Ok(())
}
