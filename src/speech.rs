use crate::game::Game;
use shakmaty::{Move, Position, Role};
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
};
pub enum VoiceAction {
    Move(Move),
    Hint,
    Undo,
    LastMove,
}
pub fn parse(text: &str, g: &Game) -> Result<VoiceAction, String> {
    let text = text.to_lowercase();
    let text = text.trim();
    match text {
        "hint" | "show hint" => return Ok(VoiceAction::Hint),
        "undo" | "take back move" => return Ok(VoiceAction::Undo),
        "show last move" => return Ok(VoiceAction::LastMove),
        _ => (),
    }
    if let Ok(m) = g.board.parse_move(text) {
        return Ok(VoiceAction::Move(m));
    }
    let text = text
        .replace("castle kingside", "O-O")
        .replace("castle queenside", "O-O-O");
    if let Ok(m) = g.board.parse_move(&text) {
        return Ok(VoiceAction::Move(m));
    }
    let mut normalized = text.clone();
    for (a, b) in [
        ("one", "1"),
        ("two", "2"),
        ("three", "3"),
        ("four", "4"),
        ("five", "5"),
        ("six", "6"),
        ("seven", "7"),
        ("eight", "8"),
    ] {
        normalized = normalized.replace(a, b);
    }
    let words: Vec<_> = normalized.split_whitespace().collect();
    let mut squares = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if words[i].len() == 2 && words[i].parse::<shakmaty::Square>().is_ok() {
            squares.push(words[i].to_string());
        } else if words[i].len() == 1
            && matches!(words[i], "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h")
            && i + 1 < words.len()
            && words[i + 1].len() == 1
            && words[i + 1].chars().all(|c| ('1'..='8').contains(&c))
        {
            squares.push(format!("{}{}", words[i], words[i + 1]));
            i += 1;
        }
        i += 1;
    }
    let role = [
        ("pawn", Role::Pawn),
        ("knight", Role::Knight),
        ("bishop", Role::Bishop),
        ("rook", Role::Rook),
        ("queen", Role::Queen),
        ("king", Role::King),
    ]
    .into_iter()
    .find(|(name, _)| text.contains(name))
    .map(|(_, role)| role);
    let promotion = text.split("promote to ").nth(1).and_then(|s| {
        [
            ("queen", Role::Queen),
            ("rook", Role::Rook),
            ("bishop", Role::Bishop),
            ("knight", Role::Knight),
            ("king", Role::King),
        ]
        .into_iter()
        .find(|(name, _)| s.contains(name))
        .map(|(_, r)| r)
    });
    let candidates: Vec<_> = g
        .board
        .moves()
        .into_iter()
        .filter(|m| {
            if let Some(role) = role
                && m.role() != role
            {
                return false;
            }
            if let Some(r) = promotion
                && m.promotion() != Some(r)
            {
                return false;
            }
            if text.contains("drop") && m.from().is_some() {
                return false;
            }
            match squares.as_slice() {
                [to] => m.to().to_string() == *to,
                [from, to] => {
                    m.from().is_some_and(|s| s.to_string() == *from)
                        && crate::render::destination(*m).to_string() == *to
                }
                _ => false,
            }
        })
        .collect();
    match candidates.as_slice() {
        [m] => Ok(VoiceAction::Move(*m)),
        [] => Err("No legal move matches that command".into()),
        _ => {
            Err("That command is ambiguous; include the starting square or promotion piece".into())
        }
    }
}
pub fn speak(g: &Game, m: Move, voice: &str) -> Result<(), String> {
    let side = if g.board.pos.turn() == shakmaty::Color::White {
        "Black"
    } else {
        "White"
    };
    let role = format!("{:?}", m.role());
    let text = if let Some(from) = m.from() {
        format!(
            "{side} {role} from {from} to {}",
            crate::render::destination(m)
        )
    } else {
        format!("{side} drops {role} on {}", m.to())
    };
    let mut child = Command::new("espeak")
        .args(["-v", voice, &text])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start espeak: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
pub struct Listener {
    child: Child,
    pub events: Receiver<Result<String, String>>,
}
impl Listener {
    pub fn start(model: &Path) -> Result<Self, String> {
        let mut child = Command::new("python3")
            .arg("-u")
            .arg("-c")
            .arg(include_str!("../scripts/listen.py"))
            .arg(model)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Speech recognizer has no output")?;
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(s) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                            if let Some(error) = v["error"].as_str() {
                                let _ = tx.send(Err(error.into()));
                            } else if let Some(text) = v["text"].as_str()
                                && !text.is_empty()
                            {
                                let _ = tx.send(Ok(text.into()));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                        break;
                    }
                }
            }
            let _ = tx.send(Err("Speech recognizer stopped".into()));
        });
        Ok(Self { child, events })
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Rules;
    #[test]
    fn voice_moves() {
        let g = Game::new(Rules::Standard);
        for s in [
            "move pawn from e two to e four",
            "pawn to e four",
            "e2e4",
            "knight to f three",
        ] {
            assert!(matches!(parse(s, &g), Ok(VoiceAction::Move(_))), "{s}");
        }
        assert!(parse("pawn to e five", &g).is_err());
    }
}
