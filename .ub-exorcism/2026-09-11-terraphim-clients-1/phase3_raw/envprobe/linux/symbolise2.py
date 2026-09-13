#!/usr/bin/env python3
# Batched symboliser: one addr2line process per binary for all offsets.
import re, subprocess, sys, collections
log = sys.argv[1]; maxblocks = int(sys.argv[2]) if len(sys.argv) > 2 else 3
frame = re.compile(r"^\s+#(\d+) (\S+)\+0x([0-9a-f]+) (\S+)$")
blocks = []; cur = None
for line in open(log, errors="replace"):
    if line.startswith("[envprobe] OVERLAP"):
        cur = [line.rstrip()]; blocks.append(cur); continue
    m = frame.match(line)
    if cur is not None and m: cur.append(m.groups())
    elif cur is not None and not line.startswith("  #"): cur = None
need = collections.defaultdict(set)
for b in blocks:
    for (n, path, off, s) in b[1:]:
        if s == "?": need[path].add(off)
res = {}
for path, offs in need.items():
    offs = sorted(offs)
    out = subprocess.run(["addr2line", "-f", "-C", "-e", path] + ["0x"+o for o in offs], capture_output=True, text=True).stdout.splitlines()
    for i, o in enumerate(offs):
        res[(path, o)] = out[2*i] if 2*i < len(out) else "??"
def nm(path, off, s): return s if s != "?" else res.get((path, off), "??")
print(f"total overlap blocks with backtraces: {len(blocks)}")
sig = collections.Counter(); names = collections.Counter()
for b in blocks:
    names[re.search(r'getenv\("([^"]*)"\)', b[0]).group(1) + " " + ("call" if "(call)" in b[0] else "pre")] += 1
    chain = [nm(p, o, s) for (n, p, o, s) in b[1:]]
    sig[" | ".join(chain[2:10])] += 1
print("-- getenv names:"); [print(f"{v:5d}  {k}") for k, v in names.most_common()]
print("-- reader chain signatures (frames 2-9):"); [print(f"{v:5d}  {k}") for k, v in sig.most_common(12)]
print("---- first blocks, full ----")
for b in blocks[:maxblocks]:
    print(b[0])
    for (n, p, o, s) in b[1:]: print(f"  #{n} {p.split('/')[-1]}+0x{o} {nm(p, o, s)}")
