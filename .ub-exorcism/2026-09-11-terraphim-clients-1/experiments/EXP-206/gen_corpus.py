#!/usr/bin/env python3
# EXP-206 corrupted-SQLite corpus generator.
# Builds a valid Cursor-shaped db (cursorDiskKV, ItemTable, an FTS5 table), then
# emits corrupted variants: header tampering, truncated pages, page-body
# scribbles, and FTS5 shadow-table corruption. Deterministic.
import os, sqlite3, struct, sys, shutil, random

OUT = sys.argv[1] if len(sys.argv) > 1 else "corpus"
os.makedirs(OUT, exist_ok=True)
random.seed(0x2060)

base = os.path.join(OUT, "_base.sqlite")
if os.path.exists(base):
    os.remove(base)
con = sqlite3.connect(base)
cur = con.cursor()
cur.execute("CREATE TABLE cursorDiskKV (key TEXT, value TEXT)")
cur.execute("CREATE TABLE ItemTable (key TEXT, value TEXT)")
for i in range(200):
    cur.execute("INSERT INTO cursorDiskKV VALUES (?,?)",
                (f"composerData:{i}", ("payload-" + "x" * (i % 97)) ))
    cur.execute("INSERT INTO ItemTable VALUES (?,?)", (f"item:{i}", "v" * (i % 50)))
try:
    cur.execute("CREATE VIRTUAL TABLE ftsdoc USING fts5(body)")
    for i in range(200):
        cur.execute("INSERT INTO ftsdoc(body) VALUES (?)",
                    (f"the quick brown fox number {i} jumps composerData", ))
    fts5 = True
except sqlite3.OperationalError as e:
    fts5 = False
    print("fts5 unavailable in generator sqlite:", e)
con.commit()
con.close()
print("base built, fts5=", fts5, "size=", os.path.getsize(base))

def save(name, data):
    p = os.path.join(OUT, name)
    with open(p, "wb") as f:
        f.write(data)
    print("wrote", name, len(data), "bytes")

raw = open(base, "rb").read()
pagesize = struct.unpack(">H", raw[16:18])[0] or 65536
print("page size", pagesize, "pages", len(raw)//pagesize)

# 0: a benign control copy (must open and query cleanly)
save("00_control.db", raw)

# 1: magic string partially clobbered (still 16 bytes, wrong)
d = bytearray(raw); d[0:6] = b"SQLit\x00"; save("01_bad_magic.corrupt", bytes(d))

# 2: declared page-size field set to an invalid (non power of two) value
d = bytearray(raw); struct.pack_into(">H", d, 16, 1000); save("02_bad_pagesize.corrupt", bytes(d))

# 3: text encoding field set to an out-of-range value (valid: 1,2,3)
d = bytearray(raw); struct.pack_into(">I", d, 56, 9); save("03_bad_encoding.corrupt", bytes(d))

# 4: database size in pages (header 28..32) inflated far past the file
d = bytearray(raw); struct.pack_into(">I", d, 28, 0x0FFFFFFF); save("04_inflated_pagecount.corrupt", bytes(d))

# 5: truncated mid-page (drop the last 300 bytes)
save("05_truncated_tail.corrupt", raw[:-300])

# 6: truncated to just the header + a partial first page
save("06_truncated_header.corrupt", raw[:pagesize + 40])

# 7: page-body scribble in an interior btree page (page 2 area), 64 bytes
d = bytearray(raw)
start = pagesize  # start of page 2
for i in range(64):
    if start + i < len(d):
        d[start + i] = (0xFF - i) & 0xFF
save("07_page2_scribble.corrupt", bytes(d))

# 8: btree page-type byte set to an invalid value on several pages
d = bytearray(raw)
for pg in range(1, len(d)//pagesize):
    off = pg * pagesize
    if off < len(d):
        d[off] = 0x7F  # not a valid btree page type (2,5,10,13)
save("08_bad_pagetype.corrupt", bytes(d))

# 9: cell-pointer array clobbered on page 2 (bytes 8..8+2N region)
d = bytearray(raw)
hdr = pagesize + 8
for i in range(40):
    if hdr + i < len(d):
        d[hdr + i] = 0xFF
save("09_cellptr_clobber.corrupt", bytes(d))

# 10: freelist head pointer (header 32..36) points to a bogus page
d = bytearray(raw); struct.pack_into(">I", d, 32, 0x00FF_FFFF); save("10_bad_freelist.corrupt", bytes(d))

# 11: random-byte fuzz over the whole file after the header (10% of bytes)
d = bytearray(raw)
for _ in range(len(d)//10):
    i = random.randint(100, len(d)-1)
    d[i] = random.randint(0, 255)
save("11_random_fuzz.corrupt", bytes(d))

# 12: FTS5 shadow-table corruption (scribble every page that mentions a shadow name)
if fts5:
    d = bytearray(raw)
    marks = [b"ftsdoc_data", b"ftsdoc_idx", b"ftsdoc_content", b"ftsdoc_config"]
    for pg in range(len(d)//pagesize):
        off = pg*pagesize
        chunk = bytes(d[off:off+pagesize])
        if any(m in chunk for m in marks):
            for i in range(pagesize//4):
                d[off + pagesize//2 + i] = random.randint(0,255)
    save("12_fts5_shadow.corrupt", bytes(d))

print("corpus files:", len(os.listdir(OUT)))
