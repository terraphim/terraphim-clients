//! Checklist definitions, loaded from embedded TOML.
//!
//! Checklists used to be two literals inside `validate_checklist`, with a comment
//! pointing at "the checklist markdown files" that never existed in the repository.
//! That made the standard invisible to reviewers, impossible to change without a code
//! edit, and unparseable by the CI gate except through regex over Rust source.
//!
//! The definitions now live in `data/checklists/*.toml` and are embedded at build time,
//! the same way `guard_patterns.rs` embeds its JSON. Embedding (rather than reading at
//! runtime) keeps packaged installs and CI runners working with no data directory.
//!
//! A category belongs to exactly one checklist: the old code kept a global category
//! table and derived each checklist's set by intersecting it with the checklist's terms,
//! which meant a category could silently belong to several checklists or none.

use std::sync::OnceLock;

use serde::Deserialize;

/// Embedded at build time; add a checklist by adding a file and listing it here.
const SOURCES: &[(&str, &str)] = &[
    (
        "code_review.toml",
        include_str!("../data/checklists/code_review.toml"),
    ),
    (
        "security.toml",
        include_str!("../data/checklists/security.toml"),
    ),
];

/// One axis of a checklist, e.g. `tests` or `encryption`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub name: String,
    pub terms: Vec<String>,
}

/// A named checklist: a set of categories a text is expected to address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checklist {
    pub name: String,
    pub title: String,
    pub categories: Vec<Category>,
}

#[derive(Deserialize)]
struct RawChecklist {
    name: String,
    title: String,
    categories: Vec<RawCategory>,
}

#[derive(Deserialize)]
struct RawCategory {
    name: String,
    terms: Vec<String>,
}

static CHECKLISTS: OnceLock<Vec<Checklist>> = OnceLock::new();

/// Every checklist known to the binary, in file order, parsed once.
///
/// # Panics
/// Panics if an embedded definition is malformed. That is a build-time defect in a
/// file shipped with the binary, so failing loudly beats silently reporting an
/// incomplete checklist as satisfied.
pub fn all() -> &'static [Checklist] {
    CHECKLISTS.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(file, text)| {
                let raw: RawChecklist = toml::from_str(text)
                    .unwrap_or_else(|e| panic!("embedded checklist {file} is malformed: {e}"));
                Checklist {
                    name: raw.name.to_lowercase(),
                    title: raw.title,
                    categories: raw
                        .categories
                        .into_iter()
                        .map(|c| Category {
                            name: c.name.to_lowercase(),
                            terms: c.terms.into_iter().map(|t| t.to_lowercase()).collect(),
                        })
                        .collect(),
                }
            })
            .collect()
    })
}

/// Names accepted by `validate --checklist`, sorted so error messages are stable.
pub fn names() -> Vec<String> {
    let mut names: Vec<String> = all().iter().map(|c| c.name.clone()).collect();
    names.sort();
    names
}

/// Look up a checklist by exact (case-insensitive) name.
pub fn get(name: &str) -> Option<&'static Checklist> {
    let wanted = name.to_lowercase();
    all().iter().find(|c| c.name == wanted)
}

/// Does `text` contain `term` as a whole-word phrase?
///
/// Both sides are split into lowercase alphanumeric tokens, so `security` matches
/// "a security-check endpoint" and "our SECURITY posture", but not "insecurity", and
/// `error handling` matches "error-handling" because the separator is not significant.
///
/// This is the second admissible proof that a category is addressed, next to a
/// knowledge-graph pattern match: with the shipped graph, 21 of 28 checklist terms have
/// no pattern at all, so graph matching alone can never credit most categories.
pub fn contains_phrase(text: &str, term: &str) -> bool {
    let needle = tokens(term);
    if needle.is_empty() {
        return false;
    }
    let haystack = tokens(text);
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_slice())
}

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_checklist_parses_and_has_categories() {
        assert_eq!(all().len(), SOURCES.len(), "one checklist per source file");
        for checklist in all() {
            assert!(
                !checklist.categories.is_empty(),
                "{} has no categories",
                checklist.name
            );
            assert!(!checklist.name.is_empty() && !checklist.title.is_empty());
        }
    }

    #[test]
    fn category_names_are_unique_within_a_checklist() {
        for checklist in all() {
            let mut seen = std::collections::HashSet::new();
            for category in &checklist.categories {
                assert!(
                    seen.insert(&category.name),
                    "{} declares category '{}' twice",
                    checklist.name,
                    category.name
                );
            }
        }
    }

    #[test]
    fn checklist_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for checklist in all() {
            assert!(
                seen.insert(&checklist.name),
                "duplicate checklist {}",
                checklist.name
            );
        }
    }

    #[test]
    fn terms_are_lowercase_and_non_empty() {
        for checklist in all() {
            for category in &checklist.categories {
                assert!(
                    !category.terms.is_empty(),
                    "{}/{} has no terms",
                    checklist.name,
                    category.name
                );
                for term in &category.terms {
                    assert_eq!(*term, term.to_lowercase(), "term {term:?} is not lowercase");
                    assert!(
                        !term.trim().is_empty(),
                        "blank term in {}/{}",
                        checklist.name,
                        category.name
                    );
                }
            }
        }
    }

    /// Migration proof: the data must declare exactly the category sets the old
    /// global-table-plus-intersection code produced, or this change silently alters
    /// which categories a checklist requires.
    #[test]
    fn explicit_categories_match_historical_intersection() {
        let expected: &[(&str, &[&str])] = &[
            (
                "code_review",
                &[
                    "tests",
                    "documentation",
                    "error_handling",
                    "security",
                    "performance",
                ],
            ),
            (
                "security",
                &[
                    "authentication",
                    "authorization",
                    "input_validation",
                    "encryption",
                    "logging",
                ],
            ),
        ];
        for (name, categories) in expected {
            let checklist = get(name).unwrap_or_else(|| panic!("{name} missing"));
            let actual: Vec<&str> = checklist
                .categories
                .iter()
                .map(|c| c.name.as_str())
                .collect();
            assert_eq!(&actual, categories, "{name} category set changed");
        }
    }

    #[test]
    fn names_are_sorted() {
        let mut expected = names();
        expected.sort();
        assert_eq!(names(), expected);
        assert!(names().contains(&"code_review".to_string()));
    }

    #[test]
    fn get_is_case_insensitive_and_rejects_unknown() {
        assert!(get("code_review").is_some());
        assert!(get("CODE_REVIEW").is_some());
        assert!(get("no_such_checklist").is_none());
    }

    #[test]
    fn contains_phrase_respects_word_boundaries() {
        assert!(contains_phrase(
            "Ran a security check on the endpoint.",
            "security"
        ));
        assert!(contains_phrase(
            "Ran a security check on the endpoint.",
            "security check"
        ));
        assert!(contains_phrase("our SECURITY posture", "security"));
        assert!(!contains_phrase("insecurity is not a concern", "security"));
        assert!(!contains_phrase("authenticate the user", "auth"));
    }

    #[test]
    fn contains_phrase_ignores_separator_style() {
        assert!(contains_phrase(
            "this is an error-handling path",
            "error handling"
        ));
        assert!(contains_phrase(
            "input_validation added",
            "input validation"
        ));
        assert!(contains_phrase("added unit\ntest coverage", "unit test"));
    }

    #[test]
    fn contains_phrase_rejects_absent_and_empty() {
        assert!(!contains_phrase("nothing relevant here", "performance"));
        assert!(!contains_phrase("anything", ""));
        assert!(!contains_phrase("", "security"));
    }
}
