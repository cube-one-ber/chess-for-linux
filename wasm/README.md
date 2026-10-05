# Chess for Wawona Runtime

A WASI Preview 1 (`wasm32-wasip1`) **Wayland client** for Wawona's downloadable
wasm catalog. The software-rendered 2D board uses `wl_compositor`, `wl_shm`,
`xdg_wm_base` and `wl_seat`, with pointer, touch and keyboard input. The rules
and computer engine are compiled directly from the native app's
[`src/game.rs`](../src/game.rs) and [`src/engine.rs`](../src/engine.rs).

This is a separate frontend, without Qt, Kirigami, Vulkan or browser APIs.
It contains no native executables or external artwork. It requires Wawona's
host imports; a plain browser or standard WASI runner cannot launch it.

## Build

Requires Rust 1.95+, Python 3.11+ and the WASI target:

```sh
rustup target add wasm32-wasip1
./scripts/build-wasm.sh
```

The output in `artifacts/wasm/` matches the repository's `/wasm/v1` layout:

```text
index.json                         catalog fragment containing one package
SHA256SUMS
packages/chess-wawona/0.1.0/
  component.wasm
  LICENSE
  LICENSE.wawona
```

The fragment contains the actual module's SHA-256 digest, `wasi: p1`,
`runtime: wawona-1`, `kind: wayland`, GitHub maintainers and capabilities.
It requests Wayland only, with no filesystem roots or network access.
`--maintainer LOGIN` can override the default `cube-one-ber`; repeat it for
multiple maintainers.
`--source-ref COMMIT` pins the catalog's source link; CI uses `GITHUB_SHA`.

Copy `component.wasm` into Wawona's Documents folder and run:

```text
wasm ./component.wasm
```

After the catalog submission has been merged and deployed:

```text
wpm install chess-wawona
wasm chess-wawona
```

## Controls and supported features

- Click or tap a piece and its destination; legal destinations are marked.
- Type a UCI or SAN move and press Enter. Crazyhouse drops use `N@e4`.
- **New**, **Undo**, **Redo**, **AI on/off**, and **Variant** buttons.
  Variant cycles Standard, Crazyhouse, Suicide and Losers and starts a new game.
- Computer play uses the shared Rust engine as Black, with a short bounded
  search on the event loop. Two-human play is the default.
- Choose a promotion piece below the board. Escape clears selection/input.
- The board resizes with the window. Keyboard text entry currently assumes
  US evdev keycodes; it does not interpret the compositor's XKB keymap.

File persistence/import/export, network play, speech, recording and the
native app's 3D board are not part of this wasm frontend. Games live in memory
until the window closes.

## Verification

```sh
cargo fmt --manifest-path wasm/Cargo.toml --check
cargo clippy --manifest-path wasm/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path wasm/Cargo.toml --locked
python3 -m venv .venv
.venv/bin/pip install -r scripts/wasm-requirements.txt
.venv/bin/python scripts/wasm_check.py
```

The integration check executes the **compiled wasm**, validates imports,
fragments incoming Wayland messages, plays moves via pointer, touch and keyboard,
executes a computer reply, checks resize and ping/pong, renders a PNG and verifies shutdown and SHM handle
cleanup. `--wayland` forwards the same module to a running compositor through
real Unix sockets and SCM_RIGHTS and waits for a server roundtrip after the
first buffer commit. CI runs this against headless Weston. These checks do not
claim device validation on Wawona's Apple/Android runtime.

## Publishing to repo.wawona.io

GitHub Actions builds packages on pushes, pull requests and manual runs.
Download the `chess-wawona-wasm` artifact. Pushing a `v<VERSION>` tag that
matches `wasm/Cargo.toml` publishes release archives after both native and
wasm checks pass.

Submit a pull request to
[`Wawona/repo.wawona.io`](https://github.com/Wawona/repo.wawona.io):

1. Copy `packages/chess-wawona/<version>/` into `wasm/v1/packages/`.
2. Merge the package row from the generated `index.json` into
   `wasm/v1/index.json`'s existing `packages` array. Preserve the existing
   packages; the generated file is a **fragment**, not a replacement catalog.
3. Register the package's GitHub maintainer in `maintainers.json` if missing.
   Use the GitHub API for the numeric ID, and supply the maintainer's email
   according to the catalog's instructions. Run
   `python3 scripts/check-packages.py --sync`, then its offline gate.
4. Verify the copied module against `SHA256SUMS` and test it on Wawona.

Catalog publishing requires the Wawona repository maintainers to merge the
submission. This project's workflow does not require their credentials and
does not write to the live catalog.

Canonical format:
[`wasm/README.md`](https://github.com/Wawona/repo.wawona.io/blob/main/wasm/README.md)
and its [current index](https://repo.wawona.io/wasm/v1/index.json).

## Host ABI and provenance

The transport and bitmap font are adapted from Alex Spaulding's MIT-licensed
[`wayland-shm` example](https://github.com/Wawona/Relay/tree/2b38c4dfe00b3e4e85fbb4dd7e04604d19563fd0/import/wasm/examples/wayland-shm/rust),
with capability-driven seat binding and explicit SHM handle cleanup.
The original notice is in [`LICENSE.wawona`](LICENSE.wawona).
New frontend code and the shared engine/rules are GPL-3.0-or-later.

The six `env` imports are `wawona_wayland_connect`,
`wawona_wayland_shm_create`, `wawona_wayland_shm_write`,
`wawona_wayland_sendmsg`, `wawona_socket_recv`, and `wawona_socket_close`.
All other imports are standard `wasi_snapshot_preview1` functions. In this
catalog `component.wasm` is the conventional filename for a **P1 core module**;
it is not a WASI Preview 2 component.
