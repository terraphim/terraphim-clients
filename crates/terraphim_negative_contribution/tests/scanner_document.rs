use terraphim_negative_contribution::NegativeContributionScanner;
use terraphim_types::{FindingCategory, FindingSeverity};

const DOCUMENT: &str = include_str!("data/checkout.rs");

#[test]
fn scans_a_real_document_through_find_matches() {
    let findings = NegativeContributionScanner::new().scan_file("src/checkout.rs", DOCUMENT);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file, "src/checkout.rs");
    assert_eq!(findings[0].line, 10);
    assert_eq!(findings[0].severity, FindingSeverity::High);
    assert_eq!(findings[0].category, FindingCategory::Quality);
    assert!(findings[0].finding.contains("todo!"));
}
