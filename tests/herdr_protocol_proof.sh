#!/usr/bin/env bash
set -euo pipefail

# This proof only targets an explicitly named, disposable Herdr session.
# Create one with: herdr --session lictor-proof-<suffix> (and allow nested
# sessions in a temporary HERDR_CONFIG_PATH if running inside Herdr).
session=${LICTOR_PROOF_SESSION:-}
source_tab=${LICTOR_PROOF_SOURCE_TAB:-}
source_pane=${LICTOR_PROOF_SOURCE_PANE:-}
if [[ ! $session =~ ^lictor-proof-[a-zA-Z0-9-]+$ || -z $source_tab || -z $source_pane ]]; then
  echo 'Set LICTOR_PROOF_SESSION, LICTOR_PROOF_SOURCE_TAB, and LICTOR_PROOF_SOURCE_PANE to owned synthetic resources.' >&2
  exit 2
fi
herdr_args=(--session "$session")
snapshot=$(herdr "${herdr_args[@]}" api snapshot)
metadata=$(python3 - "$snapshot" "$source_tab" "$source_pane" <<'PY'
import json, sys
s=json.loads(sys.argv[1])["result"]["snapshot"]
tab,pane=sys.argv[2:]
layout=next(x for x in s["layouts"] if x["tab_id"]==tab)
assert len(layout["panes"]) >= 2, "proof source tab must have at least two synthetic panes"
assert pane in {p["pane_id"] for p in layout["panes"]}
print(json.dumps({"source_tab":tab,"source_panes":[p["pane_id"] for p in layout["panes"]],"source_rects":[p["rect"] for p in layout["panes"]],"focused_pane_before":s["focused_pane_id"],"workspace_id":layout["workspace_id"]},sort_keys=True))
PY
)
printf '%s\n' "$metadata"
workspace=$(python3 -c 'import json,sys;print(json.loads(sys.argv[1])["workspace_id"])' "$metadata")

# Fresh tab only: omit tab_id/pane_id so no existing source process can be replaced.
proof_socket=$(herdr session list --json | python3 -c 'import json,sys;name=sys.argv[1];print(next(s["socket_path"] for s in json.load(sys.stdin)["sessions"] if s["name"]==name))' "$session")
python3 - "$proof_socket" "$workspace" <<'PY'
import json, socket, sys
sock=socket.socket(socket.AF_UNIX); sock.connect(sys.argv[1])
req={"id":"lictor-proof-layout","method":"layout.apply","params":{"workspace_id":sys.argv[2],"tab_label":"lictor-proof-viewers","focus":False,"root":{"type":"split","direction":"right","ratio":0.5,"first":{"type":"pane","label":"proof-left","cwd":"/tmp","command":["/bin/sh","-c","sleep 120"]},"second":{"type":"pane","label":"proof-right","cwd":"/tmp","command":["/bin/sh","-c","sleep 120"]}}}}
sock.sendall(json.dumps(req).encode()+b"\n")
reply=json.loads(sock.makefile().readline())
assert "result" in reply, reply
print(json.dumps({"fresh_layout":reply["result"]["layout"]},sort_keys=True))
PY
after=$(herdr "${herdr_args[@]}" api snapshot)
python3 - "$snapshot" "$after" "$source_tab" <<'PY'
import json,sys
before=json.loads(sys.argv[1])["result"]["snapshot"]
after=json.loads(sys.argv[2])["result"]["snapshot"]
tab=sys.argv[3]
a=next(x for x in before["layouts"] if x["tab_id"]==tab)
b=next(x for x in after["layouts"] if x["tab_id"]==tab)
assert before["focused_pane_id"]==after["focused_pane_id"], "layout apply changed focus"
assert [(x["pane_id"],x["rect"]) for x in a["panes"]]==[(x["pane_id"],x["rect"]) for x in b["panes"]], "source pane geometry changed"
print(json.dumps({"focus_preserved":True,"source_geometry_preserved":True}))
PY

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for index in 1 2; do
  (timeout 3s herdr "${herdr_args[@]}" terminal session observe "$source_pane" --cols 40 --rows 10 >"$tmp/observer-$index.ndjson" || [[ $? -eq 124 ]]) &
done
wait || true
python3 - "$tmp" <<'PY'
import json, pathlib, sys
root=pathlib.Path(sys.argv[1])
for p in sorted(root.glob("observer-*.ndjson")):
 frames=[json.loads(line) for line in p.read_text().splitlines()]
 assert frames and frames[0]["type"]=="terminal.frame" and frames[0]["full"]
 assert any(x["seq"]>1 and not x["full"] for x in frames), "missing delta frame"
 print(json.dumps({"observer":p.name,"frames":[{"seq":f["seq"],"full":f["full"],"width":f["width"],"height":f["height"]} for f in frames]},sort_keys=True))
PY

# Exercise the Rust viewer itself against the synthetic multi-pane tab. A
# finite two-presentation run leaves its owned tab retained for frame checking.
lictor=${LICTOR_BIN:-"$(cd "$(dirname "$0")/.." && pwd)/target/debug/lictor"}
created=$("$lictor" "${herdr_args[@]}" -S t,p --left-targets "$source_tab" --right-targets "$source_pane" -n 2 --no-notify)
printf '%s\n' "$created"
readarray -t owned < <(python3 - "$created" <<'PY'
import re,sys
line=sys.argv[1]
m=re.search(r"Created Lictor observation tab (\S+) \(job ([^)]+)\)",line)
assert m, line
print(m.group(1));print(m.group(2))
PY
)
viewer_tab=${owned[0]}
job_id=${owned[1]}
snapshot=$(herdr "${herdr_args[@]}" api snapshot)
viewer_pane=$(python3 - "$snapshot" "$viewer_tab" <<'PY'
import json,sys
s=json.loads(sys.argv[1])["result"]["snapshot"]
layout=next(x for x in s["layouts"] if x["tab_id"]==sys.argv[2])
assert len(layout["panes"])==2
print(layout["panes"][0]["pane_id"])
PY
)
sleep 1
timeout 2 herdr "${herdr_args[@]}" terminal session observe "$viewer_pane" --cols 100 --rows 30 >"$tmp/viewer.ndjson" || [[ $? -eq 124 ]]
python3 - "$tmp/viewer.ndjson" <<'PY'
import base64,json,sys
frame=json.loads(open(sys.argv[1]).readline())
screen=base64.b64decode(frame["bytes"]).decode("utf-8","replace")
assert "SOURCE-A" in screen and "SOURCE-B" in screen, "Lictor viewer did not compose both source panes"
print(json.dumps({"lictor_viewer_full_tab":True,"source_panes":["SOURCE-A","SOURCE-B"]}))
PY
"$lictor" "${herdr_args[@]}" --close-view --job "$job_id"
