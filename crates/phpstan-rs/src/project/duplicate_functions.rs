//! `DuplicateFunctionDeclarationRule`'s equivalent - a top-level
//! `function` name declared unconditionally more than once across the
//! whole project (not just within one file - `checker::duplicates`'s own
//! `check_duplicate_methods`/`check_duplicate_consts` already cover the
//! single-class-body shape of this same idea).
//!
//! Real PHPStan's own `DuplicateFunctionDeclarationRule` is
//! `#[ValidatesStubFiles]` - exactly like `DuplicateClassDeclarationRule`
//! (`project::duplicate_classes`'s own doc comment) - and never runs
//! against ordinary scanned project files at all, only a project's own
//! configured `stubFiles:` list in an isolated pass. This project's
//! version instead gates behind `RulesSettings::check_duplicate_function_
//! declarations` (opt-in, default `false`), following
//! `check_duplicate_class_declarations`'s own precedent: two
//! unconditionally-declared same-named top-level functions in separate
//! `Unit`/`Integration` test files that are never actually loaded
//! together in the same PHP process are a real, structural false
//! positive this checker has no general way to rule out. It also
//! deliberately diverges from the real rule's own body once opted in:
//! an added signature-difference check - see
//! `project::duplicate_fn_sites::DuplicateFunctionSites`'s own doc
//! comment for the concrete reason (phpstorm-stubs' own real `strtr`
//! two-declaration overload split).

use std::path::{Path, PathBuf};

use crate::file_id::FileId;
use crate::project::duplicate_fn_sites::{DuplicateFunctionIndex, IndexedFnDeclSite};
use diagnostics::{Diagnostic, DiagnosticBuilder};
use syntax::span::LineIndex;

fn sites_differ(a: &IndexedFnDeclSite, b: &IndexedFnDeclSite) -> bool {
    a.param_hints != b.param_hints || a.return_hint != b.return_hint
}

/// Real PHPStan's own message format (`vendor/phpstan-src/src/Rules/
/// Functions/DuplicateFunctionDeclarationRule.php`): `"Function %s
/// declared multiple times:\n%s"`, with a newline-joined `- {path}:{line}`
/// list of every declaration site. This project has no
/// `RelativePathHelper` equivalent, so the location list uses each site's
/// absolute path - a disclosed adaptation, not a silent difference.
///
/// `index` holds the lines of `path`, and `line_in` answers for any loaded
/// file the line of an offset.
pub(crate) fn check_duplicate_function_declarations(
    file: FileId,
    index: &LineIndex,
    line_in: &dyn Fn(&Path, u32) -> u32,
    path_for: &dyn Fn(FileId) -> Option<PathBuf>,
    sites: &DuplicateFunctionIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (name, decl_sites) in sites.iter() {
        if decl_sites.len() < 2 {
            continue;
        }
        // Sorted for determinism - which pairs get compared, and the
        // location list's own order, must not depend on `FxHashMap`
        // file-load-order.
        let mut sorted: Vec<(PathBuf, &IndexedFnDeclSite)> = decl_sites
            .iter()
            .filter_map(|site| Some((path_for(site.file)?, site)))
            .collect();
        sorted.sort_by(|(a_path, a), (b_path, b)| {
            (a_path, a.span.start).cmp(&(b_path, b.span.start))
        });

        // Adjacent sites with the identical native signature form one
        // "real duplicate" group; a signature difference starts a fresh
        // group (the phpstorm-stubs version-split idiom, not a bug).
        let mut groups: Vec<Vec<(PathBuf, &IndexedFnDeclSite)>> = Vec::new();
        for s in sorted {
            match groups.last_mut() {
                Some(group) if !sites_differ(group.last().unwrap().1, s.1) => group.push(s),
                _ => groups.push(vec![s]),
            }
        }

        for group in groups {
            if group.len() < 2 {
                continue;
            }
            let location_list = group
                .iter()
                .map(|(path, site)| {
                    format!("- {}:{}", path.display(), line_in(path, site.span.start))
                })
                .collect::<Vec<_>>()
                .join("\n");
            let message = format!("Function {name} declared multiple times:\n{location_list}");
            for (_, s) in &group {
                if s.file != file {
                    continue;
                }
                diagnostics.push(
                    DiagnosticBuilder::new(message.clone(), s.span, 0, "function.duplicate")
                        .non_ignorable()
                        .build(index),
                );
            }
        }
    }
    diagnostics
}
