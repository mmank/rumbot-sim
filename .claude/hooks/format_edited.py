"""PostToolUse hook: format the file Claude just edited (rustfmt / ruff)."""

import json
import shutil
import subprocess
import sys
from pathlib import Path

payload = json.load(sys.stdin)
path = (payload.get("tool_input") or {}).get("file_path")
if not path:
    sys.exit(0)
p = Path(path)
parts = {s.lower() for s in p.parts}
# external/ holds other people's code, except rumbot-sim, which is ours.
foreign = "external" in parts and "rumbot-sim" not in parts
if foreign or "vendor" in parts or not p.is_file():
    sys.exit(0)

if p.suffix == ".rs":
    cmd = ["rustfmt", str(p)]
elif p.suffix == ".py":
    cmd = ["ruff", "format", "-q", str(p)]
else:
    sys.exit(0)

if shutil.which(cmd[0]) is None:
    sys.exit(0)
r = subprocess.run(cmd, capture_output=True, text=True)
if r.returncode != 0:
    # Surface parse errors to Claude without blocking the edit.
    print(f"{cmd[0]} failed on {p}:\n{r.stderr}", file=sys.stderr)
    sys.exit(1)
