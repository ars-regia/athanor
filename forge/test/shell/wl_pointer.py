"""wl_pointer.py - a virtual pointer for the rig's headless sway, which has no input device
and so offers no wl_pointer: a client asking for a click would never get one. It speaks
the Wayland wire protocol directly (no library is in the rig image) and uses wlroots'
zwlr_virtual_pointer_manager_v1, which creates the device and with it the seat's pointer.
The scene moves it over sway's output, where cosmic-comp's window forwards what it gets.
"""

import os
import socket
import struct
import time

BTN_LEFT = 0x110
DISPLAY = 1
REGISTRY = 2
CALLBACK = 3
MANAGER = 4
POINTER = 5


def _string(text):
    raw = text.encode() + b"\0"
    return struct.pack("<I", len(raw)) + raw + b"\0" * (-len(raw) % 4)


class VirtualPointer:
    def __init__(self, display, width, height):
        runtime = os.environ["XDG_RUNTIME_DIR"]
        self.sock = socket.socket(socket.AF_UNIX)
        # A compositor that stops answering fails the check instead of hanging the rig.
        self.sock.settimeout(5)
        self.sock.connect(f"{runtime}/{display}")
        self.extent = (width, height)
        self.buffer = b""
        self._send(DISPLAY, 1, struct.pack("<I", REGISTRY))
        manager = self._roundtrip()["zwlr_virtual_pointer_manager_v1"]
        self._send(
            REGISTRY,
            0,
            struct.pack("<I", manager)
            + _string("zwlr_virtual_pointer_manager_v1")
            + struct.pack("<II", 1, MANAGER),
        )
        # No seat: the compositor attaches the device to its default one.
        self._send(MANAGER, 0, struct.pack("<II", 0, POINTER))
        self._roundtrip()
        # The seat grows its pointer, and its clients bind it, after the device exists.
        time.sleep(0.5)

    def _send(self, object_id, opcode, payload=b""):
        header = struct.pack("<II", object_id, ((8 + len(payload)) << 16) | opcode)
        self.sock.sendall(header + payload)

    def _read(self, count):
        while len(self.buffer) < count:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise ConnectionError("sway closed the Wayland connection")
            self.buffer += chunk
        data, self.buffer = self.buffer[:count], self.buffer[count:]
        return data

    def _roundtrip(self):
        """wl_display.sync, reading until its callback: the registry's globals on the way,
        by interface name."""
        found = {}
        self._send(DISPLAY, 0, struct.pack("<I", CALLBACK))
        while True:
            object_id, word = struct.unpack("<II", self._read(8))
            body = self._read((word >> 16) - 8)
            opcode = word & 0xFFFF
            if object_id == REGISTRY and opcode == 0:
                name, length = struct.unpack_from("<II", body)
                found[body[8 : 8 + length - 1].decode()] = name
            elif object_id == CALLBACK:
                return found
            elif object_id == DISPLAY and opcode == 0:
                raise RuntimeError(f"sway refused a request: {body!r}")

    @staticmethod
    def _stamp():
        return int(time.monotonic() * 1000) & 0xFFFFFFFF

    def move(self, x, y):
        self._send(POINTER, 1, struct.pack("<IIIII", self._stamp(), x, y, *self.extent))
        self._send(POINTER, 4)
        self._roundtrip()
        # The compositor inside forwards the motion, and its client takes the enter.
        time.sleep(0.4)

    def click(self):
        for state in (1, 0):
            self._send(POINTER, 2, struct.pack("<III", self._stamp(), BTN_LEFT, state))
            self._send(POINTER, 4)
            self._roundtrip()
            time.sleep(0.05)

    def close(self):
        self.sock.close()
