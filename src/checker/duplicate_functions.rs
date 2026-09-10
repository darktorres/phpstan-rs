//! Phase 63: `DuplicateFunctionDeclarationRule`'s equivalent - a top-level
//! `function` name declared unconditionally more than once across the
//! whole project (not just within one file - `checker::duplicates`'s own
//! `check_duplicate_methods`/`check_duplicate_consts` already cover the
//! single-class-body shape of this same idea). Deliberately diverges from
//! real PHPStan's own guard-less rule with an added signature-difference
//! check - see `project::duplicate_fn_sites::DuplicateFunctionSites`'s own
//! doc comment for the concrete, corpus-evidenced reason
//! (phpstorm-stubs' own real `strtr` two-declaration overload split).

use std::path::Path;

use crate::checker::Diagnostic;
use crate::project::duplicate_fn_sites::{DuplicateFunctionSites, FnDeclSite};

fn sites_differ(a: &FnDeclSite, b: &FnDeclSite) -> bool {
    a.param_hints != b.param_hints || a.return_hint != b.return_hint
}

/// Real PHPStan's own message format (`vendor/phpstan-src/src/Rules/
/// Functions/DuplicateFunctionDeclarationRule.php`): `"Function %s
/// declared multiple times:\n%s"`, with a newline-joined `- {path}:{line}`
/// list of every declaration site. This project has no
/// `RelativePathHelper` equivalent, so the location list uses each site's
/// absolute path - a disclosed adaptation, not a silent difference.
pub(crate) fn check_duplicate_function_declarations(
    path: &Path,
    sites: &DuplicateFunctionSites,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (name, decl_sites) in sites.iter() {
        if decl_sites.len() < 2 {
            continue;
        }
        // Sorted for determinism - which pairs get compared, and the
        // location list's own order, must not depend on `FxHashMap`
        // file-load-order.
        let mut sorted: Vec<&FnDeclSite> = decl_sites.iter().collect();
        sorted.sort_by(|a, b| {
            (&a.path, a.span.start.line, a.span.start.col).cmp(&(
                &b.path,
                b.span.start.line,
                b.span.start.col,
            ))
        });

        // Adjacent sites with the identical native signature form one
        // "real duplicate" group; a signature difference starts a fresh
        // group (the phpstorm-stubs version-split idiom, not a bug).
        let mut groups: Vec<Vec<&FnDeclSite>> = Vec::new();
        for s in sorted {
            match groups.last_mut() {
                Some(group) if !sites_differ(group.last().unwrap(), s) => group.push(s),
                _ => groups.push(vec![s]),
            }
        }

        for group in groups {
            if group.len() < 2 {
                continue;
            }
            let location_list = group
                .iter()
                .map(|s| format!("- {}:{}", s.path.display(), s.span.start.line))
                .collect::<Vec<_>>()
                .join("\n");
            let message = format!("Function {name} declared multiple times:\n{location_list}");
            for s in &group {
                if s.path != path {
                    continue;
                }
                diagnostics.push(Diagnostic {
                    location: name.clone(),
                    message: message.clone(),
                    span: s.span,
                    level: 0,
                    identifier: "function.duplicate".to_string(),
                    tip: None,
                    non_ignorable: true,
                    metadata: std::collections::HashMap::new(),
                    fix: None,
                });
            }
        }
    }
    diagnostics
}
