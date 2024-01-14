use std::io;
pub const MAIN_SEP: char = ':';

pub fn get_path<'a>(s: &'a str, prefix: &'a str) -> Option<&'a str> {
    // Empty strings are invalid
    if s.is_empty() {
        return None;
    }
    // The "" prefix indicates the root
    if prefix.is_empty() {
        let mut s_iter = s.split(':');
        let base = s_iter.next();
        let remainder = s_iter.next();
        if remainder.is_some() {
            return None;
        }
        return base;
    }
    let without_prefix = s.strip_prefix(prefix)?.strip_prefix(':')?;
    let mut path_split = without_prefix.split(':');
    let parent = path_split.next();
    if path_split.next().is_some() { None } else { parent }
}

/// Split a path into its constituant Basis and Dict, if the path is legal.
pub fn split_basis_and_dict<F: FnMut() -> Option<String>>(
    src: &str,
    mut default: F,
) -> io::Result<(Option<String>, Option<String>)> {
    let mut basis = None;
    let dict;
    if let Some(src) = src.strip_prefix(MAIN_SEP) {
        if let Some((maybe_basis, maybe_dict)) = src.split_once(MAIN_SEP) {
            if !maybe_basis.is_empty() {
                basis = Some(maybe_basis.to_owned());
            } else {
                basis = default();
            }

            if maybe_dict.is_empty() {
                dict = None;
            } else {
                dict = Some(maybe_dict.to_owned());
            }
        } else {
            if !src.is_empty() {
                basis = Some(src.to_owned());
            }
            dict = None;
        }
    } else {
        if src.is_empty() {
            return Ok((basis, Some("".to_owned())));
        }
        dict = Some(src.to_owned());
    }

    if let Some(basis) = &basis {
        if basis.ends_with(MAIN_SEP) {
            return Err(io::Error::new(io::ErrorKind::Other, "invalid path"));
        }
    }
    if let Some(dict) = &dict {
        if dict.ends_with(MAIN_SEP) {
            return Err(io::Error::new(io::ErrorKind::Other, "invalid path"));
        }
    }
    Ok((basis, dict))
}

#[cfg(test)]
fn default_path() -> Option<String> { Some("{DEFAULT}".to_owned()) }

#[test]
fn empty_string() {
    assert_eq!(split_basis_and_dict("", default_path).unwrap(), (None, Some("".to_owned())));
}

#[test]
fn bare_dict() {
    assert_eq!(split_basis_and_dict("one", default_path).unwrap(), (None, Some("one".to_owned())));
}

#[test]
fn dict_with_colon() {
    assert_eq!(split_basis_and_dict("one:two", default_path).unwrap(), (None, Some("one:two".to_owned())));
}

#[test]
fn dict_with_two_colons() {
    assert_eq!(
        split_basis_and_dict("one:two:three", default_path).unwrap(),
        (None, Some("one:two:three".to_owned()))
    );
}

#[test]
#[should_panic]
fn dict_with_trailing_colon() { split_basis_and_dict("one:", default_path).unwrap(); }

#[test]
#[should_panic]
fn two_dicts_with_trailing_colon() { split_basis_and_dict("one:two:", default_path).unwrap(); }

#[test]
#[should_panic]
fn basis_with_dict_with_trailing_colon() { split_basis_and_dict(":one:two:", default_path).unwrap(); }

#[test]
#[should_panic]
fn basis_with_two_dicts_with_trailing_colon() {
    split_basis_and_dict(":one:two:three:", default_path).unwrap();
}

#[test]
fn basis_missing_colon() {
    assert_eq!(split_basis_and_dict(":one", default_path).unwrap(), (Some("one".to_owned()), None));
}

#[test]
fn basis_with_one_dict() {
    assert_eq!(
        split_basis_and_dict(":one:two", default_path).unwrap(),
        (Some("one".to_owned()), Some("two".to_owned()))
    );
}

#[test]
fn basis_with_two_dicts() {
    assert_eq!(
        split_basis_and_dict(":one:two:three", default_path).unwrap(),
        (Some("one".to_owned()), Some("two:three".to_owned()))
    );
}
#[test]
fn double_colon() {
    let default = default_path();
    assert_eq!(split_basis_and_dict("::", default_path).unwrap(), (default, None));
}

#[test]
fn single_colon() {
    assert_eq!(split_basis_and_dict(":", default_path).unwrap(), (None, None));
}

#[test]
fn double_colon_two_keys() {
    let default = default_path();
    assert_eq!(
        split_basis_and_dict("::foo:bar", default_path).unwrap(),
        (default, Some("foo:bar".to_owned()))
    );
}

#[test]
fn double_colon_three_keys() {
    let default = default_path();
    assert_eq!(
        split_basis_and_dict("::foo:bar:baz", default_path).unwrap(),
        (default, Some("foo:bar:baz".to_owned()))
    );
}

#[test]
#[should_panic]
fn double_colon_three_keys_trailing_colon() { split_basis_and_dict("::foo:bar:baz:", default_path).unwrap(); }

#[test]
#[should_panic]
fn dict_with_two_keys_two_trailing_colons() { split_basis_and_dict("foo:bar::", default_path).unwrap(); }

#[test]
#[should_panic]
fn dict_with_two_keys_three_trailing_colons() { split_basis_and_dict("foo:bar:::", default_path).unwrap(); }
