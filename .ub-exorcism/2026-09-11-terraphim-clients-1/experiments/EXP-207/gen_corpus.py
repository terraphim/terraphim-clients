#!/usr/bin/env python3
# EXP-207 hostile-LMDB corpus generator.
# Given a valid populated env dir (built by `exp207 init`), emit corrupted
# copies. Keeps data.mdb <= 12 MiB (FrecencyTracker SIZE_CAP_BYTES) so the
# open-time erase_if_oversized does not delete it before open. Deterministic.
#
# usage: gen_corpus.py <valid_env_dir> <corpus_parent_out>
import os, sys, shutil, struct, random

VALID = sys.argv[1]
OUT = sys.argv[2]
random.seed(0x2070)
os.makedirs(OUT, exist_ok=True)

data0 = open(os.path.join(VALID, "data.mdb"), "rb").read()
lock0 = None
lp = os.path.join(VALID, "lock.mdb")
if os.path.exists(lp):
    lock0 = open(lp, "rb").read()
print("valid data.mdb size", len(data0))
CAP = 12 * 1024 * 1024

# LMDB page size: default = OS page size = 4096 on x86_64 Linux.
PS = 4096

def emit(name, data):
    d = os.path.join(OUT, name)
    os.makedirs(d, exist_ok=True)
    if len(data) > CAP:
        data = data[:CAP]
    with open(os.path.join(d, "data.mdb"), "wb") as f:
        f.write(data)
    # provide a fresh lock file; LMDB recreates it if absent, but include the
    # valid one to mirror a real env layout.
    if lock0 is not None:
        with open(os.path.join(d, "lock.mdb"), "wb") as f:
            f.write(lock0)
    print("wrote", name, len(data), "bytes")

# 00: benign control (must open cleanly)
emit("00_control", data0)

# LMDB meta pages are page 0 and page 1. The MDB_meta struct sits after the
# 16-byte page header; mm_magic (u32 LE 0xBEEFC0DE) is at offset 16.
def meta_field(data, page, rel_off, newbytes):
    d = bytearray(data)
    off = page * PS + rel_off
    for i, b in enumerate(newbytes):
        if off + i < len(d):
            d[off + i] = b
    return bytes(d)

# 01: magic clobbered on both meta pages
d = meta_field(data0, 0, 16, b"\xde\xad\xbe\xef")
d = meta_field(d, 1, 16, b"\xde\xad\xbe\xef")
emit("01_bad_magic", d)

# 02: version field bumped to a huge value on both meta pages (offset 20)
d = meta_field(data0, 0, 20, struct.pack("<I", 0xFFFFFFFF))
d = meta_field(d, 1, 20, struct.pack("<I", 0xFFFFFFFF))
emit("02_bad_version", d)

# 03: mm_last_pg / root page numbers scribbled to point past EOF.
# The MDB_db root pgno lives inside the meta; scribble a wide window after magic
# so root/branch/leaf pgno fields get out-of-range values on both meta pages.
d = bytearray(data0)
for page in (0, 1):
    for i in range(24, 120):
        o = page*PS + i
        if o < len(d):
            d[o] = 0xFF
emit("03_bad_root_pgno", bytes(d))

# 04: meta txnid maxed (offset near end of meta struct) to force the higher
# meta page to be chosen, combined with a bad root
d = bytearray(data0)
for page in (0, 1):
    for i in range(96, 128):
        o = page*PS + i
        if o < len(d):
            d[o] = 0xFF
emit("04_bad_txnid_root", bytes(d))

# 05: interior/leaf page (page 2+) header + body scribble
d = bytearray(data0)
if len(d) > 2*PS:
    for i in range(PS):
        o = 2*PS + i
        if o < len(d):
            d[o] = (0xFF - (i & 0xFF)) & 0xFF
emit("05_page2_scribble", bytes(d))

# 06: every page's node-pointer region scribbled
d = bytearray(data0)
npages = len(d)//PS
for pg in range(2, npages):
    base = pg*PS + 16  # after page header
    for i in range(0, 64):
        o = base + i
        if o < len(d):
            d[o] = 0xFF
emit("06_nodeptr_clobber", bytes(d))

# 07: truncated mid-page
emit("07_truncated_tail", data0[:-200] if len(data0) > 400 else data0)

# 08: truncated to just the two meta pages + a bit
emit("08_truncated_meta_only", data0[:2*PS + 64])

# 09: random fuzz over the whole file (5% of bytes)
d = bytearray(data0)
for _ in range(max(1, len(d)//20)):
    i = random.randint(0, len(d)-1)
    d[i] = random.randint(0, 255)
emit("09_random_fuzz", bytes(d))

# 10: page header pgno fields set to point far beyond the map
d = bytearray(data0)
npages = len(d)//PS
for pg in range(npages):
    o = pg*PS  # pgno is first 8 bytes of the page header
    struct.pack_into("<Q", d, o, 0x00FF_FFFF)
emit("10_bad_page_pgno", bytes(d))

print("corpus dirs:", len([x for x in os.listdir(OUT) if os.path.isdir(os.path.join(OUT,x))]))
