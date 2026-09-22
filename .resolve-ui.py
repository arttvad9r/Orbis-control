"""Resolve remaining slint section conflicts: theirs (finish-v01 rewrite) wins."""
import subprocess
import sys

files = subprocess.run(
    ["git", "diff", "--name-only", "--diff-filter=U"],
    capture_output=True, text=True,
).stdout.split()

slint = [f for f in files if f.endswith(".slint")]
print("slint conflicted:", slint)

for path in slint:
    src = open(path).read()
    out_lines = []
    state = 0  # 0 normal, 1 in-ours, 2 in-theirs
    for line in src.splitlines(keepends=True):
        if line.startswith("<<<<<<<"):
            state = 1
            continue
        if line.startswith("=======") and state == 1:
            state = 2
            continue
        if line.startswith(">>>>>>>") and state == 2:
            state = 0
            continue
        if state in (0, 2):
            out_lines.append(line)
    if state != 0:
        print(f"UNBALANCED markers in {path}")
        sys.exit(1)
    open(path, "w").writelines(out_lines)
    print(f"{path}: theirs-side kept, markers removed")
