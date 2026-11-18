# Shared normalisation for the #253 capture.
#
# Sourced by capture_golden.sh, and exercised by normalise_test.sh.
#
# Three things are removed from a captured output:
#
#   - the self-reported "Search latency:" line, because it is timing, not
#     behaviour;
#   - every non-ASCII byte, because a golden is a document and documents in
#     this repository carry no emoji or other non-ASCII glyph;
#   - trailing blank lines, so a re-capture reproduces the committed golden
#     byte for byte and `git diff --check` stays clean.
#
# The byte removal uses `tr` under LC_ALL=C, NOT a sed character class. The
# octal range syntax is not portable between BSD and GNU sed: GNU sed 4.7
# retains the UTF-8 bytes and removes the ASCII around them, which would let
# a Linux --update bless corrupted output. `tr -d` removes exactly the bytes
# above 0x7F on both.

normalise() {
    tr -d '\200-\377' \
        | sed -E '/^Search latency:/d' \
        | awk '
            { lines[NR] = $0 }
            END {
                last = NR
                while (last > 0 && lines[last] ~ /^[[:space:]]*$/) last--
                for (i = 1; i <= last; i++) print lines[i]
            }'
}
