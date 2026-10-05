#!/usr/bin/env python3
"""Produce a Wawona /wasm/v1 catalog fragment and content-addressed WASI blob."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def package(output, module, maintainers, source_ref="main"):
    version = tomllib.loads((ROOT / "wasm/Cargo.toml").read_text())["package"]["version"]
    assert re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", version), version
    if not maintainers or any(not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9-]{0,38}", m) for m in maintainers):
        raise ValueError("At least one valid GitHub maintainer username is required")
    if not re.fullmatch(r"[A-Za-z0-9._/-]+", source_ref):
        raise ValueError("Invalid source revision")
    data = module.read_bytes()
    if data[:8] != b"\0asm\x01\0\0\0":
        raise ValueError("Expected a WASI Preview 1 core WebAssembly module")
    digest = hashlib.sha256(data).hexdigest()
    relative = Path("packages/chess-wawona") / version / "component.wasm"
    blob = output / relative
    blob.parent.mkdir(parents=True, exist_ok=True)
    blob.write_bytes(data)
    row = {
        "name": "chess-wawona", "version": version, "digest": f"sha256:{digest}",
        "url": relative.as_posix(), "wasi": "p1", "kind": "wayland",
        "summary": "Chess and four variants with a 2D Wayland board and Rust engine",
        "long_description": "WASI Preview 1 Wayland client using wl_shm and xdg-shell. "
            "Play locally or against the Rust engine; Standard, Crazyhouse, Suicide and Losers. "
            "Pointer board input, typed UCI/SAN moves and drops, undo/redo and promotion choices. "
            "Software-rendered 2D frontend; no Qt, Vulkan, native executables or external assets. "
            "No file persistence, network play, speech or recording in this build.",
        "license": "GPL-3.0-or-later", "runtime": "wawona-1", "entry": "component.wasm",
        "homepage": "https://github.com/cube-one-ber/chess-for-linux",
        "source": f"https://github.com/cube-one-ber/chess-for-linux/tree/{source_ref}/wasm",
        "programs": ["chess-wawona"], "maintainers": maintainers,
        "platforms": ["macos", "ios", "ipados", "tvos", "visionos", "android", "linux"],
        "capabilities": {"wayland": True, "filesystem": [], "network": False},
    }
    (output / "index.json").write_text(json.dumps({"schema": 1, "channel": "wasm", "mode": "A", "packages": [row]}, indent=2) + "\n")
    (output / "SHA256SUMS").write_text(f"{digest}  {relative.as_posix()}\n")
    shutil.copyfile(ROOT / "LICENSE", blob.parent / "LICENSE")
    shutil.copyfile(ROOT / "wasm/LICENSE.wawona", blob.parent / "LICENSE.wawona")
    # Include upstream notices for statically linked Rust dependencies.
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--manifest-path", str(ROOT / "wasm/Cargo.toml"),
        "--locked", "--format-version", "1", "--filter-platform", "wasm32-wasip1",
    ], text=True))
    for dependency in metadata["packages"]:
        if dependency["source"] is None:
            continue
        source = Path(dependency["manifest_path"]).parent
        notices = [p for p in source.iterdir() if p.is_file() and
                   p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING"))]
        destination = blob.parent / "licenses" / f"{dependency['name']}-{dependency['version']}"
        destination.mkdir(parents=True, exist_ok=True)
        if not notices:
            # shakmaty's crates.io archive omits COPYING; its declared license
            # is the same GPL-3.0-or-later text shipped by this application.
            if dependency["name"] == "shakmaty" and dependency["license"] == "GPL-3.0-or-later":
                shutil.copyfile(ROOT / "LICENSE", destination / "LICENSE")
                shutil.copyfile(source / "README.md", destination / "README.md")
            else:
                raise ValueError(f"No license notice found for {dependency['name']}")
        for notice in notices:
            shutil.copyfile(notice, destination / notice.name)
    (blob.parent / "SOURCE.txt").write_text(row["source"] + "\n")
    shutil.copyfile(ROOT / "wasm/README.md", output / "README.md")
    print(f"Wawona catalog fragment: {output / 'index.json'}")
    print(f"WASI module: {blob} (sha256:{digest})")
    return row


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "artifacts/wasm")
    parser.add_argument("--module", type=Path, default=ROOT / "wasm/target/wasm32-wasip1/release/chess-wawona.wasm")
    parser.add_argument("--maintainer", action="append", help="GitHub login, repeatable; must be in the destination catalog's maintainers.json")
    parser.add_argument("--source-ref", default=os.environ.get("GITHUB_SHA", "main"), help="Source commit or branch; CI defaults to GITHUB_SHA")
    args = parser.parse_args()
    package(args.output, args.module, args.maintainer or ["cube-one-ber"], args.source_ref)
