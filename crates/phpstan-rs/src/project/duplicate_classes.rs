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

use std::path::{Path, PathBuf};

use crate::checker::{class_kind_identifier, class_kind_word};
use crate::file_id::FileId;
use crate::project::duplicate_class_sites::{DuplicateClassIndex, IndexedClassDeclSite};
use diagnostics::{Diagnostic, DiagnosticBuilder};
use syntax::span::LineIndex;

pub(crate) fn check_duplicate_class_declarations(
    file: FileId,
    index: &LineIndex,
    line_in: &dyn Fn(&Path, u32) -> u32,
    path_for: &dyn Fn(FileId) -> Option<PathBuf>,
    sites: &DuplicateClassIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (name, decl_sites) in sites.iter() {
        // A stub-tagged site is never itself flagged, and never counts
        // toward flagging the real declaration it overrides - see this
        // module's own doc comment.
        let mut real_sites: Vec<(PathBuf, &IndexedClassDeclSite)> = decl_sites
            .iter()
            .filter(|site| !site.is_stub)
            .filter_map(|site| Some((path_for(site.file)?, site)))
            .collect();
        if real_sites.len() < 2 {
            continue;
        }
        // Sorted for determinism, mirroring `duplicate_functions`'s own
        // identical reasoning - which location list order a reader sees
        // must not depend on `FxHashMap` file-load-order.
        real_sites.sort_by(|(a_path, a), (b_path, b)| {
            (a_path, a.span.start).cmp(&(b_path, b.span.start))
        });

        let location_list = real_sites
            .iter()
            .map(|(path, site)| format!("- {}:{}", path.display(), line_in(path, site.span.start)))
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
        for (_, s) in &real_sites {
            if s.file != file {
                continue;
            }
            diagnostics.push(
                DiagnosticBuilder::new(message.clone(), s.span, 0, identifier.clone())
                    .non_ignorable()
                    .build(index),
            );
        }
    }
    diagnostics
}
