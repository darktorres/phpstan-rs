//! The syntax only some PHP versions accept that a parse records the first
//! place of, in the file's facts.

use crate::{SyntaxMark, parse};

/// The text at the first place `source` spells `mark`.
fn first(source: &str, mark: SyntaxMark) -> Option<String> {
    let parsed = parse(source).unwrap();
    let span = parsed.facts.syntax.first(mark)?;
    Some(source[span.start as usize..span.end as usize].to_string())
}

#[test]
fn each_mark_is_recorded_where_the_file_first_spells_it() {
    let cases: [(&str, SyntaxMark, &str); 8] = [
        (
            "<?php class A { const X = 1; private const Y = 2; protected const Z = 3; }",
            SyntaxMark::ClassConstVisibility,
            "private",
        ),
        (
            "<?php use Foo\\{A, B,};",
            SyntaxMark::TrailingCommaInGroupedUse,
            ",",
        ),
        (
            "<?php f(1); g(1, 2,); h(3,);",
            SyntaxMark::TrailingCommaInArguments,
            ",",
        ),
        (
            "<?php $a = <<<EOT\nx\nEOT;\n$b = <<<EOT\n  y\n  EOT;\n",
            SyntaxMark::FlexibleHeredoc,
            "<<<EOT\n  y\n  EOT",
        ),
        (
            "<?php echo 1000, 1_000, 2_000;",
            SyntaxMark::NumericLiteralSeparator,
            "1_000",
        ),
        (
            "<?php function f($a) {} function g($a, $b,) {}",
            SyntaxMark::TrailingCommaInParameters,
            ",",
        ),
        (
            "<?php $f = function () use ($a, $b,) {};",
            SyntaxMark::TrailingCommaInClosureUse,
            ",",
        ),
        ("<?php echo 017, 0o17;", SyntaxMark::ExplicitOctal, "0o17"),
    ];
    for (source, mark, text) in cases {
        assert_eq!(first(source, mark).as_deref(), Some(text), "{mark:?}");
    }
}

#[test]
fn the_spellings_every_version_accepts_are_not_recorded() {
    let source = "<?php\nclass A { const X = 1; var $v; public $p; }\nf(1, 2);\nfunction g($a, $b) {}\n$c = function () use ($a) {};\n$d = <<<EOT\nx\nEOT;\n$e = <<<EOT\nx\nEOT\n;\necho 1000, 017, 0x1F;\nuse Foo\\{A, B};\n";
    let parsed = parse(source).unwrap();
    for mark in SyntaxMark::ALL {
        assert_eq!(parsed.facts.syntax.first(mark), None, "{mark:?}");
    }
}
