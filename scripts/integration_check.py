#!/usr/bin/env python3
"""Exercise two real native Vulkan windows through their private control sockets."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/debug/chess-linux"
ARTIFACTS = ROOT / "artifacts"
ARTIFACTS.mkdir(exist_ok=True)


def wait_for(check, description, timeout=15):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            last = check()
            if last:
                return last
        except (OSError, ValueError):
            pass
        time.sleep(0.08)
    raise AssertionError(f"Timed out: {description} (last result: {last})")


class App:
    def __init__(self, directory, name):
        self.root = directory / name
        self.root.mkdir()
        env = os.environ.copy()
        env.update(XDG_CONFIG_HOME=str(self.root / "config"),
                   XDG_STATE_HOME=str(self.root / "state"), CHESS_DISABLE_NOTIFICATIONS="1")
        config = self.root / "config/chess-linux"
        config.mkdir(parents=True)
        (config / "preferences.json").write_text(json.dumps({"speak_computer": False,
                                                            "speak_human": False}))
        self.log = (ARTIFACTS / f"integration-{name}.log").open("w")
        self.process = subprocess.Popen([str(BINARY), "--fresh"], cwd=ROOT, env=env,
                                        stdout=self.log, stderr=subprocess.STDOUT)
        runtime = Path(env.get("XDG_RUNTIME_DIR", self.root / "state"))
        self.socket = runtime / f"chess-linux-control/{self.process.pid}.sock"

    def command(self, command, expect_ok=True, **fields):
        if self.process.poll() is not None:
            raise AssertionError(f"Native app exited with code {self.process.returncode}")
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(15)
            stream.connect(str(self.socket))
            stream.sendall((json.dumps(dict(command=command, **fields)) + "\n").encode())
            with stream.makefile("r") as reply:
                response = json.loads(reply.readline())
        if expect_ok:
            assert response.get("ok"), response
            return response["data"]
        assert not response.get("ok"), response
        return response

    def status(self):
        return self.command("status")

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        self.socket.unlink(missing_ok=True)
        self.log.close()


def same_position(host, guest, ply):
    a, b = host.status(), guest.status()
    return a["ply"] == b["ply"] == ply and a["fen"] == b["fen"] and a["variant"] == b["variant"]


def bound_address(app):
    address = app.status()["network_address"]
    return address if address != "127.0.0.1:0" else None


def run():
    apps = []
    with tempfile.TemporaryDirectory(prefix="chess-linux-check-") as temp:
        try:
            for name in ("host", "guest"):
                app = App(Path(temp), name)
                apps.append(app)
                wait_for(lambda: app.status(), f"{name} control socket")
            host, guest = apps
            cli = subprocess.check_output([str(BINARY), "--command", '{"command":"status"}',
                                           "--socket", str(host.socket)], text=True)
            assert json.loads(cli)["ok"]
            for variant in ("standard", "crazyhouse", "suicide", "losers"):
                host.command("new", variant=variant)
                host.command("host", address="127.0.0.1:0")
                address = wait_for(lambda: bound_address(host),
                                   "host binds an available port")
                guest.command("join", address=address)
                wait_for(lambda: host.status()["connected"] and guest.status()["connected"]
                         and same_position(host, guest, 0), f"{variant} initial synchronization")
                guest.command("move", expect_ok=False, text="e2e4")
                for ply, move in enumerate(("e2e4", "d7d5", "e4d5", "d8d5"), start=1):
                    (host if ply % 2 else guest).command("move", text=move)
                    wait_for(lambda: same_position(host, guest, ply), f"{variant} move {move}")
                ply = 4
                if variant == "crazyhouse":
                    host.command("move", text="P@e4")
                    ply = 5
                    wait_for(lambda: same_position(host, guest, ply), "crazyhouse pocket drop")
                host.command("undo")
                wait_for(lambda: guest.status()["remote_request"] == "takeback", "takeback offer")
                guest.command("move", expect_ok=False, text="d5d8")
                guest.command("respond", accepted=True)
                wait_for(lambda: same_position(host, guest, ply - 1), "agreed takeback")
                host.command("ask", request="draw")
                wait_for(lambda: guest.status()["remote_request"] == "draw", "draw offer")
                guest.command("respond", accepted=False)
                wait_for(lambda: host.status()["pending_request"] is None, "declined draw")
                assert host.status()["result"] == guest.status()["result"] == "*"
                host.command("ask", request="draw")
                wait_for(lambda: guest.status()["remote_request"] == "draw", "second draw offer")
                guest.command("respond", accepted=True)
                wait_for(lambda: host.status()["result"] == guest.status()["result"] == "1/2-1/2",
                         "agreed draw")
                expected = host.status()["fen"]
                path = ARTIFACTS / f"integration-{variant}.chess-linux"
                host.command("save", path=str(path))
                assert json.loads(path.read_text())["rules"] == variant
                host.command("disconnect")
                wait_for(lambda: not guest.status()["network_active"], "opponent disconnect")
                host.command("open", path=str(path))
                assert host.status()["fen"] == expected
                print(f"{variant}: moves, turn enforcement, takeback, declined/accepted draw, save/reopen passed",
                      flush=True)
            host.command("new")
            host.command("host", address="127.0.0.1:0")
            address = wait_for(lambda: bound_address(host), "second host")
            guest.command("join", address=address)
            wait_for(lambda: host.status()["connected"] and guest.status()["connected"], "reconnect")
            guest.command("resign")
            wait_for(lambda: host.status()["result"] == guest.status()["result"] == "1-0", "resignation")
            host.command("disconnect")
            wait_for(lambda: not guest.status()["network_active"], "final disconnect")
            host.command("new")
            host.command("move", text="e2e4")
            time.sleep(0.35)
            for extension in ("pgn", "chess"):
                path = ARTIFACTS / f"integration-document.{extension}"
                fen = host.status()["fen"]
                host.command("save", path=str(path))
                host.command("open", path=str(path))
                assert host.status()["fen"] == fen
            host.command("screenshot", path=str(ARTIFACTS / "integration-vulkan.png"))
            assert (ARTIFACTS / "integration-vulkan.png").stat().st_size > 1000
            host.command("hint")
            wait_for(lambda: not host.status()["thinking"], "asynchronous hint")
            print("Native two-instance integration passed: four variants, network, scripting, documents, Vulkan screenshot, hint",
                  flush=True)
        finally:
            for app in apps:
                app.close()


if __name__ == "__main__":
    run()
