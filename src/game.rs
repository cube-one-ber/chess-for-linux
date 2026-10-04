use serde::{Deserialize, Serialize};
use shakmaty::{
    CastlingMode, Color, EnPassantMode, Move, Position, Role, Square,
    fen::Fen,
    san::SanPlus,
    uci::UciMove,
    variant::{Variant, VariantPosition},
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rules {
    #[default]
    #[serde(rename = "standard", alias = "Standard", alias = "normal")]
    Standard,
    #[serde(rename = "crazyhouse", alias = "Crazyhouse")]
    Crazyhouse,
    #[serde(rename = "suicide", alias = "Suicide", alias = "antichess")]
    Suicide,
    #[serde(rename = "losers", alias = "Losers")]
    Losers,
}
impl Rules {
    pub const ALL: [Self; 4] = [
        Self::Standard,
        Self::Crazyhouse,
        Self::Suicide,
        Self::Losers,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Crazyhouse => "Crazyhouse",
            Self::Suicide => "Suicide",
            Self::Losers => "Losers",
        }
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "standard" | "normal" | "chess" => Ok(Self::Standard),
            "crazyhouse" => Ok(Self::Crazyhouse),
            "suicide" | "antichess" => Ok(Self::Suicide),
            "losers" => Ok(Self::Losers),
            _ => Err(format!("Unsupported variant: {s}")),
        }
    }
    pub fn variant(self) -> Variant {
        match self {
            Self::Crazyhouse => Variant::Crazyhouse,
            Self::Suicide => Variant::Antichess,
            _ => Variant::Chess,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Board {
    pub pos: VariantPosition,
    pub rules: Rules,
}
impl Board {
    pub fn new(rules: Rules) -> Self {
        Self {
            pos: VariantPosition::new(rules.variant()),
            rules,
        }
    }
    pub fn from_fen(rules: Rules, text: &str) -> Result<Self, String> {
        let fen: Fen = text.parse().map_err(|e| format!("Invalid FEN: {e}"))?;
        let pos =
            VariantPosition::from_setup(rules.variant(), fen.into_setup(), CastlingMode::Standard)
                .map_err(|e| format!("Invalid position: {e}"))?;
        Ok(Self { pos, rules })
    }
    pub fn fen(&self) -> String {
        Fen::from_position(&self.pos, EnPassantMode::Always).to_string()
    }
    pub fn key(&self) -> String {
        Fen::from_position(&self.pos, EnPassantMode::Legal)
            .to_string()
            .split_whitespace()
            .take(4)
            .collect::<Vec<_>>()
            .join(" ")
    }
    pub fn moves(&self) -> Vec<Move> {
        let mut moves: Vec<_> = self.pos.legal_moves().into_iter().collect();
        if self.rules == Rules::Losers && moves.iter().any(|m| m.is_capture()) {
            moves.retain(|m| m.is_capture());
        }
        moves
    }
    pub fn parse_move(&self, text: &str) -> Result<Move, String> {
        let text = text.trim();
        let m = if let Ok(uci) = text.parse::<UciMove>() {
            uci.to_move(&self.pos).map_err(|e| e.to_string())?
        } else {
            text.parse::<SanPlus>()
                .map_err(|e| e.to_string())?
                .san
                .to_move(&self.pos)
                .map_err(|e| e.to_string())?
        };
        if self.moves().contains(&m) {
            Ok(m)
        } else {
            Err("A capture is compulsory in this variant".into())
        }
    }
    pub fn play(&mut self, m: Move) {
        self.pos.play_unchecked(m);
    }
    pub fn terminal(&self) -> Option<String> {
        let side = self.pos.turn();
        let win = |color: Color| {
            if color == Color::White {
                "1-0".into()
            } else {
                "0-1".into()
            }
        };
        if matches!(self.rules, Rules::Losers | Rules::Suicide) {
            let count = self.pos.board().by_color(side).count();
            if count == 0 || (self.rules == Rules::Losers && count == 1) || self.moves().is_empty()
            {
                return Some(win(side));
            }
            if self.pos.halfmoves() >= 100 {
                return Some("1/2-1/2".into());
            }
            return None;
        }
        let outcome = self.pos.outcome();
        if outcome.is_known() {
            return Some(outcome.to_string());
        }
        if self.pos.halfmoves() >= 100 {
            return Some("1/2-1/2".into());
        }
        None
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedGame {
    pub version: u32,
    pub rules: Rules,
    pub initial_fen: String,
    pub moves: Vec<String>,
    pub cursor: usize,
    pub headers: BTreeMap<String, String>,
    pub comments: BTreeMap<usize, String>,
    #[serde(default)]
    pub variations: Vec<Vec<String>>,
    pub result: String,
    #[serde(default)]
    pub computer: [bool; 2],
}
#[derive(Clone)]
pub struct Game {
    pub board: Board,
    pub data: SavedGame,
    pub sans: Vec<String>,
    pub states: Vec<Board>,
}
impl Game {
    pub fn new(rules: Rules) -> Self {
        let board = Board::new(rules);
        let mut headers = BTreeMap::new();
        for (k, v) in [
            ("Event", "Casual game"),
            ("Site", "Linux"),
            ("Date", "????.??.??"),
            ("Round", "-"),
            ("White", "White"),
            ("Black", "Black"),
        ] {
            headers.insert(k.into(), v.into());
        }
        let data = SavedGame {
            version: 1,
            rules,
            initial_fen: board.fen(),
            moves: vec![],
            cursor: 0,
            headers,
            comments: BTreeMap::new(),
            variations: vec![],
            result: "*".into(),
            computer: [false, true],
        };
        Self {
            states: vec![board.clone()],
            board,
            data,
            sans: vec![],
        }
    }
    pub fn load(data: SavedGame) -> Result<Self, String> {
        if !matches!(data.result.as_str(), "*" | "1-0" | "0-1" | "1/2-1/2") {
            return Err("Invalid saved result".into());
        }
        if data.version != 1 {
            return Err("Unsupported document version".into());
        }
        if data.cursor > data.moves.len() {
            return Err("Document cursor is outside move history".into());
        }
        let board = Board::from_fen(data.rules, &data.initial_fen)?;
        let mut g = Self {
            board: board.clone(),
            states: vec![board],
            sans: vec![],
            data: data.clone(),
        };
        g.data.moves.clear();
        g.data.comments.clear();
        g.data.cursor = 0;
        g.data.result = "*".into();
        for text in &data.moves {
            let m = g.board.parse_move(text)?;
            g.push(m)?;
        }
        g.data = data;
        g.seek(g.data.cursor);
        Ok(g)
    }
    pub fn push(&mut self, m: Move) -> Result<(), String> {
        if self.result() != "*" {
            return Err("The game has ended".into());
        }
        if !self.board.moves().contains(&m) {
            return Err("Illegal move".into());
        }
        if self.data.cursor < self.data.moves.len() {
            self.data.variations.push(self.data.moves.clone());
            self.data.moves.truncate(self.data.cursor);
            self.sans.truncate(self.data.cursor);
            self.states.truncate(self.data.cursor + 1);
            self.data.comments.retain(|&ply, _| ply <= self.data.cursor);
        }
        self.sans
            .push(SanPlus::from_move(self.board.pos.clone(), m).to_string());
        self.data
            .moves
            .push(UciMove::from_move(m, CastlingMode::Standard).to_string());
        self.board.play(m);
        self.states.push(self.board.clone());
        self.data.cursor += 1;
        self.data.result = self.computed_result();
        Ok(())
    }
    fn computed_result(&self) -> String {
        if let Some(r) = self.board.terminal() {
            return r;
        }
        let key = self.board.key();
        if self.states[..=self.data.cursor]
            .iter()
            .filter(|s| s.key() == key)
            .count()
            >= 3
        {
            return "1/2-1/2".into();
        }
        "*".into()
    }
    pub fn result(&self) -> String {
        if self.data.cursor == self.data.moves.len() && self.data.result != "*" {
            self.data.result.clone()
        } else {
            self.computed_result()
        }
    }
    pub fn seek(&mut self, cursor: usize) {
        self.data.cursor = cursor.min(self.data.moves.len());
        self.board = self.states[self.data.cursor].clone();
    }
    pub fn status(&self) -> String {
        match self.result().as_str() {
            "1-0" => "White wins".into(),
            "0-1" => "Black wins".into(),
            "1/2-1/2" => "Draw".into(),
            _ => format!(
                "{} to move{}",
                if self.board.pos.turn() == Color::White {
                    "White"
                } else {
                    "Black"
                },
                if self.board.pos.is_check() && self.data.rules != Rules::Suicide {
                    " · Check"
                } else {
                    ""
                }
            ),
        }
    }
    pub fn set_fen(&mut self, text: &str) -> Result<(), String> {
        let board = Board::from_fen(self.data.rules, text)?;
        self.board = board.clone();
        self.states = vec![board];
        self.sans.clear();
        self.data.initial_fen = text.into();
        self.data.moves.clear();
        self.data.comments.clear();
        self.data.variations.clear();
        self.data.cursor = 0;
        self.data.result = "*".into();
        Ok(())
    }
    pub fn at(&self, sq: Square) -> Option<shakmaty::Piece> {
        self.board.pos.board().piece_at(sq)
    }
    pub fn promotion_roles(&self) -> Vec<Role> {
        let mut v = vec![Role::Queen, Role::Rook, Role::Bishop, Role::Knight];
        if self.data.rules == Rules::Suicide {
            v.push(Role::King);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn perft(b: &Board, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        b.moves()
            .into_iter()
            .map(|m| {
                let mut n = b.clone();
                n.play(m);
                perft(&n, depth - 1)
            })
            .sum()
    }
    #[test]
    fn standard_perft() {
        let b = Board::new(Rules::Standard);
        assert_eq!(perft(&b, 3), 8902);
    }
    #[test]
    fn losers_compulsory_legal_capture() {
        let b = Board::from_fen(Rules::Losers, "4k3/8/8/8/8/8/p7/R3K3 w Q - 0 1").unwrap();
        assert!(b.moves().iter().all(|m| m.is_capture()));
        assert!(b.parse_move("e1f1").is_err());
    }
    #[test]
    fn castling_en_passant_promotion() {
        for (fen, m) in [
            ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1g1"),
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"),
            ("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8n"),
        ] {
            let mut b = Board::from_fen(Rules::Standard, fen).unwrap();
            let m = b.parse_move(m).unwrap();
            b.play(m);
            assert_eq!(b.pos.turn(), Color::Black);
        }
    }
    #[test]
    fn crazyhouse_drop_and_promoted_capture() {
        let mut b = Board::from_fen(Rules::Crazyhouse, "4k3/8/8/8/8/8/8/4K3[N] w - - 0 1").unwrap();
        let m = b.parse_move("N@e4").unwrap();
        b.play(m);
        assert_eq!(
            b.pos.board().piece_at(Square::E4).unwrap().role,
            Role::Knight
        );
    }
    #[test]
    fn suicide_king_capture_and_promotion() {
        let b = Board::from_fen(Rules::Suicide, "k7/P7/8/8/8/8/8/7K w - - 0 1").unwrap();
        assert!(!b.pos.is_check());
        assert!(b.parse_move("a7b8k").is_err());
        let b = Board::from_fen(Rules::Suicide, "8/P7/8/8/8/8/8/7k w - - 0 1").unwrap();
        assert!(b.parse_move("a7a8k").is_ok());
    }
    #[test]
    fn checkmate_repetition_and_history() {
        let mut g = Game::new(Rules::Standard);
        for m in ["f2f3", "e7e5", "g2g4", "d8h4"] {
            let m = g.board.parse_move(m).unwrap();
            g.push(m).unwrap();
        }
        assert_eq!(g.result(), "0-1");
        g.seek(2);
        assert_eq!(g.result(), "*");
        g.seek(4);
        assert_eq!(g.result(), "0-1");
        let copy = Game::load(g.data.clone()).unwrap();
        assert_eq!(copy.board.fen(), g.board.fen());
        let mut g = Game::new(Rules::Standard);
        for m in [
            "g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8",
        ] {
            let m = g.board.parse_move(m).unwrap();
            g.push(m).unwrap();
        }
        assert_eq!(g.result(), "1/2-1/2");
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn losers_pinned_capture_is_not_compulsory() {
        let b = Board::from_fen(Rules::Losers, "k3r3/8/8/8/8/3p4/4B3/4K3 w - - 0 1").unwrap();
        assert!(b.moves().iter().all(|m| !m.is_capture()));
        assert!(b.parse_move("e2d3").is_err());
        assert!(b.parse_move("e1f1").is_ok());
    }
    #[test]
    fn losers_checkmate_and_stalemate_win() {
        for fen in [
            "7k/6Q1/5K2/8/8/8/8/8 b - - 0 1",
            "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1",
        ] {
            let b = Board::from_fen(Rules::Losers, fen).unwrap();
            assert_eq!(b.terminal().as_deref(), Some("0-1"));
        }
    }
    #[test]
    fn crazyhouse_promoted_capture_returns_a_pawn() {
        let mut b =
            Board::from_fen(Rules::Crazyhouse, "4k3/8/8/8/8/8/q~7/R3K3[] w - - 0 1").unwrap();
        let m = b.parse_move("a1a2").unwrap();
        b.play(m);
        assert_eq!(b.pos.pockets().unwrap()[Color::White][Role::Pawn], 1);
        assert_eq!(b.pos.pockets().unwrap()[Color::White][Role::Queen], 0);
    }
    #[test]
    fn branches_and_adjudication_survive_recovery() {
        let mut g = Game::new(Rules::Standard);
        for text in ["e2e4", "e7e5", "g1f3"] {
            let m = g.board.parse_move(text).unwrap();
            g.push(m).unwrap();
        }
        g.seek(1);
        let m = g.board.parse_move("c7c5").unwrap();
        g.push(m).unwrap();
        assert_eq!(g.data.variations[0], vec!["e2e4", "e7e5", "g1f3"]);
        g.data.result = "0-1".into();
        g.data.comments.insert(1, "Opening".into());
        let json = serde_json::to_vec(&g.data).unwrap();
        let h = Game::load(serde_json::from_slice(&json).unwrap()).unwrap();
        assert_eq!(h.result(), "0-1");
        assert_eq!(h.data.comments[&1], "Opening");
        assert_eq!(h.data.variations.len(), 1);
        let mut bad = h.data;
        bad.cursor = 99;
        assert!(Game::load(bad).is_err());
    }
}
