use crate::{
    document,
    engine::{self, Analysis},
    game::{Game, Rules, SavedGame},
    network::{self, Message, Peer},
    recording::Recording,
    render::{self, BoardRenderer, Motion, View},
    speech::{self, Listener, VoiceAction},
};
use eframe::egui::{self, Color32, RichText};
use shakmaty::{CastlingMode, Color, Move, Position, Role, Square, uci::UciMove};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Preferences {
    view: View,
    seconds: f32,
    depth: u8,
    speak_computer: bool,
    speak_human: bool,
    voices: [String; 2],
    model: String,
    sjeng_path: String,
    recent: Vec<PathBuf>,
    show_log: bool,
    engine_log: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            view: View::default(),
            seconds: 1.0,
            depth: 8,
            speak_computer: true,
            speak_human: false,
            voices: ["en".into(), "en+f3".into()],
            model: String::new(),
            sjeng_path: String::new(),
            recent: vec![],
            show_log: true,
            engine_log: false,
        }
    }
}
#[derive(Clone)]
struct Session {
    game: Game,
    path: Option<PathBuf>,
    dirty: bool,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Recovery {
    games: Vec<SavedGame>,
    #[serde(default)]
    paths: Vec<Option<PathBuf>>,
    #[serde(default)]
    dirty: Vec<bool>,
    active: usize,
}
enum FileAction {
    Open(PathBuf),
    Save(PathBuf),
    Cancelled,
    Screenshot(PathBuf),
    Record(PathBuf),
}
struct Job {
    receiver: Receiver<Result<Analysis, String>>,
    cancel: Arc<AtomicBool>,
    key: String,
    cursor: usize,
    hint: bool,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
pub struct ChessApp {
    game: Game,
    path: Option<PathBuf>,
    dirty: bool,
    tabs: Vec<Session>,
    active: usize,
    prefs: Preferences,
    renderer: BoardRenderer,
    state: eframe::egui_wgpu::RenderState,
    selected: Option<Square>,
    keyboard_square: Square,
    drop_role: Option<Role>,
    promotion: Vec<Move>,
    hint: Option<Move>,
    show_last: bool,
    job: Option<Job>,
    analysis: Option<Analysis>,
    paused: bool,
    message: String,
    motion: Option<Motion>,
    motion_start: Instant,
    new_dialog: bool,
    new_rules: Rules,
    new_computer: [bool; 2],
    settings: bool,
    tuner: bool,
    control: Option<crate::automation::Control>,
    info: bool,
    help: bool,
    position_dialog: bool,
    fen_text: String,
    network_dialog: bool,
    address: String,
    peer: Option<Peer>,
    remote_request: Option<String>,
    pending_request: Option<String>,
    chat: String,
    chat_log: Vec<String>,
    input: String,
    listener: Option<Listener>,
    recording: Option<Recording>,
    finishing: Vec<Receiver<Result<(), String>>>,
    last_capture: Instant,
    file_rx: Receiver<FileAction>,
    file_tx: Sender<FileAction>,
    screenshot_path: Option<PathBuf>,
    last_autosave: Instant,
    confirm_close: Option<usize>,
    awaiting_close: Option<usize>,
    quit_requested: bool,
    smoke: bool,
    smoke_step: usize,
    smoke_started: Instant,
}
fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("chess-linux")
}
fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("chess-linux")
}
impl ChessApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut path: Option<PathBuf>,
        fresh: bool,
        smoke: bool,
    ) -> Self {
        let mut prefs: Preferences = std::fs::read(config_dir().join("preferences.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        prefs.view.board_style = prefs.view.board_style.min(3);
        prefs.view.piece_style = prefs.view.piece_style.min(3);
        prefs.seconds = prefs.seconds.clamp(0.05, 30.0);
        prefs.depth = prefs.depth.clamp(1, 16);
        if smoke {
            prefs = Preferences::default();
            prefs.speak_computer = false;
        }
        let mut game = Game::new(Rules::Standard);
        let mut message = String::new();
        let mut tabs = Vec::new();
        let mut active = 0;
        if !fresh
            && path.is_none()
            && let Ok(bytes) = std::fs::read(state_dir().join("recovery.json"))
            && let Ok(recovery) = serde_json::from_slice::<Recovery>(&bytes)
        {
            for (i, data) in recovery.games.into_iter().enumerate() {
                if let Ok(g) = Game::load(data) {
                    tabs.push(Session {
                        game: g,
                        path: recovery.paths.get(i).cloned().flatten(),
                        dirty: recovery.dirty.get(i).copied().unwrap_or(true),
                    });
                }
            }
            if !tabs.is_empty() {
                active = recovery.active.min(tabs.len() - 1);
                game = tabs[active].game.clone();
                path = tabs[active].path.clone();
                message = "Restored your previous session".into();
            }
        }
        if (fresh || tabs.is_empty())
            && let Some(path_value) = &path
        {
            match document::read(path_value) {
                Ok(g) => game = g,
                Err(e) => {
                    message = e;
                    path = None;
                }
            }
        }
        if tabs.is_empty() {
            tabs.push(Session {
                game: game.clone(),
                path: path.clone(),
                dirty: false,
            });
        }
        let state = cc.wgpu_render_state.clone().expect("Vulkan render state");
        let adapter = state.adapter.get_info();
        assert_eq!(adapter.backend, wgpu::Backend::Vulkan);
        let renderer = BoardRenderer::new(state.device.clone(), state.queue.clone(), adapter.name);
        let (tx, rx) = mpsc::channel();
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(24, 28, 33);
        style.visuals.window_fill = Color32::from_rgb(30, 35, 41);
        style.visuals.selection.bg_fill = Color32::from_rgb(77, 110, 91);
        style.spacing.item_spacing = egui::vec2(9.0, 9.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        cc.egui_ctx.set_style(style);
        Self {
            game,
            path,
            dirty: tabs[active].dirty,
            tabs,
            active,
            prefs,
            renderer,
            state,
            selected: None,
            keyboard_square: Square::E2,
            drop_role: None,
            promotion: vec![],
            hint: None,
            show_last: true,
            job: None,
            analysis: None,
            paused: false,
            message,
            motion: None,
            motion_start: Instant::now(),
            new_dialog: false,
            new_rules: Rules::Standard,
            new_computer: [false, true],
            settings: false,
            tuner: false,
            control: if smoke {
                None
            } else {
                crate::automation::Control::start().ok()
            },
            info: false,
            help: false,
            position_dialog: false,
            fen_text: String::new(),
            network_dialog: false,
            address: "0.0.0.0:7878".into(),
            peer: None,
            remote_request: None,
            pending_request: None,
            chat: String::new(),
            chat_log: vec![],
            input: String::new(),
            listener: None,
            recording: None,
            finishing: vec![],
            last_capture: Instant::now(),
            file_rx: rx,
            file_tx: tx,
            screenshot_path: None,
            last_autosave: Instant::now(),
            confirm_close: None,
            awaiting_close: None,
            quit_requested: false,
            smoke,
            smoke_step: 0,
            smoke_started: Instant::now(),
        }
    }
    fn snapshot(&self) -> Session {
        Session {
            game: self.game.clone(),
            path: self.path.clone(),
            dirty: self.dirty,
        }
    }
    fn switch(&mut self, index: usize) {
        if index == self.active {
            return;
        }
        self.tabs[self.active] = self.snapshot();
        let session = self.tabs[index].clone();
        self.active = index;
        self.game = session.game;
        self.path = session.path;
        self.dirty = session.dirty;
        self.reset_transient();
    }
    fn reset_transient(&mut self) {
        self.job = None;
        self.selected = None;
        self.drop_role = None;
        self.promotion.clear();
        self.hint = None;
        self.analysis = None;
        self.motion = None;
        self.paused = true;
    }
    fn new_game(&mut self) {
        self.tabs[self.active] = self.snapshot();
        let mut game = Game::new(self.new_rules);
        game.data.computer = self.new_computer;
        self.game = game;
        self.path = None;
        self.dirty = false;
        self.tabs.push(self.snapshot());
        self.active = self.tabs.len() - 1;
        self.reset_transient();
        self.paused = false;
        self.new_dialog = false;
        self.message.clear();
    }
    fn close_tab(&mut self, index: usize) {
        if self.tabs.len() == 1 {
            self.game = Game::new(Rules::Standard);
            self.path = None;
            self.dirty = false;
            self.tabs[0] = self.snapshot();
            self.reset_transient();
        } else {
            self.tabs[self.active] = self.snapshot();
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
            self.reset_transient();
        }
    }
    fn save(&mut self, path: PathBuf) {
        match document::write(&path, &self.game) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.dirty = false;
                self.recent(path);
                self.message = "Game saved".into();
            }
            Err(e) => self.message = e,
        }
    }
    fn recent(&mut self, path: PathBuf) {
        self.prefs.recent.retain(|p| *p != path);
        self.prefs.recent.insert(0, path);
        self.prefs.recent.truncate(12);
    }
    fn open(&mut self, path: PathBuf) {
        match document::read(&path) {
            Ok(g) => {
                self.tabs[self.active] = self.snapshot();
                self.game = g;
                self.path = Some(path.clone());
                self.dirty = false;
                self.tabs.push(self.snapshot());
                self.active = self.tabs.len() - 1;
                self.reset_transient();
                self.recent(path);
                self.message = "Game opened; press Resume to continue computer play".into();
            }
            Err(e) => self.message = e,
        }
    }
    fn file_dialog(&self, kind: &str) {
        let tx = self.file_tx.clone();
        let kind = kind.to_string();
        std::thread::spawn(move || {
            let action = match kind.as_str() {
                "open" => rfd::FileDialog::new()
                    .add_filter("Chess games", &["chess-linux", "chess", "pgn", "json"])
                    .pick_file()
                    .map(FileAction::Open),
                "save" => rfd::FileDialog::new()
                    .add_filter("Chess Linux document", &["chess-linux"])
                    .add_filter("PGN", &["pgn"])
                    .add_filter("Apple Chess", &["chess"])
                    .set_file_name("Game.chess-linux")
                    .save_file()
                    .map(FileAction::Save),
                "screenshot" => rfd::FileDialog::new()
                    .add_filter("PNG image", &["png"])
                    .set_file_name("Chess.png")
                    .save_file()
                    .map(FileAction::Screenshot),
                _ => rfd::FileDialog::new()
                    .add_filter("MP4 video", &["mp4"])
                    .set_file_name("Chess.mp4")
                    .save_file()
                    .map(FileAction::Record),
            };
            let _ = tx.send(action.unwrap_or(FileAction::Cancelled));
        });
    }
    fn local_turn(&self) -> bool {
        if self.remote_request.is_some() || self.pending_request.is_some() {
            return false;
        }
        if let Some(peer) = &self.peer {
            peer.connected && (self.game.board.pos.turn() == Color::White) == peer.host
        } else {
            !self.game.data.computer[usize::from(self.game.board.pos.turn() == Color::Black)]
        }
    }
    fn play(&mut self, m: Move, remote: bool) {
        if !remote && self.peer.is_some() && !self.local_turn() {
            self.message = "Waiting for your opponent".into();
            return;
        }
        let before = self.game.board.key();
        let ply = self.game.data.cursor;
        let previous = self.game.clone();
        match self.game.push(m) {
            Ok(()) => {
                self.dirty = true;
                if self.game.result() != "*" {
                    notify_end(&self.game);
                }
                self.job = None;
                self.hint = None;
                self.selected = None;
                self.drop_role = None;
                self.analysis = None;
                self.motion_start = Instant::now();
                if self.prefs.view.animations {
                    self.motion = Some(Motion {
                        m,
                        previous,
                        progress: 0.0,
                    });
                }
                let computer = remote
                    || self.game.data.computer
                        [usize::from(self.game.board.pos.turn() == Color::White)];
                if ((computer && self.prefs.speak_computer)
                    || (!computer && self.prefs.speak_human))
                    && let Err(e) = speech::speak(
                        &self.game,
                        m,
                        &self.prefs.voices[usize::from(self.game.board.pos.turn() == Color::White)],
                    )
                {
                    self.message = e;
                }
                if !remote && let Some(peer) = &self.peer {
                    let _ = peer.send(Message::Move {
                        ply,
                        before,
                        text: UciMove::from_move(m, CastlingMode::Standard).to_string(),
                    });
                }
            }
            Err(e) => self.message = e,
        }
    }
    fn square(&mut self, sq: Square) {
        self.keyboard_square = sq;
        if !self.local_turn() || self.game.result() != "*" {
            return;
        }
        let moves = self.game.board.moves();
        let candidates: Vec<_> = moves
            .iter()
            .copied()
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
            self.play(candidates[0], false);
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
            self.message = "Select a piece and a legal destination".into();
        }
    }
    fn seek(&mut self, cursor: usize) {
        if self.peer.is_some() {
            return;
        }
        self.game.seek(cursor);
        self.reset_transient();
        self.dirty = true;
    }
    fn undo(&mut self) {
        if self.peer.is_some() {
            self.request("takeback");
            return;
        }
        let step = if self.game.data.computer.iter().filter(|&&c| c).count() == 1
            && self.game.data.cursor >= 2
        {
            2
        } else {
            1
        };
        self.seek(self.game.data.cursor.saturating_sub(step));
    }
    fn analyze(&mut self, hint: bool) {
        if self.game.result() != "*" {
            return;
        }
        self.job = None;
        let board = self.game.board.clone();
        let key = board.key();
        let cursor = self.game.data.cursor;
        let seconds = self.prefs.seconds;
        let depth = self.prefs.depth;
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let game = self.game.clone();
        let path = self.prefs.sjeng_path.clone();
        let (tx, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = if path.is_empty() {
                Ok(engine::analyze(board, seconds, depth, flag))
            } else {
                crate::legacy_engine::analyze(
                    game,
                    std::path::Path::new(&path),
                    seconds,
                    depth,
                    flag,
                )
            };
            let _ = tx.send(result);
        });
        self.job = Some(Job {
            receiver,
            cancel,
            key,
            cursor,
            hint,
        });
    }
    fn request(&mut self, request: &str) {
        if let Some(peer) = &self.peer {
            if peer.connected && self.pending_request.is_none() && self.remote_request.is_none() {
                let _ = peer.send(Message::Request(request.into()));
                self.pending_request = Some(request.into());
                self.message = format!("Requested {request}");
            }
        } else if request == "draw" {
            self.game.data.result = "1/2-1/2".into();
            notify_end(&self.game);
            self.job = None;
            self.dirty = true;
        }
    }
    fn respond(&mut self, accepted: bool) -> Result<(), String> {
        let request = self
            .remote_request
            .as_ref()
            .ok_or("No opponent request is pending")?;
        let peer = self.peer.as_ref().ok_or("No network session is active")?;
        peer.send(Message::Reply {
            request: request.clone(),
            accepted,
        })?;
        let request = self.remote_request.take().unwrap();
        if accepted {
            self.apply_request(&request);
        }
        Ok(())
    }
    fn disconnect(&mut self) {
        self.peer = None;
        self.pending_request = None;
        self.remote_request = None;
    }
    fn apply_request(&mut self, request: &str) {
        match request {
            "draw" => {
                self.game.data.result = "1/2-1/2".into();
                notify_end(&self.game);
                self.job = None;
                self.dirty = true;
            }
            "takeback" => {
                self.game.seek(self.game.data.cursor.saturating_sub(1));
                self.game.data.moves.truncate(self.game.data.cursor);
                self.game.sans.truncate(self.game.data.cursor);
                self.game.states.truncate(self.game.data.cursor + 1);
                self.game.data.result = "*".into();
                self.game
                    .data
                    .comments
                    .retain(|&i, _| i <= self.game.data.cursor);
                self.reset_transient();
                self.dirty = true;
            }
            _ => (),
        }
    }
    fn resign(&mut self) {
        if self.game.result() != "*" {
            return;
        }
        let local_white = self
            .peer
            .as_ref()
            .map(|p| p.host)
            .unwrap_or(self.game.board.pos.turn() == Color::White);
        let result = if local_white { "0-1" } else { "1-0" };
        self.game.data.result = result.into();
        notify_end(&self.game);
        self.dirty = true;
        self.job = None;
        if let Some(peer) = &self.peer {
            let _ = peer.send(Message::Result(result.into()));
        }
    }
    fn voice(&mut self, text: &str) {
        match speech::parse(text, &self.game) {
            Ok(VoiceAction::Move(m)) => self.play(m, false),
            Ok(VoiceAction::Hint) => self.analyze(true),
            Ok(VoiceAction::Undo) => self.undo(),
            Ok(VoiceAction::LastMove) => self.show_last = true,
            Err(e) => self.message = e,
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let requests: Vec<_> = self
            .control
            .as_ref()
            .map(|c| c.requests.try_iter().collect())
            .unwrap_or_default();
        for request in requests {
            let result = self.automation(request.command, ctx);
            let response = match result {
                Ok(value) => serde_json::json!({"ok":true,"data":value}),
                Err(error) => serde_json::json!({"ok":false,"error":error}),
            };
            let _ = request.reply.send(response);
        }

        while let Ok(action) = self.file_rx.try_recv() {
            match action {
                FileAction::Open(p) => self.open(p),
                FileAction::Save(p) => {
                    if let Some(i) = self.awaiting_close {
                        self.switch(i);
                    }
                    self.save(p);
                    if let Some(i) = self.awaiting_close.take() {
                        if !self.dirty {
                            self.close_tab(i);
                        } else {
                            self.confirm_close = Some(i);
                        }
                    }
                }
                FileAction::Cancelled => {
                    if self.awaiting_close.take().is_some() {
                        self.quit_requested = false;
                    }
                }
                FileAction::Screenshot(p) => {
                    self.screenshot_path = Some(p);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                        "still",
                    )));
                }
                FileAction::Record(p) => {
                    let size = ctx
                        .input(|i| {
                            i.viewport()
                                .inner_rect
                                .map(|r| [r.width() as u32, r.height() as u32])
                        })
                        .unwrap_or([1280, 800]);
                    match Recording::start(&p, size) {
                        Ok(r) => {
                            self.message = format!("Recording to {}", p.display());
                            self.recording = Some(r);
                        }
                        Err(e) => self.message = e,
                    }
                }
            }
        }
        let result = self.job.as_ref().and_then(|j| j.receiver.try_recv().ok());
        if let Some(result) = result {
            let job = self.job.take().unwrap();
            let result = match result {
                Ok(r) => r,
                Err(e) => {
                    self.message = e;
                    self.paused = true;
                    return;
                }
            };
            if job.key == self.game.board.key() && job.cursor == self.game.data.cursor {
                if self.prefs.engine_log {
                    eprintln!(
                        "Search depth {} · score {} · nodes {}",
                        result.depth, result.score, result.nodes
                    );
                }
                self.analysis = Some(result.clone());
                if job.hint {
                    self.hint = result.best;
                } else if let Some(m) = result.best {
                    self.play(m, false);
                }
            }
        }
        let voice_events: Vec<_> = self
            .listener
            .as_ref()
            .map(|l| l.events.try_iter().collect())
            .unwrap_or_default();
        for event in voice_events {
            match event {
                Ok(text) => {
                    self.message = format!("Heard: {text}");
                    self.voice(&text);
                }
                Err(e) => {
                    self.message = e;
                    self.listener = None;
                }
            }
        }
        let events: Vec<_> = self
            .peer
            .as_ref()
            .map(|p| p.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                network::Event::Listening(a) => {
                    self.message = format!("Waiting for a connection on {a}");
                    self.address = a;
                }
                network::Event::Connected => {
                    let peer = self.peer.as_mut().unwrap();
                    peer.connected = true;
                    self.game.data.computer = [false, false];
                    self.job = None;
                    if peer.host {
                        self.game.seek(self.game.data.moves.len());
                        let _ = peer.send(Message::Game(self.game.data.clone()));
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
        let done: Vec<_> = self
            .finishing
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.try_recv().ok().map(|v| (i, v)))
            .collect();
        for (i, result) in done.into_iter().rev() {
            self.finishing.remove(i);
            self.message = match result {
                Ok(()) => "Recording saved".into(),
                Err(e) => e,
            };
        }
        if let Some(m) = &mut self.motion {
            m.progress = (self.motion_start.elapsed().as_secs_f32() / 0.28).min(1.0);
            if m.progress >= 1.0 {
                self.motion = None;
            }
        }
        let screenshots = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| {
                    if let egui::Event::Screenshot {
                        image, user_data, ..
                    } = e
                    {
                        Some((image.clone(), user_data.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        for (image, data) in screenshots {
            let kind = data
                .data
                .as_ref()
                .and_then(|d| d.downcast_ref::<&str>())
                .copied()
                .unwrap_or("");
            if kind == "still" {
                if let Some(p) = self.screenshot_path.take() {
                    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    self.message = match image::save_buffer(
                        &p,
                        &bytes,
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                    ) {
                        Ok(()) => format!("Screenshot saved to {}", p.display()),
                        Err(e) => e.to_string(),
                    };
                }
            } else if kind == "record"
                && let Some(r) = &mut self.recording
            {
                r.frame(&image);
            }
        }
        if self.recording.is_some() && self.last_capture.elapsed() >= Duration::from_millis(33) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                "record",
            )));
            self.last_capture = Instant::now();
        }
        if let Some(result) = self.recording.as_ref().and_then(|r| r.done.try_recv().ok()) {
            self.recording = None;
            self.message = match result {
                Ok(()) => "Recording saved".into(),
                Err(e) => e,
            };
        }
        if self.job.is_none()
            && !self.paused
            && self.peer.is_none()
            && self.game.data.cursor == self.game.data.moves.len()
            && self.game.result() == "*"
            && self.game.data.computer[usize::from(self.game.board.pos.turn() == Color::Black)]
            && self.motion.is_none()
        {
            self.analyze(false);
        }
    }
    fn network_message(&mut self, m: Message) {
        let host = self.peer.as_ref().is_some_and(|p| p.host);
        match m {
            Message::Game(mut data) if !host => {
                data.computer = [false, false];
                match Game::load(data) {
                    Ok(g) => {
                        self.tabs[self.active] = self.snapshot();
                        self.game = g;
                        self.path = None;
                        self.dirty = true;
                        self.reset_transient();
                    }
                    Err(e) => self.message = e,
                }
            }
            Message::Move { ply, before, text } => {
                let remote_white = !host;
                if ply != self.game.data.cursor
                    || before != self.game.board.key()
                    || (self.game.board.pos.turn() == Color::White) != remote_white
                {
                    self.message = "Rejected an out-of-sync network move".into();
                    return;
                }
                match self.game.board.parse_move(&text) {
                    Ok(m) => self.play(m, true),
                    Err(e) => self.message = e,
                }
            }
            Message::Request(r) if matches!(r.as_str(), "draw" | "takeback") => {
                if self.pending_request.is_some() || self.remote_request.is_some() {
                    if let Some(peer) = &self.peer {
                        let _ = peer.send(Message::Reply {
                            request: r,
                            accepted: false,
                        });
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
                    self.apply_request(&request);
                }
                self.message = if accepted {
                    format!("{request} accepted")
                } else {
                    format!("{request} declined")
                };
            }
            Message::Result(r) => {
                let expected = if host { "1-0" } else { "0-1" };
                if r == expected {
                    self.game.data.result = r;
                    notify_end(&self.game);
                    self.job = None;
                    self.dirty = true;
                }
            }
            Message::Chat(s) => {
                self.chat_log.push(format!(
                    "Opponent: {}",
                    s.chars().take(1000).collect::<String>()
                ));
            }
            _ => (),
        }
    }
    fn stop_recording(&mut self) {
        if let Some(r) = self.recording.take() {
            self.finishing.push(r.finish());
            self.message = "Finishing recording…".into();
        }
    }
    fn persist(&mut self) {
        if self.smoke {
            return;
        }
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
    fn menus(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menus").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.label(
                    RichText::new("CHESS")
                        .strong()
                        .color(Color32::from_rgb(206, 211, 199)),
                );
                ui.separator();
                ui.menu_button("Game", |ui| {
                    ui.add_enabled_ui(self.peer.is_none(), |ui| {
                        if ui.button("New game…       Ctrl+N").clicked() {
                            self.new_dialog = true;
                            ui.close();
                        }
                        if ui.button("Open…               Ctrl+O").clicked() {
                            self.file_dialog("open");
                            ui.close();
                        }
                        ui.menu_button("Open recent", |ui| {
                            for path in self.prefs.recent.clone() {
                                if ui.button(path.display().to_string()).clicked() {
                                    self.open(path);
                                    ui.close();
                                }
                            }
                            if ui.button("Clear recent games").clicked() {
                                self.prefs.recent.clear();
                            }
                        });
                        if ui.button("Duplicate game").clicked() {
                            self.tabs[self.active] = self.snapshot();
                            self.path = None;
                            self.dirty = true;
                            self.tabs.push(self.snapshot());
                            self.active = self.tabs.len() - 1;
                            self.reset_transient();
                            ui.close();
                        }
                    });
                    if ui.button("Save                   Ctrl+S").clicked() {
                        if let Some(p) = self.path.clone() {
                            self.save(p);
                        } else {
                            self.file_dialog("save");
                        }
                        ui.close();
                    }
                    if ui.button("Save as…").clicked() {
                        self.file_dialog("save");
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Edit game information…").clicked() {
                        self.info = true;
                        ui.close();
                    }
                    if ui.button("Copy PGN").clicked() {
                        ctx.copy_text(document::to_pgn(&self.game));
                        ui.close();
                    }
                    if ui.button("Copy FEN").clicked() {
                        ctx.copy_text(self.game.board.fen());
                        ui.close();
                    }
                    if ui
                        .add_enabled(self.peer.is_none(), egui::Button::new("Set up position…"))
                        .clicked()
                    {
                        self.fen_text = self.game.board.fen();
                        self.position_dialog = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Quit                      Ctrl+Q").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close();
                    }
                });
                ui.menu_button("Moves", |ui| {
                    if ui.button("Take back move     Ctrl+Z").clicked() {
                        self.undo();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.peer.is_none(),
                            egui::Button::new("Redo move              Ctrl+Shift+Z"),
                        )
                        .clicked()
                    {
                        self.seek(self.game.data.cursor + 1);
                        ui.close();
                    }
                    if ui.button("Show hint                   ]").clicked() {
                        self.analyze(true);
                        ui.close();
                    }
                    ui.checkbox(&mut self.show_last, "Show last move");
                    ui.checkbox(&mut self.prefs.show_log, "Game log");
                    ui.separator();
                    if ui.button("Resign").clicked() {
                        self.resign();
                        ui.close();
                    }
                    if ui.button("Offer draw").clicked() {
                        self.request("draw");
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Rotate board                  F").clicked() {
                        self.prefs.view.yaw += 180.0;
                        ui.close();
                    }
                    ui.checkbox(&mut self.prefs.view.flat, "2D board / accessible squares");
                    ui.checkbox(&mut self.prefs.view.coordinates, "Edge notation");
                    ui.checkbox(&mut self.prefs.view.animations, "Animate moves");
                    if ui.button("Reset camera").clicked() {
                        self.prefs.view = View {
                            board_style: self.prefs.view.board_style,
                            piece_style: self.prefs.view.piece_style,
                            ..View::default()
                        };
                    }
                    if ui.button("Full screen                    F11").clicked() {
                        let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
                        ui.close();
                    }
                    if ui.button("Float above other windows").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                            egui::WindowLevel::AlwaysOnTop,
                        ));
                        ui.close();
                    }
                    if ui.button("Normal window level").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                            egui::WindowLevel::Normal,
                        ));
                        ui.close();
                    }
                });
                ui.menu_button("Share", |ui| {
                    if ui.button("Network game…").clicked() {
                        self.network_dialog = true;
                        ui.close();
                    }
                    if ui.button("Save screenshot…").clicked() {
                        self.file_dialog("screenshot");
                        ui.close();
                    }
                    if self.recording.is_some() {
                        if ui.button("Stop recording").clicked() {
                            self.stop_recording();
                            ui.close();
                        }
                    } else if ui.button("Record game…").clicked() {
                        self.file_dialog("record");
                        ui.close();
                    }
                });
                if ui.button("Preferences").clicked() {
                    self.settings = true;
                }
                if ui.button("Help").clicked() {
                    self.help = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("VULKAN")
                            .small()
                            .color(Color32::from_rgb(132, 150, 137)),
                    );
                    if let Some(r) = &self.recording {
                        ui.label(
                            RichText::new(format!("● REC · {} frames", r.count))
                                .color(Color32::LIGHT_RED),
                        )
                        .on_hover_text(r.path.display().to_string());
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                let mut switch = None;
                let mut close = None;
                for (i, session) in self.tabs.iter().enumerate() {
                    let (path, dirty) = if i == self.active {
                        (self.path.as_ref(), self.dirty)
                    } else {
                        (session.path.as_ref(), session.dirty)
                    };
                    let name = path
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| format!("Game {}", i + 1));
                    if ui
                        .add_enabled(
                            self.peer.is_none(),
                            egui::Button::new(format!("{name}{}", if dirty { " •" } else { "" }))
                                .selected(i == self.active),
                        )
                        .clicked()
                    {
                        switch = Some(i);
                    }
                    if ui
                        .add_enabled(self.peer.is_none(), egui::Button::new("×").small())
                        .on_hover_text("Close game")
                        .clicked()
                    {
                        close = Some(i);
                    }
                }
                if let Some(i) = switch {
                    self.switch(i);
                }
                if let Some(i) = close {
                    let dirty = if i == self.active {
                        self.dirty
                    } else {
                        self.tabs[i].dirty
                    };
                    if dirty {
                        self.confirm_close = Some(i);
                    } else {
                        self.close_tab(i);
                    }
                }
            });
        });
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        if !self.prefs.show_log {
            return;
        }
        egui::SidePanel::right("game log")
            .default_width(280.0)
            .min_width(230.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(self.game.data.rules.name().to_uppercase())
                        .small()
                        .color(Color32::from_rgb(157, 172, 159)),
                );
                ui.add_space(5.0);
                ui.heading(self.game.status());
                ui.label(format!(
                    "{}  /  {}",
                    self.game
                        .data
                        .headers
                        .get("White")
                        .map(String::as_str)
                        .unwrap_or("White"),
                    self.game
                        .data
                        .headers
                        .get("Black")
                        .map(String::as_str)
                        .unwrap_or("Black")
                ));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.peer.is_none(), egui::Button::new("|<"))
                        .on_hover_text("Initial position")
                        .clicked()
                    {
                        self.seek(0);
                    }
                    if ui.button("<").on_hover_text("Take back").clicked() {
                        self.undo();
                    }
                    if ui
                        .add_enabled(self.peer.is_none(), egui::Button::new(">"))
                        .on_hover_text("Next move")
                        .clicked()
                    {
                        self.seek(self.game.data.cursor + 1);
                    }
                    if ui
                        .add_enabled(self.peer.is_none(), egui::Button::new(">|"))
                        .on_hover_text("Latest position")
                        .clicked()
                    {
                        self.seek(self.game.data.moves.len());
                    }
                    if ui.button("Hint").clicked() {
                        self.analyze(true);
                    }
                });
                if self.peer.is_none() {
                    ui.horizontal(|ui| {
                        if ui
                            .button(if self.paused { "Resume" } else { "Pause" })
                            .clicked()
                        {
                            self.paused = !self.paused;
                            if self.paused {
                                self.job = None;
                            }
                        }
                        if self.job.is_some() {
                            ui.spinner();
                            ui.label("Thinking…");
                        }
                    });
                } else {
                    ui.label(if self.local_turn() {
                        "Your turn"
                    } else {
                        "Opponent's turn"
                    });
                }
                if let Some(a) = &self.analysis {
                    ui.label(
                        RichText::new(format!(
                            "Depth {} · {:+.2} · {} nodes",
                            a.depth,
                            a.score as f32 / 100.0,
                            a.nodes
                        ))
                        .small()
                        .color(Color32::GRAY),
                    );
                }
                if let Some(hint) = self.hint {
                    ui.label(format!(
                        "Hint: {}",
                        shakmaty::san::SanPlus::from_move(self.game.board.pos.clone(), hint)
                    ));
                }
                if let Some(pockets) = self.game.board.pos.pockets() {
                    let pockets = *pockets;
                    ui.separator();
                    ui.label("Pieces in hand · choose a piece to drop");
                    for color in [Color::White, Color::Black] {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(if color == Color::White {
                                "White"
                            } else {
                                "Black"
                            });
                            for role in [
                                Role::Pawn,
                                Role::Knight,
                                Role::Bishop,
                                Role::Rook,
                                Role::Queen,
                            ] {
                                let count = pockets[color][role];
                                if count > 0
                                    && ui
                                        .add_enabled(
                                            color == self.game.board.pos.turn()
                                                && self.local_turn(),
                                            egui::Button::new(format!(
                                                "{} ×{count}",
                                                symbol(role, color)
                                            ))
                                            .selected(
                                                self.drop_role == Some(role)
                                                    && color == self.game.board.pos.turn(),
                                            ),
                                        )
                                        .clicked()
                                {
                                    self.drop_role = Some(role);
                                    self.selected = None;
                                }
                            }
                        });
                    }
                }
                ui.separator();
                ui.label(RichText::new("MOVE HISTORY").small().color(Color32::GRAY));
                let mut seek = None;
                egui::ScrollArea::vertical()
                    .id_salt("moves")
                    .max_height((ui.available_height() - 180.0).max(90.0))
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        egui::Grid::new("move grid")
                            .num_columns(3)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                let initial_white = self.game.states[0].pos.turn() == Color::White;
                                let base = u32::from(self.game.states[0].pos.fullmoves());
                                let total = self.game.sans.len() + usize::from(!initial_white);
                                for row in 0..total.div_ceil(2) {
                                    ui.label(
                                        RichText::new(format!("{}.", base + row as u32))
                                            .color(Color32::GRAY),
                                    );
                                    for half in 0..2 {
                                        let offset = row * 2 + half;
                                        let index = offset.checked_sub(usize::from(!initial_white));
                                        if let Some(i) = index.filter(|&i| i < self.game.sans.len())
                                        {
                                            let label = format!(
                                                "{}{}",
                                                self.game.sans[i],
                                                if self.game.data.comments.contains_key(&(i + 1)) {
                                                    " ·"
                                                } else {
                                                    ""
                                                }
                                            );
                                            let response = ui.selectable_label(
                                                self.game.data.cursor == i + 1,
                                                label,
                                            );
                                            if response.clicked() {
                                                seek = Some(i + 1);
                                            }
                                            if let Some(c) = self.game.data.comments.get(&(i + 1)) {
                                                response.on_hover_text(c);
                                            }
                                        } else {
                                            ui.label("");
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                if let Some(i) = seek {
                    self.seek(i);
                }
                ui.separator();
                ui.label("Comment on this position");
                let mut comment = self
                    .game
                    .data
                    .comments
                    .get(&self.game.data.cursor)
                    .cloned()
                    .unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut comment)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    if comment.is_empty() {
                        self.game.data.comments.remove(&self.game.data.cursor);
                    } else {
                        self.game
                            .data
                            .comments
                            .insert(self.game.data.cursor, comment);
                    }
                    self.dirty = true;
                }
                if !self.game.data.variations.is_empty() {
                    ui.collapsing(
                        format!("{} saved variations", self.game.data.variations.len()),
                        |ui| {
                            let mut restore = None;
                            for (i, line) in self.game.data.variations.iter().enumerate() {
                                if ui
                                    .add_enabled(
                                        self.peer.is_none(),
                                        egui::Button::new(format!(
                                            "Variation {} · {} plies",
                                            i + 1,
                                            line.len()
                                        )),
                                    )
                                    .clicked()
                                {
                                    restore = Some(i);
                                }
                            }
                            if let Some(i) = restore {
                                let mut data = self.game.data.clone();
                                let line = data.variations[i].clone();
                                data.variations[i] = data.moves.clone();
                                data.moves = line;
                                data.cursor = data.moves.len();
                                data.result = "*".into();
                                data.comments.clear();
                                match Game::load(data) {
                                    Ok(g) => {
                                        self.game = g;
                                        self.dirty = true;
                                        self.reset_transient();
                                    }
                                    Err(e) => self.message = e,
                                }
                            }
                        },
                    );
                }
            });
    }
    fn board(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(17, 19, 22)))
            .show(ctx, |ui| {
                let available = ui.available_size();
                if available.x < 10.0 || available.y < 10.0 {
                    return;
                }
                if self.prefs.view.flat {
                    self.flat_board(ui);
                    return;
                }
                let pixels = ctx.pixels_per_point();
                self.renderer.resize(
                    [(available.x * pixels) as u32, (available.y * pixels) as u32],
                    &self.state,
                );
                let last = if self.show_last && self.game.data.cursor > 0 {
                    let prev = &self.game.states[self.game.data.cursor - 1];
                    prev.parse_move(&self.game.data.moves[self.game.data.cursor - 1])
                        .ok()
                } else {
                    None
                };
                self.renderer.render(
                    &self.game,
                    &self.prefs.view,
                    self.selected,
                    self.hint,
                    last,
                    self.motion.as_ref(),
                );
                let response = ui.add(
                    egui::Image::new((self.renderer.texture_id.unwrap(), available))
                        .sense(egui::Sense::click_and_drag()),
                );
                let rect = response.rect;
                if self.prefs.view.coordinates {
                    let painter = ui.painter();
                    for i in 0..8 {
                        for pos in [
                            glam::Vec3::new(i as f32 - 3.5, 0.03, 4.24),
                            glam::Vec3::new(i as f32 - 3.5, 0.03, -4.24),
                        ] {
                            if let Some(p) = self.prefs.view.project(pos, rect) {
                                painter.text(
                                    p,
                                    egui::Align2::CENTER_CENTER,
                                    ((b'a' + i as u8) as char).to_string(),
                                    egui::FontId::proportional(13.0),
                                    Color32::from_rgb(
                                        (210.0 * self.prefs.view.label_intensity) as u8,
                                        (207.0 * self.prefs.view.label_intensity) as u8,
                                        (190.0 * self.prefs.view.label_intensity) as u8,
                                    ),
                                );
                            }
                        }
                        for pos in [
                            glam::Vec3::new(4.25, 0.03, 3.5 - i as f32),
                            glam::Vec3::new(-4.25, 0.03, 3.5 - i as f32),
                        ] {
                            if let Some(p) = self.prefs.view.project(pos, rect) {
                                painter.text(
                                    p,
                                    egui::Align2::CENTER_CENTER,
                                    (i + 1).to_string(),
                                    egui::FontId::proportional(13.0),
                                    Color32::from_rgb(
                                        (210.0 * self.prefs.view.label_intensity) as u8,
                                        (207.0 * self.prefs.view.label_intensity) as u8,
                                        (190.0 * self.prefs.view.label_intensity) as u8,
                                    ),
                                );
                            }
                        }
                    }
                }
                if response.clicked()
                    && let Some(p) = response.interact_pointer_pos()
                    && let Some(sq) = self.prefs.view.pick(p, rect, &self.game)
                {
                    self.square(sq);
                }
                if response.drag_started_by(egui::PointerButton::Primary)
                    && let Some(p) = ctx.input(|i| i.pointer.press_origin())
                    && let Some(sq) = self.prefs.view.pick(p, rect, &self.game)
                {
                    self.square(sq);
                }
                if response.drag_stopped_by(egui::PointerButton::Primary)
                    && let Some(p) = response.interact_pointer_pos()
                    && let Some(sq) = self.prefs.view.pick(p, rect, &self.game)
                    && Some(sq) != self.selected
                {
                    self.square(sq);
                }
                if response.dragged_by(egui::PointerButton::Secondary) {
                    let delta = ctx.input(|i| i.pointer.delta());
                    self.prefs.view.yaw += delta.x * 0.45;
                    self.prefs.view.elevation =
                        (self.prefs.view.elevation + delta.y * 0.25).clamp(20.0, 89.0);
                }
                if response.hovered() {
                    let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        self.prefs.view.distance =
                            (self.prefs.view.distance - scroll * 0.008).clamp(9.0, 22.0);
                    }
                }
                let title = self
                    .game
                    .data
                    .headers
                    .get("Event")
                    .map(String::as_str)
                    .unwrap_or("Casual game");
                ui.painter().text(
                    rect.left_top() + egui::vec2(28.0, 22.0),
                    egui::Align2::LEFT_TOP,
                    title,
                    egui::FontId::proportional(17.0),
                    Color32::from_rgb(174, 184, 174),
                );
                ui.painter().text(
                    rect.left_bottom() + egui::vec2(28.0, -22.0),
                    egui::Align2::LEFT_BOTTOM,
                    "Click or drag to move  ·  Right-drag to orbit  ·  Scroll to zoom",
                    egui::FontId::proportional(12.0),
                    Color32::from_rgb(125, 137, 132),
                );
            });
    }
    fn flat_board(&mut self, ui: &mut egui::Ui) {
        let side = ui.available_width().min(ui.available_height()).max(64.0) - 40.0;
        let cell = side / 8.0;
        let flip = (self.prefs.view.yaw.rem_euclid(360.0) > 90.0)
            && (self.prefs.view.yaw.rem_euclid(360.0) < 270.0);
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - side) / 2.0).max(0.0));
            ui.vertical(|ui| {
                ui.add_space(12.0);
                egui::Grid::new("accessible board")
                    .spacing([0.0, 0.0])
                    .show(ui, |ui| {
                        for row in 0..8 {
                            for col in 0..8 {
                                let rank = if flip { row } else { 7 - row };
                                let file = if flip { 7 - col } else { col };
                                let sq = Square::new((rank * 8 + file) as u32);
                                let piece = self.game.at(sq);
                                let dark = (rank + file) % 2 == 0;
                                let color = if self.selected == Some(sq) {
                                    Color32::from_rgb(159, 153, 78)
                                } else if dark {
                                    Color32::from_rgb(92, 119, 99)
                                } else {
                                    Color32::from_rgb(216, 216, 188)
                                };
                                let glyph = piece.map(|p| symbol(p.role, p.color)).unwrap_or("");
                                let label = if let Some(p) = piece {
                                    format!("{} {:?} {:?}", sq, p.color, p.role)
                                } else {
                                    format!("{sq}, empty")
                                };
                                let response = ui.add_sized(
                                    [cell, cell],
                                    egui::Button::new(
                                        RichText::new(glyph).size(cell * 0.68).color(
                                            if piece.is_some_and(|p| p.color == Color::Black) {
                                                Color32::from_rgb(25, 30, 27)
                                            } else {
                                                Color32::from_rgb(251, 250, 228)
                                            },
                                        ),
                                    )
                                    .fill(color)
                                    .corner_radius(0),
                                );
                                let response = response.on_hover_text(&label);
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        &label,
                                    )
                                });
                                if self.prefs.view.coordinates {
                                    ui.painter().text(
                                        response.rect.left_top() + egui::vec2(4.0, 3.0),
                                        egui::Align2::LEFT_TOP,
                                        sq.to_string(),
                                        egui::FontId::proportional(10.0),
                                        Color32::from_rgb(45, 55, 40),
                                    );
                                }
                                if response.clicked() {
                                    self.square(sq);
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        });
    }
    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.game.status()).strong());
                ui.separator();
                ui.label(format!(
                    "{} · {} plies",
                    self.game.data.rules.name(),
                    self.game.data.cursor
                ));
                if let Some(sq) = self.selected {
                    ui.label(format!("Selected {sq}"));
                }
                if self.drop_role.is_some() {
                    ui.label("Choose a square to drop the piece");
                }
                if self.listener.is_some() {
                    ui.label(RichText::new("● Listening").color(Color32::LIGHT_GREEN));
                }
            });
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.input)
                        .hint_text("Enter a move or speech command · e2e4, Nf3, pawn to e four")
                        .desired_width(430.0),
                );
                if ui.button("Play").clicked()
                    || (response.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    let text = std::mem::take(&mut self.input);
                    self.voice(&text);
                }
                if ui.small_button("Clear status").clicked() {
                    self.message.clear();
                }
                ui.label(RichText::new(&self.message).small());
            });
        });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        let mut open = self.new_dialog;
        egui::Window::new("New game").open(&mut open).collapsible(false).show(ctx,|ui|{ui.label("Choose your game");egui::ComboBox::from_label("Variant").selected_text(self.new_rules.name()).show_ui(ui,|ui|{for r in Rules::ALL{ui.selectable_value(&mut self.new_rules,r,r.name());}});for(i,label)in ["White","Black"].into_iter().enumerate(){egui::ComboBox::from_label(label).selected_text(if self.new_computer[i]{"Computer"}else{"Human"}).show_ui(ui,|ui|{ui.selectable_value(&mut self.new_computer[i],false,"Human");ui.selectable_value(&mut self.new_computer[i],true,"Computer");});}ui.label(match self.new_rules{Rules::Standard=>"The classic game. Protect your king and checkmate your opponent.",Rules::Crazyhouse=>"Captured pieces join your pocket and can be dropped back onto the board.",Rules::Suicide=>"Captures are compulsory. Lose all your pieces to win; kings can be captured.",Rules::Losers=>"Captures are compulsory, but your king must remain safe. Lose your other pieces or be mated to win."});if ui.button("Start game").clicked(){self.new_game();}});
        self.new_dialog = open && self.new_dialog;
        let mut open = self.settings;
        egui::Window::new("Preferences").open(&mut open).resizable(false).show(ctx,|ui|{
            ui.strong("Appearance");egui::ComboBox::from_label("Board material").selected_text(["Wood","Marble","Metal","Grass"][self.prefs.view.board_style]).show_ui(ui,|ui|{for(i,name)in ["Wood","Marble","Metal","Grass"].into_iter().enumerate(){ui.selectable_value(&mut self.prefs.view.board_style,i,name);}});egui::ComboBox::from_label("Piece material").selected_text(["Wood","Marble","Metal","Fur"][self.prefs.view.piece_style]).show_ui(ui,|ui|{for(i,name)in ["Wood","Marble","Metal","Fur"].into_iter().enumerate(){ui.selectable_value(&mut self.prefs.view.piece_style,i,name);}});ui.add(egui::Slider::new(&mut self.prefs.view.elevation,20.0..=89.0).text("Board angle"));ui.add(egui::Slider::new(&mut self.prefs.view.yaw,0.0..=360.0).text("Board rotation"));ui.checkbox(&mut self.prefs.view.coordinates,"Edge notation");ui.checkbox(&mut self.prefs.view.animations,"Animate moves");ui.checkbox(&mut self.prefs.view.flat,"2D board with screen reader support");ui.separator();
            if ui.button("Lighting and material controls…").clicked(){self.tuner=true;}
            ui.strong("Computer players");ui.add(egui::Slider::new(&mut self.prefs.seconds,0.05..=30.0).logarithmic(true).text("Thinking time (seconds)"));ui.add(egui::Slider::new(&mut self.prefs.depth,1..=16).text("Maximum search depth"));if self.peer.is_none(){for(i,label)in ["Computer plays White","Computer plays Black"].into_iter().enumerate(){if ui.checkbox(&mut self.game.data.computer[i],label).changed(){self.job=None;self.dirty=true;}}}ui.checkbox(&mut self.prefs.engine_log,"Log engine analysis to the terminal");ui.label("Optional original Sjeng executable (leave empty for Rust engine)");if ui.text_edit_singleline(&mut self.prefs.sjeng_path).changed(){self.job=None;}ui.label(RichText::new("Build the supplied engine with scripts/build-sjeng.sh to retain its original search, opening books and learning.").small());ui.separator();
            ui.strong("Speech");ui.checkbox(&mut self.prefs.speak_computer,"Speak computer and remote moves");ui.checkbox(&mut self.prefs.speak_human,"Speak human moves");for(i,label)in ["White voice (espeak)","Black voice (espeak)"].into_iter().enumerate(){ui.horizontal(|ui|{ui.label(label);ui.text_edit_singleline(&mut self.prefs.voices[i]);});}ui.label("Offline recognition model folder (Vosk)");ui.text_edit_singleline(&mut self.prefs.model);if self.listener.is_some(){if ui.button("Stop listening").clicked(){self.listener=None;}}else if ui.button("Listen for spoken moves").clicked(){match Listener::start(&PathBuf::from(&self.prefs.model)){Ok(l)=>self.listener=Some(l),Err(e)=>self.message=e}}ui.label(RichText::new("Recognition requires Python, vosk, sounddevice, and a downloaded model.\nUse the command field to enter the same spoken phrases.").small());
        });
        self.settings = open;
        let mut open = self.tuner;
        egui::Window::new("Lighting and materials")
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(600.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.prefs.view.reflectivity, 0.0..=1.0)
                                .text("Board reflectivity"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.prefs.view.label_intensity, 0.0..=1.0)
                                .text("Edge notation brightness"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.prefs.view.ambient, 0.0..=1.0)
                                .text("Ambient light"),
                        );
                        for (i, name) in ["Light X", "Light Y", "Light Z"].into_iter().enumerate() {
                            ui.add(
                                egui::Slider::new(&mut self.prefs.view.light[i], -30.0..=30.0)
                                    .text(name),
                            );
                        }
                        for (i, name) in [
                            "White pieces",
                            "Black pieces",
                            "White squares",
                            "Black squares",
                            "Border",
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            ui.collapsing(name, |ui| {
                                let material = &mut self.prefs.view.materials[i];
                                ui.add(
                                    egui::Slider::new(&mut material.diffuse, 0.0..=1.5)
                                        .text("Diffuse"),
                                );
                                ui.add(
                                    egui::Slider::new(&mut material.specular, 0.0..=2.0)
                                        .text("Specular"),
                                );
                                ui.add(
                                    egui::Slider::new(&mut material.shininess, 1.0..=150.0)
                                        .text("Shininess"),
                                );
                                ui.add(
                                    egui::Slider::new(&mut material.alpha, 0.05..=1.0)
                                        .text("Opacity"),
                                );
                            });
                        }
                        if ui.button("Save settings").clicked() {
                            self.persist();
                            self.message = "Lighting and material settings saved".into();
                        }
                        if ui.button("Reset lighting and materials").clicked() {
                            let defaults = View::default();
                            self.prefs.view.materials = defaults.materials;
                            self.prefs.view.light = defaults.light;
                            self.prefs.view.ambient = defaults.ambient;
                            self.prefs.view.reflectivity = defaults.reflectivity;
                            self.prefs.view.label_intensity = defaults.label_intensity;
                        }
                    });
            });
        self.tuner = open;
        let mut open = self.info;
        egui::Window::new("Game information")
            .open(&mut open)
            .show(ctx, |ui| {
                egui::Grid::new("headers").show(ui, |ui| {
                    for key in [
                        "Event",
                        "Site",
                        "Date",
                        "Round",
                        "White",
                        "Black",
                        "City",
                        "Country",
                        "StartTime",
                    ] {
                        ui.label(key);
                        let value = self.game.data.headers.entry(key.into()).or_default();
                        if ui.text_edit_singleline(value).changed() {
                            self.dirty = true;
                        }
                        ui.end_row();
                    }
                });
                ui.label(format!("Result: {}", self.game.result()));
            });
        self.info = open;
        let mut open = self.position_dialog;
        egui::Window::new("Set up position")
            .open(&mut open)
            .default_width(650.0)
            .show(ctx, |ui| {
                ui.label("Forsyth–Edwards notation (FEN)");
                ui.add(
                    egui::TextEdit::multiline(&mut self.fen_text)
                        .desired_width(f32::INFINITY)
                        .desired_rows(3),
                );
                ui.label(
                    "This starts a new game in the current variant from the entered position.",
                );
                if ui.button("Open position as a new game").clicked() {
                    let mut game = Game::new(self.game.data.rules);
                    match game.set_fen(&self.fen_text) {
                        Ok(()) => {
                            game.data.computer = [false, false];
                            self.tabs[self.active] = self.snapshot();
                            self.game = game;
                            self.path = None;
                            self.dirty = true;
                            self.tabs.push(self.snapshot());
                            self.active = self.tabs.len() - 1;
                            self.reset_transient();
                            self.position_dialog = false;
                        }
                        Err(e) => self.message = e,
                    }
                }
            });
        self.position_dialog = open && self.position_dialog;
        if !self.promotion.is_empty() {
            egui::Window::new("Promote pawn")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Choose a promotion piece");
                    let choices = self.promotion.clone();
                    ui.horizontal(|ui| {
                        for m in choices {
                            if let Some(role) = m.promotion()
                                && self.game.promotion_roles().contains(&role)
                                && ui
                                    .button(format!(
                                        "{} {:?}",
                                        symbol(role, self.game.board.pos.turn()),
                                        role
                                    ))
                                    .clicked()
                            {
                                self.promotion.clear();
                                self.play(m, false);
                            }
                        }
                    });
                    if ui.button("Cancel").clicked() {
                        self.promotion.clear();
                    }
                });
        }
        let mut open = self.network_dialog;
        egui::Window::new("Network game")
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| {
                if self.peer.is_none() {
                    ui.label("Host plays White. The joining player plays Black.");
                    ui.label("Host bind address or opponent address");
                    ui.text_edit_singleline(&mut self.address);
                    ui.horizontal(|ui| {
                        if ui.button("Host current game").clicked() {
                            self.reset_transient();
                            self.peer = Some(Peer::start(self.address.clone(), true));
                        }
                        if ui.button("Join game").clicked() {
                            self.reset_transient();
                            self.peer = Some(Peer::start(self.address.clone(), false));
                        }
                    });
                } else {
                    ui.label(if self.peer.as_ref().unwrap().connected {
                        "Connected"
                    } else {
                        "Connecting…"
                    });
                    if ui.button("Disconnect").clicked() {
                        self.disconnect();
                    }
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for text in &self.chat_log {
                            ui.label(text);
                        }
                    });
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.chat);
                    if ui
                        .add_enabled(
                            self.peer.as_ref().is_some_and(|p| p.connected),
                            egui::Button::new("Send"),
                        )
                        .clicked()
                    {
                        let text = std::mem::take(&mut self.chat);
                        let text = text.chars().take(1000).collect::<String>();
                        if !text.is_empty() {
                            if let Some(p) = &self.peer {
                                let _ = p.send(Message::Chat(text.clone()));
                            }
                            self.chat_log.push(format!("You: {text}"));
                        }
                    }
                });
            });
        self.network_dialog = open;
        if let Some(request) = self.remote_request.clone() {
            egui::Window::new("Opponent's request")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(format!("Your opponent offers a {request}."));
                    ui.horizontal(|ui| {
                        for (accepted, label) in [(true, "Accept"), (false, "Decline")] {
                            if ui.button(label).clicked()
                                && let Err(error) = self.respond(accepted)
                            {
                                self.message = error;
                            }
                        }
                    });
                });
        }
        if let Some(index) = self.confirm_close {
            egui::Window::new("Unsaved game")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Save the game before closing it?");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.switch(index);
                            if let Some(p) = self.path.clone() {
                                self.save(p);
                                if !self.dirty {
                                    self.close_tab(index);
                                    self.confirm_close = None;
                                }
                            } else {
                                self.awaiting_close = Some(index);
                                self.file_dialog("save");
                                self.confirm_close = None;
                            }
                        }
                        if ui.button("Discard changes").clicked() {
                            self.close_tab(index);
                            self.confirm_close = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_close = None;
                            self.quit_requested = false;
                        }
                    });
                });
        }
        let mut open = self.help;
        egui::Window::new("About Chess / Help").open(&mut open).default_width(560.0).show(ctx,|ui|{ui.heading("Chess for Linux");ui.label("A Rust rebuild of the supplied Apple Chess source, rendered with Vulkan.");ui.separator();ui.label("Click a piece, then a highlighted destination, or drag the piece.\nRight-drag rotates the 3D board. Scroll zooms. F flips the board.\nArrow keys choose a square; Enter selects it. Escape clears selection.\nCtrl+N: new game · Ctrl+O: open · Ctrl+S: save\nCtrl+Z: take back · Ctrl+Shift+Z: redo · ]: hint · [: last move\nF11: full screen · Ctrl+Q: quit");ui.label("The 2D board exposes individually labelled buttons to Linux screen readers.\nThe command field accepts SAN, UCI, drops (N@e4) and spoken phrases.\nExamples: 'move pawn from e two to e four', 'knight to f three',\n'castle kingside', 'drop knight on e four', 'show hint', 'take back move'.");ui.separator();ui.label("PGN and Apple .chess files can be opened and saved. Native .chess-linux files preserve history, comments and saved branches. Sessions recover automatically on restart.");ui.label("Direct network play replaces shared sessions. Game Center is excluded.\nOptional speech: espeak for output, Vosk + sounddevice for microphone input.\nOptional video recording: ffmpeg with libx264.");ui.separator();ui.label(format!("GPU: {} · Vulkan · 4× MSAA",self.renderer.adapter_name));ui.label("Original artwork and geometry: Apple Sample Code License (README).\nRust application: GPL-3.0-or-later. Chess rules: shakmaty.\nThe original Sjeng source remains in sjeng/ under its original GPL license.");});
        self.help = open;
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let command = ctx.input(|i| i.modifiers.command);
        let shift = ctx.input(|i| i.modifiers.shift);
        if command {
            for key in [
                egui::Key::N,
                egui::Key::O,
                egui::Key::S,
                egui::Key::Z,
                egui::Key::Q,
            ] {
                if ctx.input(|i| i.key_pressed(key)) {
                    match key {
                        egui::Key::N if self.peer.is_none() => self.new_dialog = true,
                        egui::Key::O if self.peer.is_none() => self.file_dialog("open"),
                        egui::Key::S => {
                            if let Some(p) = self.path.clone() {
                                self.save(p);
                            } else {
                                self.file_dialog("save");
                            }
                        }
                        egui::Key::Z if !ctx.wants_keyboard_input() => {
                            if shift {
                                self.seek(self.game.data.cursor + 1);
                            } else {
                                self.undo();
                            }
                        }
                        egui::Key::Q => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        _ => (),
                    }
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F)) {
            self.prefs.view.yaw += 180.0;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
            self.analyze(true);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
            self.show_last = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selected = None;
            self.drop_role = None;
            self.hint = None;
        }
        let mut file = i32::from(self.keyboard_square.file());
        let mut rank = i32::from(self.keyboard_square.rank());
        for (key, df, dr) in [
            (egui::Key::ArrowLeft, -1, 0),
            (egui::Key::ArrowRight, 1, 0),
            (egui::Key::ArrowUp, 0, 1),
            (egui::Key::ArrowDown, 0, -1),
        ] {
            if ctx.input(|i| i.key_pressed(key)) {
                file = (file + df).clamp(0, 7);
                rank = (rank + dr).clamp(0, 7);
                self.keyboard_square = Square::new((rank * 8 + file) as u32);
                self.message = format!(
                    "Keyboard square {} · {}",
                    self.keyboard_square,
                    self.game
                        .at(self.keyboard_square)
                        .map(|p| format!("{:?} {:?}", p.color, p.role))
                        .unwrap_or("empty".into())
                );
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.square(self.keyboard_square);
        }
    }
    fn automation(
        &mut self,
        command: crate::automation::Command,
        ctx: &egui::Context,
    ) -> Result<serde_json::Value, String> {
        use crate::automation::Command;
        let network_host = matches!(command, Command::Host { .. });
        if self.peer.is_some()
            && !matches!(
                command,
                Command::Status
                    | Command::Screenshot { .. }
                    | Command::SetView { .. }
                    | Command::Move { .. }
                    | Command::Undo
                    | Command::Save { .. }
                    | Command::Hint
                    | Command::Disconnect
                    | Command::Ask { .. }
                    | Command::Respond { .. }
                    | Command::Resign
                    | Command::Quit
            )
        {
            return Err("Document scripting is disabled during network play".into());
        }
        match command {
            Command::Status => (),
            Command::Move { text } => {
                if self.game.result() != "*" {
                    return Err("The game has ended".into());
                }
                if !self.local_turn() {
                    return Err("Waiting for the computer, opponent or a pending request".into());
                }
                let m = self.game.board.parse_move(&text)?;
                self.play(m, false);
            }
            Command::New { variant, computer } => {
                self.new_rules = variant;
                self.new_computer = computer;
                self.new_game();
            }
            Command::Undo => self.undo(),
            Command::Seek { ply } => {
                if ply > self.game.data.moves.len() {
                    return Err("Ply exceeds game history".into());
                }
                self.seek(ply);
            }
            Command::Open { path } => {
                let game = document::read(&path)?;
                self.tabs[self.active] = self.snapshot();
                self.game = game;
                self.path = Some(path.clone());
                self.dirty = false;
                self.tabs.push(self.snapshot());
                self.active = self.tabs.len() - 1;
                self.reset_transient();
                self.recent(path);
            }
            Command::Save { path } => {
                document::write(&path, &self.game)?;
                self.path = Some(path.clone());
                self.dirty = false;
                self.recent(path);
            }
            Command::SetFen { fen } => {
                let mut game = Game::new(self.game.data.rules);
                game.set_fen(&fen)?;
                game.data.computer = [false, false];
                self.tabs[self.active] = self.snapshot();
                self.game = game;
                self.path = None;
                self.dirty = true;
                self.tabs.push(self.snapshot());
                self.active = self.tabs.len() - 1;
                self.reset_transient();
            }
            Command::Hint => self.analyze(true),
            Command::Pause { paused } => {
                self.paused = paused;
                if paused {
                    self.job = None;
                }
            }
            Command::Host { address } | Command::Join { address } => {
                self.reset_transient();
                self.address = address.clone();
                self.peer = Some(Peer::start(address, network_host));
            }
            Command::Disconnect => self.disconnect(),
            Command::Ask { request } => {
                if !matches!(request.as_str(), "draw" | "takeback") {
                    return Err("Request must be draw or takeback".into());
                }
                if self.peer.as_ref().is_none_or(|peer| !peer.connected) {
                    return Err("No connected opponent".into());
                }
                if self.pending_request.is_some() || self.remote_request.is_some() {
                    return Err("Another request is pending".into());
                }
                self.request(&request);
            }
            Command::Respond { accepted } => self.respond(accepted)?,
            Command::Resign => self.resign(),
            Command::SetView { view } => {
                if view.board_style > 3
                    || view.piece_style > 3
                    || !(9.0..=22.0).contains(&view.distance)
                    || !(20.0..=89.0).contains(&view.elevation)
                    || !view.yaw.is_finite()
                {
                    return Err("Invalid view settings".into());
                }
                self.prefs.view = view;
            }
            Command::Screenshot { path } => {
                self.renderer.render(
                    &self.game,
                    &self.prefs.view,
                    self.selected,
                    self.hint,
                    None,
                    None,
                );
                self.renderer.save_png(&path)?;
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
        Ok(
            serde_json::json!({"fen":self.game.board.fen(),"status":self.game.status(),"variant":self.game.data.rules,"ply":self.game.data.cursor,"result":self.game.result(),"thinking":self.job.is_some(),"paused":self.paused,"network_active":self.peer.is_some(),"connected":self.peer.as_ref().is_some_and(|p|p.connected),"host":self.peer.as_ref().is_some_and(|p|p.host),"network_address":self.address,"pending_request":self.pending_request,"remote_request":self.remote_request}),
        )
    }
    fn smoke_test(&mut self, ctx: &egui::Context) {
        if !self.smoke {
            return;
        }
        let elapsed = self.smoke_started.elapsed().as_secs_f32();
        match self.smoke_step {
            0 if elapsed > 1.0 => {
                std::fs::create_dir_all("artifacts").unwrap();
                self.game.data.computer = [false, false];
                self.square(Square::E2);
                assert_eq!(self.selected, Some(Square::E2));
                self.square(Square::E4);
                self.square(Square::E7);
                self.square(Square::E5);
                assert_eq!(self.game.data.cursor, 2);
                self.smoke_step = 1;
            }
            1 if elapsed > 1.5 => {
                self.screenshot_path = Some("artifacts/linux-vulkan.png".into());
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    "still",
                )));
                self.smoke_step = 2;
            }
            2 if elapsed > 2.0 => {
                self.undo();
                assert_eq!(self.game.data.cursor, 1);
                self.seek(2);
                assert_eq!(self.game.data.cursor, 2);
                self.new_rules = Rules::Crazyhouse;
                self.new_computer = [false, false];
                self.new_game();
                let m = self.game.board.parse_move("e2e4").unwrap();
                self.play(m, false);
                self.switch(0);
                assert_eq!(self.game.data.cursor, 2);
                self.prefs.view.board_style = 1;
                self.prefs.view.piece_style = 2;
                self.settings = true;
                self.recording = Some(
                    Recording::start(
                        std::path::Path::new("artifacts/linux-recording.mp4"),
                        [1280, 850],
                    )
                    .unwrap(),
                );
                self.smoke_step = 3;
            }
            3 if elapsed > 3.0 => {
                self.screenshot_path = Some("artifacts/linux-preferences.png".into());
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    "still",
                )));
                self.smoke_step = 4;
            }
            4 if elapsed > 3.5 => {
                self.settings = false;
                self.stop_recording();
                self.prefs.view.flat = true;
                self.smoke_step = 5;
            }
            5 if elapsed > 4.0 => {
                self.screenshot_path = Some("artifacts/linux-accessible.png".into());
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    "still",
                )));
                self.smoke_step = 6;
            }
            6 if elapsed > 5.5 => {
                assert!(
                    std::fs::metadata("artifacts/linux-recording.mp4")
                        .unwrap()
                        .len()
                        > 1000
                );
                assert!(self.finishing.is_empty(), "Recording did not finish");
                println!(
                    "Native Vulkan GUI smoke test passed: moves, undo/redo, variants, tabs, preferences, accessible board, screenshots, video recording"
                );
                self.dirty = false;
                for t in &mut self.tabs {
                    t.dirty = false;
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                self.smoke_step = 7;
            }
            _ => (),
        }
    }
}
impl eframe::App for ChessApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.shortcuts(ctx);
        self.menus(ctx);
        self.status_bar(ctx);
        self.sidebar(ctx);
        self.board(ctx);
        self.dialogs(ctx);
        self.smoke_test(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.smoke {
            self.tabs[self.active] = self.snapshot();
            if let Some(index) = self.tabs.iter().position(|s| s.dirty) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.quit_requested = true;
                self.confirm_close = Some(index);
            }
        }
        if self.quit_requested && self.confirm_close.is_none() && self.awaiting_close.is_none() {
            if let Some(index) = self.tabs.iter().position(|s| s.dirty) {
                self.confirm_close = Some(index);
            } else {
                self.quit_requested = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.last_autosave.elapsed() > Duration::from_secs(2) {
            self.persist();
            self.last_autosave = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(
            if self.motion.is_some() || self.recording.is_some() {
                16
            } else if self.job.is_some()
                || self.peer.is_some()
                || self.listener.is_some()
                || self.smoke
            {
                30
            } else {
                100
            },
        ));
    }
    fn on_exit(&mut self) {
        self.job = None;
        self.listener = None;
        self.peer = None;
        self.stop_recording();
        self.persist();
        for done in &self.finishing {
            let _ = done.recv_timeout(Duration::from_secs(10));
        }
    }
}
fn symbol(role: Role, color: Color) -> &'static str {
    match (color, role) {
        (Color::White, Role::King) => "♔",
        (Color::White, Role::Queen) => "♕",
        (Color::White, Role::Rook) => "♖",
        (Color::White, Role::Bishop) => "♗",
        (Color::White, Role::Knight) => "♘",
        (Color::White, Role::Pawn) => "♙",
        (Color::Black, Role::King) => "♚",
        (Color::Black, Role::Queen) => "♛",
        (Color::Black, Role::Rook) => "♜",
        (Color::Black, Role::Bishop) => "♝",
        (Color::Black, Role::Knight) => "♞",
        (Color::Black, Role::Pawn) => "♟",
    }
}

fn notify_end(game: &Game) {
    if std::env::var_os("CHESS_DISABLE_NOTIFICATIONS").is_some() {
        return;
    }
    let message = game.status();
    std::thread::spawn(move || {
        let _ = std::process::Command::new("notify-send")
            .args(["Chess", &message])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    });
}
