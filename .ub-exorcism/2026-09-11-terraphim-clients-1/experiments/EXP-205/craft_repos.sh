#!/bin/bash
# EXP-205 hostile-repo corpus builder.
# Builds, under $1 (parent dir), several git repos that the harness drives through
# FilePicker::collect_files -> Repository::discover + status:
#   00_benign             control (must status cleanly)
#   01_deep_config        .git/config with a deep include.path chain + huge values
#   02_config_loop        self-referential include.path (parser loop/hang probe)
#   03_parent_config      hostile .git/config in a PARENT dir discover walks into
#   04_delta_inflate      packfile whose last delta object's result-size varint is
#                         inflated to ~2 GiB (CVE-2026-53585 allocation probe)
set -u
PARENT="${1:?usage: craft_repos.sh <parent>}"
rm -rf "$PARENT"
mkdir -p "$PARENT"

mkrepo() { # <dir>
  local d="$1"; mkdir -p "$d"; ( cd "$d"
    git init -q
    git config user.email a@b.c; git config user.name a
    printf 'needle line one\nsecond line\n' > file1.txt
    git add -A; git commit -qm c1
  )
}

# 00 benign control
mkrepo "$PARENT/00_benign"

# 01 deep include.path chain + oversized values
mkrepo "$PARENT/01_deep_config"
( cd "$PARENT/01_deep_config"
  cfg=.git/config
  {
    echo '[core]'
    echo "	bigvalue = $(head -c 200000 /dev/zero | tr '\0' 'A')"
  } >> "$cfg"
  prev="$cfg"
  for i in $(seq 1 200); do
    inc=".git/inc_$i"
    echo "[include]" >> "$prev"
    echo "	path = inc_$i" >> "$prev"
    echo "[core]" > "$inc"
    echo "	k$i = v$i" >> "$inc"
    prev="$inc"
  done
)

# 02 self-referential include loop
mkrepo "$PARENT/02_config_loop"
( cd "$PARENT/02_config_loop"
  echo '[include]' >> .git/config
  echo '	path = loopcfg' >> .git/config
  echo '[include]' > .git/loopcfg
  echo '	path = config' >> .git/loopcfg   # points back at .git/config
)

# 03 hostile config in a parent dir that discover walks through
mkdir -p "$PARENT/03_parent_config"
( cd "$PARENT/03_parent_config"
  git init -q --bare hidden_parent_marker 2>/dev/null || true
  # place a hostile .git/config at the parent, then a real repo below it
  mkdir -p .git
  {
    echo '[include]'
    for i in $(seq 1 300); do echo "	path = /nonexistent/deep/$i"; done
  } > .git/config
  mkrepo child
)

# 04 delta result-size inflation
mkrepo "$PARENT/04_delta_inflate"
( cd "$PARENT/04_delta_inflate"
  # second, similar file to encourage delta encoding, then force a delta pack
  printf 'needle line one\nsecond line\nthird similar line\n' > file2.txt
  cp file1.txt file3.txt; printf 'extra\n' >> file3.txt
  git add -A; git commit -qm c2
  git repack -a -d -f --window=250 --depth=250 -q 2>/dev/null
)
python3 "$(dirname "$0")/inflate_delta.py" "$PARENT/04_delta_inflate" 2>&1 || echo "inflate_delta.py: no delta object patched (documented)"

echo "=== corpus built under $PARENT ==="
ls -1 "$PARENT"
