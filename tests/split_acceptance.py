"""Live split acceptance, restricted to an explicitly owned Herdr proof session.

Set the same LICTOR_PROOF_* variables as herdr_protocol_proof.sh. Sources must
print SOURCE-A and SOURCE-B. This script never targets the default session.
"""
import base64
import json
import os
import pathlib
import re
import socket
import subprocess
import sys
import time


def main():
    binary = str(pathlib.Path(sys.argv[1]).resolve())
    session = os.environ.get("LICTOR_PROOF_SESSION", "")
    tab = os.environ.get("LICTOR_PROOF_SOURCE_TAB", "")
    source = os.environ.get("LICTOR_PROOF_SOURCE_PANE", "")
    if not re.fullmatch(r"lictor-proof-[a-zA-Z0-9-]+", session) or not tab or not source:
        raise SystemExit("Set LICTOR_PROOF_SESSION, LICTOR_PROOF_SOURCE_TAB, and LICTOR_PROOF_SOURCE_PANE to owned synthetic resources.")
    sessions = json.loads(subprocess.check_output(["herdr", "session", "list", "--json"], text=True))["sessions"]
    api_socket = next(s["socket_path"] for s in sessions if s["name"] == session)
    base = [binary, "--session", session, "--no-notify"]
    owned_jobs = []
    owned_workspaces = []
    processes = []

    def api(method, params=None):
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(5)
            connection.connect(api_socket)
            connection.sendall((json.dumps({"id": "lictor-live-acceptance", "method": method, "params": params or {}}) + "\n").encode())
            reply = json.loads(connection.makefile("rb").readline())
            assert "result" in reply, reply
            return reply["result"]

    def snapshot():
        return api("session.snapshot")["snapshot"]

    def owned_layout(identifier):
        return next(l for l in snapshot()["layouts"] if l["tab_id"] == identifier)

    def control(action, job):
        result = subprocess.run(base + ["--" + action, "--job", job], capture_output=True, text=True, timeout=8)
        assert result.returncode == 0, (action, result.stderr)
        return result.stdout

    def frame(pane):
        process = subprocess.Popen(["herdr", "--session", session, "terminal", "session", "observe", pane, "--cols", "100", "--rows", "30"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            # Readiness is bounded by a separate select rather than a blocking readline.
            import select
            ready, _, _ = select.select([process.stdout], [], [], 5)
            assert ready, "No observer frame"
            record = json.loads(process.stdout.readline())
            assert record["full"], record
            return base64.b64decode(record["bytes"]).decode("utf-8", "replace")
        finally:
            process.terminate()
            process.communicate(timeout=3)

    def start(arguments):
        process = subprocess.Popen(base + arguments, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        processes.append(process)
        import select
        assert select.select([process.stdout], [], [], 5)[0], "Split did not create a tab"
        line = process.stdout.readline()
        match = re.search(r"Created Lictor observation tab (\S+) \(job ([^)]+)\)", line)
        assert match, line + process.stderr.read() if process.poll() is not None else line
        identifier, job = match.groups()
        owned_jobs.append(job)
        panes = [p["pane_id"] for p in owned_layout(identifier)["panes"]]
        return process, identifier, job, panes

    before = snapshot()
    layout = next(l for l in before["layouts"] if l["tab_id"] == tab)
    sources = [p["pane_id"] for p in layout["panes"]]
    assert len(sources) >= 2 and source in sources
    source_terminals = {p["pane_id"]: p["terminal_id"] for p in before["panes"] if p["tab_id"] == tab}
    rectangles = [(p["pane_id"], p["rect"]) for p in layout["panes"]]
    workspace = layout["workspace_id"]
    try:
        # The destination must follow the current workspace, independently of
        # the first source in the automatically selected workspace queue.
        api("tab.focus", {"tab_id": tab})
        current = api("workspace.create", {"label": "lictor-proof-current", "cwd": "/tmp", "focus": True})["workspace"]["workspace_id"]
        owned_workspaces.append(current)
        process, identifier, job, panes = start(["-W", "-S", "w,p", "--right-targets", source, "-s", "0.15", "-n", "4", "-v"])
        _, stderr = process.communicate(timeout=10)
        assert process.returncode == 0, stderr
        assert owned_layout(identifier)["workspace_id"] == current, "View opened in the first source workspace"
        assert snapshot()["focused_tab_id"] == identifier, "New split tab was not focused"
        assert "advanced left lane" in stderr and "advanced right lane" in stderr, stderr
        assert "SOURCE-A" in frame(panes[1]), "Right lane lost its source terminal"
        control("close-view", job)
        owned_jobs.remove(job)
        api("workspace.close", {"workspace_id": current})
        owned_workspaces.remove(current)
        print(json.dumps({"split_opens_in_current_workspace": True, "both_lanes_advance": True}))

        # Workspace source must remain its originally active tab after the created
        # observation tab acquires focus.
        api("tab.focus", {"tab_id": tab})
        process, identifier, job, panes = start(["-S", "w,p", "--left-targets", workspace, "--right-targets", source, "-n", "2"])
        process.communicate(timeout=8)
        assert process.returncode == 0
        screen = frame(panes[0])
        assert "SOURCE-A" in screen and "SOURCE-B" in screen, "Workspace lost its original source tab"
        control("close-view", job)
        owned_jobs.remove(job)
        print(json.dumps({"original_active_workspace_tab_preserved": True}))

        # A strict single-advance limit leaves the second lane as a placeholder.
        process, identifier, job, panes = start(["-S", "t,p", "--left-targets", tab, "--right-targets", source, "-n", "1"])
        process.communicate(timeout=8)
        waiting = frame(panes[1])
        assert "Waiting for first lane" in waiting and "SOURCE-" not in waiting, waiting
        control("close-view", job)
        owned_jobs.remove(job)
        print(json.dumps({"single_advance_limit": True}))

        process, identifier, job, panes = start(["-S", "p,p", "--left-targets", ",".join(sources[:2]), "--right-targets", source, "-s", "0.25"])
        time.sleep(0.4)
        control("pause", job)
        paused = frame(panes[0])
        marker = "SOURCE-A" if "SOURCE-A" in paused else "SOURCE-B"
        assert marker in paused
        time.sleep(0.6)
        assert marker in frame(panes[0]), "Paused lane advanced"
        control("resume", job)
        time.sleep(0.35)
        control("stop", job)
        process.communicate(timeout=5)
        stopped = frame(panes[0])
        marker = "SOURCE-A" if "SOURCE-A" in stopped else "SOURCE-B"
        assert marker in stopped
        time.sleep(0.6)
        after_stop = frame(panes[0])
        assert marker in after_stop, "Stopped lane advanced"
        assert stopped != after_stop, "Retained view did not continue receiving source output"
        control("close-view", job)
        owned_jobs.remove(job)
        print(json.dumps({"split_pause_resume": True, "stop_freezes_target": True, "retained_output_stays_live": True}))

        process, identifier, job, panes = start(["-S", "t,p", "--left-targets", tab, "--right-targets", source, "-n", "2", "--close-on-exit"])
        process.communicate(timeout=8)
        assert process.returncode == 0
        assert all(t["tab_id"] != identifier for t in snapshot()["tabs"])
        owned_jobs.remove(job)
        print(json.dumps({"close_on_exit": True}))
        after = snapshot()
        source_layout = next(l for l in after["layouts"] if l["tab_id"] == tab)
        assert rectangles == [(p["pane_id"], p["rect"]) for p in source_layout["panes"]]
        assert source_terminals == {p["pane_id"]: p["terminal_id"] for p in after["panes"] if p["tab_id"] == tab}
        print(json.dumps({"source_geometry_and_terminal_identity_preserved": True}))
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
                process.communicate(timeout=8)
        for job in owned_jobs:
            subprocess.run(base + ["--close-view", "--job", job], capture_output=True, timeout=8)
        for workspace in owned_workspaces:
            api("workspace.close", {"workspace_id": workspace})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
