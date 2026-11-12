//! Refuse a directory tree before fff-search indexes it.
//!
//! Two mechanisms converge on the same place, `FileItem::write_absolute_path`
//! (`fff-search-0.8.4/src/types.rs:337-357`). It writes the base path, a
//! separator and the relative path into ONE `[u8; PATH_BUF_SIZE]` buffer, lets
//! `ChunkedString::read_to_buf` truncate the relative portion to whatever space
//! is left, and returns the result through
//! `core::str::from_utf8_unchecked(&buf[..total])`.
//!
//! 1. **Lossy storage inflates hostile names.** fff-search stores every relative
//!    path as `to_string_lossy().into_owned()`, so on Linux each invalid byte in
//!    an entry name becomes a three-byte U+FFFD and the name inflates threefold.
//!    The truncation then cuts inside a replacement character.
//!
//! 2. **The base path shares the budget.** Because base and relative are written
//!    into the same 4096 bytes, a long base path leaves little room for the
//!    relative portion, and the cut lands mid-character in ordinary valid
//!    multi-byte names. No hostile name is needed: the entry names of EXP-201
//!    are sufficient but not necessary. A base path over 4096 bytes is worse
//!    still -- `buf[..base_len].copy_from_slice(base)` panics outright.
//!
//! Either way the returned `&str` is not valid UTF-8. The call is reached from
//! `FileItem::absolute_path` and from `write_absolute_path`'s five callers
//! (`file_picker.rs:424`, `file_picker.rs:669`, `types.rs:428`, `types.rs:471`,
//! `scan.rs:408`), so both `get_content_for_search` and `collect_files`'
//! git-status lookup take it for every file.
//!
//! The base path is attacker-chosen: `terraphim-grep --paths` and the MCP
//! tools' `path` argument both hand it straight to fff-search. Upstream offers
//! no path-length cap, no strict-UTF-8 mode and no non-lossy storage switch, so
//! the remedy has to sit in front of the library. See UF-040 / #296.

use std::ffi::OsStr;
use std::path::Path;

/// Mirrors fff-search's `PATH_BUF_SIZE` (`simd_path.rs`).
///
/// Not configurable upstream, so it is reproduced here. If a future fff-search
/// changes it, this guard becomes conservative rather than unsound.
const PATH_BUF_SIZE: usize = 4096;

/// The number of bytes `write_absolute_path` consumes before the relative path.
///
/// Mirrors that function's own arithmetic exactly (`types.rs:343-351`) so the
/// two cannot drift: the base path's encoded bytes, plus one separator unless
/// the base already ends in `MAIN_SEPARATOR`.
///
/// This counts the root's *raw* bytes, while the call sites hand fff-search
/// `base_path: search_path.to_string_lossy().to_string()`. The two agree only
/// because `reject_hostile_tree` has already refused a root that is not valid
/// UTF-8, which makes `to_string_lossy` a no-op on it. That is the invariant the
/// whole length rule rests on: without it a root with invalid bytes would be
/// stored up to three times longer than measured here, and the arithmetic would
/// understate what the buffer actually holds.
fn base_prefix_len(base: &Path) -> usize {
    let bytes = base.as_os_str().as_encoded_bytes();
    let base_len = bytes.len();
    let sep_len =
        usize::from(base_len > 0 && bytes[base_len - 1] != std::path::MAIN_SEPARATOR as u8);
    base_len + sep_len
}

/// Decide one directory entry, returning its length relative to the search root.
///
/// `base_end_idx` is what `base_prefix_len` returned for the search root: the
/// buffer is shared, so the length rule has to be applied to the total, not to
/// the relative portion alone.
///
/// Split out from the walk so both rules can be exercised without asking the
/// host filesystem for a geometry it refuses to create -- macOS caps a single
/// pathname at 1024 bytes and Linux at 4096, so the interesting combinations of
/// a long base and a long relative path cannot always be built on disk even
/// though fff-search would mishandle them.
fn check_entry(
    base: &Path,
    dir: &Path,
    name: &OsStr,
    base_end_idx: usize,
    parent_rel_len: usize,
) -> Result<usize, String> {
    if name.to_str().is_none() {
        return Err(format!(
            "refusing to search {}: entry {:?} under {} has a name that is not \
             valid UTF-8. fff-search stores paths lossily, which inflates each \
             invalid byte threefold, and the shared {PATH_BUF_SIZE}-byte path \
             buffer then truncates inside a replacement character and returns \
             the result through `from_utf8_unchecked` (UF-040 / #296).",
            base.display(),
            name,
            dir.display(),
        ));
    }

    // Relative length as fff-search stores it: the parent's relative length, a
    // separator once there is a parent component, and the name.
    let name_len = name.as_encoded_bytes().len();
    let rel_len = if parent_rel_len == 0 {
        name_len
    } else {
        parent_rel_len + 1 + name_len
    };

    // `write_absolute_path` fills one buffer with base, separator and relative,
    // so the budget is shared.
    //
    // Its exact truncation threshold is `base_end_idx + rel_len > PATH_BUF_SIZE`:
    // the relative portion is read into `&mut buf[base_end_idx..]`, and
    // `read_to_buf` takes `byte_len.min(buf.len())`, so a total of exactly
    // `PATH_BUF_SIZE` fits and nothing is cut. This guard uses `>=`, one byte
    // stricter, DELIBERATELY -- do not "correct" it to `>`:
    //
    // - One rule then also covers `write_relative_cstr`, whose own threshold is
    //   `rel_len >= PATH_BUF_SIZE` because it reserves its buffer's last byte for
    //   a NUL (`types.rs:366-375`). Since `base_end_idx >= 0`, this condition
    //   subsumes that one and no second branch is needed.
    // - It keeps the guard off the exact dependency boundary, which would
    //   otherwise have to be re-derived on every fff-search upgrade.
    //
    // The cost is that an absolute path summing to exactly `PATH_BUF_SIZE` bytes
    // is refused although `write_absolute_path` alone would handle it. That is
    // intended.
    if base_end_idx + rel_len >= PATH_BUF_SIZE {
        return Err(format!(
            "refusing to search {}: the absolute path to {:?} under {} would be \
             {} bytes ({base_end_idx} for the search root and its separator, plus \
             {rel_len} relative), at or over fff-search's {PATH_BUF_SIZE}-byte path \
             buffer. `write_absolute_path` writes both halves into that one buffer \
             and truncates whatever does not fit, then returns the result through \
             `from_utf8_unchecked` -- which cuts a multi-byte character in half. \
             Searching from a shorter root is usually enough to fix this \
             (UF-040 / #296).",
            base.display(),
            name,
            dir.display(),
            base_end_idx + rel_len,
        ));
    }

    Ok(rel_len)
}

/// Reject a tree that would defeat fff-search's `from_utf8_unchecked`.
///
/// Fails, naming the offending entry, when any of
///
/// - the search root's own path is not valid UTF-8 -- it would be stored lossily,
///   inflating it and leaving a name that no longer refers to the directory;
/// - an entry name is not valid UTF-8 -- lossy storage inflates it threefold and
///   the truncation then cuts inside a replacement character;
/// - the search root alone fills `PATH_BUF_SIZE`, which makes
///   `write_absolute_path`'s `copy_from_slice` panic rather than truncate; or
/// - the root plus a relative path reaches `PATH_BUF_SIZE`. Base and relative
///   share one buffer (`types.rs:337-357`), so the budget is joint.
///
/// The third rule is not merely a conservative extension. It covers ordinary
/// trees of *valid* names reached from a long root, where the truncation cuts a
/// multi-byte character in half with no hostile name involved -- the EXP-201
/// geometry is sufficient but not necessary. EXP-201 did not test this
/// combination, so it is reasoned from `write_absolute_path` and `read_to_buf`
/// rather than observed.
///
/// The walk reads names only -- `DirEntry::file_type` uses the directory
/// entry's type, with no `stat` -- and does not follow symbolic links, matching
/// the `follow_symlinks: false` the pickers now assert. An unreadable directory
/// is skipped rather than reported: fff-search will skip it too, and failing
/// here would turn an ordinary permission quirk into a refused search.
///
/// Residual risk: a TOCTOU window remains. An entry created between this walk
/// and fff-search's `collect_files` still reaches the inflation. That needs an
/// active concurrent writer inside the searched tree, whereas the confirmed
/// exposure is a static hostile checkout.
///
/// The walk does not honour `.gitignore`, so `target/` and `node_modules/` are
/// walked. For a CLI invocation and a per-call MCP picker that cost is
/// acceptable; both already pay a full `collect_files` scan immediately after.
pub fn reject_hostile_tree(base: &Path) -> Result<(), String> {
    // fff-search receives `base_path` as a `String`. `terraphim_grep`'s call site
    // builds it with `to_string_lossy()`, which replaces each invalid byte with a
    // three-byte U+FFFD, so a root with invalid bytes would be stored up to three
    // times longer than `base_prefix_len` measures -- defeating the arithmetic
    // below, and panicking `copy_from_slice` outright once the inflated root
    // alone reaches the buffer. Refusing such a root restores the invariant.
    //
    // It is the right answer regardless of the arithmetic: the lossily stored
    // name no longer names the on-disk entry, so fff-search cannot open anything
    // under it. That is the same silent-drop defect the audit recorded for
    // hostile entry names, and a named error beats searching and finding nothing.
    //
    // Reachable through `terraphim-grep --paths` and `with_search_paths`, since
    // clap builds `Vec<PathBuf>` from `OsString` and invalid bytes survive on
    // Linux. The MCP tools are immune -- their `path` arrives as JSON and is
    // always valid UTF-8 -- but both copies carry the check so neither depends on
    // where it was called from.
    //
    // The root is never an entry the walk discovers, so the per-entry UTF-8 rule
    // in `check_entry` does not cover it.
    if base.to_str().is_none() {
        return Err(format!(
            "refusing to search {:?}: the search root's own path is not valid \
             UTF-8. fff-search stores it lossily, which both inflates each invalid \
             byte threefold past the {PATH_BUF_SIZE}-byte path buffer and leaves a \
             stored name that no longer refers to the directory on disk, so no file \
             under it could be opened (UF-040 / #296).",
            base.as_os_str(),
        ));
    }

    // `write_absolute_path` writes the base path into the same fixed buffer as
    // the relative path, so the root's own length is part of the budget -- and a
    // root at or over the buffer makes `copy_from_slice` panic outright.
    let base_end_idx = base_prefix_len(base);
    if base_end_idx >= PATH_BUF_SIZE {
        return Err(format!(
            "refusing to search {}: the search root alone is {base_end_idx} bytes \
             with its separator, at or over fff-search's {PATH_BUF_SIZE}-byte path \
             buffer, which leaves no room for any relative path. \
             `write_absolute_path` copies the root into that buffer unconditionally, \
             so a root this long panics the library rather than merely truncating \
             (UF-040 / #296).",
            base.display(),
        ));
    }

    let mut stack = vec![(base.to_path_buf(), 0usize)];

    while let Some((dir, rel_len)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let name = entry.file_name();
            let entry_rel_len = check_entry(base, &dir, &name, base_end_idx, rel_len)?;

            // `file_type` reports a symbolic link as such, so links are never
            // descended into -- no `metadata` call, and no loop to detect.
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                let child = entry.path();
                if never_indexed(base, &child) {
                    // fff-search will not index this directory, so none of its
                    // names can reach the shared path buffer: there is nothing
                    // here for the guard to validate.
                    continue;
                }
                stack.push((child, entry_rel_len));
            }
        }
    }

    Ok(())
}

/// Directories fff-search never indexes, so the guard does not walk them.
///
/// The list mirrors fff-search's own ignore rules (`fff_search::ignore`):
/// `node_modules`, `__pycache__`, `venv` and `.venv` are excluded outright, and
/// inside a `target` directory the `debug`, `release`, `rust-analyzer` and
/// `criterion` subtrees are. `.git` is added because the `ignore` crate skips
/// it in every repository.
///
/// Skipping them keeps the guard's walk over the same scope as the scan that
/// follows it. That is where the cost of the double walk in #452 went: the
/// guard walked `target/debug` - tens of thousands of build artefacts - while
/// fff-search skipped it, so the second walk was pure overhead.
const NEVER_INDEXED_DIRS: &[&str] = &["node_modules", "__pycache__", "venv", ".venv"];

/// Subdirectories of `target` that fff-search excludes.
const NEVER_INDEXED_TARGET_SUBDIRS: &[&str] = &["debug", "release", "rust-analyzer", "criterion"];

/// Whether `dir` is a directory fff-search will not index.
fn never_indexed(base: &Path, dir: &Path) -> bool {
    let Ok(relative) = dir.strip_prefix(base) else {
        return false;
    };
    let components: Vec<String> = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    let mut components = components.iter().peekable();
    while let Some(component) = components.next() {
        if component == ".git" || NEVER_INDEXED_DIRS.contains(&component.as_str()) {
            return true;
        }
        if component == "target"
            && let Some(child) = components.peek()
            && NEVER_INDEXED_TARGET_SUBDIRS.contains(&child.as_str())
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_tree_is_accepted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("src/inner")).expect("create dirs");
        std::fs::write(tmp.path().join("src/inner/lib.rs"), "fn main() {}\n").expect("write");

        assert!(reject_hostile_tree(tmp.path()).is_ok());
    }

    /// `base_prefix_len` must mirror `write_absolute_path`'s `sep_len` exactly:
    /// one separator is added unless the root already ends in one.
    #[test]
    fn base_prefix_len_matches_the_library_arithmetic() {
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(base_prefix_len(Path::new("")), 0);
        assert_eq!(base_prefix_len(Path::new(&format!("{sep}abc"))), 5);
        assert_eq!(base_prefix_len(Path::new(&format!("{sep}abc{sep}"))), 5);
    }

    /// Rule (a): a name that is not valid UTF-8 is refused, and the error shows
    /// the offending bytes.
    ///
    /// Exercised through `check_entry` rather than a real tree because APFS
    /// refuses to create such a name at all, so the filesystem would fail before
    /// the guard was reached. The bytes here are the same ones the Linux-only
    /// integration test plants on disk.
    #[test]
    #[cfg(unix)]
    fn non_utf8_name_is_rejected() {
        use std::os::unix::ffi::OsStrExt;

        let raw = vec![0xFFu8; 250];
        let name = OsStr::from_bytes(&raw);

        let err = check_entry(Path::new("/base"), Path::new("/base/sub"), name, 6, 0)
            .expect_err("a non-UTF-8 name must be refused");
        assert!(
            err.contains("not valid UTF-8"),
            "error must say why, got: {err}"
        );
        assert!(
            err.contains("\\xff") || err.contains("\\xFF"),
            "error must show the offending bytes, got: {err}"
        );
    }

    /// A search root whose own path is not valid UTF-8 is refused.
    ///
    /// This is what keeps `base_prefix_len`'s arithmetic honest: the call site
    /// hands fff-search `to_string_lossy()` of this path, which would be up to
    /// three times longer than the raw bytes measured here. The root need not
    /// exist -- the check runs before any `read_dir` -- which is what lets this
    /// test run on macOS, where such a path cannot be created at all.
    #[test]
    #[cfg(unix)]
    fn non_utf8_base_path_is_refused() {
        use std::os::unix::ffi::OsStrExt;

        let sep = std::path::MAIN_SEPARATOR;
        let mut raw = format!("{sep}tmp{sep}").into_bytes();
        raw.extend_from_slice(&[0xFFu8; 8]);
        let base = std::path::PathBuf::from(OsStr::from_bytes(&raw));

        let err = reject_hostile_tree(&base)
            .expect_err("a search root that is not valid UTF-8 must be refused");
        assert!(
            err.contains("search root's own path is not valid UTF-8"),
            "error must blame the root's encoding, got: {err}"
        );
        assert!(
            err.contains("\\xff") || err.contains("\\xFF"),
            "error must show the offending bytes, got: {err}"
        );
    }

    /// A valid-UTF-8 root containing multi-byte characters is still accepted.
    ///
    /// The check above rejects an *invalid* root, not a non-ASCII one. Refusing
    /// non-Latin paths would be a real functional regression, and is the obvious
    /// way to get this wrong.
    #[test]
    fn multi_byte_utf8_base_path_is_accepted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Cyrillic, Japanese and an emoji: three, three and four bytes each.
        let root = tmp.path().join("проект").join("検索").join("fo\u{00f6}");
        std::fs::create_dir_all(root.join("src")).expect("create multi-byte tree");
        std::fs::write(root.join("src/lib.rs"), "fn main() {}\n").expect("write");

        assert!(
            root.to_str().is_some(),
            "the fixture root must itself be valid UTF-8"
        );
        assert_eq!(
            reject_hostile_tree(&root),
            Ok(()),
            "a valid multi-byte root must be accepted, not mistaken for a hostile one"
        );
    }

    /// A search root that alone fills the buffer is refused before the walk.
    ///
    /// This is the panic arm: `write_absolute_path` copies the root into the
    /// buffer unconditionally, so `copy_from_slice` aborts the process rather
    /// than truncating. The root need not exist -- the check runs before any
    /// `read_dir`, which is what lets this test run on macOS, where a pathname
    /// this long cannot be created at all.
    #[test]
    fn over_long_base_path_is_refused() {
        let sep = std::path::MAIN_SEPARATOR;
        let base = format!("{sep}{}", "b".repeat(PATH_BUF_SIZE));

        let err = reject_hostile_tree(Path::new(&base))
            .expect_err("a search root at or over the buffer must be refused");
        assert!(
            err.contains("search root alone"),
            "error must blame the root, not an entry, got: {err}"
        );
        assert!(
            err.contains("no room for any relative path"),
            "error must explain the consequence, got: {err}"
        );
    }

    /// A long root plus short, ordinary ASCII entry names is refused.
    ///
    /// This is the case the first version of this guard missed: the relative
    /// path is plainly short, so a rule that looked only at the relative portion
    /// let it through, yet `write_absolute_path` shares one buffer between the
    /// two and truncates the relative half mid-character.
    #[test]
    fn long_base_with_short_entry_names_is_refused() {
        let sep = std::path::MAIN_SEPARATOR;
        // 40 bytes short of the buffer, so even an 8-byte name overflows it.
        let base = format!("{sep}{}", "b".repeat(PATH_BUF_SIZE - 41));
        let base = Path::new(&base);
        let base_end_idx = base_prefix_len(base);
        assert!(
            base_end_idx < PATH_BUF_SIZE,
            "the root itself must be legal"
        );

        let err = check_entry(
            base,
            base,
            OsStr::new("main.rs"),
            base_end_idx,
            // A parent directory of 40 ASCII bytes -- nothing unusual.
            40,
        )
        .expect_err("a long root plus an ordinary name must be refused");

        assert!(
            err.contains("for the search root and its separator"),
            "error must apportion the budget so the user can see the root is the \
             cause, got: {err}"
        );
        assert!(
            err.contains("main.rs"),
            "error must name the offending entry, got: {err}"
        );
    }

    /// Boundary: `base_end_idx + rel_len == PATH_BUF_SIZE - 1` is accepted, and
    /// one byte more is refused.
    ///
    /// This pins the guard's rule, not the library's. `write_absolute_path`
    /// would itself handle a total of exactly `PATH_BUF_SIZE` without
    /// truncating; the guard refuses it by choice, one byte early, so that the
    /// same rule covers `write_relative_cstr` too. See `check_entry`.
    ///
    /// No filesystem can present this geometry -- macOS caps a single pathname
    /// at 1024 bytes and Linux at 4096, and `std::fs` passes absolute paths --
    /// so `check_entry` is the honest place to pin the arithmetic.
    #[test]
    fn total_length_boundary_is_exact() {
        let base = Path::new("/base");
        let base_end_idx = 100;
        let name = "a".repeat(10);
        // 100 + parent + 1 + 10 == 4095 exactly.
        let parent_rel_len = PATH_BUF_SIZE - 1 - base_end_idx - 1 - name.len();

        let rel_len = check_entry(base, base, OsStr::new(&name), base_end_idx, parent_rel_len)
            .expect("a total of exactly PATH_BUF_SIZE - 1 bytes must be accepted");
        assert_eq!(base_end_idx + rel_len, PATH_BUF_SIZE - 1);

        let err = check_entry(
            base,
            base,
            OsStr::new(&name),
            base_end_idx,
            parent_rel_len + 1,
        )
        .expect_err("one byte more must be refused");
        assert!(
            err.contains("at or over fff-search's"),
            "error must explain the length rule, got: {err}"
        );
    }

    /// Directories fff-search never indexes are not walked, so their names
    /// never reach the shared path buffer and the guard has nothing to check
    /// there. The rule mirrors fff-search's own ignore list.
    #[test]
    fn never_indexed_directories_are_recognised() {
        let base = Path::new("/base");
        for relative in [
            "/base/.git",
            "/base/node_modules",
            "/base/pkg/__pycache__",
            "/base/.venv",
            "/base/target/debug",
            "/base/target/release",
            "/base/target/criterion",
            "/base/target/rust-analyzer",
        ] {
            assert!(
                never_indexed(base, Path::new(relative)),
                "{relative} must be treated as never indexed"
            );
        }
        for relative in [
            "/base/src",
            "/base/target",
            "/base/target/custom",
            "/base/venvish",
            "/base/my_node_modules",
        ] {
            assert!(
                !never_indexed(base, Path::new(relative)),
                "{relative} must not be skipped"
            );
        }
    }

    /// A hostile name inside a directory fff-search ignores does not refuse
    /// the search: those names never reach the library's path buffer, so
    /// refusing would reject a tree the library would have scanned happily.
    #[test]
    #[cfg(unix)]
    fn hostile_name_in_an_ignored_directory_does_not_refuse_the_search() {
        use std::os::unix::ffi::OsStrExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let debug_dir = tmp.path().join("target/debug");
        std::fs::create_dir_all(&debug_dir).unwrap();
        let raw = vec![0xFFu8; 16];
        std::fs::write(debug_dir.join(std::ffi::OsStr::from_bytes(&raw)), b"x").unwrap();

        assert!(
            reject_hostile_tree(tmp.path()).is_ok(),
            "an ignored directory must not refuse the search"
        );
    }

    /// A hostile name in a directory that *is* indexed still refuses the
    /// search -- the skip rule must not weaken the guard.
    #[test]
    #[cfg(unix)]
    fn hostile_name_in_an_indexed_directory_still_refuses_the_search() {
        use std::os::unix::ffi::OsStrExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let raw = vec![0xFFu8; 16];
        std::fs::write(src.join(std::ffi::OsStr::from_bytes(&raw)), b"x").unwrap();

        let error = reject_hostile_tree(tmp.path())
            .expect_err("a hostile name in an indexed directory must refuse the search");
        assert!(
            error.contains("not valid UTF-8"),
            "error must say why, got: {error}"
        );
    }
}
