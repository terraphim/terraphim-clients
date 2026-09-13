// EXP-204 (UF-043): verbatim copy of `parse_correction_pattern`
// (crates/terraphim_agent/src/learnings/hook.rs:227-275 at ae06cd2, all three
// arms), driven with ASCII and non-ASCII prompts. Byte offsets are computed
// on `to_lowercase().trim_start()` copies and applied to the original text.
fn parse_correction_pattern(text: &str) -> Option<(String, String)> {
    let lower = text.to_lowercase();
    let trimmed = lower.trim_start();

    // "use X instead of Y" (must start with "use")
    if let Some(use_idx) = trimmed.find("use ")
        && use_idx == 0
    {
        let text_after_use = &text[text.to_lowercase().trim_start().find("use ").unwrap() + 4..];
        let lower_after_use = text_after_use.to_lowercase();
        if let Some(instead_idx) = lower_after_use.find(" instead of ") {
            let corrected = text_after_use[..instead_idx].trim().to_string();
            let original = text_after_use[instead_idx + 12..]
                .trim()
                .trim_end_matches('.')
                .to_string();
            if !corrected.is_empty() && !original.is_empty() {
                return Some((original, corrected));
            }
        }
        // "use X not Y"
        if let Some(not_idx) = lower_after_use.find(" not ") {
            let corrected = text_after_use[..not_idx].trim().to_string();
            let original = text_after_use[not_idx + 5..]
                .trim()
                .trim_end_matches('.')
                .to_string();
            if !corrected.is_empty() && !original.is_empty() {
                return Some((original, corrected));
            }
        }
    }

    // "prefer X over Y" (must start with "prefer")
    if let Some(prefer_idx) = trimmed.find("prefer ")
        && prefer_idx == 0
    {
        let text_after_prefer =
            &text[text.to_lowercase().trim_start().find("prefer ").unwrap() + 7..];
        let lower_after_prefer = text_after_prefer.to_lowercase();
        if let Some(over_idx) = lower_after_prefer.find(" over ") {
            let corrected = text_after_prefer[..over_idx].trim().to_string();
            let original = text_after_prefer[over_idx + 6..]
                .trim()
                .trim_end_matches('.')
                .to_string();
            if !corrected.is_empty() && !original.is_empty() {
                return Some((original, corrected));
            }
        }
    }

    None
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info.payload().downcast_ref::<String>().cloned()
            .or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        println!("    panic: {msg}");
    }));
    let cases: &[(&str, &str)] = &[
        ("control ASCII", "use bun instead of npm"),
        ("control 'not' arm", "use bun not npm"),
        ("control 'prefer' arm", "prefer bun over npm"),
        ("ASCII leading spaces (silent corruption)", "  use bun instead of npm"),
        ("ASCII leading tab", "\tuse bun instead of npm"),
        ("ASCII leading spaces, prefer arm", "   prefer bun over npm"),
        ("em-space leading (U+2003 x2)", "\u{2003}\u{2003}use bun instead of npm"),
        ("NBSP leading (U+00A0)", "\u{a0}use bun instead of npm"),
        ("U+0130 in X (case fold grows)", "use \u{130}\u{130} instead of npm"),
        ("U+0130 in X, not arm", "use \u{130} not npm"),
        ("U+0130 in X, prefer arm", "prefer \u{130}\u{130}\u{130} over npm"),
        ("Kelvin sign U+212A in X (fold shrinks)", "use \u{212a}\u{212a}\u{212a} instead of npm"),
        ("Cyrillic (ASCII-length-preserving fold)", "use Бан instead of npm"),
        ("emoji in Y", "use bun instead of npm 😀"),
        ("uppercase prefix", "USE bun INSTEAD OF npm"),
        ("ideographic space leading (U+3000)", "\u{3000}use bun instead of npm"),
        ("U+0130 x10 in X (offset past end)", "use \u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130} instead of n"),
        ("U+0130 x3 in X, 'not' arm short Y", "use \u{130}\u{130}\u{130} not n"),
        ("em-space between words", "use bun\u{2003}instead of npm"),
        ("U+0130 then em-space", "use \u{130} instead of\u{2003}npm"),
    ];
    let mut panics = 0;
    let mut wrong = 0;
    for (label, input) in cases {
        print!("[{label}] {input:?} -> ");
        match std::panic::catch_unwind(|| parse_correction_pattern(input)) {
            Ok(v) => {
                println!("{v:?}");
                if let Some((o, c)) = &v {
                    if o != "npm" && o != "npm 😀" || !(c == "bun" || c.starts_with('\u{130}') || c.starts_with('\u{212a}') || c == "Бан") {
                        wrong += 1;
                        println!("    WRONG tuple (expected original=npm, corrected=X)");
                    }
                }
            }
            Err(_) => { panics += 1; println!("PANIC"); }
        }
    }
    println!("EXP-204: cases={} panics={} wrong_tuples={}", cases.len(), panics, wrong);
    println!("EXP-204 NO_EVIDENCE for UB (safe-Rust panic / data corruption only){}", if panics > 0 { "; panic confirmed" } else { "" });
}
