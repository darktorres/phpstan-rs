//! A line ends at a line feed, at a carriage return and line feed, or at a
//! carriage return alone. PHP counts all three: they end a `//` comment, are
//! the newline a closing tag takes, separate the lines of a heredoc, and
//! number the lines `__LINE__` reads. A project checked out on Windows is
//! written with the second.
use super::super::*;
use crate::php_string::PhpString;
use crate::span::{LineCol, LineIndex};
use crate::token::StringPart;

/// `src`, which writes each line break as `\n`, written with each of the
/// three line breaks.
fn written_three_ways(src: &str) -> [(&'static str, String); 3] {
    ["\n", "\r\n", "\r"].map(|line_break| (line_break, src.replace('\n', line_break)))
}

/// The text of a heredoc or nowdoc token that holds no interpolation.
fn literal_text(token: &Token) -> PhpString {
    match token {
        Token::StringLit(text) => text.clone(),
        Token::InterpString(parts) => match parts.as_slice() {
            [StringPart::Literal(text, _)] => text.clone(),
            other => panic!("expected one literal part, got {other:?}"),
        },
        other => panic!("expected a string, got {other:?}"),
    }
}

fn first_string(src: &str) -> Token {
    Lexer::new(src)
        .tokenize()
        .into_iter()
        .find(|token| matches!(token, Token::StringLit(_) | Token::InterpString(_)))
        .expect("a string")
}

#[test]
fn a_line_comment_ends_at_any_line_break() {
    let lf = "<?php\n// first\n$a = 1;\n# second\n$b = 2;\n";
    let expected = Lexer::new(lf).tokenize();
    for (line_break, src) in written_three_ways(lf) {
        let (tokens, comments, _) = Lexer::new(&src).tokenize_with_comments();
        let tokens: Vec<Token> = tokens.into_iter().map(|(token, ..)| token).collect();
        assert_eq!(tokens, expected, "{line_break:?}");
        let texts: Vec<&str> = comments.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(texts, ["// first", "# second"], "{line_break:?}");
    }
}

#[test]
fn a_line_comment_ends_before_a_close_tag_as_well() {
    for (line_break, src) in written_three_ways("<?php // note ?>\nafter\n") {
        let (_, comments, _) = Lexer::new(&src).tokenize_with_comments();
        let texts: Vec<&str> = comments.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(texts, ["// note "], "{line_break:?}");
    }
}

#[test]
fn a_close_tag_takes_the_one_line_break_after_it() {
    for (line_break, src) in written_three_ways("<?php echo 1; ?>\nHello\n") {
        let html = Lexer::new(&src)
            .tokenize()
            .into_iter()
            .find_map(|token| match token {
                Token::InlineHtml(html) => Some(html),
                _ => None,
            })
            .expect("inline html");
        assert_eq!(&*html, format!("Hello{line_break}"), "{line_break:?}");
    }
    for (line_break, src) in written_three_ways("<?php echo 1; ?>\n\nHello") {
        let html = Lexer::new(&src)
            .tokenize()
            .into_iter()
            .find_map(|token| match token {
                Token::InlineHtml(html) => Some(html),
                _ => None,
            })
            .expect("inline html");
        assert_eq!(&*html, format!("{line_break}Hello"), "{line_break:?}");
    }
}

#[test]
fn a_heredoc_body_runs_to_the_closing_label_on_a_line_of_its_own() {
    for (line_break, src) in written_three_ways("<?php\n$x = <<<EOT\nhello\nworld\nEOT;\n$y = 1;\n")
    {
        assert_eq!(
            literal_text(&first_string(&src)),
            PhpString::from(format!("hello{line_break}world")),
            "{line_break:?}"
        );
        let tokens = Lexer::new(&src).tokenize();
        assert!(tokens.contains(&Token::Variable("y".to_string().into())));
    }
}

#[test]
fn a_nowdoc_body_runs_to_the_closing_label_on_a_line_of_its_own() {
    for (line_break, src) in written_three_ways("<?php\n$x = <<<'EOT'\nhello $a\nworld\nEOT;\n") {
        assert_eq!(
            literal_text(&first_string(&src)),
            PhpString::from(format!("hello $a{line_break}world")),
            "{line_break:?}"
        );
    }
}

#[test]
fn a_heredoc_label_inside_a_line_or_before_more_letters_is_body() {
    let lf = "<?php\n$x = <<<EOT\na EOT\nEOTX\nEOT;\n";
    for (line_break, src) in written_three_ways(lf) {
        assert_eq!(
            literal_text(&first_string(&src)),
            PhpString::from(format!("a EOT{line_break}EOTX")),
            "{line_break:?}"
        );
    }
}

#[test]
fn a_heredoc_takes_its_closing_labels_indentation_off_every_line() {
    let lf = "<?php\n$x = <<<EOT\n    a\n      b\n\n    c\n    EOT;\n";
    for (line_break, src) in written_three_ways(lf) {
        assert_eq!(
            literal_text(&first_string(&src)),
            PhpString::from(format!("a{line_break}  b{line_break}{line_break}c")),
            "{line_break:?}"
        );
    }
}

#[test]
fn an_empty_heredoc_and_one_of_a_single_blank_line_are_what_php_reads() {
    for (line_break, src) in written_three_ways("<?php\n$x = <<<EOT\nEOT;\n") {
        assert_eq!(
            literal_text(&first_string(&src)),
            PhpString::from(""),
            "{line_break:?}"
        );
    }
    for (line_break, src) in written_three_ways("<?php\n$x = <<<EOT\n\nEOT;\n") {
        assert_eq!(
            literal_text(&first_string(&src)),
            PhpString::from(""),
            "{line_break:?}"
        );
    }
    for (line_break, src) in written_three_ways("<?php\n$x = <<<EOT\na\n\nEOT;\n") {
        assert_eq!(
            literal_text(&first_string(&src)),
            PhpString::from(format!("a{line_break}")),
            "{line_break:?}"
        );
    }
}

#[test]
fn an_interpolated_heredoc_keeps_each_part_where_it_is_on_its_line() {
    let lf = "<?php\n$x = <<<EOT\n  Hi $name,\n  {$o->p}\n  EOT;\n$y;\n";
    let reference = positions_of_parts(lf);
    for (line_break, src) in written_three_ways(lf) {
        assert_eq!(positions_of_parts(&src), reference, "{line_break:?}");
    }
}

/// The line and column each part of the first heredoc starts and ends at.
fn positions_of_parts(src: &str) -> Vec<(LineCol, LineCol)> {
    let index = LineIndex::new(src.as_bytes());
    let Token::InterpString(parts) = first_string(src) else {
        panic!("expected an interpolated string");
    };
    parts
        .iter()
        .map(|part| {
            let span = match part {
                StringPart::Literal(_, span) | StringPart::SimpleVar(_, span) => *span,
                StringPart::PropertyAccess { var_start, end, .. } => Span {
                    start: *var_start,
                    end: *end,
                },
                other => panic!("unexpected part {other:?}"),
            };
            (index.line_col(span.start), index.line_col(span.end))
        })
        .collect()
}

#[test]
fn every_token_is_on_the_same_line_and_column_whatever_the_line_breaks() {
    let lf = concat!(
        "<?php\n",
        "// a comment\n",
        "namespace App;\n",
        "\n",
        "/**\n",
        " * @param int $a\n",
        " */\n",
        "function f(int $a): string {\n",
        "    # another\n",
        "    $s = 'héllo\n  there';\n",
        "    $t = <<<EOT\n",
        "      body\n",
        "      EOT;\n",
        "    return $s . $t; // end\n",
        "}\n",
        "?>\n",
        "<p>after</p>\n",
    );
    let positions = |src: &str| -> Vec<(LineCol, LineCol)> {
        let index = LineIndex::new(src.as_bytes());
        Lexer::new(src)
            .tokenize_with_spans()
            .into_iter()
            .map(|(_, span, _)| (index.line_col(span.start), index.line_col(span.end)))
            .collect()
    };
    let reference = positions(lf);
    assert!(reference.len() >= 20, "{}", reference.len());
    for (line_break, src) in written_three_ways(lf) {
        assert_eq!(positions(&src), reference, "{line_break:?}");
    }
}

#[test]
fn a_line_break_inside_a_string_literal_is_text_and_a_line_to_the_index() {
    for (line_break, src) in written_three_ways("<?php\n$s = 'a\nb';\n$t = 1;\n") {
        let tokens = Lexer::new(&src).tokenize_with_spans();
        let index = LineIndex::new(src.as_bytes());
        let (_, span, _) = tokens
            .iter()
            .find(|(token, ..)| *token == Token::Variable("t".to_string().into()))
            .expect("$t");
        assert_eq!(
            index.line_col(span.start),
            LineCol { line: 4, col: 1 },
            "{line_break:?}"
        );
        let Token::StringLit(text) = &tokens[2].0 else {
            panic!("expected a string, got {:?}", tokens[2].0);
        };
        assert_eq!(
            *text,
            PhpString::from(format!("a{line_break}b")),
            "{line_break:?}"
        );
    }
}

#[test]
fn random_sources_lex_to_the_same_tokens_at_the_same_places_whatever_the_line_breaks() {
    const FRAGMENTS: [&str; 16] = [
        "$a = 1;\n",
        "// a comment\n",
        "# another\n",
        "/* block\ncomment */\n",
        "/**\n * @param int $x\n */\n",
        "$s = 'one\ntwo';\n",
        "$d = \"say $x\nthen $y\";\n",
        "$h = <<<EOT\n  line $a\n  more\n  EOT;\n",
        "$n = <<<'EOT'\nraw\n\nraw\nEOT;\n",
        "foo(1,\n    2);\n",
        "?>\nhtml\n<?php\n",
        "echo 1; // x ?>\n<b>\n<?php\n",
        "\n",
        "  \n",
        "if ($a) { // brace\n}\n",
        "$x = [1, // one\n 2];\n",
    ];
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move |n: usize| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 33) as usize) % n
    };
    // What a source lexes to, without the text of what holds a line break:
    // each token's kind, and where it starts and ends.
    let lexed = |src: &str| -> Vec<(std::mem::Discriminant<Token>, LineCol, LineCol)> {
        let index = LineIndex::new(src.as_bytes());
        Lexer::new(src)
            .tokenize_with_spans()
            .into_iter()
            .map(|(token, span, _)| {
                (
                    std::mem::discriminant(&token),
                    index.line_col(span.start),
                    index.line_col(span.end),
                )
            })
            .collect()
    };
    for _ in 0..3_000 {
        let mut lf = String::from("<?php\n");
        for _ in 0..1 + next(8) {
            lf.push_str(FRAGMENTS[next(FRAGMENTS.len())]);
        }
        let expected = lexed(&lf);
        for (line_break, src) in written_three_ways(&lf) {
            assert_eq!(lexed(&src), expected, "{line_break:?} {lf:?}");
        }
    }
}
