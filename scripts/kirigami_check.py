#!/usr/bin/env python3
"""Native Kirigami smoke check, using isolated preferences and recovery."""
import json
from pathlib import Path
import struct
import tempfile
import time
from integration_check import App, ARTIFACTS, wait_for


def run():
    with tempfile.TemporaryDirectory(prefix="chess-kirigami-check-") as directory:
        app = App(Path(directory), "kirigami")
        try:
            wait_for(lambda: app.status(), "Kirigami control socket")
            app.command("new", variant="standard")
            # Avoid extra tabs in the main screenshot.
            app.command("action", name="close", data={"index": 0})
            def action(name, data=None):
                return app.command("action", name=name, data=data or {})
            def screenshot(name):
                path = ARTIFACTS / name
                path.unlink(missing_ok=True)
                action("gui_screenshot", {"path": str(path)})
                wait_for(lambda: path.exists() and path.stat().st_size > 1000, name)
                return path
            screenshot("kirigami-opening.png")
            for move in ["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6"]:
                app.command("move", text=move)
            time.sleep(0.4)
            screenshot("kirigami.png")
            for page in ["appearance", "computer", "speech", "materials"]:
                action("gui", {"action": page})
                time.sleep(0.15)
                screenshot(f"kirigami-{page}.png")
            action("gui", {"action": "close_dialogs"})
            action("preferences", {"view": {"flat": True}})
            screenshot("kirigami-accessible.png")
            action("preferences", {"view": {"flat": False}})
            action("gui", {"action": "new"})
            screenshot("kirigami-new-game.png")
            action("gui", {"action": "close_dialogs"})
            action("metadata", {"Event": "Controller verification", "White": "Human"})
            action("comment", {"text": "An annotated opening"})
            assert app.status()["headers"]["Event"] == "Controller verification"
            assert app.status()["comment"] == "An annotated opening"
            app.command("undo")
            assert app.status()["ply"] == 5
            app.command("seek", ply=6)
            assert app.status()["comment"] == "An annotated opening"
            app.command("seek", ply=4)
            app.command("move", text="d2d4")
            assert app.status()["variations"] == [6]
            action("variation", {"index": 0})
            assert app.status()["ply"] == 6
            saved = ARTIFACTS / "kirigami-annotated.chess-linux"
            app.command("save", path=str(saved))
            assert json.loads(saved.read_text())["headers"]["Event"] == "Controller verification"
            action("duplicate")
            action("close", {"index": app.status()["active"]})
            assert app.status()["close_index"] is not None
            action("close_response", {"choice": "discard"})
            action("preferences", {"seconds": 0.05, "depth": 2, "engine_log": True})
            app.command("new", variant="standard", computer=[False, True])
            app.command("move", text="e2e4")
            wait_for(lambda: app.status()["ply"] == 2, "computer response in Kirigami")
            app.command("pause", paused=True)
            assert app.status()["paused"]
            for variant in ["crazyhouse", "suicide", "losers"]:
                app.command("new", variant=variant)
                app.command("move", text="e2e4")
                screenshot(f"kirigami-{variant}.png")
            app.command("new", variant="standard")
            app.command("set_fen", fen="4k3/P7/8/8/8/8/8/4K3 w - - 0 1")
            action("square", {"square": "a7"})
            action("square", {"square": "a8"})
            assert "Queen" in app.status()["promotion"]
            screenshot("kirigami-promotion.png")
            action("promote", {"role": "Queen"})
            assert app.status()["ply"] == 1
            action("ui_resize", {"width": 800, "height": 680})
            compact = screenshot("kirigami-compact.png")
            width, height = struct.unpack(">II", compact.read_bytes()[16:24])
            assert abs(width / height - 800 / 680) < 0.01, (width, height)
            action("ui_resize", {"width": 1280, "height": 880})
            movie = ARTIFACTS / "kirigami-recording.mp4"
            action("record", {"path": str(movie), "size": [1280, 880]})
            time.sleep(1.2)
            action("stop_record")
            wait_for(lambda: not app.status()["finishing_recording"], "Kirigami movie finalization")
            assert movie.stat().st_size > 1000
            app.command("quit")
            assert app.status()["close_index"] is not None
            screenshot("kirigami-unsaved.png")
            action("close_response", {"choice": "cancel"})
            # Verify that ordinary UI shutdown cleans up the controller and socket.
            while len(app.status()["tabs"]) > 1:
                action("close", {"index": len(app.status()["tabs"]) - 1})
                if app.status()["close_index"] is not None:
                    action("close_response", {"choice": "discard"})
            app.command("save", path=str(ARTIFACTS / "kirigami-test.chess-linux"))
            app.command("quit")
            wait_for(lambda: app.process.poll() is not None, "normal Kirigami shutdown")
            assert app.process.returncode == 0
            log = (ARTIFACTS / "integration-kirigami.log").read_text()
            assert "Qt Quick Vulkan scene graph verified" in log
            errors = [line for line in log.splitlines() if any(marker in line for marker in
                      ["ReferenceError", "TypeError", "Unable to assign", "failed to load", "Binding loop", "Error:"])]
            assert not errors, "\n".join(errors[:12])
            assert "Search depth" in log
            print("Kirigami Vulkan smoke passed: native windows, settings, variants, history/branches, metadata, computer play, promotion, accessible board, compact layout, video, save prompts and clean shutdown")
        finally:
            app.close()


if __name__ == "__main__":
    run()
