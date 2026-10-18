//! A fix that adds a line to a file ends it with the line break the file
//! writes, so a file checked out with `\r\n` stays one.

use super::super::*;
use crate::parent_calls::{HasContent, Invocation, ParentCallRule};
use crate::project::session::load_test_file;
use diagnostics::DiagnosticFix;

const SOURCE: &str = concat!(
    "<?php\n",
    "class Base { public function boot(): void { $this->x = 1; } }\n",
    "class Child extends Base {\n",
    "    public function boot(): void {\n",
    "        $this->y = 2;\n",
    "    }\n",
    "}\n",
    "class Bare extends Base {\n",
    "    public function boot(): void {}\n",
    "}\n",
);

/// `src` with the fixes of the rule that `Base::boot()` be called as
/// `invocation` says, applied.
fn fixed(src: &str, invocation: Invocation) -> String {
    let mut project = Project::new();
    load_test_file(&mut project, PathBuf::from("a.php"), src, true);
    project.finish_loading();
    let rule = ParentCallRule {
        class: Some("Base".to_string()),
        method: "boot".to_string(),
        invocation,
        has_content: HasContent::Maybe,
    };
    let found = project.check_class_shapes(false, &[rule]);
    let fixes: Vec<&DiagnosticFix> = found
        .values()
        .flatten()
        .filter_map(|d| d.fix.as_ref())
        .collect();
    assert!(!fixes.is_empty(), "got: {found:?}");
    String::from_utf8(diagnostics::fix::apply_fixes(src.as_bytes(), fixes).0).unwrap()
}

#[test]
fn a_parent_call_added_first_ends_its_line_with_the_files_line_break() {
    let lf = fixed(SOURCE, Invocation::First);
    assert!(lf.contains("        parent::boot();\n        $this->y = 2;\n"));
    assert!(lf.contains("{\n        parent::boot();\n    }"));
    for line_break in ["\r\n", "\r"] {
        assert_eq!(
            fixed(&SOURCE.replace('\n', line_break), Invocation::First),
            lf.replace('\n', line_break),
            "{line_break:?}"
        );
    }
}

#[test]
fn a_parent_call_added_last_starts_its_line_with_the_files_line_break() {
    let lf = fixed(SOURCE, Invocation::Last);
    assert!(lf.contains("$this->y = 2;\n        parent::boot();\n    }"));
    for line_break in ["\r\n", "\r"] {
        assert_eq!(
            fixed(&SOURCE.replace('\n', line_break), Invocation::Last),
            lf.replace('\n', line_break),
            "{line_break:?}"
        );
    }
}

#[test]
fn a_file_that_mixes_line_breaks_gets_the_first_it_writes() {
    let mixed = SOURCE.replacen('\n', "\r\n", 1);
    let fixed = fixed(&mixed, Invocation::First);
    assert!(
        fixed.contains("\r\n        parent::boot();\r\n"),
        "{fixed:?}"
    );
}
