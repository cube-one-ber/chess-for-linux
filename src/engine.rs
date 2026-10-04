use crate::game::{Board, Rules};
use shakmaty::{Color, Move, Position, Role};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

#[derive(Clone, Debug)]
pub struct Analysis {
    pub best: Option<Move>,
    pub score: i32,
    pub depth: u8,
    pub nodes: u64,
}
struct Search {
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    nodes: u64,
    table: HashMap<String, (u8, i32, Option<Move>)>,
}
fn value(role: Role) -> i32 {
    match role {
        Role::Pawn => 100,
        Role::Knight => 320,
        Role::Bishop => 330,
        Role::Rook => 500,
        Role::Queen => 900,
        Role::King => 0,
    }
}
fn evaluate(b: &Board) -> i32 {
    let side = b.pos.turn();
    let mut score = 0;
    for (sq, piece) in b.pos.board().iter() {
        let rank = if piece.color == Color::White {
            u32::from(sq.rank())
        } else {
            7 - u32::from(sq.rank())
        };
        let center =
            7 - (2 * i32::from(sq.file()) - 7).abs() - (2 * i32::from(sq.rank()) - 7).abs();
        let bonus = match piece.role {
            Role::Pawn => rank as i32 * 7,
            Role::Knight | Role::Bishop => center * 5,
            _ => 0,
        };
        let v = value(piece.role) + bonus;
        score += if piece.color == side { v } else { -v };
    }
    if let Some(pockets) = b.pos.pockets() {
        for color in [Color::White, Color::Black] {
            for role in [
                Role::Pawn,
                Role::Knight,
                Role::Bishop,
                Role::Rook,
                Role::Queen,
            ] {
                let v = i32::from(pockets[color][role]) * value(role) * 4 / 5;
                score += if color == side { v } else { -v };
            }
        }
    }
    if matches!(b.rules, Rules::Suicide | Rules::Losers) {
        -score
    } else {
        score
    }
}
impl Search {
    fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) || Instant::now() >= self.deadline
    }
    fn negamax(
        &mut self,
        b: &Board,
        depth: u8,
        mut alpha: i32,
        beta: i32,
        ply: i32,
    ) -> Option<i32> {
        self.nodes += 1;
        if self.stopped() {
            return None;
        }
        if let Some(result) = b.terminal() {
            return Some(if result == "1/2-1/2" {
                0
            } else if (result == "1-0") == (b.pos.turn() == Color::White) {
                30000 - ply
            } else {
                -30000 + ply
            });
        }
        if depth == 0 {
            return Some(evaluate(b));
        }
        let key = b.fen();
        let tt = self.table.get(&key).copied();
        if let Some((d, score, _)) = tt
            && d >= depth
        {
            return Some(score);
        }
        let mut moves = b.moves();
        moves.sort_by_key(|m| {
            std::cmp::Reverse(
                (if Some(*m) == tt.and_then(|t| t.2) {
                    100000
                } else {
                    0
                }) + m.capture().map_or(0, |r| value(r) * 10 - value(m.role()))
                    + m.promotion().map_or(0, value),
            )
        });
        let mut best = None;
        let mut score = -31000;
        let mut cutoff = false;
        for m in moves {
            let mut n = b.clone();
            n.play(m);
            let s = -self.negamax(&n, depth - 1, -beta, -alpha, ply + 1)?;
            if s > score {
                score = s;
                best = Some(m);
            }
            alpha = alpha.max(s);
            if alpha >= beta {
                cutoff = true;
                break;
            }
        }
        // Cache only fully searched exact values; fail-low values are bounds.
        if !cutoff && score > -31000 && score >= alpha {
            self.table.insert(key, (depth, score, best));
        }
        Some(score)
    }
}
pub fn analyze(board: Board, seconds: f32, max_depth: u8, cancel: Arc<AtomicBool>) -> Analysis {
    let mut search = Search {
        deadline: Instant::now() + Duration::from_secs_f32(seconds.max(0.02)),
        cancel,
        nodes: 0,
        table: HashMap::new(),
    };
    let mut result = Analysis {
        best: board.moves().first().copied(),
        score: 0,
        depth: 0,
        nodes: 0,
    };
    for depth in 1..=max_depth {
        if search.stopped() {
            break;
        }
        let mut best = None;
        let mut best_score = -31000;
        let mut complete = true;
        let mut moves = board.moves();
        moves.sort_by_key(|m| {
            std::cmp::Reverse(if Some(*m) == result.best {
                100000
            } else {
                m.capture().map_or(0, value)
            })
        });
        for m in moves {
            let mut n = board.clone();
            n.play(m);
            match search.negamax(&n, depth - 1, -31000, -best_score, 1) {
                Some(score) => {
                    let score = -score;
                    if score > best_score {
                        best_score = score;
                        best = Some(m);
                    }
                }
                None => {
                    complete = false;
                    break;
                }
            }
        }
        if complete {
            result = Analysis {
                best,
                score: best_score,
                depth,
                nodes: search.nodes,
            };
        } else {
            break;
        }
    }
    result.nodes = search.nodes;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_mate() {
        let b = Board::from_fen(Rules::Standard, "7k/5Q2/6K1/8/8/8/8/8 w - - 0 1").unwrap();
        let a = analyze(b.clone(), 0.5, 3, Arc::new(AtomicBool::new(false)));
        let mut b = b;
        b.play(a.best.unwrap());
        assert_eq!(b.terminal().as_deref(), Some("1-0"));
    }
    #[test]
    fn all_variants_return_legal_move() {
        for rules in Rules::ALL {
            let b = Board::new(rules);
            let a = analyze(b.clone(), 0.03, 3, Arc::new(AtomicBool::new(false)));
            assert!(b.moves().contains(&a.best.unwrap()));
        }
    }
}
