use super::super::*;

/// The names, variables and doc comments of `tokens`, as text.
fn spellings(tokens: &[Token]) -> Vec<String> {
    tokens
        .iter()
        .filter_map(|t| match t {
            Token::Identifier(n) | Token::Variable(n) => Some(n.to_string()),
            Token::DocComment(d) => Some(d.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_names_of_a_file_that_is_not_utf8_are_read_lossily_as_the_names_of_its_lossy_text() {
    let files: [&[u8]; 5] = [
        b"<?php $caf\xe9 = 1; class \xffX { function \xc0\xaf() {} } /** doc \xf0\x9f */ f();",
        b"<?php \xed\xa0\x80name();",
        b"<?php $a\xe2\x82 = $b;",
        b"<?php /** \xff */ class A {}",
        "<?php $caf\u{e9} = \u{4e2d}\u{6587}(); /** \u{10348} */".as_bytes(),
    ];
    for file in files {
        let from_bytes = Lexer::from_bytes(file).tokenize();
        let text = String::from_utf8_lossy(file);
        let from_text = Lexer::new(&text).tokenize();
        assert_eq!(spellings(&from_bytes), spellings(&from_text), "{file:?}");
    }
}

#[test]
fn a_file_that_is_utf8_is_lexed_alike_as_bytes_and_as_text() {
    let text =
        "<?php namespace \u{e9}\\b; $\u{4e2d} = 'x'; /** d\u{10348} */ class C { const A = 1; }";
    assert_eq!(
        format!(
            "{:?}",
            Lexer::from_bytes(text.as_bytes()).tokenize_with_spans()
        ),
        format!("{:?}", Lexer::new(text).tokenize_with_spans())
    );
}
