use aho_corasick::{AhoCorasickBuilder, MatchKind};
fn main() {
    let text = "a\u{e9}b caf\u{e9} \u{1F600}x";
    for pats in [vec!["", "caf"], vec!["caf", ""], vec![""]] {
        let ac = AhoCorasickBuilder::new().ascii_case_insensitive(true).match_kind(MatchKind::LeftmostLongest).build(&pats).unwrap();
        let mut bad = 0; let mut n = 0;
        for m in ac.find_iter(text) {
            n += 1;
            if !text.is_char_boundary(m.start()) || !text.is_char_boundary(m.end()) { bad += 1; }
            let r = std::panic::catch_unwind(|| { let _ = &text[m.start()..m.end()]; });
            if r.is_err() { println!("PANIC slicing {}..{} pats={:?}", m.start(), m.end(), pats); }
        }
        println!("pats={:?} matches={} non_boundary={}", pats, n, bad);
    }
    // Non-empty valid-UTF-8 pattern control: boundaries must always hold.
    let ac = AhoCorasickBuilder::new().ascii_case_insensitive(true).match_kind(MatchKind::LeftmostLongest).build(["\u{e9}", "af"]).unwrap();
    let bad = ac.find_iter(text).filter(|m| !text.is_char_boundary(m.start()) || !text.is_char_boundary(m.end())).count();
    println!("non-empty patterns non_boundary={bad}");
}
