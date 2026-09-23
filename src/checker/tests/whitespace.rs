//! Output a file sends around its PHP code: a BOM, and whitespace before
//! the first `<?php` or after a final `?>`.

use super::helpers::*;

fn ids(src: &str) -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = diagnostics_for(src)
        .into_iter()
        .filter(|d| d.identifier.starts_with("whitespace."))
        .map(|d| (d.identifier, d.span.start.line))
        .collect();
    v.sort();
    v
}

fn owned(v: &[(&str, u32)]) -> Vec<(String, u32)> {
    v.iter().map(|(s, l)| (s.to_string(), *l)).collect()
}

#[test]
fn a_leading_bom_is_reported() {
    assert_eq!(
        ids("\u{feff}<?php\necho 1;\n"),
        owned(&[("whitespace.bom", 1)])
    );
}

#[test]
fn leading_whitespace_before_the_open_tag_is_reported() {
    assert_eq!(
        ids("\n\n<?php\necho 1;\n"),
        owned(&[("whitespace.fileStart", 1)])
    );
    assert_eq!(
        ids("\u{feff} <?php\necho 1;\n"),
        owned(&[("whitespace.bom", 1), ("whitespace.fileStart", 1)])
    );
}

#[test]
fn trailing_whitespace_after_the_closing_tag_is_reported() {
    assert_eq!(
        ids("<?php\necho 1;\n?>\n\n"),
        owned(&[("whitespace.fileEnd", 4)])
    );
    assert_eq!(
        ids("<?php\necho 1;\n?>  "),
        owned(&[("whitespace.fileEnd", 3)])
    );
}

#[test]
fn the_single_newline_after_a_closing_tag_is_part_of_the_tag() {
    assert!(ids("<?php\necho 1;\n?>\n").is_empty());
    assert!(ids("<?php\necho 1;\n?>\r\n").is_empty());
    assert!(ids("<?php\necho 1;\n?>").is_empty());
}

#[test]
fn templates_and_code_without_a_closing_tag_are_fine() {
    assert!(ids("<html>\n<?php echo 1; ?>\n</html>\n").is_empty());
    assert!(ids("<?php\necho 1;\n").is_empty());
    assert!(ids("#!/usr/bin/env php\n<?php\necho 1;\n").is_empty());
}

#[test]
fn each_namespace_and_declare_block_end_is_checked() {
    assert_eq!(
        ids("<?php\nnamespace A;\necho 1;\n?>\n\n<?php\nnamespace B;\necho 2;\n"),
        owned(&[("whitespace.fileEnd", 5)])
    );
    assert_eq!(
        ids("<?php\ndeclare(ticks=1) {\necho 1;\n?>\n \n<?php\n}\n"),
        owned(&[("whitespace.fileEnd", 5)])
    );
}
