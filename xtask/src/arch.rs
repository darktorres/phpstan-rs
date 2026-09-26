//! The architecture gate: rules on the source that hold from the phase
//! that lands them on.
//!
//! - A `Diagnostic` is built through its builder: no `Diagnostic { .. }`
//!   expression outside the `diagnostics` crate.
//! - No `"$$`-prefixed type-string sentinel.
//! - Every library crate denies the print macros in its non-test code.

use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::modgraph::walk_dir;

/// The extracted library crates, which must not print.
const LIBRARY_CRATES: &[&str] = &[
    "syntax",
    "types",
    "phpdoc",
    "sql",
    "diagnostics",
    "composer",
    "stub-sources",
];
const PRINT_DENY: &str =
    "#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]";

pub(crate) fn arch(root: &Path) -> Result<(), String> {
    let mut violations = Vec::new();
    let mut files = Vec::new();
    walk_dir(&root.join("crates"), &mut files)?;
    files.sort();
    for path in &files {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let ast = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut finder = Finder {
            in_diagnostics: path.starts_with(root.join("crates/diagnostics")),
            found: Vec::new(),
        };
        finder.visit_file(&ast);
        let shown = path.strip_prefix(root).unwrap_or(path).display();
        violations.extend(
            finder
                .found
                .into_iter()
                .map(|(line, what)| format!("{shown}:{line}: {what}")),
        );
    }
    for krate in LIBRARY_CRATES {
        let lib: PathBuf = root.join("crates").join(krate).join("src/lib.rs");
        let text = std::fs::read_to_string(&lib).map_err(|e| format!("{}: {e}", lib.display()))?;
        if !text.contains(PRINT_DENY) {
            violations.push(format!("crates/{krate}/src/lib.rs: missing `{PRINT_DENY}`"));
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "architecture rules broken:\n{}",
            violations.join("\n")
        ))
    }
}

struct Finder {
    in_diagnostics: bool,
    found: Vec<(usize, &'static str)>,
}

impl<'ast> Visit<'ast> for Finder {
    fn visit_expr_struct(&mut self, e: &'ast syn::ExprStruct) {
        if !self.in_diagnostics
            && e.path
                .segments
                .last()
                .is_some_and(|s| s.ident == "Diagnostic")
        {
            self.found.push((
                e.span().start().line,
                "`Diagnostic` literal outside the builder",
            ));
        }
        syn::visit::visit_expr_struct(self, e);
    }

    fn visit_lit_str(&mut self, lit: &'ast syn::LitStr) {
        if lit.value().starts_with("$$") {
            self.found
                .push((lit.span().start().line, "`$$` type-string sentinel"));
        }
    }
}
