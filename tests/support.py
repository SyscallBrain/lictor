"""Isolated Herdr socket fixture; never connects to a user's actual session."""
import copy
import json
import pathlib
import socket
import tempfile
import threading

def pane(workspace, tab, identifier, title, status="idle"):
    return {"workspace_id": workspace, "tab_id": tab, "pane_id": identifier,
            "terminal_id": "term_" + identifier, "terminal_title_stripped": title,
            "agent_status": status, "revision": 1, "focused": identifier == "wJ:p13"}


panes = [pane("wJ", "wJ:t9", "wJ:p13", "shell1"),
         pane("wJ", "wJ:t9", "wJ:p42", "reviewer", "working"),
         pane("wJ", "wJ:t20", "wJ:p66", "logs"),
         pane("wE", "wE:t8", "wE:p7", "builder", "blocked")]
agents = [dict(panes[1], name="reviewer", agent="claude", display_agent="claude"),
          dict(panes[3], name="builder", agent="claude", display_agent="claude")]
snapshot = {
    "version": "0.9.3", "protocol": 22,
    "focused_workspace_id": "wJ", "focused_tab_id": "wJ:t9", "focused_pane_id": "wJ:p13",
    "workspaces": [
        {"workspace_id": "wJ", "number": 1, "label": "api", "active_tab_id": "wJ:t20", "tab_count": 2},
        {"workspace_id": "wE", "number": 3, "label": "web", "active_tab_id": "wE:t8", "tab_count": 1}],
    "tabs": [
        {"workspace_id": "wJ", "tab_id": "wJ:t9", "number": 1, "label": "shells", "pane_count": 2},
        {"workspace_id": "wJ", "tab_id": "wJ:t20", "number": 2, "label": "logs", "pane_count": 1},
        {"workspace_id": "wE", "tab_id": "wE:t8", "number": 1, "label": "agents", "pane_count": 1}],
    "panes": panes, "agents": agents,
    "layouts": [{"workspace_id": w, "tab_id": t, "focused_pane_id": p,
                 "area": {"x": 0, "y": 0, "width": 80, "height": 24},
                 "panes": [{"pane_id": item["pane_id"], "rect": {"x": i * 40, "y": 0, "width": 40, "height": 24}}
                           for i, item in enumerate(item for item in panes if item["tab_id"] == t)],
                 "splits": [], "zoomed": False}
                for w, t, p in [("wJ", "wJ:t9", "wJ:p13"), ("wJ", "wJ:t20", "wJ:p66"), ("wE", "wE:t8", "wE:p7")]]}


class FakeHerdr:
    def __init__(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lictor-acceptance-")
        self.socket_path = pathlib.Path(self.temporary.name) / "test.sock"
        self.snapshot = copy.deepcopy(snapshot)
        self.calls = []
        self.listener = socket.socket(socket.AF_UNIX)
        self.listener.bind(str(self.socket_path))
        self.listener.listen()
        threading.Thread(target=self.serve, daemon=True).start()

    def serve(self):
        while True:
            try:
                connection, _ = self.listener.accept()
            except OSError:
                return
            try:
                with connection:
                    request = json.loads(connection.makefile("rb").readline())
                    self.calls.append((request["method"], request.get("params")))
                    result = {"type": "session_snapshot", "snapshot": self.snapshot} if request["method"] == "session.snapshot" else {"type": "ok"}
                    connection.sendall((json.dumps({"id": request["id"], "result": result}) + "\n").encode())
            except (OSError, ValueError, KeyError):
                pass

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.listener.close()
        self.temporary.cleanup()
