#!/usr/bin/env python3
"""Execute the built WASI module with Wawona's actual host function signatures.

Default: scripted Wayland peer exercises input, moves, resize, ping and close.
--wayland: forward protocol bytes + SCM_RIGHTS to a real Wayland compositor.
This test adapter is not a substitute for testing on a Wawona device.
"""
import argparse
import array
from collections import deque
import os
from pathlib import Path
import socket
import struct
import tempfile
import zlib

import wasmtime

ROOT = Path(__file__).resolve().parent.parent


def words(*values):
    return struct.pack("<" + "I" * len(values), *values)


def event(obj, opcode, payload=b""):
    return words(obj, ((len(payload) + 8) << 16) | opcode) + payload


def string(value):
    data = value.encode() + b"\0"
    return words(len(data)) + data + bytes(-len(data) % 4)


def write_png(path, frame, width, height):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            b, g, r, _ = frame[(y * width + x) * 4:(y * width + x + 1) * 4]
            rows.extend((r, g, b))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
                    + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


class Host:
    def __init__(self, real):
        self.real = real
        self.sock = None
        self.incoming = bytearray()
        self.objects = {1: "wl_display"}
        self.handles = {}
        self.next_fd = 100
        self.pool_data = {}
        self.buffer_data = {}
        self.commits = 0
        self.last_frame = None
        self.sizes = set()
        self.pong = False
        self.acked = False
        self.actions = deque()
        self.sync_id = None
        self.closed = False

    def memory(self, caller):
        return caller.get("memory")

    def write_i32(self, caller, address, value):
        self.memory(caller).write(caller, struct.pack("<i", value), address)

    def connect(self, caller, out):
        if self.real:
            self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.sock.settimeout(15)
            display = os.environ.get("WAYLAND_DISPLAY", "wayland-0")
            path = display if display.startswith("/") else str(Path(os.environ["XDG_RUNTIME_DIR"]) / display)
            self.sock.connect(path)
        self.write_i32(caller, out, 10)
        return 0

    def create(self, caller, size, out):
        fd = os.memfd_create("chess-wasm-check")
        os.ftruncate(fd, size)
        handle = self.next_fd
        self.next_fd += 1
        self.handles[handle] = fd
        self.write_i32(caller, out, handle)
        return 0

    def shm_write(self, caller, handle, offset, address, length):
        data = self.memory(caller).read(caller, address, address + length)
        assert os.pwrite(self.handles[handle], data, offset) == length
        return 0

    def close(self, caller, handle):
        if handle == 10:
            if self.sock:
                self.sock.close()
            self.closed = True
        else:
            os.close(self.handles.pop(handle))
        return 0

    def queue(self, obj, opcode, payload=b""):
        self.incoming.extend(event(obj, opcode, payload))

    def add(self, obj, kind):
        self.objects[obj] = kind
        if kind == "wl_pointer":
            self.pointer = obj
        elif kind == "wl_keyboard":
            self.keyboard = obj
        elif kind == "wl_touch":
            self.touch = obj
        elif kind == "xdg_toplevel":
            self.toplevel = obj
        elif kind == "xdg_surface":
            self.xdg_surface = obj
        elif kind == "xdg_wm_base":
            self.xdg_wm = obj

    def send(self, caller, handle, address, length, scm):
        assert handle == 10
        data = bytes(self.memory(caller).read(caller, address, address + length))
        obj, size_op = struct.unpack_from("<II", data)
        opcode, size = size_op & 0xffff, size_op >> 16
        assert size == len(data)
        payload = data[8:]
        kind = self.objects[obj]
        if self.real:
            rights = [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", [self.handles[scm]]))] if scm >= 0 else []
            sent = self.sock.sendmsg([data], rights)
            if sent < len(data):
                self.sock.sendall(data[sent:])
        if kind == "wl_display":
            new = struct.unpack_from("<I", payload)[0]
            self.add(new, "wl_registry" if opcode == 1 else "wl_callback")
            if self.real and opcode == 0 and self.commits:
                self.sync_id = new
            if not self.real:
                if opcode == 1:
                    for name, iface, version in [(1, "wl_compositor", 4), (2, "wl_shm", 1), (3, "xdg_wm_base", 2), (4, "wl_seat", 5)]:
                        self.queue(new, 0, words(name) + string(iface) + words(version))
                else:
                    self.queue(new, 0, words(1))
        elif kind == "wl_registry" and opcode == 0:
            name, n = struct.unpack_from("<II", payload)
            iface = payload[8:8 + n - 1].decode()
            version, new = struct.unpack_from("<II", payload, 8 + ((n + 3) // 4) * 4)
            self.add(new, iface)
            if not self.real and iface == "wl_seat":
                self.queue(new, 0, words(7))
        elif kind == "wl_compositor" and opcode == 0:
            self.add(struct.unpack_from("<I", payload)[0], "wl_surface")
        elif kind == "wl_seat" and opcode in (0, 1, 2):
            self.add(struct.unpack_from("<I", payload)[0], ["wl_pointer", "wl_keyboard", "wl_touch"][opcode])
        elif kind == "xdg_wm_base":
            if opcode == 2:
                self.add(struct.unpack_from("<I", payload)[0], "xdg_surface")
            elif opcode == 3:
                assert payload == words(42)
                self.pong = True
        elif kind == "xdg_surface":
            if opcode == 1:
                self.add(struct.unpack_from("<I", payload)[0], "xdg_toplevel")
            elif opcode == 4:
                self.acked = True
        elif kind == "wl_shm" and opcode == 0:
            new, size = struct.unpack("<II", payload)
            assert scm in self.handles
            self.add(new, "wl_shm_pool")
            self.pool_data[new] = os.pread(self.handles[scm], size, 0)
        elif kind == "wl_shm_pool":
            if opcode == 0:
                new, offset, width, height, stride, fmt = struct.unpack("<IIIIII", payload)
                assert offset == 0 and stride == width * 4 and fmt == 1
                self.add(new, "wl_buffer")
                self.buffer_data[new] = (self.pool_data[obj], width, height)
            elif opcode == 1:
                self.pool_data.pop(obj)
        elif kind == "wl_buffer" and opcode == 0:
            self.buffer_data.pop(obj)
        elif kind == "wl_surface":
            if opcode == 1:
                self.attached = struct.unpack_from("<I", payload)[0]
            elif opcode == 6:
                if not hasattr(self, "attached"):
                    if not self.real:
                        self.queue(self.toplevel, 0, words(640, 800, 0))
                        self.queue(self.xdg_surface, 0, words(1))
                else:
                    self.commits += 1
                    self.last_frame = self.buffer_data[self.attached]
                    self.sizes.add(self.last_frame[1:])
                    if not self.real:
                        self.next_action()
        return 0

    def next_action(self):
        if self.commits == 1:
            self.queue(self.xdg_wm, 0, words(42))
            def click(x, y):
                return (event(self.pointer, 0, words(1, 0, x * 256, y * 256))
                        + event(self.pointer, 3, words(1, 0, 0x110, 1)))
            # Board cell=76, origin=(16,88): e2 then e4.
            self.actions.extend([click(358, 582), click(358, 430)])
            # Type e7e5 followed by Enter, exercising the built wasm code.
            for key in [18, 8, 18, 6, 28]:
                self.actions.append(event(self.keyboard, 3, words(1, 0, key, 1)))
            self.actions.append(event(self.toplevel, 0, words(400, 560, 0)) + event(self.xdg_surface, 0, words(2)))
            def touch(x, y):
                return event(self.touch, 0, words(1, 0, 0, 0, x * 256, y * 256))
            # New game, enable AI, then play e2-e4 through wl_touch at compact size.
            self.actions.extend([touch(24, 56), touch(268, 56), touch(223, 387), touch(223, 295)])
            self.actions.append(event(self.toplevel, 1))
        if self.actions:
            self.incoming.extend(self.actions.popleft())

    def receive(self, caller, handle, address, length, out):
        assert handle == 10
        if self.real and not self.incoming:
            # Read complete protocol messages so the injected close cannot split one.
            header = self.exact(8)
            obj, size_op = struct.unpack("<II", header)
            payload = self.exact((size_op >> 16) - 8)
            if obj == self.sync_id:
                self.queue(self.toplevel, 1)
            else:
                self.incoming.extend(header + payload)
        assert self.incoming, "Guest blocked without a queued Wayland event"
        # Deliberately fragment the wire stream to test recv_exact.
        data = self.incoming[:min(length, 3)]
        del self.incoming[:len(data)]
        self.memory(caller).write(caller, data, address)
        self.write_i32(caller, out, len(data))
        return 0

    def exact(self, size):
        data = bytearray()
        while len(data) < size:
            part = self.sock.recv(size - len(data))
            assert part, "Compositor disconnected"
            data.extend(part)
        return bytes(data)


def run(module_path, real=False):
    host = Host(real)
    engine = wasmtime.Engine()
    module = wasmtime.Module.from_file(engine, str(module_path))
    allowed = {"wawona_wayland_connect", "wawona_wayland_shm_create", "wawona_wayland_shm_write",
               "wawona_wayland_sendmsg", "wawona_socket_recv", "wawona_socket_close"}
    for imp in module.imports:
        assert imp.module == "wasi_snapshot_preview1" or (imp.module == "env" and imp.name in allowed), (imp.module, imp.name)
    linker = wasmtime.Linker(engine)
    linker.define_wasi()
    for name, (count, callback) in {
        "wawona_wayland_connect": (1, host.connect),
        "wawona_wayland_shm_create": (2, host.create),
        "wawona_wayland_shm_write": (4, host.shm_write),
        "wawona_wayland_sendmsg": (4, host.send),
        "wawona_socket_recv": (4, host.receive),
        "wawona_socket_close": (1, host.close),
    }.items():
        linker.define_func("env", name, wasmtime.FuncType([wasmtime.ValType.i32()] * count, [wasmtime.ValType.i32()]), callback, access_caller=True)
    with tempfile.TemporaryDirectory(prefix="chess-wasm-test-") as temp:
        config = wasmtime.WasiConfig()
        config.argv = ["chess-wawona"]
        config.stdout_file = str(Path(temp) / "stdout")
        config.stderr_file = str(Path(temp) / "stderr")
        store = wasmtime.Store(engine)
        store.set_wasi(config)
        try:
            instance = linker.instantiate(store, module)
            instance.exports(store)["_start"](store)
        except wasmtime.ExitTrap as error:
            assert error.code == 0, Path(temp, "stderr").read_text()
        finally:
            output = Path(temp, "stdout").read_text()
            print(output, end="")
            print(Path(temp, "stderr").read_text(), end="")
            for fd in host.handles.values():
                os.close(fd)
            if host.sock:
                host.sock.close()
        assert host.closed and host.commits > 0 and host.acked
        assert not host.handles, "Guest leaked SHM handles"
        if not real:
            assert "4P3" in output and "4p3" in output, "Pointer/keyboard moves were not played"
            assert host.pong and (400, 560) in host.sizes
            assert output.count("position:") == 4, "Touch input / computer response failed"
        frame, width, height = host.last_frame
        assert len(set(struct.iter_unpack("<I", frame))) > 6, "Board was not rendered"
        write_png(ROOT / "artifacts" / ("wasm-weston.png" if real else "wasm-chess.png"), frame, width, height)
        print(f"WASI Wayland {'compositor' if real else 'interaction'} check passed ({host.commits} frames)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("module", nargs="?", type=Path, default=ROOT / "wasm/target/wasm32-wasip1/release/chess-wawona.wasm")
    parser.add_argument("--wayland", action="store_true")
    args = parser.parse_args()
    run(args.module, args.wayland)
