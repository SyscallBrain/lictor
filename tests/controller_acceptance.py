"""Exercise the real TUI in a PTY without touching a user's Herdr session."""
import fcntl
import json
import os
import pathlib
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time
from support import FakeHerdr


class Controller:
    def __init__(self, binary, server, *arguments):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 110, 0, 0))
        environment = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor", XDG_RUNTIME_DIR=server.temporary.name)
        environment.pop("NO_COLOR", None)
        self.process = subprocess.Popen([binary, "--socket-path", str(server.socket_path), "--tui", "--no-notify", *arguments], stdin=slave, stdout=slave, stderr=slave, start_new_session=True, env=environment)
        os.close(slave)
        self.output = b""
        self.read_until(b"LICTOR")

    def read(self, duration=0.2):
        end = time.monotonic() + duration
        while time.monotonic() < end:
            ready, _, _ = select.select([self.master], [], [], min(0.05, max(0, end-time.monotonic())))
            if ready:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    break
                if not data:
                    break
                self.output += data

    def read_until(self, marker, timeout=4):
        end = time.monotonic() + timeout
        def contains():
            text = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", self.output)
            return re.sub(rb"\s+", b"", marker) in re.sub(rb"\s+", b"", text)
        while not contains() and time.monotonic() < end:
            self.read(0.1)
        assert contains(), f"Missing {marker!r}: {self.output[-1200:]!r}"

    def send(self, keys):
        os.write(self.master, keys)
        self.read()

    def close(self):
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.read()
        os.close(self.master)


def main():
    binary = str(pathlib.Path(sys.argv[1]).resolve())
    with FakeHerdr() as server:
        controller = Controller(binary, server)
        try:
            assert not any(method.endswith(".focus") for method, _ in server.calls), "Opening TUI rotated focus"
            assert b"38;2;122;162;247" in controller.output, "Tokyo Night blue not rendered"
            controller.read_until(b"LICTOR")
            controller.send(b"/q")
            assert controller.process.poll() is None, "Typing q in search exited"
            controller.send(b"\x1b")
            controller.send(b"ggv j ")
            controller.read_until(b"\xe2\x8f\xb1 3s")
            controller.send(b"t")
            controller.read_until(b"seconds")
            # Ctrl-u replaces the field; zero is rejected without saving it.
            controller.send(b"\x150\r")
            # Ratatui emits only changed cells; this fragment is newly drawn.
            controller.read_until(b"must be finite")
            controller.send(b"\x15" + b"0.02\r")
            controller.read_until(b"0.02s")
            controller.send(b"i")
            controller.read_until(b"Desktop notification")
            # Iterations: 2, so the launched job finishes on its own.
            controller.send(b"j\r\x7f2\r")
            controller.send(b"\x1b")
            # Clear and deliberately select the first workspace, then launch.
            controller.send(b"ugg ")
            controller.send(b"\r")
            deadline = time.monotonic() + 3
            while len([c for c in server.calls if c[0].endswith(".focus")]) < 2 and time.monotonic() < deadline:
                controller.read(0.05)
            focuses = [c for c in server.calls if c[0].endswith(".focus")]
            assert focuses == [("workspace.focus", {"workspace_id": "wJ"})] * 2, focuses
            # The controller stays open after starting a job.
            assert controller.process.poll() is None, "Controller exited after starting a job"
            controller.send(b"q")
            controller.process.wait(timeout=5)
            controller.read()
            assert controller.process.returncode == 0, controller.output[-1200:]
            assert b"\x1b[?1049l" in controller.output, "Alternate screen was not restored"
            print(json.dumps({"tui_pty": True, "search_mode": True, "interval_shortcut": True, "interval_validation": True, "settings": True, "detached_finite_rotation": True, "stays_open": True, "terminal_restored": True}))
        finally:
            controller.close()
        controller = Controller(binary, server)
        try:
            controller.read_until(b"m split mode")
            # Split mode (workspaces | agents), swap to agents | workspaces, and
            # select every target of each lane without CLI arguments.
            controller.send(b"m")
            controller.read_until(b"Right")
            controller.send(b"s")
            controller.send(b"a")
            controller.send(b"l")
            controller.send(b"a")
            controller.send(b"\r")
            deadline = time.monotonic() + 4
            while not any(method == "layout.apply" for method, _ in server.calls) and time.monotonic() < deadline:
                controller.read(0.1)
            layouts = [params for method, params in server.calls if method == "layout.apply"]
            assert layouts, "Starting from the TUI did not request a split tab"
            root = layouts[-1]["root"]
            assert root["direction"] == "right"
            assert root["first"]["command"][2] == "agent", root
            assert root["second"]["command"][2] == "workspace", root
            # The fake API records the request without launching viewer panes.
            controller.send(b"?")
            controller.read_until(b"Keys")
            controller.read_until(b"Navigate")
            controller.send(b"\x1b")
            controller.send(b"q")
            controller.process.wait(timeout=3)
            assert controller.process.returncode == 0
            print(json.dumps({"split_mode": True, "split_lanes_swapped": True, "split_select_all": True, "split_launch_request": True, "help": True, "quit": True}))
        finally:
            controller.close()
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
