//! `is_reserved_type_word`, which asks the same table of keywords
//! `parse_type_keyword_or_class` matches on.
use super::super::*;
use crate::parse::{TYPE_KEYWORD_WORDS, TypeKeyword};

#[test]
fn every_spelling_is_lower_case_and_listed_once() {
    for (i, (word, _)) in TYPE_KEYWORD_WORDS.iter().enumerate() {
        assert_eq!(*word, word.to_ascii_lowercase());
        assert!(
            TYPE_KEYWORD_WORDS[..i]
                .iter()
                .all(|(other, _)| other != word),
            "{word} is listed twice"
        );
    }
}

#[test]
fn a_keyword_is_reserved_unless_it_is_read_as_a_class() {
    for (word, keyword) in TYPE_KEYWORD_WORDS {
        assert_eq!(
            is_reserved_type_word(word),
            *keyword != TypeKeyword::Closure,
            "{word}"
        );
    }
}

#[test]
fn a_reserved_word_is_never_read_as_a_class_of_that_name() {
    for (word, keyword) in TYPE_KEYWORD_WORDS {
        if *keyword == TypeKeyword::Closure || word.starts_with('$') {
            continue;
        }
        let ty = parse_written_type(word);
        assert!(
            ty.object_class_names()
                .iter()
                .all(|name| !name.as_str().eq_ignore_ascii_case(word)),
            "{word} is read as a class"
        );
    }
}

#[test]
fn case_and_a_leading_backslash_do_not_matter() {
    assert!(is_reserved_type_word("INT"));
    assert!(is_reserved_type_word("\\String"));
    assert!(is_reserved_type_word("Non-Empty-String"));
    assert!(!is_reserved_type_word("Foo\\int"));
    assert!(!is_reserved_type_word("Closure"));
    assert!(!is_reserved_type_word(
        "non-empty-string-of-a-very-long-name-indeed"
    ));
}
