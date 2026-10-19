//! The architecture gate: rules on the source that hold from the phase
//! that lands them on.
//!
//! - A `Diagnostic` is built through its builder: no `Diagnostic { .. }`
//!   expression outside the `diagnostics` crate.
//! - No `"$$`-prefixed type-string sentinel.
//! - Every library crate denies the print macros in its non-test code.
//! - The engine resolves no path against the process: no
//!   `std::env::current_dir`, `fs::canonicalize` or `std::path::absolute`
//!   in the package outside the session, the binary and the language
//!   server (which set the project's absolute root) and tests.

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
    "path-bytes",
    "stub-sources",
    "config",
    "stack",
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
            in_engine: is_engine_source(root, path),
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

/// Whether `path` is package source the path-resolution rule covers:
/// not the session, the binary, the language server or a test.
pub(crate) fn is_engine_source(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root.join("crates/phpstan-rs/src")) else {
        return false;
    };
    let names: Vec<_> = rel
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
    let exempt = matches!(
        names.as_slice(),
        ["project", "session.rs"] | ["main.rs"] | ["lsp.rs"] | ["main" | "lsp", ..]
    );
    !exempt
        && !names
            .iter()
            .any(|name| matches!(*name, "tests" | "tests.rs"))
}

pub(crate) struct Finder {
    pub(crate) in_diagnostics: bool,
    pub(crate) in_engine: bool,
    pub(crate) found: Vec<(usize, &'static str)>,
}

const PATH_RESOLUTION: &str =
    "path resolved against the process (current_dir/canonicalize/absolute) in the engine";

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

    fn visit_expr_path(&mut self, e: &'ast syn::ExprPath) {
        let segments: Vec<String> = e
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        let tail: Vec<&str> = segments.iter().rev().take(2).map(String::as_str).collect();
        if self.in_engine
            && matches!(
                tail.as_slice(),
                ["current_dir", "env"] | ["canonicalize", "fs"] | ["absolute", "path"]
            )
        {
            self.found.push((e.span().start().line, PATH_RESOLUTION));
        }
        syn::visit::visit_expr_path(self, e);
    }

    fn visit_expr_method_call(&mut self, e: &'ast syn::ExprMethodCall) {
        if self.in_engine && e.method == "canonicalize" {
            self.found.push((e.span().start().line, PATH_RESOLUTION));
        }
        syn::visit::visit_expr_method_call(self, e);
    }

    fn visit_lit_str(&mut self, lit: &'ast syn::LitStr) {
        if lit.value().starts_with("$$") {
            self.found
                .push((lit.span().start().line, "`$$` type-string sentinel"));
        }
    }
}
