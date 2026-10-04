# Chess for Linux

A native Linux rebuild of the supplied Apple Chess application. The desktop interface uses **Qt 6 and KDE Kirigami**, with the game controller and engine written in **Rust**. Qt Quick renders the interface through **Vulkan**; wgpu renders the 3D board through Vulkan. Wayland and X11 are supported. The original source, license notices, piece geometry, and artwork are retained.

## Build and run

Requires Rust **1.95 or newer**, a C++17 compiler, pkg-config, Qt **6.5 or newer** development packages (Core, Gui, QML, Quick and Quick Controls, including `moc` and `rcc`), and a working Vulkan driver/loader. Install the **Kirigami 6** and **qqc2-desktop-style** QML modules for the interface. Building Sjeng is optional.

On Arch Linux or CachyOS:

```sh
sudo pacman -S --needed base-devel qt6-base qt6-declarative kirigami qqc2-desktop-style
```

Other distributions need the equivalent Qt 6 development and KDE QML runtime packages. Qt modules are discovered through pkg-config; `QT_HOST_PATH` can select a Qt installation for its build tools. The [Kirigami setup guide](https://develop.kde.org/docs/getting-started/kirigami/setup-cpp/) describes the KDE/Qt dependencies.

```sh
cargo build --release --locked
./target/release/chess-linux
```

Open a game directly, or skip recovery of the previous session:

```sh
./target/release/chess-linux game.chess
./target/release/chess-linux game.pgn
./target/release/chess-linux --fresh
```

Install the binary, desktop launcher, icon, MIME types and documentation into `~/.local`:

```sh
./scripts/install.sh
```

`PREFIX=/path ./scripts/install.sh` selects another installation prefix. No installation is needed to run the release binary; its artwork, QML files and Noto fonts are embedded. Qt, Kirigami and the desktop controls style remain system dependencies. Native file dialogs use Qt's desktop platform integration, including the XDG portal where available.

The small C++ QObject bridge exposes Rust state and commands to QML. The Rust Vulkan board is read back into an owned RGBA image and supplied to Qt through a `QQuickImageProvider`; Qt then composes it with the interface. This adds a GPU readback/upload per changed board frame. The previous egui interface is available with `--egui`; `--gui-smoke` exercises that compatibility interface.

## Functionality

| Original functionality | Linux implementation |
| --- | --- |
| Standard, Crazyhouse, Suicide and Losers | Rust rules, including compulsory legal captures, drops, castling, en passant, all legal promotions, checkmate and draw detection |
| Human/computer combinations | Human versus human, either computer side, and computer versus computer; adjustable thinking time/depth and pause/resume |
| Original computer engine | Optional separate Sjeng process, preserving the supplied search, opening books and on-disk learning |
| Interactive 3D board | Vulkan triangle renderer, original Metal piece geometry and supplied textures, GGX specular lighting, planar reflections, contact shadows, 4× MSAA, movement animation, mouse picking and dragging |
| Board and piece appearance | Wood, Marble, Metal and Grass boards; Wood, Marble, Metal and Fur pieces; independent styles, camera angle, rotation and zoom |
| Material and lighting tuning | Live controls for five materials, light position, ambient light, board reflectivity and notation brightness; saved preferences |
| Game log and hints | SAN history, last-move and hint overlays, review navigation, undo/redo, comments and saved alternative continuations |
| Game documents | Native JSON documents, XML/binary Apple `.chess` import, Apple `.chess` export, and single-game PGN import/export |
| Game information | Editable event, site, date, round, player names, city, country and starting time |
| Multiple documents | Tabs, duplication, recent files, save/save as, close/save prompts and session recovery |
| Spoken moves | Optional espeak output with separate voices for White and Black |
| Spoken input | Optional offline Vosk microphone recognition; the command field accepts the same spoken phrases |
| Accessibility | A 2D board with separately labelled Qt square buttons and Qt accessibility support; keyboard and text controls |
| Shared play | Direct two-player TCP sessions, game synchronization, chat, agreed takebacks, draw offers and resignation |
| Recording | PNG screenshots and optional ffmpeg H.264/MP4 recording of the application, including either board view |
| Window controls | Resizing, full screen, and floating above other windows |
| Scripting | Private Unix socket with JSON commands and a command-line client |
| End-of-game notifications | Linux desktop notifications through optional `notify-send` |
| Game Center | Excluded as requested |

Apple's SharePlay transport is replaced by direct network play; this does not connect to Apple's SharePlay service. The Vulkan renderer reuses the original assets and implements physical specular lighting and planar reflections, with lighting that differs from the Metal renderer. Linux scripting and the integrated material editor replace the macOS interfaces. Native Rust search is a new engine, not a claim of identical Sjeng playing strength; select Sjeng when retaining its engine behavior matters.

## Controls

- Click a piece and its destination, or drag the piece.
- Right-drag to rotate and tilt; scroll to zoom; **F** rotates by 180°.
- **Ctrl+N/O/S**: new/open/save. **Ctrl+Z**: take back. **Ctrl+Shift+Z**: redo.
- **]**: show hint. **[**: show last move. **F11**: full screen.
- Arrow keys choose a square; **Enter** selects/moves; **Escape** clears selection.
- Choose a captured piece in the Crazyhouse pocket, then its drop square.
- Promotions present a choice of every legal promotion piece, including King in Suicide.
- The command field accepts UCI (`e2e4`), SAN (`Nf3`, `O-O`), drops (`N@e4`), and spoken phrases such as `move pawn from e two to e four`, `knight to f three`, `drop knight on e four`, `show hint`, and `take back move`.
- Open **Preferences** for appearance, computer-side selection, search limits, voices and microphone settings. **Lighting and material controls** opens the live editor. The 2D board offers square buttons for Linux screen readers.

Reviewing move history pauses computer play. Use **Resume** to continue. Playing a different move from an earlier position retains the previous continuation as a saved variation.

## Original Sjeng engine (optional)

The original C engine remains a separate executable; it is not linked into the Rust application. Its legacy Berkeley DB interface requires the GDBM compatibility development library and headers (`ndbm.h`, `libgdbm_compat`, `libgdbm`), plus a C compiler.

```sh
./scripts/build-sjeng.sh
```

In **Preferences → Optional original Sjeng executable**, enter the absolute path to `target/sjeng/sjeng`. Leave the field empty to use the Rust engine. The compatibility script supplies a Linux endian header in the build directory and leaves the original engine sources intact.

Books, engine configuration and learned positions live under `$XDG_STATE_HOME/chess-linux/sjeng`, normally `~/.local/state/chess-linux/sjeng`. Initial `.opn` books and `sjeng.rc` are copied from the supplied source. Sjeng uses whole-second thinking limits. For a custom initial Crazyhouse FEN containing promoted-piece `~` markers, use the Rust engine; historical promotions replayed from the normal starting board work with either engine.

## Speech and video (optional)

- Install an **espeak** executable for spoken output. Voice identifiers in Preferences are espeak voice names; `en` and `en+f3` are defaults.
- For microphone recognition, install Python 3 with **vosk** and **sounddevice**, and extract a compatible Vosk model. Set the model folder in Preferences and click **Listen for spoken moves**. Recognition stays on the local machine. [Vosk installation and models](https://alphacephei.com/vosk/install).
- Install **ffmpeg** with the **libx264** encoder for video. Choose **Share → Record game**, then **Stop recording** to finish the MP4. Recording normalizes window resizing to its initial dimensions and keeps a 30 fps timeline.

Missing optional executables or recognition models produce a status message. They are not required for board play or document handling. Spoken-command parsing and video encoding have been verified; actual microphone recognition and screen-reader output require the corresponding local services and hardware.

## Files and recovery

`.chess-linux` is the full-fidelity format: starting FEN, variant, moves, review cursor, result, comments, saved branches, headers and computer-side selection. Saves use a sibling temporary file and rename it into place.

Apple exports include the traditional `Variant`, `Position`, `Holding`, `Moves`, player types and game information. A `ChessLinuxData` extension retains the additional Linux document state for round trips; Apple ignores that extension. When a game starts from a custom FEN, Apple receives its final position, while the extension retains its starting position and history.

PGN imports read one game and its main line and comments. Recursive annotation variations are skipped rather than played as main-line moves; PGN exports contain the main line and comments. Use the native format or Apple export with the Linux extension to retain saved alternative continuations and the review cursor.

Preferences are stored at `$XDG_CONFIG_HOME/chess-linux/preferences.json` (normally `~/.config/chess-linux`). Session recovery is stored at `$XDG_STATE_HOME/chess-linux/recovery.json` (normally `~/.local/state/chess-linux`). `--fresh` skips recovery at startup.

## Linux scripting

Launch the application, then send a JSON request from another terminal:

```sh
./target/release/chess-linux --command '{"command":"status"}'
./target/release/chess-linux --command '{"command":"new","variant":"crazyhouse","computer":[false,false]}'
./target/release/chess-linux --command '{"command":"move","text":"e2e4"}'
./target/release/chess-linux --command '{"command":"save","path":"/tmp/game.chess-linux"}'
```

Replies contain `ok` and either `data` or `error`. Status includes FEN, variant, review ply, result, search/pause state and network state. Move text accepts SAN, UCI and Crazyhouse drops.

| Command | Additional fields |
| --- | --- |
| `status`, `undo`, `hint`, `disconnect`, `resign`, `quit` | None |
| `new` | `variant`: `standard`, `crazyhouse`, `suicide` or `losers`; `computer`: `[white, black]` booleans; defaults to standard and two humans |
| `move`, `seek`, `set_fen`, `pause` | Respectively `text`, `ply`, `fen`, `paused` |
| `open`, `save`, `screenshot` | Absolute `path`; scripted screenshots capture the 3D board |
| `set_view` | `view` object using the fields of `render::View`; omitted fields use defaults |
| `host`, `join` | `address`, for example `127.0.0.1:7878`; the host can use port `0` and read its assigned address from status |
| `ask`, `respond` | Respectively `request`: `draw` or `takeback`; `accepted`: boolean |
| `action` | Kirigami action `name` and optional `data` object; see below |

Kirigami actions include `preferences` (a partial preferences object), `computer` (`computer` booleans), `square` (`square` such as `e2`), `promote` (`role`), `switch`/`close`/`variation` (`index`), `metadata` (header strings), `comment` (`text`), `chat` (`text`), `flip`, `duplicate`, `copy_pgn`, `listen` (`enabled`), `record` (`path`, `size`), `stop_record` and `close_response` (`choice`: `save`, `discard` or `cancel`, optional `path`). `gui` opens a named dialog with `data.action`: `new`, `settings`, `appearance`, `computer`, `speech`, `materials`, `network` or `close_dialogs`. `gui_screenshot` captures the entire window to `path`; `ui_resize` takes `width` and `height`. These use the same private control socket as game commands.

Each instance creates `$XDG_RUNTIME_DIR/chess-linux-control/PID.sock`, with a private directory and a socket accessible only to its owner. If the runtime directory is unavailable, it uses the state directory. With multiple instances, pass `--socket /absolute/path/PID.sock` to target one. The wire format is one JSON request and one JSON reply per connection, each terminated by a newline. Requests run on the application thread and obey legal moves, player turns, pending network offers and unsaved-document prompts. New/open/setup/history commands are disabled during network play; moves, saving and agreed takebacks remain available.

## Verification

```sh
./scripts/check.sh
CHESS_GUI_CHECK=1 ./scripts/check.sh
```

The first command checks formatting, strict Clippy linting, rules/document/network/engine tests, and a real offscreen Vulkan render. The second also exercises a native Kirigami window, verifies its Vulkan scene graph, captures all four settings pages, checks variants, history and saved branches, metadata, computer play, promotion, the 2D board, compact layout, video recording, unsaved-document prompts and clean shutdown, and rejects QML runtime errors. It then launches two native instances with isolated settings to verify all four variants over TCP, turn enforcement, pocket drops, accepted takebacks, declined/accepted draws, resignation, save/reopen, scripting and hint search. Existing preferences and recovery files are left untouched. Output is written to the ignored `artifacts/` directory. The GUI checks require a desktop session, Python 3 and ffmpeg with libx264.

Run the two-instance integration check against a particular build:

```sh
python3 scripts/integration_check.py target/release/chess-linux
python3 scripts/kirigami_check.py target/release/chess-linux
```

The optional original engine has a separate integration check:

```sh
./scripts/build-sjeng.sh
cargo test original_engine_variants -- --ignored
```

For a standalone GPU check or command-line analysis:

```sh
./target/release/chess-linux --render-check board.png
./target/release/chess-linux --analyze 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1' standard
```

The Vulkan render and native GUI have been exercised on an NVIDIA GeForce RTX 4060 Laptop GPU. Other Linux GPU/driver combinations have not been hardware-tested here.

## Source and licenses

- `src/game.rs`, `src/document.rs`: variant rules, history and documents.
- `src/engine.rs`: native Rust iterative alpha-beta search.
- `src/legacy_engine.rs`: optional original Sjeng process adapter.
- `src/render.rs`, `src/board.wgsl`: Vulkan board renderer and shaders.
- `qml/`: Kirigami application, dialogs and reusable interface controls.
- `src/kirigami.rs`: Rust controller for the Kirigami application.
- `native/bridge.h`, `native/bridge.cpp`, `build.rs`: Qt/QML bridge and resource build.
- `src/app.rs`: shared preferences/recovery and previous egui interface.
- `src/network.rs`, `src/speech.rs`, `src/recording.rs`, `src/automation.rs`: Linux integrations.
- `assets/*.mesh`: portable triangle buffers derived from the supplied Metal/USD geometry, preserving normals and texture coordinates. Regeneration requires the OpenUSD Python bindings (`pip install usd-core`), then `python scripts/convert_meshes.py`. Normal builds need no USD tools.

New Rust application, QML interface and Qt bridge code are GPL-3.0-or-later, compatible with the GPL-3.0-or-later [shakmaty](https://github.com/niklasf/shakmaty) rules library. Bundled Noto Sans and Noto Serif fonts use the SIL Open Font License 1.1 in `assets/fonts/LICENSE`; the installer includes that notice. Original artwork, geometry and frontend source retain the Apple Sample Code License in the root `README`; original Sjeng retains `sjeng/COPYING`. See `NOTICE` and `LICENSE`. This port is not endorsed by Apple.
