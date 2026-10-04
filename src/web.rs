//! Browser frontend. Platform services stay in the native application modules.
use crate::{
    document, engine,
    game::{Game, Rules},
    render::{self, BoardRenderer, View},
};
use eframe::egui::{self, Color32};
use shakmaty::{Color, Move, Position, Role, Square};
use std::sync::{Arc, atomic::AtomicBool};
use wasm_bindgen::JsCast;

pub fn start() {
    let _ = eframe::WebLogger::init(log::LevelFilter::Warn);
    wasm_bindgen_futures::spawn_local(async {
        let result = async {
            let document = web_sys::window().unwrap().document().unwrap();
            let canvas = document
                .get_element_by_id("chess-canvas")
                .ok_or_else(|| wasm_bindgen::JsValue::from_str("Missing chess canvas"))?
                .dyn_into::<web_sys::HtmlCanvasElement>()?;
            eframe::WebRunner::new()
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(|cc| Ok(Box::new(WebApp::new(cc)))),
                )
                .await
        }
        .await;
        if let Some(loading) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("loading"))
        {
            match result {
                Ok(()) => loading.remove(),
                Err(error) => loading.set_text_content(Some(&format!(
                    "Unable to start Chess. Use a browser with WebGPU or WebGL2 enabled. {error:?}"
                ))),
            }
        }
    });
}

struct WebApp {
    game: Game,
    view: View,
    renderer: BoardRenderer,
    state: eframe::egui_wgpu::RenderState,
    selected: Option<Square>,
    drop_role: Option<Role>,
    promotions: Vec<Move>,
    hint: Option<Move>,
    input: String,
    message: String,
    rules: Rules,
    paused: bool,
    search_pending: bool,
    documents: bool,
    document_text: String,
}

impl WebApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let rules = web_sys::window()
            .and_then(|w| w.location().hash().ok())
            .and_then(|hash| Rules::parse(hash.trim_start_matches('#')).ok())
            .unwrap_or_default();
        let game = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, "game"))
            .and_then(|data| Game::load(data).ok())
            .unwrap_or_else(|| Game::new(rules));
        let view = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, "view"))
            .unwrap_or_default();
        let state = cc.wgpu_render_state.clone().expect("WebGPU render state");
        let renderer = BoardRenderer::new(
            state.device.clone(),
            state.queue.clone(),
            state.adapter.get_info().name,
        );
        Self {
            rules: game.data.rules,
            game,
            view,
            renderer,
            state,
            selected: None,
            drop_role: None,
            promotions: vec![],
            hint: None,
            input: String::new(),
            message: String::new(),
            paused: false,
            search_pending: false,
            documents: false,
            document_text: String::new(),
        }
    }

    fn reset_selection(&mut self) {
        self.selected = None;
        self.drop_role = None;
        self.promotions.clear();
        self.hint = None;
        self.search_pending = false;
    }

    fn play(&mut self, m: Move) {
        match self.game.push(m) {
            Ok(()) => {
                self.message.clear();
                self.reset_selection();
            }
            Err(error) => self.message = error,
        }
    }

    fn square(&mut self, square: Square) {
        if self.game.result() != "*"
            || !self.promotions.is_empty()
            || (!self.paused
                && self.game.data.computer[usize::from(self.game.board.pos.turn() == Color::Black)])
        {
            return;
        }
        let candidates: Vec<_> = self
            .game
            .board
            .moves()
            .into_iter()
            .filter(|m| {
                render::destination(*m) == square
                    && if let Some(role) = self.drop_role {
                        matches!(m, Move::Put { role: r, .. } if *r == role)
                    } else {
                        self.selected.is_some() && m.from() == self.selected
                    }
            })
            .collect();
        if candidates.len() == 1 {
            self.play(candidates[0]);
        } else if candidates.len() > 1 {
            self.promotions = candidates;
        } else {
            self.selected = self
                .game
                .at(square)
                .filter(|p| p.color == self.game.board.pos.turn())
                .map(|_| square);
            self.drop_role = None;
        }
    }

    fn analyze(&mut self, hint: bool) {
        // Browsers cannot use std::thread. Keep searches short on the UI thread;
        // schedule computer turns after painting so computer/computer can yield.
        let analysis = engine::analyze(
            self.game.board.clone(),
            0.05,
            4,
            Arc::new(AtomicBool::new(false)),
        );
        self.message = format!(
            "Depth {} · score {} · {} positions",
            analysis.depth, analysis.score, analysis.nodes
        );
        if hint {
            self.hint = analysis.best;
        } else if let Some(m) = analysis.best {
            self.play(m);
        }
    }

    fn controls(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Chess").on_hover_text(format!(
                    "{:?} · {}",
                    self.state.adapter.get_info().backend,
                    self.renderer.adapter_name
                ));
                egui::ComboBox::from_id_salt("variant")
                    .selected_text(self.rules.name())
                    .show_ui(ui, |ui| {
                        for rules in Rules::ALL {
                            ui.selectable_value(&mut self.rules, rules, rules.name());
                        }
                    });
                if ui.button("New game").clicked() {
                    self.game = Game::new(self.rules);
                    self.paused = false;
                    self.reset_selection();
                    self.message.clear();
                }
                if ui
                    .add_enabled(self.game.data.cursor > 0, egui::Button::new("Undo"))
                    .clicked()
                {
                    self.game.seek(self.game.data.cursor - 1);
                    self.paused = true;
                    self.reset_selection();
                }
                if ui
                    .add_enabled(
                        self.game.data.cursor < self.game.data.moves.len(),
                        egui::Button::new("Redo"),
                    )
                    .clicked()
                {
                    self.game.seek(self.game.data.cursor + 1);
                    self.reset_selection();
                }
                ui.checkbox(&mut self.paused, "Pause computer");
                if ui
                    .add_enabled(self.game.result() == "*", egui::Button::new("Hint"))
                    .clicked()
                {
                    self.analyze(true);
                }
                if ui.button("Flip").clicked() {
                    self.view.yaw += 180.0;
                }
                ui.checkbox(&mut self.view.flat, "2D board");
                if ui.button("Import / export").clicked() {
                    self.documents = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(self.game.status());
                for (i, label) in ["Computer White", "Computer Black"].into_iter().enumerate() {
                    ui.checkbox(&mut self.game.data.computer[i], label);
                }
                egui::ComboBox::from_label("Board")
                    .selected_text(
                        ["Wood", "Marble", "Metal", "Grass"][self.view.board_style.min(3)],
                    )
                    .show_ui(ui, |ui| {
                        for (i, name) in
                            ["Wood", "Marble", "Metal", "Grass"].into_iter().enumerate()
                        {
                            ui.selectable_value(&mut self.view.board_style, i, name);
                        }
                    });
                egui::ComboBox::from_label("Pieces")
                    .selected_text(["Wood", "Marble", "Metal", "Fur"][self.view.piece_style.min(3)])
                    .show_ui(ui, |ui| {
                        for (i, name) in ["Wood", "Marble", "Metal", "Fur"].into_iter().enumerate()
                        {
                            ui.selectable_value(&mut self.view.piece_style, i, name);
                        }
                    });
            });
            ui.horizontal_wrapped(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.input).hint_text("Move: e2e4, Nf3, N@e4"),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Play move").clicked() || enter {
                    if !self.paused
                        && self.game.data.computer
                            [usize::from(self.game.board.pos.turn() == Color::Black)]
                    {
                        self.message = "Pause the computer to enter its move".into();
                    } else {
                        match self.game.board.parse_move(&self.input) {
                            Ok(m) => {
                                self.play(m);
                                self.input.clear();
                            }
                            Err(error) => self.message = error,
                        }
                    }
                }
                ui.label(&self.message);
            });
            if let Some(pockets) = self.game.board.pos.pockets() {
                let pocket = pockets[self.game.board.pos.turn()];
                ui.horizontal(|ui| {
                    ui.label("Drop:");
                    for role in [
                        Role::Pawn,
                        Role::Knight,
                        Role::Bishop,
                        Role::Rook,
                        Role::Queen,
                    ] {
                        if pocket[role] > 0
                            && ui
                                .selectable_label(
                                    self.drop_role == Some(role),
                                    format!("{role:?} ×{}", pocket[role]),
                                )
                                .clicked()
                        {
                            self.drop_role = Some(role);
                            self.selected = None;
                        }
                    }
                });
            }
        });
        if !self.promotions.is_empty() {
            egui::Window::new("Promote pawn")
                .collapsible(false)
                .show(ctx, |ui| {
                    for role in self.game.promotion_roles() {
                        if let Some(m) = self
                            .promotions
                            .iter()
                            .find(|m| m.promotion() == Some(role))
                            .copied()
                            && ui.button(format!("{role:?}")).clicked()
                        {
                            self.play(m);
                        }
                    }
                });
        }
    }

    fn flat_board(&mut self, ui: &mut egui::Ui) {
        let size = (ui.available_width().min(ui.available_height()) / 8.0).max(16.0);
        let flipped =
            self.view.yaw.rem_euclid(360.0) > 90.0 && self.view.yaw.rem_euclid(360.0) < 270.0;
        egui::Grid::new("squares")
            .spacing(egui::Vec2::ZERO)
            .show(ui, |ui| {
                for y in 0..8 {
                    for x in 0..8 {
                        let file = if flipped { 7 - x } else { x };
                        let rank = if flipped { y } else { 7 - y };
                        let sq = Square::new(rank * 8 + file);
                        let piece = self.game.at(sq);
                        let text = piece.map_or(String::new(), |p| {
                            let c = match p.role {
                                Role::King => '♚',
                                Role::Queen => '♛',
                                Role::Rook => '♜',
                                Role::Bishop => '♝',
                                Role::Knight => '♞',
                                Role::Pawn => '♟',
                            };
                            c.to_string()
                        });
                        let fill = if self.selected == Some(sq) {
                            Color32::from_rgb(100, 145, 100)
                        } else if (file + rank) % 2 == 0 {
                            Color32::from_rgb(100, 78, 58)
                        } else {
                            Color32::from_rgb(198, 178, 145)
                        };
                        let color = if piece.is_some_and(|p| p.color == Color::White) {
                            Color32::WHITE
                        } else {
                            Color32::BLACK
                        };
                        let label = format!(
                            "{sq}: {}",
                            piece.map_or("empty".into(), |p| format!("{:?} {:?}", p.color, p.role))
                        );
                        if ui
                            .add_sized(
                                [size, size],
                                egui::Button::new(
                                    egui::RichText::new(text).size(size * 0.65).color(color),
                                )
                                .fill(fill),
                            )
                            .on_hover_text(label)
                            .clicked()
                        {
                            self.square(sq);
                        }
                    }
                    ui.end_row();
                }
            });
    }

    fn board(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.view.flat {
                self.flat_board(ui);
                return;
            }
            let size = ui.available_size();
            if size.x < 1.0 || size.y < 1.0 {
                return;
            }
            let pixels = ctx.pixels_per_point();
            self.renderer.resize(
                [(size.x * pixels) as u32, (size.y * pixels) as u32],
                &self.state,
            );
            let last = self.game.data.cursor.checked_sub(1).and_then(|ply| {
                self.game.states[ply]
                    .parse_move(&self.game.data.moves[ply])
                    .ok()
            });
            self.renderer
                .render(&self.game, &self.view, self.selected, self.hint, last, None);
            let response = ui.add(
                egui::Image::new((self.renderer.texture_id.unwrap(), size))
                    .sense(egui::Sense::click_and_drag()),
            );
            if response.clicked()
                && let Some(p) = response.interact_pointer_pos()
                && let Some(sq) = self.view.pick(p, response.rect, &self.game)
            {
                self.square(sq);
            }
            if response.dragged_by(egui::PointerButton::Secondary) {
                let delta = ctx.input(|i| i.pointer.delta());
                self.view.yaw += delta.x * 0.45;
                self.view.elevation = (self.view.elevation + delta.y * 0.25).clamp(20.0, 89.0);
            }
            if response.hovered() {
                self.view.distance = (self.view.distance
                    - ctx.input(|i| i.smooth_scroll_delta.y) * 0.008)
                    .clamp(9.0, 22.0);
            }
            // Draw notation in screen space, using the shared camera projection.
            for i in 0..8 {
                if let Some(p) = self
                    .view
                    .project(glam::Vec3::new(i as f32 - 3.5, 0.03, 4.24), response.rect)
                {
                    ui.painter().text(
                        p,
                        egui::Align2::CENTER_CENTER,
                        (b'a' + i as u8) as char,
                        egui::FontId::proportional(13.0),
                        Color32::LIGHT_GRAY,
                    );
                }
            }
        });
    }

    fn documents(&mut self, ctx: &egui::Context) {
        let mut open = self.documents;
        egui::Window::new("Import / export").open(&mut open).default_width(550.0).show(ctx, |ui| {
            ui.label("Paste PGN, native JSON or Apple XML below to import. Export, then copy the text to save a game.");
            ui.horizontal(|ui| {
                if ui.button("Import").clicked() {
                    match document::from_bytes(self.document_text.as_bytes()) {
                        Ok(game) => {
                            self.rules = game.data.rules;
                            self.game = game;
                            self.paused = true;
                            self.reset_selection();
                            self.message = "Game imported".into();
                        }
                        Err(error) => self.message = error,
                    }
                }
                if ui.button("Export PGN").clicked() { self.document_text = document::to_pgn(&self.game); }
                if ui.button("Export JSON").clicked() { self.document_text = serde_json::to_string_pretty(&self.game.data).unwrap(); }
                if ui.button("Export Apple XML").clicked() {
                    let mut bytes = Vec::new();
                    match document::to_apple(&self.game).to_writer_xml(&mut bytes) {
                        Ok(()) => self.document_text = String::from_utf8(bytes).unwrap(),
                        Err(error) => self.message = error.to_string(),
                    }
                }
            });
            ui.add(egui::TextEdit::multiline(&mut self.document_text).desired_rows(16).desired_width(f32::INFINITY));
        });
        self.documents = open;
    }
}

impl eframe::App for WebApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
        self.controls(ctx);
        self.documents(ctx);
        egui::TopBottomPanel::bottom("history").show(ctx, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (ply, san) in self.game.sans.iter().enumerate() {
                        ui.label(format!("{}. {san}", ply + 1));
                    }
                });
            });
        });
        self.board(ctx);
        if !self.paused
            && self.game.result() == "*"
            && self.game.data.cursor == self.game.data.moves.len()
            && self.game.data.computer[usize::from(self.game.board.pos.turn() == Color::Black)]
        {
            if self.search_pending {
                self.search_pending = false;
                self.analyze(false);
            } else {
                self.search_pending = true;
            }
            ctx.request_repaint();
        } else {
            self.search_pending = false;
        }
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(1)
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "game", &self.game.data);
        eframe::set_value(storage, "view", &self.view);
    }
}
