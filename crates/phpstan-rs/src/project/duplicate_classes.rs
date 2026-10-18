//! A same-shape sibling of `project::duplicate_functions`, but going
//! beyond real PHPStan rather than porting one of its existing rules.
//! Real PHPStan's own `DuplicateClassDeclarationRule` is tagged only
//! `#[ValidatesStubFiles]`, wired exclusively into `StubValidator`'s own
//! isolated, from-scratch reflector over a project's configured
//! `stubFiles:` entries alone - it never runs against a project's
//! ordinary analyzed/vendor tree at all. Real PHPStan therefore has no
//! whole-project "same class declared twice" detection to port here;
//! this is phpstan-rs choosing to go stricter, not filling a gap real
//! PHPStan already closes elsewhere.
//!
//! Unlike the function-side check, this one is opt-in
//! (`RulesSettings::check_duplicate_class_declarations`, default `false`): its
//! one known corpus false-positive shape (a vendored built-in stub
//! override, e.g. `Imagick.stub.php`, colliding with the real PHP
//! extension's own class) can only be defused by a project's
//! `stub_paths` config being correctly populated, and no existing
//! `phpstan-rs.toml` in the wild does that yet (they put such files under
//! plain `scan_paths` instead, indistinguishable from an ordinary context
//! file). Every `ClassDeclSite` this check sees already carries its own
//! `is_stub` provenance (`project::duplicate_class_sites::
//! DuplicateClassSites`) - a site tagged `is_stub` is filtered out
//! entirely before counting, so a stub override is never itself flagged
//! and never counts toward flagging the real declaration it overrides.
//! Unlike `duplicate_functions`'s own signature-differs grouping (built
//! for a real, confirmed phpstorm-stubs version-split idiom on the
//! function side), no such grouping exists here - real PHPStan's own
//! rule has no equivalent concept for a class-like declaration, and no
//! corpus evidence motivates inventing one.

use std::path::Path;

use crate::checker::{class_kind_identifier, class_kind_word};
use crate::file_id::FileId;
use crate::project::duplicate_class_sites::{ClassDeclSite, DuplicateClassSites};
use diagnostics::{Diagnostic, DiagnosticBuilder, report_path};
use syntax::span::LineIndex;

/// `file` is the file being checked, and `index` holds its lines. `path_of`
/// names the file of a site, and `line_in` answers for any loaded file the
/// line of an offset.
pub(crate) fn check_duplicate_class_declarations<'a>(
    file: FileId,
    root: Option<&Path>,
    index: &LineIndex,
    path_of: &dyn Fn(FileId) -> &'a Path,
    line_in: &dyn Fn(FileId, u32) -> u32,
    sites: &DuplicateClassSites,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (name, decl_sites) in sites.iter() {
        // A stub-tagged site is never itself flagged, and never counts
        // toward flagging the real declaration it overrides - see this
        // module's own doc comment.
        let mut real_sites: Vec<&(FileId, ClassDeclSite)> =
            decl_sites.iter().filter(|(_, s)| !s.is_stub).collect();
        if real_sites.len() < 2 {
            continue;
        }
        // Sorted for determinism, mirroring `duplicate_functions`'s own
        // identical reasoning - which location list order a reader sees
        // must not depend on `FxHashMap` file-load-order.
        real_sites
            .sort_by(|a, b| (path_of(a.0), a.1.span.start).cmp(&(path_of(b.0), b.1.span.start)));

        // Each site as reports name a file, with `/` between its names on
        // every platform, so a baseline made on one matches on another.
        let location_list = real_sites
            .iter()
            .map(|(site_file, site)| {
                format!(
                    "- {}:{}",
                    report_path(super::shown_path(root, path_of(*site_file))),
                    line_in(*site_file, site.span.start)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        // Every real site's own `kind` is expected to agree in practice
        // (the same fully-qualified name naming a class in one file and,
        // say, an interface in another would already be a stranger bug
        // than plain duplication) - the first site's own kind names the
        // message/identifier, matching this project's general "don't
        // guess past what's already known" bias rather than inventing a
        // dedicated mixed-kind message no real corpus evidence motivates.
        let kind = real_sites[0].1.kind;
        let word = class_kind_word(kind);
        let identifier = format!("{}.duplicate", class_kind_identifier(&kind));
        let message = format!(
            "{}{} {name} declared multiple times:\n{location_list}",
            word[..1].to_uppercase(),
            &word[1..]
        );
        for (site_file, site) in &real_sites {
            if *site_file != file {
                continue;
            }
            diagnostics.push(
                DiagnosticBuilder::new(message.clone(), site.span, 0, identifier.clone())
                    .non_ignorable()
                    .build(index),
            );
        }
    }
    diagnostics
}
