//! THEME paths: PDDB path-grammar corners that upstream (portable) std::fs
//! tests cannot know about -- basis-prefix forms, the flat (never-resolved)
//! dict namespace, and the byte-oriented name-length limits.
//!
//! Ground truth for the basis-prefix grammar is
//! `services/pddb/src/libstd/utils.rs::split_basis_and_dict` (see its host
//! `#[test]`s for the exact `:`-splitting truth table) plus the call sites in
//! `services/pddb/src/libstd/mod.rs` (`stat_path`, `list_path`, `open_key`,
//! `delete_key`, `create_dict`): a leading `:basis:` is peeled off FIRST by
//! `split_basis_and_dict` (its own separator is `:`), and only THEN is the
//! remainder's *final* `/` used by `open_key`/`delete_key` to split dict from
//! key (`rsplit_once(std::path::MAIN_SEPARATOR)`, MAIN_SEPARATOR = `/`).
//! `create_dict`/`list_path` never split the remainder again -- the whole
//! thing is one literal dict name (see dirs.rs's module note on flat dicts).
//! Rust's `std::path::Path` itself is never normalized (a portable std
//! guarantee, not xous-specific): `.`, `..`, trailing `/`, and doubled `/`
//! all survive verbatim into the string PDDB receives, so none of the
//! resolution/collapsing tricks common on Unix apply here.
//!
//! All assertions state the CORRECT/documented behavior; anything uncertain
//! enough to be a plausible NEW bug is called out in the test's doc comment
//! (see the crate-level review notes for provisional XFAILs, if any).
//! Per the assignment brief this theme does not create or unlock any basis
//! -- every test below probes basis grammar strictly against the always-
//! mounted default `.System` basis.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::harness::{TmpDict, check};

fn read_back(path: &str) -> Vec<u8> {
    let mut f = check!(File::open(path));
    let mut buf = Vec::new();
    check!(std::io::Read::read_to_end(&mut f, &mut buf));
    buf
}

/// Explicit `:basis:dict/key` form, naming the default basis by its real
/// name (`.System`, `PDDB_DEFAULT_SYSTEM_BASIS` in services/pddb/src/api.rs).
/// `split_basis_and_dict` peels `.System` off as an explicit (non-default)
/// basis and hands the rest (`dict/key`) to the same `rsplit_once('/')` logic
/// every unprefixed path goes through, so a key written via the explicit
/// form must read back identically through the unprefixed form, and a
/// delete through one form must be visible through the other.
pub fn basis_prefix_explicit_default_matches_unprefixed() {
    let tmp = TmpDict::new("basis_prefix_explicit_default");
    let unprefixed = tmp.path("k");
    let explicit = format!(":.System:{}/k", tmp.dict());

    // Create through the explicit form, read back through both.
    {
        let mut f = check!(File::create(&explicit));
        check!(f.write_all(b"via-explicit"));
    }
    assert_eq!(&read_back(&explicit)[..], b"via-explicit", "read-back via explicit form");
    assert_eq!(&read_back(&unprefixed)[..], b"via-explicit", "same key must be visible unprefixed");

    // Overwrite through the unprefixed form, read back through the explicit one.
    check!(fs::write(&unprefixed, b"via-unprefixed"));
    assert_eq!(&read_back(&explicit)[..], b"via-unprefixed", "explicit form must see the unprefixed write");

    // Delete through the explicit form; must be gone via the unprefixed one too.
    check!(fs::remove_file(&explicit));
    assert!(
        File::open(&unprefixed).is_err(),
        "key must be gone via the unprefixed form after explicit delete"
    );
    assert!(File::open(&explicit).is_err(), "key must be gone via the explicit form too");
}

/// `::dict/key` (empty basis name between the two leading colons) means
/// "the default basis", per `split_basis_and_dict`'s `default()` callback
/// (`basis_cache.basis_latest()`, services/pddb/src/backend/basis.rs) --
/// verified directly by the crate's own host tests `double_colon`/
/// `double_colon_two_keys`. With only `.System` ever mounted in this theme,
/// `basis_latest()` always resolves to it, so `::dict/key` must behave
/// identically to both the unprefixed and the `:.System:` explicit forms.
pub fn basis_prefix_double_colon_matches_unprefixed() {
    let tmp = TmpDict::new("basis_prefix_double_colon");
    let unprefixed = tmp.path("k");
    let double_colon = format!("::{}/k", tmp.dict());

    check!(fs::write(&unprefixed, b"default-basis"));
    assert_eq!(&read_back(&double_colon)[..], b"default-basis", "'::' prefix must resolve to the same key");

    check!(fs::write(&double_colon, b"via-double-colon"));
    assert_eq!(&read_back(&unprefixed)[..], b"via-double-colon", "unprefixed form must see the '::' write");

    check!(fs::remove_file(&unprefixed));
    assert!(File::open(&double_colon).is_err(), "key must be gone via '::' form after unprefixed delete");
}

/// Opening a path prefixed with a basis name that is not currently mounted
/// must error. `open_key`'s basis loop (services/pddb/src/libstd/mod.rs
/// ~313-317) only enters its body for bases matching `requested_basis`; a
/// name absent from `basis_cache.access_list()` matches nothing, so the loop
/// falls off the end and returns `PddbRetcode::BasisLost` regardless of
/// `create_file`/`create_path` -- i.e. even a `File::create` cannot
/// materialize a dict/key in a basis that doesn't exist. This theme creates
/// no basis (see module note), so any name other than `.System` qualifies.
pub fn open_nonexistent_basis_errors() {
    let tmp = TmpDict::new("open_nonexistent_basis");
    let path = tmp.path("k");
    check!(fs::write(&path, b"real"));
    assert_eq!(&read_back(&path)[..], b"real");

    let bogus_basis_read = format!(":NoSuchBasisXYZ:{}/k", tmp.dict());
    assert!(File::open(&bogus_basis_read).is_err(), "open in a nonexistent basis must error");

    let bogus_basis_create = format!(":AlsoMissingBasis:{}/new_key", tmp.dict());
    assert!(
        File::create(&bogus_basis_create).is_err(),
        "create in a nonexistent basis must error (create_path is always false)"
    );

    // The real key survives untouched.
    assert_eq!(&read_back(&path)[..], b"real", "unrelated key must be unaffected");
    check!(fs::remove_file(&path));
}

/// Characterization: a key name containing `:` (e.g. `dict/a:b`). This is
/// legal here even though the native (non-std) pddb-lib API reserves `:`
/// throughout its own path grammar: `split_basis_and_dict` only ever strips
/// a LEADING `:` (its own doc: "Split a path into its constituent Basis and
/// Dict"), and everything after the final `/` becomes the key verbatim --
/// `KeyName::try_from_str` (services/pddb/src/backend/key.rs) validates only
/// byte length, no character set. So `dict/a:b` creates a key literally
/// named `a:b`, and it must create/stat/read_dir/delete exactly like any
/// other key name.
pub fn key_name_with_embedded_colon() {
    let tmp = TmpDict::new("key_name_with_embedded_colon");
    let key_name = "a:b";
    let path = tmp.path(key_name);

    {
        let mut f = check!(File::create(&path));
        check!(f.write_all(b"colon-key"));
    }
    assert_eq!(&read_back(&path)[..], b"colon-key");
    assert!(check!(fs::metadata(&path)).is_file(), "'a:b' key should stat as a file");

    let entries: Vec<String> = check!(fs::read_dir(tmp.dict()))
        .map(|e| check!(e).file_name().into_string().expect("utf8 filename"))
        .collect();
    assert!(
        entries.iter().any(|n| n == key_name),
        "read_dir should list the literal key name 'a:b': got {:?}",
        entries
    );

    check!(fs::remove_file(&path));
    assert!(File::open(&path).is_err(), "'a:b' key should be gone after remove_file");
}

/// Unicode dict and key names (CJK + emoji), staying well within the
/// byte-oriented 110/94 limits (`DictName`/`KeyName::try_from_str`,
/// services/pddb/src/backend/{dictionary,key}.rs, both `name.as_bytes().len()`
/// checks with NO character-count logic) -- multi-byte UTF-8 means a handful
/// of CJK/emoji characters can already be a meaningful fraction of the byte
/// budget even though `.chars().count()` looks tiny, which this test asserts
/// explicitly rather than just assuming.
pub fn unicode_dict_and_key_names() {
    let tmp = TmpDict::new("unicode_dict_and_key_names");
    // 5 CJK chars (3 bytes each in UTF-8) + 2 emoji (4 bytes each) = 23 bytes.
    let dict_suffix = "\u{6863}\u{6848}\u{6a94}\u{6587}\u{4ef6}\u{1f4c1}\u{1f5c2}";
    let dict = format!("{}_{}", tmp.dict(), dict_suffix);
    assert!(
        dict_suffix.len() > dict_suffix.chars().count(),
        "sanity: suffix really is multi-byte (byte len {} vs char count {})",
        dict_suffix.len(),
        dict_suffix.chars().count()
    );
    assert!(
        dict.len() < 110,
        "unicode dict name must stay under the 110-byte DictName limit, got {}",
        dict.len()
    );
    check!(fs::create_dir(&dict));

    // Key name: mixed CJK + emoji, no ASCII prefix needed (uniqueness comes
    // from the dict).
    let key_name = "\u{952e}\u{540d}\u{1f511}\u{1f4c4}"; // "key name" (CJK) + key/page emoji
    assert!(
        key_name.len() < 94,
        "unicode key name must stay under the 94-byte KeyName limit, got {}",
        key_name.len()
    );
    assert!(key_name.len() > key_name.chars().count(), "sanity: key name really is multi-byte");
    let path = format!("{dict}/{key_name}");

    {
        let mut f = check!(File::create(&path));
        check!(f.write_all("unicode-content-\u{4f60}\u{597d}".as_bytes()));
    }
    assert_eq!(&read_back(&path)[..], "unicode-content-\u{4f60}\u{597d}".as_bytes());
    assert!(check!(fs::metadata(&path)).is_file(), "unicode key should stat as a file");
    assert!(Path::new(&dict).is_dir(), "unicode dict should stat as a dict");

    let entries: Vec<String> = check!(fs::read_dir(&dict))
        .map(|e| check!(e).file_name().into_string().expect("utf8 filename"))
        .collect();
    assert!(
        entries.iter().any(|n| n == key_name),
        "read_dir should list the unicode key name: got {:?}",
        entries
    );

    check!(fs::remove_file(&path));
    check!(fs::remove_dir(&dict));
}

/// Two-level nested dict path (`a/b/c` where `a/b` is meant as "the dict"):
/// portable std's `create_dir_all` attempts `mkdir(full_path)` FIRST and only
/// recurses to `Path::parent()` when that initial mkdir fails (upstream
/// `DirBuilder::create_dir_all`: `Err(NotFound)` falls through to the parent
/// walk, anything else returns). On PDDB the initial mkdir of `<tmp>/a/b`
/// SUCCEEDS outright -- `create_dict` takes the whole remainder as one
/// literal flat dict name, and a flat namespace has no "missing parent" to
/// trip on -- so `create_dir_all` returns early having created ONLY the dict
/// literally named `<tmp>/a/b`; the would-be ancestor `<tmp>/a` is never
/// materialized (empirically confirmed on the suite cold run: this test's
/// original two-dicts assertion FAILed with `<tmp>/a` absent). A key created
/// at `<tmp>/a/b/c` is visible in a `read_dir` of `<tmp>/a/b` (the dict that
/// literally owns it) but `<tmp>/a/b` never appears as an entry when reading
/// `<tmp>` itself: PDDB has no real directory tree at any depth.
pub fn nested_multilevel_dict_path_two_levels() {
    let tmp = TmpDict::new("nested_multilevel_dict_path");
    let level1 = format!("{}/a", tmp.dict());
    let level2 = format!("{}/a/b", tmp.dict());
    let file_path = format!("{level2}/c");

    check!(fs::create_dir_all(&level2));
    assert!(
        !Path::new(&level1).exists(),
        "'<tmp>/a' must NOT be created: create_dir_all's first mkdir of the full path \
         succeeds on the flat namespace, so the ancestor walk never runs"
    );
    assert!(check!(fs::metadata(&level2)).is_dir(), "'<tmp>/a/b' should stat as its own dict");

    {
        let mut f = check!(File::create(&file_path));
        check!(f.write_all(b"leaf"));
    }
    assert_eq!(&read_back(&file_path)[..], b"leaf");

    let level2_entries: Vec<String> = check!(fs::read_dir(&level2))
        .map(|e| check!(e).file_name().into_string().expect("utf8 filename"))
        .collect();
    assert_eq!(
        level2_entries,
        vec!["c".to_string()],
        "'<tmp>/a/b' listing should contain only its own key 'c'"
    );

    let top_entries: Vec<String> = check!(fs::read_dir(tmp.dict()))
        .map(|e| check!(e).file_name().into_string().expect("utf8 filename"))
        .collect();
    assert!(
        !top_entries.iter().any(|n| n == "a"),
        "'<tmp>/a' must not appear as a child entry of '<tmp>' (PDDB dicts are flat): got {:?}",
        top_entries
    );

    check!(fs::remove_file(&file_path));
    check!(fs::remove_dir(&level2));
}

/// `.` and `..` path segments are never resolved on xous: `Path` is not
/// normalized by Rust's portable std (a general guarantee, not xous-
/// specific), and `create_dict`/`open_key` take the post-basis-prefix
/// remainder as a literal string (with, at most, one final `/` split for the
/// key) -- there is no cwd, no "current dict", and no notion of a parent to
/// walk up to. So `<tmp>/.` and `<tmp>/..` are simply two more distinct,
/// unrelated flat dict names; they do not alias `<tmp>` itself, each other,
/// or any real parent, and neither shows up in `<tmp>`'s own read_dir.
pub fn dot_and_dotdot_segments_not_resolved() {
    let tmp = TmpDict::new("dot_and_dotdot_segments_not_resolved");
    let base = tmp.dict().to_string();
    let dot_dict = format!("{base}/.");
    let dotdot_dict = format!("{base}/..");
    // Defensive cleanup in case a prior aborted run left these behind.
    let _ = fs::remove_dir_all(&dot_dict);
    let _ = fs::remove_dir_all(&dotdot_dict);

    check!(fs::create_dir(&dot_dict));
    check!(fs::create_dir(&dotdot_dict));
    assert!(Path::new(&dot_dict).is_dir(), "'<tmp>/.' should exist as its own independent dict");
    assert!(Path::new(&dotdot_dict).is_dir(), "'<tmp>/..' should exist as its own independent dict");
    assert!(Path::new(&base).is_dir(), "base dict must be unaffected by the dotted siblings");

    // A key written to the base dict must not be reachable through either
    // dotted name (no resolution back onto the base or onto a "parent").
    let base_key = tmp.path("marker");
    check!(fs::write(&base_key, b"base"));
    assert!(
        File::open(&format!("{dot_dict}/marker")).is_err(),
        "'.' dict must not alias the base dict's keys"
    );
    assert!(
        File::open(&format!("{dotdot_dict}/marker")).is_err(),
        "'..' dict must not alias the base dict's keys"
    );

    let base_entries: Vec<String> = check!(fs::read_dir(&base))
        .map(|e| check!(e).file_name().into_string().expect("utf8 filename"))
        .collect();
    assert!(
        !base_entries.iter().any(|n| n == "." || n == ".."),
        "dotted dict names must never appear as children of the base dict: got {:?}",
        base_entries
    );

    check!(fs::remove_file(&base_key));
    check!(fs::remove_dir(&dot_dict));
    check!(fs::remove_dir(&dotdot_dict));
}

/// A trailing `/` appended to an existing key's path is not trimmed: it
/// shifts which `/` `open_key`'s `rsplit_once(MAIN_SEPARATOR)` treats as the
/// dict/key separator, so `<dict>/key/` resolves to dict=`<dict>/key`
/// (a distinct, never-created flat dict) and key=`""` -- not the original
/// key at all, and not an error about a "directory used as a file" either
/// (PDDB has no such concept). The lookup simply fails because that dict
/// doesn't exist.
pub fn trailing_slash_on_open_targets_distinct_dict() {
    let tmp = TmpDict::new("trailing_slash_on_open");
    let path = tmp.path("key");
    check!(fs::write(&path, b"content"));
    assert_eq!(&read_back(&path)[..], b"content");

    let trailing = format!("{path}/");
    assert!(File::open(&trailing).is_err(), "trailing '/' must not resolve back onto the real key");

    // The real key must be untouched by the failed lookup.
    assert_eq!(&read_back(&path)[..], b"content", "original key must survive the failed trailing-slash open");
    check!(fs::remove_file(&path));
}

/// A trailing `/` on a dict path given to `create_dir` is likewise never
/// trimmed: `create_dict` takes the whole remainder as ONE literal dict name
/// (no `/`-splitting at all -- see the module note), and only a trailing
/// `:` is rejected by `split_basis_and_dict` (its own separator), not a
/// trailing `/`. So `<dict>` and `<dict>/` are two independently-existing
/// dict names. A doubled `/` in a key path behaves consistently with this:
/// `open_key` still splits on the LAST `/`, so `<dict>//key` targets a key
/// named `key` inside the dict literally named `<dict>/` -- proven here by
/// writing through that exact path and confirming the original `<dict>` is
/// untouched.
pub fn trailing_slash_create_dir_and_double_slash_key() {
    let tmp = TmpDict::new("trailing_slash_create_dir");
    let base = tmp.dict().to_string();
    let shadow_dict = format!("{base}/");
    let shadow_key_path = format!("{base}//marker"); // dict "<base>/" + key "marker"
    let _ = fs::remove_dir_all(&shadow_dict);

    check!(fs::create_dir(&shadow_dict));
    assert!(Path::new(&shadow_dict).is_dir(), "'<base>/' should exist as its own independent dict");
    assert!(Path::new(&base).is_dir(), "original dict must be unaffected");

    {
        let mut f = check!(File::create(&shadow_key_path));
        check!(f.write_all(b"shadow"));
    }
    assert_eq!(&read_back(&shadow_key_path)[..], b"shadow");

    let shadow_entries: Vec<String> = check!(fs::read_dir(&shadow_dict))
        .map(|e| check!(e).file_name().into_string().expect("utf8 filename"))
        .collect();
    assert_eq!(shadow_entries, vec!["marker".to_string()], "'<base>/' listing should contain only 'marker'");

    let base_entries: Vec<String> = check!(fs::read_dir(&base))
        .map(|e| check!(e).file_name().into_string().expect("utf8 filename"))
        .collect();
    assert!(
        base_entries.is_empty(),
        "the real dict must not be touched by the '<base>/' shadow dict: got {:?}",
        base_entries
    );

    check!(fs::remove_file(&shadow_key_path));
    check!(fs::remove_dir(&shadow_dict));
}

/// Root-path `stat` on the empty string. `stat_path`
/// (services/pddb/src/libstd/mod.rs) special-cases an empty
/// `split_basis_and_dict` remainder as the literal root: "The root is a
/// dict" -- it writes `FileType::Dict` unconditionally, the exact same code
/// path every ordinary existing dict's metadata takes. So `fs::metadata("")`
/// must succeed and report `is_dir()`.
pub fn root_path_stat_empty_string_is_dict() {
    let meta = check!(fs::metadata(""));
    assert!(meta.is_dir(), "fs::metadata(\"\") should report the root as a dict");
}

/// Root-path `stat` on a bare `:`. `split_basis_and_dict(":", ...)` yields
/// `(None, None)` (pinned by this crate's own host test `single_colon`), and
/// `stat_path` explicitly special-cases `basis.is_none() && remainder.is_none()`
/// as "does not exist" (writes `FileType::None`, the identical retcode every
/// ordinary nonexistent path already relies on -- see e.g. every
/// `Path::exists() == false` assertion elsewhere in this suite). So a bare
/// `:` must behave like any other nonexistent path: `metadata` errors and
/// `exists()` is false.
pub fn root_path_stat_single_colon_errors() {
    assert!(fs::metadata(":").is_err(), "fs::metadata(\":\") should error (no basis, no dict)");
    assert!(!Path::new(":").exists(), "bare ':' should not report as existing");
}

/// This theme's registry (aggregated by tests::all_tests / all_xfails).
pub const TESTS: &[(&str, fn())] = &[
    (
        "paths::basis_prefix_explicit_default_matches_unprefixed",
        basis_prefix_explicit_default_matches_unprefixed as fn(),
    ),
    (
        "paths::basis_prefix_double_colon_matches_unprefixed",
        basis_prefix_double_colon_matches_unprefixed as fn(),
    ),
    ("paths::open_nonexistent_basis_errors", open_nonexistent_basis_errors as fn()),
    ("paths::key_name_with_embedded_colon", key_name_with_embedded_colon as fn()),
    ("paths::unicode_dict_and_key_names", unicode_dict_and_key_names as fn()),
    ("paths::nested_multilevel_dict_path_two_levels", nested_multilevel_dict_path_two_levels as fn()),
    ("paths::dot_and_dotdot_segments_not_resolved", dot_and_dotdot_segments_not_resolved as fn()),
    (
        "paths::trailing_slash_on_open_targets_distinct_dict",
        trailing_slash_on_open_targets_distinct_dict as fn(),
    ),
    (
        "paths::trailing_slash_create_dir_and_double_slash_key",
        trailing_slash_create_dir_and_double_slash_key as fn(),
    ),
    ("paths::root_path_stat_empty_string_is_dict", root_path_stat_empty_string_is_dict as fn()),
    ("paths::root_path_stat_single_colon_errors", root_path_stat_single_colon_errors as fn()),
];

pub const XFAILS: &[(&str, &str)] = &[];
