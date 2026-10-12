"""Verify pause, stop acknowledgement, and dynamic membership with a fake socket."""
import json
import os
import pathlib
import subprocess
import sys
import time
from support import FakeHerdr


def wait_for(condition, description, timeout=4):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if condition():
            return
        time.sleep(0.02)
    raise AssertionError("Timed out: " + description)


def main():
    binary = str(pathlib.Path(sys.argv[1]).resolve())
    with FakeHerdr() as server:
        env = dict(os.environ, XDG_RUNTIME_DIR=server.temporary.name)
        base = [binary, "--socket-path", str(server.socket_path), "--no-notify"]
        focuses = lambda: [call for call in server.calls if call[0].endswith(".focus")]
        process = subprocess.Popen(base + ["-p", "2", "-s", "0.08"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        try:
            directory = pathlib.Path(server.temporary.name) / f"lictor-{os.getuid()}"
            wait_for(lambda: bool(list(directory.glob("*.json"))), "job registration")
            record = json.loads(next(directory.glob("*.json")).read_text())
            identifier = record["id"]
            wait_for(lambda: len(focuses()) >= 1, "first focus")
            pause = subprocess.run(base + ["--pause", "--job", identifier], capture_output=True, text=True, env=env, timeout=5)
            assert pause.returncode == 0, pause.stderr
            before = len(focuses())
            time.sleep(0.3)
            assert len(focuses()) == before, "Focus advanced after pause acknowledgement"
            resume = subprocess.run(base + ["--resume", "--job", identifier], capture_output=True, text=True, env=env, timeout=5)
            assert resume.returncode == 0, resume.stderr
            wait_for(lambda: len(focuses()) > before, "resumed focus")
            stop = subprocess.run(base + ["--stop", "--job", identifier], capture_output=True, text=True, env=env, timeout=5)
            assert stop.returncode == 0, stop.stderr
            stopped_count = len(focuses())
            process.wait(timeout=2)
            time.sleep(0.15)
            assert len(focuses()) == stopped_count, "Focus advanced after stop acknowledgement"
            assert not list(directory.glob("*.json")), "Stopped focus job was not deregistered"
            print(json.dumps({"pause_acknowledgement": True, "resume": True, "stop_acknowledgement": True, "registry_cleanup": True}))
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=3)
        # A record with a correct UID/PID/start time but the wrong executable must
        # not signal a separate, harmless synthetic process.
        sleeper = subprocess.Popen(["sleep", "30"])
        try:
            stat = pathlib.Path(f"/proc/{sleeper.pid}/stat").read_text().rsplit(")", 1)[1].split()
            executable = pathlib.Path(binary).stat()
            forged = dict(record, id="wrong-executable", pid=sleeper.pid, start_ticks=int(stat[19]), executable_device=executable.st_dev, executable_inode=executable.st_ino)
            (directory / "wrong-executable.json").write_text(json.dumps(forged))
            rejected = subprocess.run(base + ["--stop", "--job", "wrong-executable"], capture_output=True, text=True, env=env, timeout=5)
            assert rejected.returncode == 3, rejected.stderr
            assert sleeper.poll() is None, "An unrelated executable received a Lictor stop signal"
            print(json.dumps({"executable_identity_checked": True}))
        finally:
            sleeper.terminate()
            sleeper.wait(timeout=3)
        # A restarted server may reuse opaque pane/tab handles. An old ownership
        # record must neither hide nor close new terminals with different IDs.
        stale = dict(record, id="stale-view", state="view-only", tab_id="wJ:t9", pane_ids=["wJ:p13", "wJ:p42"], terminal_ids=["old-terminal-one", "old-terminal-two"], socket=str(server.socket_path))
        (directory / "stale-view.json").write_text(json.dumps(stale))
        listing = subprocess.run(base + ["-T", "--list"], capture_output=True, text=True, env=env, timeout=5)
        assert listing.returncode == 0 and "wJ:t9" in listing.stdout, "Stale view ownership hid a new source tab"
        cleanup = subprocess.run(base + ["--close-view", "--job", "stale-view"], capture_output=True, text=True, env=env, timeout=5)
        assert not any(method == "tab.close" for method, _ in server.calls), "Stale ownership closed new source terminals"
        print(json.dumps({"reused_handles_do_not_hide_or_close_sources": True}))
        server.calls.clear()
        process = subprocess.Popen(base + ["-A", "--status", "done", "-s", "0.03", "-n", "1"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        try:
            time.sleep(0.15)
            assert not focuses(), "Filtered agent focused prematurely"
            server.snapshot["agents"][0]["agent_status"] = "done"
            process.wait(timeout=3)
            assert process.returncode == 0, process.stderr.read()
            assert focuses() == [("pane.focus", {"pane_id": "wJ:p42"})], focuses()
            print(json.dumps({"filtered_queue_waits": True, "live_status_rechecked": True}))
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=3)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
