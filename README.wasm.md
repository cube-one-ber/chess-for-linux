# Chess in a browser

The Rust application also compiles to `wasm32-unknown-unknown`. It uses the same
rules, search engine, document converters, piece meshes, textures and board
shader as the Linux build. Browser rendering prefers WebGPU and falls back to
WebGL2.

## Build and run

Install Rust **1.95 or newer** with rustup and [Trunk](https://trunk-rs.github.io/trunk/):

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
trunk serve --open --locked
```

Open `http://localhost:8080` in a browser with WebGPU or WebGL2 enabled.
WebGPU requires a secure context: localhost during development, or HTTPS when deployed. A loading
message reports initialization errors if both graphics backends are unavailable.
Opening `index.html` directly from the filesystem does not work.

Compile without bundling, or create a deployable release:

```sh
cargo check --locked --target wasm32-unknown-unknown
cargo build --release --locked --target wasm32-unknown-unknown
trunk build --release --locked
```

Trunk writes the HTML, JavaScript loader and WASM module to `dist/`. Serve that
whole directory as a static site. All board artwork is embedded in the module;
no game server is required. For deployment under a subdirectory, use
`trunk build --release --locked --public-url /chess/`.

## Browser controls and scope

- Choose Standard, Crazyhouse, Suicide or Losers, then **New game**.
- Click a piece and destination on the 3D board or select **2D board**. Crazyhouse
  pockets have drop buttons; promotions offer every legal piece.
- Right-drag to rotate and tilt the 3D board, scroll to zoom, or choose **Flip**.
- Enter UCI (`e2e4`), SAN (`Nf3`) or a drop (`N@e4`) in the move field.
- Enable either computer side, pause it, or ask for a hint. Searches run on the
  browser's main thread with a 50 ms budget and depth limit of four; computer
  versus computer yields between moves. These limits favor UI responsiveness
  and are lower than the desktop defaults.
- Undo/redo navigates history. Undo pauses computer play; uncheck **Pause
  computer** to resume. Playing another move retains the previous continuation
  in the native document's variations.
- **Import / export** accepts pasted PGN, native `.chess-linux` JSON, and Apple
  XML documents. Export one of those formats and copy the text to save it.
  Native JSON retains comments, variations and the review cursor.
- The current game and view are saved in browser local storage through eframe.
  Storage is specific to the site's origin and is unavailable in some privacy
  modes. Export a document for a portable save.

A URL fragment such as `#crazyhouse` selects the initial variant when there is no
saved session. Linux file dialogs, TCP play, Unix scripting, speech subprocesses,
Sjeng and ffmpeg recording remain desktop features.

The native Linux build still uses `cargo build --release --locked`; its window
system, portal and Vulkan dependencies are excluded from the WASM build. See
[README.linux.md](README.linux.md) for native setup and the original licensing
notices in [README](README), [LICENSE](LICENSE) and [NOTICE](NOTICE).
