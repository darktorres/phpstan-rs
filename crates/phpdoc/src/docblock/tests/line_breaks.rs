//! A doc comment reads the same whether its lines end in a line feed, in a
//! carriage return and line feed (a checkout on Windows), or in a carriage
//! return alone.
use super::super::*;

/// `raw`, which writes each line break as `\n`, written with each of the
/// three line breaks.
fn written_three_ways(raw: &str) -> [(&'static str, String); 3] {
    ["\n", "\r\n", "\r"].map(|line_break| (line_break, raw.replace('\n', line_break)))
}

#[test]
fn a_doc_comment_reads_the_same_whatever_its_line_breaks() {
    let lf = concat!(
        "/**\n",
        " * Adds one to a number.\n",
        " *\n",
        " * @param int $x the input\n",
        " * @param array{id: int,\n",
        " *   name: string} $row wrapped shape\n",
        " * @return list<string>\n",
        " * @template T of object\n",
        " * @phpstan-assert-if-true non-empty-string $x\n",
        " * @throws \\RuntimeException\n",
        " */",
    );
    let expected = parse(lf);
    assert_eq!(expected.params.len(), 2);
    assert!(expected.return_type.is_some());
    assert_eq!(expected.templates, ["T"]);
    for (line_break, raw) in written_three_ways(lf) {
        assert_eq!(parse(&raw), expected, "{line_break:?}");
    }
}

#[test]
fn a_tag_that_wraps_onto_the_next_line_joins_it_whatever_the_line_break() {
    let lf = "/**\n * @return array{\n *   id: int,\n *   name: string,\n * }\n */";
    let expected = parse(lf).return_type.expect("a return type");
    for (line_break, raw) in written_three_ways(lf) {
        assert_eq!(
            parse(&raw).return_type.as_ref(),
            Some(&expected),
            "{line_break:?}"
        );
    }
}

#[test]
fn a_comment_in_a_wrapped_shape_ends_at_its_line_break() {
    let lf = "/**\n * @var array{\n *   a: int, // the first\n *   b: string, // the second\n * } $x\n */";
    let expected = parse(lf);
    assert!(expected.var_type.is_some());
    for (line_break, raw) in written_three_ways(lf) {
        assert_eq!(parse(&raw), expected, "{line_break:?}");
    }
}

#[test]
fn an_unknown_phpstan_tag_is_found_on_any_line() {
    let lf = "/**\n * @param int $x\n * @phpstan-bogus foo\n * @return int\n */";
    for (line_break, raw) in written_three_ways(lf) {
        assert_eq!(unknown_phpstan_tags(&raw), ["bogus"], "{line_break:?}");
    }
}
