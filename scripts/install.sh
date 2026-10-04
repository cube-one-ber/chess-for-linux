#!/bin/sh
# Install into ~/.local by default; no administrator access is needed.
set -eu
cd "$(dirname "$0")/.."
chess_install_prefix=${PREFIX:-"$HOME/.local"}
cargo build --release --locked
install -Dm755 target/release/chess-linux "$chess_install_prefix/bin/chess-linux"
install -Dm644 packaging/chess-linux.desktop "$chess_install_prefix/share/applications/chess-linux.desktop"
install -Dm644 Resources/Icons/Chess_256x256.png "$chess_install_prefix/share/icons/hicolor/256x256/apps/chess-linux.png"
install -Dm644 packaging/chess-linux.xml "$chess_install_prefix/share/mime/packages/chess-linux.xml"
install -Dm644 README.linux.md "$chess_install_prefix/share/doc/chess-linux/README.linux.md"
install -Dm644 LICENSE "$chess_install_prefix/share/licenses/chess-linux/LICENSE"
install -Dm644 NOTICE "$chess_install_prefix/share/licenses/chess-linux/NOTICE"
install -Dm644 README "$chess_install_prefix/share/licenses/chess-linux/Apple-Sample-Code-License"
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$chess_install_prefix/share/applications"
fi
if command -v update-mime-database >/dev/null 2>&1; then
    update-mime-database "$chess_install_prefix/share/mime"
fi
printf 'Installed Chess to %s. Ensure %s/bin is in PATH.\n' "$chess_install_prefix" "$chess_install_prefix"
