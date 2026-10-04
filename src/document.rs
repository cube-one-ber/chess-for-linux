use crate::game::{Game, Rules, SavedGame};
use shakmaty::Position;
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
pub fn read(path: &Path) -> Result<Game, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    from_bytes(&bytes)
}

pub fn from_bytes(bytes: &[u8]) -> Result<Game, String> {
    if let Ok(data) = serde_json::from_slice::<SavedGame>(bytes) {
        return Game::load(data);
    }
    if let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(bytes)) {
        return from_apple(value);
    }
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    from_pgn(text)
}
#[cfg(not(target_arch = "wasm32"))]
pub fn write(path: &Path, game: &Game) -> Result<(), String> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let bytes = if ext.eq_ignore_ascii_case("pgn") {
        to_pgn(game).into_bytes()
    } else if ext.eq_ignore_ascii_case("chess") {
        let mut out = Vec::new();
        to_apple(game)
            .to_writer_xml(&mut out)
            .map_err(|e| e.to_string())?;
        out
    } else {
        serde_json::to_vec_pretty(&game.data).map_err(|e| e.to_string())?
    };
    // Rename a sibling temporary file so interrupted saves cannot corrupt an existing game.
    let tmp = path.with_extension(format!("{ext}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| e.to_string())
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\r', '\n'], " ")
}
pub fn to_pgn(g: &Game) -> String {
    let mut tags = g.data.headers.clone();
    if let Some(time) = tags.get("StartTime").cloned() {
        tags.insert("Time".into(), time);
    }
    tags.insert("Result".into(), g.data.result.clone());
    if g.data.rules != Rules::Standard {
        tags.insert("Variant".into(), g.data.rules.name().into());
    }
    if g.data.initial_fen != crate::game::Board::new(g.data.rules).fen() {
        tags.insert("SetUp".into(), "1".into());
        tags.insert("FEN".into(), g.data.initial_fen.clone());
    }
    for (i, k) in ["WhiteType", "BlackType"].into_iter().enumerate() {
        tags.insert(
            k.into(),
            if g.data.computer[i] {
                "program"
            } else {
                "human"
            }
            .into(),
        );
    }
    let mut out = String::new();
    for (k, v) in tags {
        out.push_str(&format!("[{k} \"{}\"]\n", escape(&v)));
    }
    out.push('\n');
    let initial = &g.states[0].pos;
    let mut num = u32::from(initial.fullmoves());
    let mut white = initial.turn() == shakmaty::Color::White;
    if let Some(c) = g.data.comments.get(&0) {
        out.push_str(&format!("{{{}}} ", c.replace(['{', '}'], "")));
    }
    for (i, san) in g.sans.iter().enumerate() {
        if white {
            out.push_str(&format!("{num}. "));
        } else if i == 0 {
            out.push_str(&format!("{num}... "));
        }
        out.push_str(san);
        out.push(' ');
        if let Some(c) = g.data.comments.get(&(i + 1)) {
            out.push_str(&format!("{{{}}} ", c.replace(['{', '}'], "")));
        }
        if !white {
            num += 1;
        }
        white = !white;
    }
    out.push_str(&g.data.result);
    out.push('\n');
    out
}
pub fn from_pgn(text: &str) -> Result<Game, String> {
    let mut headers = BTreeMap::new();
    let mut body = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if !body.trim().is_empty() {
                return Err("Open one PGN game at a time".into());
            }
            let split = line.find(' ').ok_or("Invalid PGN tag")?;
            let key = &line[1..split];
            let val = line[split..]
                .trim()
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix("\"]"))
                .ok_or("Invalid PGN tag value")?;
            headers.insert(key.into(), val.replace("\\\"", "\"").replace("\\\\", "\\"));
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    let rules = headers
        .get("Variant")
        .map(|s: &String| Rules::parse(s))
        .transpose()?
        .unwrap_or_default();
    let mut g = Game::new(rules);
    if let Some(fen) = headers.get("FEN") {
        g.set_fen(fen)?;
    }
    g.data.headers.extend(headers.clone());
    if let Some(time) = headers.get("Time") {
        g.data
            .headers
            .entry("StartTime".into())
            .or_insert_with(|| time.clone());
    }
    g.data.computer = [
        headers
            .get("WhiteType")
            .or_else(|| headers.get("WhiteType:"))
            .is_some_and(|s| s == "program"),
        headers
            .get("BlackType")
            .or_else(|| headers.get("BlackType:"))
            .is_some_and(|s| s == "program"),
    ];
    let chars: Vec<_> = body.chars().collect();
    let mut i = 0;
    let mut token = String::new();
    let mut variation = 0usize;
    let mut ended = false;
    let apply = |token: &mut String, g: &mut Game, ended: &mut bool| -> Result<(), String> {
        if token.is_empty() {
            return Ok(());
        }
        let t = std::mem::take(token);
        if t.starts_with('$') {
            return Ok(());
        }
        let t = t.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
        if matches!(t, "" | "!") {
            return Ok(());
        } // Result tokens handled before stripping move numbers below.
        let raw = t.trim_end_matches(['!', '?']);
        if *ended {
            return Err("Moves after PGN result".into());
        }
        let m = g
            .board
            .parse_move(raw)
            .map_err(|e| format!("PGN move {raw}: {e}"))?;
        g.push(m)
    };
    // Tokenize comments, NAGs and recursive annotation variations without confusing their moves with the main line.
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            if variation == 0 {
                process_token(&mut token, &mut g, &mut ended, &apply)?;
            }
            let mut comment = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '}' {
                comment.push(chars[i]);
                i += 1;
            }
            if i == chars.len() {
                return Err("Unclosed PGN comment".into());
            }
            if variation == 0 {
                g.data
                    .comments
                    .entry(g.data.cursor)
                    .and_modify(|s| {
                        s.push(' ');
                        s.push_str(comment.trim());
                    })
                    .or_insert(comment.trim().into());
            }
        } else if c == ';' {
            if variation == 0 {
                process_token(&mut token, &mut g, &mut ended, &apply)?;
            }
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '(' {
            if variation == 0 {
                process_token(&mut token, &mut g, &mut ended, &apply)?;
            }
            variation += 1;
        } else if c == ')' {
            if variation == 0 {
                return Err("Unexpected PGN variation close".into());
            }
            variation -= 1;
        } else if variation == 0 {
            if c.is_whitespace() {
                process_token(&mut token, &mut g, &mut ended, &apply)?;
            } else {
                token.push(c);
            }
        }
        i += 1;
    }
    if variation > 0 {
        return Err("Unclosed PGN variation".into());
    }
    process_token(&mut token, &mut g, &mut ended, &apply)?;
    if !ended && let Some(r) = headers.get("Result") {
        validate_result(r)?;
        g.data.result = r.clone();
    }
    Ok(g)
}
fn validate_result(s: &str) -> Result<(), String> {
    if matches!(s, "*" | "1-0" | "0-1" | "1/2-1/2") {
        Ok(())
    } else {
        Err("Invalid game result".into())
    }
}
fn process_token<F>(
    token: &mut String,
    g: &mut Game,
    ended: &mut bool,
    apply: &F,
) -> Result<(), String>
where
    F: Fn(&mut String, &mut Game, &mut bool) -> Result<(), String>,
{
    if matches!(token.as_str(), "*" | "1-0" | "0-1" | "1/2-1/2") {
        validate_result(token)?;
        g.data.result = std::mem::take(token);
        *ended = true;
        Ok(())
    } else {
        apply(token, g, ended)
    }
}
fn apple_fen(position: &str, holding: Option<&str>, rules: Rules) -> Result<String, String> {
    let mut fields: Vec<String> = position.split_whitespace().map(str::to_string).collect();
    if fields.len() != 6 {
        return Err("Invalid Apple Chess position".into());
    }
    if rules == Rules::Crazyhouse && !fields[0].contains('[') {
        let mut pieces = String::new();
        if let Some(holding) = holding {
            let mut groups = holding.split('[').skip(1);
            if let Some(white) = groups.next() {
                pieces.extend(
                    white
                        .split(']')
                        .next()
                        .unwrap_or("")
                        .chars()
                        .filter(|c| "PNBRQ".contains(*c)),
                );
            }
            if let Some(black) = groups.next() {
                pieces.extend(
                    black
                        .split(']')
                        .next()
                        .unwrap_or("")
                        .chars()
                        .filter(|c| "PNBRQ".contains(*c))
                        .map(|c| c.to_ascii_lowercase()),
                );
            }
        }
        fields[0].push_str(&format!("[{pieces}]"));
    }
    Ok(fields.join(" "))
}
fn from_apple(value: plist::Value) -> Result<Game, String> {
    let d = value
        .as_dictionary()
        .ok_or("Invalid Apple Chess document")?;
    if let Some(bytes) = d.get("ChessLinuxData").and_then(plist::Value::as_data) {
        let data = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        return Game::load(data);
    }
    let get = |k: &str| d.get(k).and_then(plist::Value::as_string);
    let rules = Rules::parse(get("Variant").unwrap_or("normal"))?;
    let mut g = Game::new(rules);
    let moves = get("Moves").unwrap_or("");
    if moves.trim().is_empty() {
        if let Some(fen) = get("Position") {
            g.set_fen(&apple_fen(fen, get("Holding"), rules)?)?;
        }
    } else {
        for text in moves.split_whitespace() {
            let m = g.board.parse_move(text)?;
            g.push(m)?;
        }
        if let Some(fen) = get("Position") {
            let expected =
                crate::game::Board::from_fen(rules, &apple_fen(fen, get("Holding"), rules)?)?;
            // Apple omits promoted markers in FEN; move replay preserves that information.
            if expected.pos.board() != g.board.pos.board()
                || expected.pos.turn() != g.board.pos.turn()
                || expected.pos.pockets() != g.board.pos.pockets()
            {
                return Err("Apple document position disagrees with its move history".into());
            }
        }
    }
    for k in [
        "Event",
        "Site",
        "Date",
        "Round",
        "White",
        "Black",
        "City",
        "Country",
        "StartDate",
        "StartTime",
    ] {
        if let Some(v) = get(k) {
            g.data.headers.insert(k.into(), v.into());
        }
    }
    if get("Date").is_none()
        && let Some(date) = get("StartDate")
    {
        g.data.headers.insert("Date".into(), date.into());
    }
    if get("Site").is_none() && (get("City").is_some() || get("Country").is_some()) {
        let site = [get("City"), get("Country")]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        g.data.headers.insert("Site".into(), site);
    }
    g.data.computer = [
        get("WhiteType") == Some("program"),
        get("BlackType") == Some("program"),
    ];
    if let Some(r) = get("Result") {
        validate_result(r)?;
        g.data.result = r.into();
    }
    Ok(g)
}
pub fn to_apple(g: &Game) -> plist::Value {
    let mut d = plist::Dictionary::new();
    for (k, v) in &g.data.headers {
        d.insert(k.clone(), plist::Value::String(v.clone()));
    }
    if let Some(date) = g.data.headers.get("Date") {
        d.insert("StartDate".into(), plist::Value::String(date.clone()));
    }
    let final_board = g.states.last().unwrap();
    let mut fields: Vec<String> = final_board
        .fen()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if let Some(index) = fields[0].find('[') {
        fields[0].truncate(index);
    }
    fields[0] = fields[0].replace('~', "");
    let holding = if let Some(pockets) = final_board.pos.pockets() {
        let group = |color: shakmaty::Color| {
            let mut s = String::new();
            for role in [
                shakmaty::Role::Queen,
                shakmaty::Role::Bishop,
                shakmaty::Role::Knight,
                shakmaty::Role::Rook,
                shakmaty::Role::Pawn,
            ] {
                for _ in 0..pockets[color][role] {
                    s.push(role.upper_char());
                }
            }
            s
        };
        format!(
            "[{}] [{}]",
            group(shakmaty::Color::White),
            group(shakmaty::Color::Black)
        )
    } else {
        "[] []".into()
    };
    // Apple replays Moves from the standard start. For a custom starting FEN it
    // receives the final position, while the extension retains full Rust history.
    let moves = if g.data.initial_fen == crate::game::Board::new(g.data.rules).fen() {
        g.data.moves.join("\n")
    } else {
        String::new()
    };
    for (k, v) in [
        (
            "Variant",
            if g.data.rules == Rules::Standard {
                "normal".into()
            } else {
                g.data.rules.name().to_lowercase()
            },
        ),
        ("Position", fields.join(" ")),
        ("Holding", holding),
        ("Moves", moves),
        ("Result", g.data.result.clone()),
        (
            "WhiteType",
            if g.data.computer[0] {
                "program"
            } else {
                "human"
            }
            .into(),
        ),
        (
            "BlackType",
            if g.data.computer[1] {
                "program"
            } else {
                "human"
            }
            .into(),
        ),
    ] {
        d.insert(k.into(), plist::Value::String(v));
    }
    d.insert(
        "ChessLinuxData".into(),
        plist::Value::Data(serde_json::to_vec(&g.data).expect("Serializable game")),
    );
    plist::Value::Dictionary(d)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_document_bytes_without_a_filesystem() {
        for rules in Rules::ALL {
            let mut game = Game::new(rules);
            let m = game.board.parse_move("e2e4").unwrap();
            game.push(m).unwrap();
            game.data.comments.insert(1, "Browser import".into());
            let mut xml = Vec::new();
            to_apple(&game).to_writer_xml(&mut xml).unwrap();
            let mut binary = Vec::new();
            to_apple(&game).to_writer_binary(&mut binary).unwrap();
            for bytes in [
                serde_json::to_vec(&game.data).unwrap(),
                to_pgn(&game).into_bytes(),
                xml,
                binary,
            ] {
                let loaded = from_bytes(&bytes).unwrap();
                assert_eq!(loaded.data.rules, rules);
                assert_eq!(loaded.board.fen(), game.board.fen());
                assert_eq!(loaded.data.comments[&1], "Browser import");
            }
        }
        assert!(from_bytes(&[0xff, 0xfe]).is_err());
    }

    #[test]
    fn pgn_comments_and_variations() {
        let g =
            from_pgn("[Event \"Test\"]\n\n1.e4 {hi} e5 (1... c5 (2.Nf3)) 2. Nf3 Nc6 *").unwrap();
        assert_eq!(g.data.moves.len(), 4);
        assert_eq!(g.data.comments[&1], "hi");
        assert_eq!(from_pgn(&to_pgn(&g)).unwrap().board.fen(), g.board.fen());
    }
    #[test]
    fn black_to_move_fen() {
        let g = from_pgn("[FEN \"4k3/8/8/8/8/8/8/4K3 b - - 0 23\"]\n\n23... Kf7 *");
        assert!(g.is_err());
        let g = from_pgn("[FEN \"4k3/p7/8/8/8/8/P7/4K3 b - - 0 23\"]\n\n23... Kf7 *").unwrap();
        assert!(to_pgn(&g).contains("23... Kf7"));
    }
    #[test]
    fn apple_roundtrip() {
        let mut g = Game::new(Rules::Standard);
        for s in ["e2e4", "c7c5"] {
            let m = g.board.parse_move(s).unwrap();
            g.push(m).unwrap();
        }
        assert_eq!(from_apple(to_apple(&g)).unwrap().board.fen(), g.board.fen());
    }
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    #[test]
    fn original_apple_metadata_survives_linux_edit_and_export() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Variant</key><string>normal</string>
<key>Moves</key><string>e2e4</string>
<key>StartDate</key><string>2024.04.12</string>
<key>StartTime</key><string>15:30:00</string>
<key>City</key><string>London</string>
<key>Country</key><string>UK</string>
</dict></plist>"#;
        let value = plist::Value::from_reader(std::io::Cursor::new(xml)).unwrap();
        let mut g = from_apple(value).unwrap();
        assert_eq!(g.data.headers["Date"], "2024.04.12");
        assert_eq!(g.data.headers["Site"], "London, UK");
        g.data.headers.insert("Date".into(), "2026.10.04".into());
        let mut exported = to_apple(&g);
        let d = exported.as_dictionary_mut().unwrap();
        assert_eq!(d["StartDate"].as_string(), Some("2026.10.04"));
        d.remove("ChessLinuxData");
        assert_eq!(
            from_apple(exported).unwrap().data.headers["Date"],
            "2026.10.04"
        );
        let pgn = to_pgn(&g);
        assert!(pgn.contains("[Time \"15:30:00\"]"));
        let pgn = "[Time \"15:30:00\"]\n\n1. e4 *";
        assert_eq!(from_pgn(pgn).unwrap().data.headers["StartTime"], "15:30:00");
    }
    #[test]
    fn apple_holding_groups_and_player_types() {
        let fen = apple_fen(
            "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
            Some("[PN] [Q]"),
            Rules::Crazyhouse,
        )
        .unwrap();
        let g = crate::game::Board::from_fen(Rules::Crazyhouse, &fen).unwrap();
        let p = g.pos.pockets().unwrap();
        assert_eq!(p[shakmaty::Color::White][shakmaty::Role::Knight], 1);
        assert_eq!(p[shakmaty::Color::Black][shakmaty::Role::Queen], 1);
        let g = Game::new(Rules::Standard);
        let mut apple = to_apple(&g);
        apple.as_dictionary_mut().unwrap().remove("ChessLinuxData");
        assert_eq!(from_apple(apple).unwrap().data.computer, [false, true]);
    }
    #[test]
    fn custom_fen_history_roundtrips_through_apple_extension() {
        let mut g = Game::new(Rules::Standard);
        g.set_fen("4k3/p7/8/8/8/8/P7/4K3 b - - 0 23").unwrap();
        let m = g.board.parse_move("a7a6").unwrap();
        g.push(m).unwrap();
        g.data.comments.insert(1, "Custom start".into());
        let h = from_apple(to_apple(&g)).unwrap();
        assert_eq!(g.board.fen(), h.board.fen());
        assert_eq!(h.data.comments[&1], "Custom start");
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn export_native_apple_and_pgn_files() {
        let dir = std::env::temp_dir().join(format!("chess-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut g = Game::new(Rules::Crazyhouse);
        for text in ["e2e4", "d7d5", "e4d5", "d8d5", "P@e4"] {
            let m = g.board.parse_move(text).unwrap();
            g.push(m).unwrap();
        }
        for ext in ["chess-linux", "chess", "pgn"] {
            let path = dir.join(format!("Game.{ext}"));
            write(&path, &g).unwrap();
            let h = read(&path).unwrap();
            assert_eq!(h.board.fen(), g.board.fen(), "{ext}");
            assert_eq!(h.data.computer, g.data.computer);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
