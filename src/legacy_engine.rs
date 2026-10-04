//! Optional CECP process adapter keeps the original Sjeng search, books and learning available.
use crate::{
    engine::Analysis,
    game::{Game, Rules},
};
use shakmaty::{Color, Position, Role};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
fn engine_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("chess-linux/sjeng")
}
pub fn analyze(
    game: Game,
    path: &Path,
    seconds: f32,
    depth: u8,
    cancel: Arc<AtomicBool>,
) -> Result<Analysis, String> {
    let dir = engine_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (name, bytes) in [
        (
            "normal.opn",
            include_bytes!("../sjeng/books/normal.opn").as_slice(),
        ),
        (
            "suicide.opn",
            include_bytes!("../sjeng/books/suicide.opn").as_slice(),
        ),
        (
            "losers.opn",
            include_bytes!("../sjeng/books/losers.opn").as_slice(),
        ),
        (
            "bug.opn",
            include_bytes!("../sjeng/books/bug.opn").as_slice(),
        ),
        ("sjeng.rc", include_bytes!("../sjeng/sjeng.rc").as_slice()),
    ] {
        if !dir.join(name).exists() {
            std::fs::write(dir.join(name), bytes).map_err(|e| e.to_string())?;
        }
    }
    let path = path
        .canonicalize()
        .map_err(|e| format!("Sjeng executable: {e}"))?;
    let mut child = Command::new(path)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let stdout = child.stdout.take().ok_or("No engine output")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let input = child.stdin.as_mut().ok_or("No engine input")?;
        writeln!(
            input,
            "xboard\nnew\nforce\nvariant {}\neasy\npost",
            if game.data.rules == Rules::Standard {
                "normal"
            } else {
                match game.data.rules {
                    Rules::Crazyhouse => "crazyhouse",
                    Rules::Suicide => "suicide",
                    Rules::Losers => "losers",
                    _ => "normal",
                }
            }
        )
        .map_err(|e| e.to_string())?;
        if game.data.initial_fen != crate::game::Board::new(game.data.rules).fen() {
            if game.data.initial_fen.contains('~') {
                return Err("Original Sjeng cannot load promoted markers in a custom starting FEN; use the Rust engine".into());
            }
            let initial = &game.states[0];
            let mut fields: Vec<String> = initial
                .fen()
                .split_whitespace()
                .map(str::to_string)
                .collect();
            if let Some(index) = fields[0].find('[') {
                fields[0].truncate(index);
            }
            writeln!(input, "setboard {}", fields.join(" ")).map_err(|e| e.to_string())?;
            if let Some(pockets) = initial.pos.pockets() {
                let holding = |color: Color| {
                    let mut s = String::new();
                    for r in [
                        Role::Pawn,
                        Role::Knight,
                        Role::Bishop,
                        Role::Rook,
                        Role::Queen,
                    ] {
                        for _ in 0..pockets[color][r] {
                            s.push(r.upper_char());
                        }
                    }
                    s
                };
                writeln!(
                    input,
                    "holding [{}] [{}]",
                    holding(Color::White),
                    holding(Color::Black)
                )
                .map_err(|e| e.to_string())?;
            }
        }
        for m in &game.data.moves[..game.data.cursor] {
            writeln!(input, "{m}").map_err(|e| e.to_string())?;
        }
        writeln!(
            input,
            "st {}\nsd {depth}\ngo",
            seconds.ceil().max(1.0) as u32
        )
        .map_err(|e| e.to_string())?;
        input.flush().map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs_f32(seconds.ceil().max(1.0) + 8.0);
        let mut analysis = Analysis {
            best: None,
            score: 0,
            depth: 0,
            nodes: 0,
        };
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Search cancelled".into());
            }
            if Instant::now() > deadline {
                return Err("Original engine timed out".into());
            }
            match rx.recv_timeout(Duration::from_millis(25)) {
                Ok(line) => {
                    if let Some(m) = line.strip_prefix("move ") {
                        analysis.best = Some(game.board.parse_move(m.trim())?);
                        return Ok(analysis);
                    }
                    if line.contains("Illegal move") || line.contains("Error") {
                        return Err(format!("Original engine: {line}"));
                    }
                    let cols: Vec<_> = line.split_whitespace().collect();
                    if cols.len() >= 4
                        && let (Ok(depth), Ok(score), Ok(nodes)) = (
                            cols[0].parse::<u8>(),
                            cols[1].parse::<i32>(),
                            cols[3].parse::<u64>(),
                        )
                    {
                        analysis.depth = depth;
                        analysis.score = score;
                        analysis.nodes = nodes;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(_) => return Err("Original engine stopped".into()),
            }
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Build optional Sjeng first with scripts/build-sjeng.sh"]
    fn original_engine_variants() {
        for rules in Rules::ALL {
            let g = Game::new(rules);
            let a = analyze(
                g.clone(),
                Path::new("target/sjeng/sjeng"),
                0.1,
                2,
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            assert!(g.board.moves().contains(&a.best.unwrap()));
        }
    }
}
