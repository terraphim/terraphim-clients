// EXP-301 (UF-044): differential test of fff-search 0.8.4 case-insensitive
// memmem kernels against a scalar oracle, under ASan on x86_64 (AVX2 path) and
// natively on aarch64 macOS (NEON path, host-divergence arm).
//
// Public entry points reached (fff-search src/case_insensitive_memmem.rs):
//   search(hay, needle_lower)            -> dispatches verify_avx2 for needle >= 32 (x86_64)
//   search_packed_pair(hay, needle_lower)-> dispatches search_packed_pair_avx2 (x86_64)
// Both take the needle already lower-cased and match case-insensitively against
// the haystack. The oracle is: lowercase(haystack) contains lowercase(needle).
//
// Corpus (from the block): needle lengths 1,2,15,16,17,31,32,33 and > haystack;
// haystack lengths at multiples of 16 and 32 +/-1; a match only in the final
// tail slice; bytes 0x80-0xFF; empty needle and empty haystack; plus >= 10,000
// seeded random cases. Any kernel/oracle disagreement, or any ASan report, is a
// finding. A final FNV-1a fingerprint over all results lets the x86_64 and
// aarch64 runs be compared for host divergence.

use fff_search::case_insensitive_memmem as cim;

fn oracle(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    let h = hay.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    h.windows(n.len()).any(|w| w == &n[..])
}

// Deterministic xorshift64* PRNG so the same seed reproduces byte-for-byte on
// both hosts (no rand crate -> lean -Zbuild-std ASan build, reproducible corpus).
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
    fn byte(&mut self) -> u8 {
        // Mix of ASCII letters (both cases), digits/punct, and 0x80-0xFF so the
        // ascii-swap-case and high-byte lanes are exercised.
        match self.next_u64() % 4 {
            0 => b'a' + (self.next_u64() % 26) as u8,
            1 => b'A' + (self.next_u64() % 26) as u8,
            2 => 0x80u8.wrapping_add((self.next_u64() % 0x80) as u8),
            _ => (self.next_u64() % 128) as u8,
        }
    }
}

struct Stats {
    cases: u64,
    divergences: u64,
    fp: u64, // FNV-1a over (search, packed_pair) results, in order
    first: Option<String>,
}
impl Stats {
    fn new() -> Self {
        Stats { cases: 0, divergences: 0, fp: 0xcbf29ce484222325, first: None }
    }
    fn feed(&mut self, tag: &str, hay: &[u8], needle: &[u8]) {
        let needle_lower = needle.to_ascii_lowercase();
        let exp = oracle(hay, needle);
        let r_search = cim::search(hay, &needle_lower);
        let r_pair = cim::search_packed_pair(hay, &needle_lower);
        for b in [r_search, r_pair] {
            self.fp ^= b as u64;
            self.fp = self.fp.wrapping_mul(0x100000001b3);
        }
        self.cases += 1;
        if r_search != exp || r_pair != exp {
            self.divergences += 1;
            if self.first.is_none() {
                self.first = Some(format!(
                    "[{tag}] hlen={} nlen={} oracle={} search={} packed_pair={} needle={:?} hay_head={:?}",
                    hay.len(),
                    needle.len(),
                    exp,
                    r_search,
                    r_pair,
                    &needle[..needle.len().min(48)],
                    &hay[..hay.len().min(48)],
                ));
            }
        }
    }
}

fn main() {
    let seed: u64 = std::env::var("EXP301_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_1301_ABCD_EF01);
    let random_cases: u64 = std::env::var("EXP301_RANDOM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_000);

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "other"
    };
    println!("EXP-301 start: arch={arch} seed={seed:#x} random_cases={random_cases}");

    let mut st = Stats::new();

    // ---- Enumerated boundary corpus -------------------------------------
    let needle_lens = [0usize, 1, 2, 15, 16, 17, 31, 32, 33, 40];
    let bases = [16usize, 32, 48, 64, 96, 128, 256];
    let mut hay_lens: Vec<usize> = vec![0, 1];
    for b in bases {
        hay_lens.push(b - 1);
        hay_lens.push(b);
        hay_lens.push(b + 1);
    }

    // Two needle byte kinds: ASCII letters (case-swap lane) and 0x80-0xFF (high lane).
    let needle_kinds: [fn(usize) -> Vec<u8>; 2] = [
        |n| (0..n).map(|i| b"AbCdEfGhIjKlMnOpQrStUvWxYz"[i % 26]).collect(),
        |n| (0..n).map(|i| 0x80u8.wrapping_add((i % 0x80) as u8)).collect(),
    ];

    for &nlen in &needle_lens {
        for kind in needle_kinds {
            let needle = kind(nlen);
            for &hlen in &hay_lens {
                // Filler distinct from the needle bytes where possible.
                let mut hay: Vec<u8> = (0..hlen).map(|i| b"0123456789"[i % 10]).collect();

                // no-match variant
                st.feed("enum-nomatch", &hay, &needle);

                // match only in the final tail slice: overwrite the last nlen bytes
                // with the needle (opposite case for letters, to force case-insensitive path)
                if nlen > 0 && nlen <= hlen {
                    let start = hlen - nlen;
                    for (j, &nb) in needle.iter().enumerate() {
                        hay[start + j] = if nb.is_ascii_alphabetic() {
                            nb ^ 0x20 // flip case in the haystack
                        } else {
                            nb
                        };
                    }
                    st.feed("enum-tailmatch", &hay, &needle);
                    // restore
                    for j in 0..nlen {
                        hay[start + j] = b"0123456789"[(start + j) % 10];
                    }
                }

                // needle strictly longer than haystack
                if nlen > 0 {
                    let longer: Vec<u8> = kind(hlen + 1);
                    st.feed("enum-needle>hay", &hay, &longer);
                }
            }
        }
    }

    // empty haystack + empty needle explicit
    st.feed("empty-both", &[], &[]);
    st.feed("empty-hay", &[], b"a");
    st.feed("empty-needle", b"abc", &[]);

    let enum_cases = st.cases;
    let enum_div = st.divergences;
    println!("enumerated boundary cases: {enum_cases} divergences={enum_div}");

    // ---- Seeded random corpus ------------------------------------------
    let mut rng = Rng(seed);
    for _ in 0..random_cases {
        let hlen = rng.below(320);
        let mut hay: Vec<u8> = (0..hlen).map(|_| rng.byte()).collect();
        // 50%: carve the needle out of the haystack (guaranteed structural match
        // pre-case-flip); 50%: fully random needle (usually no match).
        let needle: Vec<u8> = if hlen > 0 && rng.next_u64() % 2 == 0 {
            let nlen = 1 + rng.below(hlen.min(40));
            let start = rng.below(hlen - nlen + 1);
            let mut nd = hay[start..start + nlen].to_vec();
            // random case flips on both sides to exercise case-insensitive lanes
            for b in nd.iter_mut() {
                if b.is_ascii_alphabetic() && rng.next_u64() % 2 == 0 {
                    *b ^= 0x20;
                }
            }
            for h in hay.iter_mut() {
                if h.is_ascii_alphabetic() && rng.next_u64() % 3 == 0 {
                    *h ^= 0x20;
                }
            }
            nd
        } else {
            let nlen = rng.below(42);
            (0..nlen).map(|_| rng.byte()).collect()
        };
        st.feed("rand", &hay, &needle);
    }

    println!(
        "EXP-301 done: arch={arch} seed={seed:#x} total_cases={} total_divergences={} fingerprint={:#018x}",
        st.cases, st.divergences, st.fp
    );
    if let Some(f) = &st.first {
        println!("first divergence: {f}");
    }
    if st.divergences == 0 {
        println!("EXP-301 NO_EVIDENCE (this arm): zero kernel/oracle disagreements; ASan report separately");
    } else {
        println!("EXP-301 DIVERGENCE (this arm): {} disagreements -> correctness bug (NOT-UB unless ASan also fired)", st.divergences);
    }
}
