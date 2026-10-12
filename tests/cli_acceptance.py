"""Independent CLI acceptance checks against a synthetic Herdr socket."""
import json
import pathlib
import os
import subprocess
import sys
from support import FakeHerdr


def main():
    binary = str(pathlib.Path(sys.argv[1]).resolve())
    with FakeHerdr() as server:
        socket_path, calls = server.socket_path, server.calls
        def focused(kind, identifier):
            return (kind + ".focus", {kind + "_id": identifier})


        cases = [
            ("clustered compatibility group order", ["-wA", "-n", "1"], 0, [focused("tab", "wJ:t9")]),
            ("dry-run filtered wait", ["-A", "--status", "done", "-d", "-n", "1"], 0, []),
            ("attached long status", ["--status=working", "-n", "1"], 0, [focused("pane", "wJ:p42")]),
            ("clustered group order", ["-AW", "-n", "1"], 0, [focused("pane", "wJ:p42")]),
            ("default pane scope", ["-n", "3"], 0, [focused("pane", "wJ:p13"), focused("pane", "wJ:p42"), focused("pane", "wJ:p13")]),
            ("default tab scope", ["-T", "-n", "3"], 0, [focused("tab", "wJ:t9"), focused("tab", "wJ:t20"), focused("tab", "wJ:t9")]),
            ("zero agent ordinal", ["-A", "-p", "0", "-n", "1"], 3, []),
            ("unknown typed prefix", ["-W", "-p", "foo=wJ", "-n", "1"], 3, []),
            ("typed split lane mismatch", ["-S", "w,a", "--left-targets", "pane=wJ:p13", "--dry-run"], 3, []),
            ("pane position", ["-p", "2", "-n", "1"], 0, [focused("pane", "wJ:p42")]),
            ("tab.pane position", ["-p", "2.1", "-n", "1"], 0, [focused("pane", "wJ:p66")]),
            ("workspace label location", ["-p", "web:1.1", "-n", "1"], 0, [focused("pane", "wE:p7")]),
            ("workspace number location", ["-p", "3:1.1", "-n", "1"], 0, [focused("pane", "wE:p7")]),
            ("agent ordinal", ["-A", "-p", "2", "-n", "1"], 0, [focused("pane", "wE:p7")]),
            ("tab position", ["-T", "-p", "2", "-n", "1"], 0, [focused("tab", "wJ:t20")]),
            ("explicit pane in tab mode", ["-T", "-p", "wE:p7", "-n", "1"], 0, [focused("pane", "wE:p7")]),
            ("workspace-only focus", ["-W", "-p", "api", "-n", "1"], 0, [focused("workspace", "wJ")]),
            ("mixed workspace first tab", ["-W", "-A", "-n", "1"], 0, [focused("tab", "wJ:t9")]),
            ("active tab alias", ["-W", "-A", "--active-tab", "-n", "1"], 0, [focused("workspace", "wJ")]),
            ("agent kind expands", ["-A", "-p", "claude", "-n", "2"], 0, [focused("pane", "wJ:p42"), focused("pane", "wE:p7")]),
            ("duplicate dwell", ["-p", "2,2,1", "-n", "3"], 0, [focused("pane", "wJ:p42"), focused("pane", "wJ:p42"), focused("pane", "wJ:p13")]),
            ("target context", ["-t", "web", "-p", "1", "-n", "1"], 0, [focused("pane", "wE:p7")]),
            ("case-sensitive name", ["-W", "-p", "API", "-n", "1"], 3, []),
            ("invalid status", ["-A", "--status", "finished"], 2, []),
            ("invalid selector atomicity", ["-p", "1,999", "-n", "1"], 3, []),
            ("dry-run side effects", ["-W", "-A", "--dry-run", "-n", "8"], 0, []),
            ("contradictory split group", ["-T", "-S", "w,a", "--dry-run"], 2, []),
            ("one-sided split selection", ["-S", "w,a", "--left-targets", "api", "--dry-run"], 0, []),
        ]
        failures = 0
        for label, arguments, expected_code, expected_focus in cases:
            calls.clear()
            try:
                process = subprocess.run([binary, "--socket-path", str(socket_path), "--seconds", "0.001", "--no-notify", *arguments], capture_output=True, text=True, timeout=4, env=dict(os.environ, XDG_RUNTIME_DIR=server.temporary.name))
                changed = [call for call in calls if call[0].endswith(".focus")]
                okay = process.returncode == expected_code and changed == expected_focus
                # Dry-run may read snapshots, but it may not mutate or register active jobs.
                if "--dry-run" in arguments:
                    okay = okay and all(call[0] in {"session.snapshot", "ping"} for call in calls)
                print(json.dumps({"case": label, "passed": okay, "exit": process.returncode,
                                  "focus_calls": changed, "error": process.stderr.strip()[:200]}))
            except subprocess.TimeoutExpired:
                okay = False
                print(json.dumps({"case": label, "passed": False, "timeout": True}))
            failures += not okay
        listings = [
            ("baseline pane listing", ["-l"], [("pane", "wJ:p13"), ("pane", "wJ:p42")]),
            ("all pane listing", ["-l", "-a"], [("pane", "wJ:p13"), ("pane", "wJ:p42"), ("pane", "wJ:p66"), ("pane", "wE:p7")]),
            ("workspace listing", ["-W", "-l"], [("workspace", "wJ"), ("workspace", "wE")]),
            ("baseline tab listing", ["-T", "-l"], [("tab", "wJ:t9"), ("tab", "wJ:t20")]),
            ("agent listing", ["-A", "-l"], [("agent", "wJ:p42"), ("agent", "wE:p7")]),
            ("agent status listing", ["-A", "--status", "blocked", "-l"], [("agent", "wE:p7")]),
            ("target listing scope", ["-t", "web", "-l"], [("pane", "wE:p7")]),
        ]
        for label, arguments, expected in listings:
            calls.clear()
            process = subprocess.run([binary, "--socket-path", str(socket_path), *arguments], capture_output=True, text=True, timeout=4, env=dict(os.environ, XDG_RUNTIME_DIR=server.temporary.name))
            rows = [tuple(line.split("\t")[:2]) for line in process.stdout.splitlines() if "\t" in line]
            okay = process.returncode == 0 and rows == expected and all(call[0] == "session.snapshot" for call in calls)
            failures += not okay
            print(json.dumps({"case": label, "passed": okay, "rows": rows}))
        print(f"{len(cases) + len(listings) - failures}/{len(cases) + len(listings)} independent CLI checks passed")
        return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
