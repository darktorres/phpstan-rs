//! Attribute groups.
use super::super::*;

#[test]
fn a_general_attribute_group_is_not_fooled_by_an_unbalanced_bracket_in_a_string_argument() {
    let src = "<?php #[Assert\\Regex('/[^\\]]+/')]\nclass C {}\n";
    let toks = Lexer::new(src).tokenize();
    assert_eq!(
        toks,
        vec![
            Token::AttributeStart,
            Token::Identifier("Assert".to_string().into()),
            Token::Backslash,
            Token::Identifier("Regex".to_string().into()),
            Token::LParen,
            Token::StringLit("/[^\\]]+/".into()),
            Token::RParen,
            Token::RBracket,
            Token::Keyword(Keyword::Class),
            Token::Identifier("C".to_string().into()),
            Token::LBrace,
            Token::RBrace,
            Token::Eof,
        ]
    );
}
