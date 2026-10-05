use crate::{
    engine, font,
    game::{Game, Rules},
};
use shakmaty::{Color, Move, Position, Role, Square};
use std::sync::{Arc, atomic::AtomicBool};

pub struct Ui {
    pub game: Game,
    pub input: String,
    pub message: String,
    selected: Option<Square>,
    promotion: Vec<Move>,
    computer: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            game: Game::new(Rules::Standard),
            input: String::new(),
            message: "Select a piece, then a square".into(),
            selected: None,
            promotion: vec![],
            computer: false,
        }
    }
}

pub fn board_geometry(width: i32, height: i32) -> (i32, i32, i32) {
    let cell = ((width - 32).min(height - 192) / 8).max(16);
    ((width - cell * 8) / 2, 88, cell)
}

impl Ui {
    fn play(&mut self, m: Move) {
        match self.game.push(m) {
            Ok(()) => {
                self.selected = None;
                self.promotion.clear();
                self.message = self.game.status();
                println!("position: {}", self.game.board.fen());
                if self.computer
                    && self.game.board.pos.turn() == Color::Black
                    && self.game.result() == "*"
                {
                    self.computer_move();
                }
            }
            Err(error) => self.message = error,
        }
    }

    fn computer_move(&mut self) {
        let analysis = engine::analyze(
            self.game.board.clone(),
            0.15,
            3,
            Arc::new(AtomicBool::new(false)),
        );
        if let Some(m) = analysis.best {
            match self.game.push(m) {
                Ok(()) => {
                    self.selected = None;
                    self.message = self.game.status();
                    println!("position: {}", self.game.board.fen());
                }
                Err(error) => self.message = error,
            }
        }
    }

    fn new_game(&mut self, rules: Rules) {
        self.game = Game::new(rules);
        self.game.data.computer = [false, self.computer];
        self.selected = None;
        self.promotion.clear();
        self.input.clear();
        self.message = "Select a piece, then a square".into();
    }

    pub fn click(&mut self, x: i32, y: i32, width: i32, height: i32) {
        if (40..72).contains(&y) && (8..width - 8).contains(&x) {
            let button = (x - 8) * 5 / (width - 16);
            match button {
                0 => self.new_game(self.game.data.rules),
                1 => {
                    let plies = if self.computer { 2 } else { 1 };
                    self.game.seek(self.game.data.cursor.saturating_sub(plies));
                    self.selected = None;
                    self.promotion.clear();
                    self.message = self.game.status();
                }
                2 => {
                    self.game.seek(self.game.data.cursor + 1);
                    self.selected = None;
                    self.promotion.clear();
                    self.message = self.game.status();
                }
                3 => {
                    self.computer = !self.computer;
                    self.game.data.computer = [false, self.computer];
                    if self.computer
                        && self.game.board.pos.turn() == Color::Black
                        && self.game.result() == "*"
                    {
                        self.computer_move();
                    }
                }
                4 => {
                    let index = Rules::ALL
                        .iter()
                        .position(|&r| r == self.game.data.rules)
                        .unwrap();
                    self.new_game(Rules::ALL[(index + 1) % Rules::ALL.len()]);
                }
                _ => {}
            }
            return;
        }
        let (bx, by, cell) = board_geometry(width, height);
        if !self.promotion.is_empty() {
            let py = by + cell * 8 + 20;
            if (py..py + 24).contains(&y) && x >= bx {
                let index = ((x - bx) / 48) as usize;
                if let Some(m) = self.promotion.get(index).copied() {
                    self.play(m);
                }
            }
            return;
        }
        if x < bx || y < by || x >= bx + 8 * cell || y >= by + 8 * cell {
            return;
        }
        let square = Square::new(((7 - (y - by) / cell) * 8 + (x - bx) / cell) as u32);
        if let Some(from) = self.selected {
            let candidates: Vec<_> = self
                .game
                .board
                .moves()
                .into_iter()
                .filter(|m| {
                    // Castling is shown as king-to-destination, rather than king-to-rook.
                    let destination = match m {
                        Move::Castle { king, rook } => Square::from_coords(
                            if rook.file() > king.file() {
                                shakmaty::File::G
                            } else {
                                shakmaty::File::C
                            },
                            king.rank(),
                        ),
                        _ => m.to(),
                    };
                    m.from() == Some(from) && destination == square
                })
                .collect();
            if candidates.len() > 1 {
                self.promotion = candidates;
                self.message = "Choose promotion below the board".into();
                return;
            }
            if let Some(m) = candidates.first() {
                self.play(*m);
                return;
            }
        }
        self.selected = self
            .game
            .at(square)
            .filter(|piece| piece.color == self.game.board.pos.turn())
            .map(|_| square);
    }

    pub fn submit(&mut self) {
        if self.input.trim().is_empty() {
            return;
        }
        match self.game.board.parse_move(&self.input) {
            Ok(m) => {
                self.input.clear();
                self.play(m);
            }
            Err(error) => self.message = error,
        }
    }

    pub fn cancel(&mut self) {
        self.selected = None;
        self.promotion.clear();
        self.input.clear();
        self.message = self.game.status();
    }

    pub fn paint(&self, pixels: &mut [u8], width: i32, height: i32) {
        let mut canvas = Canvas {
            pixels,
            width,
            height,
        };
        canvas.rect(0, 0, width, height, 0xff18212b);
        canvas.text(
            12,
            12,
            &format!("CHESS / {}", self.game.data.rules.name()),
            0xfff0e5d0,
            2,
        );
        let button_width = (width - 16) / 5;
        for (i, label) in [
            "New",
            "Undo",
            "Redo",
            if self.computer { "AI on" } else { "AI off" },
            "Variant",
        ]
        .iter()
        .enumerate()
        {
            let x = 8 + i as i32 * button_width;
            canvas.rect(x, 40, button_width - 3, 32, 0xff334353);
            canvas.text(x + 6, 51, label, 0xfff0e5d0, 1);
        }
        let (bx, by, cell) = board_geometry(width, height);
        let legal: Vec<_> = self
            .game
            .board
            .moves()
            .into_iter()
            .filter(|m| m.from() == self.selected && self.selected.is_some())
            .collect();
        for rank in 0..8 {
            for file in 0..8 {
                let square = Square::new((rank * 8 + file) as u32);
                let x = bx + file * cell;
                let y = by + (7 - rank) * cell;
                let color = if self.selected == Some(square) {
                    0xffcfaa46
                } else if (rank + file) % 2 == 0 {
                    0xff799083
                } else {
                    0xffe2dec9
                };
                canvas.rect(x, y, cell, cell, color);
                if legal.iter().any(|m| m.to() == square) {
                    canvas.rect(x + cell / 2 - 3, y + cell / 2 - 3, 6, 6, 0xff426859);
                }
                if let Some(piece) = self.game.at(square) {
                    let scale = (cell / 18).max(1);
                    let (fill, edge) = if piece.color == Color::White {
                        (0xfffff9e8, 0xff3c4a48)
                    } else {
                        (0xff263a42, 0xffd5dfd1)
                    };
                    canvas.piece(
                        x + (cell - 12 * scale) / 2,
                        y + (cell - 12 * scale) / 2,
                        piece.role,
                        scale,
                        fill,
                        edge,
                    );
                }
                if file == 0 {
                    canvas.text(x + 2, y + 2, &(rank + 1).to_string(), 0xff364541, 1);
                }
                if rank == 0 {
                    canvas.text(
                        x + cell - 8,
                        y + cell - 10,
                        &((b'a' + file as u8) as char).to_string(),
                        0xff364541,
                        1,
                    );
                }
            }
        }
        let y = by + cell * 8 + 8;
        if self.promotion.is_empty() {
            canvas.text(bx, y, &self.game.status().replace('·', "/"), 0xfff0e5d0, 1);
            canvas.text(bx, y + 16, &self.message.replace('·', "/"), 0xff9cbdaf, 1);
            canvas.text(
                bx,
                y + 32,
                &format!("Move: {}_  [Enter]", self.input),
                0xfff0e5d0,
                1,
            );
            let history = self.game.sans[..self.game.data.cursor]
                .iter()
                .rev()
                .take(6)
                .cloned()
                .collect::<Vec<_>>();
            canvas.text(
                bx,
                y + 48,
                &history.into_iter().rev().collect::<Vec<_>>().join(" "),
                0xff9cbdaf,
                1,
            );
            if let Some(pockets) = self.game.board.pos.pockets() {
                let pocket = |color: Color| {
                    [
                        Role::Pawn,
                        Role::Knight,
                        Role::Bishop,
                        Role::Rook,
                        Role::Queen,
                    ]
                    .into_iter()
                    .map(|r| format!("{}:{}", r.upper_char(), pockets[color][r]))
                    .collect::<Vec<_>>()
                    .join(" ")
                };
                canvas.text(
                    bx,
                    y + 64,
                    &format!("Pocket {}", pocket(self.game.board.pos.turn())),
                    0xfff0e5d0,
                    1,
                );
            }
            canvas.text(
                bx,
                height - 14,
                "Type UCI/SAN; drops N@e4; Esc clears",
                0xff9cbdaf,
                1,
            );
        } else {
            canvas.text(bx, y, "Promotion: click a piece", 0xfff0e5d0, 1);
            for (index, m) in self.promotion.iter().enumerate() {
                let x = bx + index as i32 * 48;
                canvas.rect(x, y + 12, 44, 24, 0xff334353);
                canvas.text(
                    x + 16,
                    y + 20,
                    &m.promotion().unwrap().upper_char().to_string(),
                    0xfff0e5d0,
                    1,
                );
            }
        }
    }
}

struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: i32,
    height: i32,
}
impl Canvas<'_> {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        font::fill_rect(
            self.pixels,
            self.width * 4,
            self.width,
            self.height,
            x,
            y,
            w,
            h,
            color,
        );
    }
    fn text(&mut self, x: i32, y: i32, text: &str, color: u32, scale: i32) {
        if scale == 1 {
            font::draw_text(
                self.pixels,
                self.width * 4,
                self.width,
                self.height,
                x,
                y,
                text,
                color,
            );
        } else {
            let w = text.len() as i32 * 6;
            let mut small = vec![0; (w * 8 * 4) as usize];
            font::draw_text(&mut small, w * 4, w, 8, 0, 0, text, color);
            for py in 0..8 {
                for px in 0..w {
                    let i = ((py * w + px) * 4) as usize;
                    if small[i + 3] != 0 {
                        self.rect(x + px * scale, y + py * scale, scale, scale, color);
                    }
                }
            }
        }
    }
    fn piece(&mut self, x: i32, y: i32, role: Role, scale: i32, fill: u32, edge: u32) {
        let sprite: [u16; 12] = match role {
            Role::Pawn => [
                0, 0x060, 0x0f0, 0x0f0, 0x060, 0x060, 0x0f0, 0x1f8, 0x1f8, 0x3fc, 0x3fc, 0,
            ],
            Role::Knight => [
                0, 0x040, 0x1e0, 0x3f0, 0x7b8, 0x778, 0x078, 0x0f0, 0x1f0, 0x3fc, 0x3fc, 0,
            ],
            Role::Bishop => [
                0, 0x060, 0x0f0, 0x1d8, 0x1b8, 0x0f0, 0x060, 0x0f0, 0x1f8, 0x3fc, 0x3fc, 0,
            ],
            Role::Rook => [
                0, 0x318, 0x3fc, 0x3fc, 0x1f8, 0x0f0, 0x0f0, 0x0f0, 0x1f8, 0x3fc, 0x3fc, 0,
            ],
            Role::Queen => [
                0, 0x492, 0x7fe, 0x3fc, 0x1f8, 0x0f0, 0x0f0, 0x0f0, 0x1f8, 0x3fc, 0x3fc, 0,
            ],
            Role::King => [
                0x060, 0x0f0, 0x060, 0x1f8, 0x1f8, 0x0f0, 0x060, 0x0f0, 0x1f8, 0x3fc, 0x3fc, 0,
            ],
        };
        for row in 0..12 {
            for col in 0..12 {
                if sprite[row] & (1 << (11 - col)) == 0 {
                    continue;
                }
                let occupied = |r: i32, c: i32| {
                    (0..12).contains(&r)
                        && (0..12).contains(&c)
                        && sprite[r as usize] & (1 << (11 - c)) != 0
                };
                let border = !occupied(row as i32 - 1, col)
                    || !occupied(row as i32 + 1, col)
                    || !occupied(row as i32, col - 1)
                    || !occupied(row as i32, col + 1);
                self.rect(
                    x + col * scale,
                    y + row as i32 * scale,
                    scale,
                    scale,
                    if border { edge } else { fill },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clicks_play_legal_moves_and_reject_illegal_moves() {
        let mut ui = Ui::default();
        let (x, y, cell) = board_geometry(640, 800);
        let click = |ui: &mut Ui, file, rank| {
            ui.click(
                x + file * cell + cell / 2,
                y + (7 - rank) * cell + cell / 2,
                640,
                800,
            )
        };
        click(&mut ui, 4, 1);
        click(&mut ui, 4, 4);
        assert_eq!(ui.game.data.cursor, 0);
        click(&mut ui, 4, 1);
        click(&mut ui, 4, 3);
        assert_eq!(ui.game.data.moves, ["e2e4"]);
    }
    #[test]
    fn typed_moves_and_crazyhouse_drops() {
        let mut ui = Ui::default();
        ui.new_game(Rules::Crazyhouse);
        for text in ["e2e4", "d7d5", "e4d5", "d8d5", "P@e4"] {
            ui.input = text.into();
            ui.submit();
        }
        assert_eq!(ui.game.data.cursor, 5);
        assert_eq!(ui.game.at(Square::E4).unwrap().role, Role::Pawn);
    }
    #[test]
    fn promotion_requires_a_choice() {
        let mut ui = Ui::default();
        ui.game.set_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let (x, y, c) = board_geometry(640, 800);
        ui.click(x + c / 2, y + c + c / 2, 640, 800);
        ui.click(x + c / 2, y + c / 2, 640, 800);
        assert_eq!(ui.promotion.len(), 4);
        assert_eq!(ui.game.data.cursor, 0);
        ui.click(x + 16, y + c * 8 + 24, 640, 800);
        assert_eq!(ui.game.data.cursor, 1);
    }
}
