import re
src = open("/home/artt/Orbis-control-implementation/ui/components/icon.slint").read()
# Split per kind block
kinds = {}
cur = None
for line in src.splitlines():
    m = re.search(r"root\.kind == (\d+)", line)
    if m:
        cur = int(m.group(1))
        kinds.setdefault(cur, [])
    if cur is not None:
        mx = re.search(r"\bx:\s*([\d.]+)px", line)
        if mx:
            kinds[cur].append(float(mx.group(1)))
for k in sorted(kinds):
    xs = kinds[k]
    if xs:
        print(f"kind {k}: min_x={min(xs):.1f} ({xs})")
