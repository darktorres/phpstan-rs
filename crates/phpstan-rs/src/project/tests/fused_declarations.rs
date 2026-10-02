//! What `parse_file` finds of a file's declarations in one walk is what the
//! separate walks it replaced found.
use super::super::*;

const SOURCES: &[&str] = &[
    "<?php function a() {} function B() {} const X = 1; define('Y', 2); class C {}",
    "<?php namespace App\\Util; use Lib\\Thing; function f() {} class K extends Thing {} interface I {} trait T {} enum E {}",
    "<?php namespace A { function g() {} class G {} } namespace B { const Z = 3; function g() {} }",
    "<?php if (!function_exists('h')) { function h() {} } if (!class_exists('H')) { class H {} } define('W', 1);",
    "<?php function outer() { function inner() {} class Local {} define('INSIDE', 1); } $f = function () { class InClosure {} };",
    "<?php $o = new class { public function m() {} }; $p = new class extends Base {};",
    "<?php /** @template T */ class Box {} /** @param int $x */ function typed(int $x): string { return ''; } \\define('Q', 5);",
    "<?php namespace N; define('N\\\\C1', 1); define('C2', 2); const D = 4; function c() {} class c {} ",
    "<?php class Dup {} class Dup {} function dup() {} function dup() {}",
    "<?php declare(strict_types=1); namespace D; { class InBlock {} } function after() {}",
    "<?php switch (1) { case 1: function inSwitch() {} } try { class InTry {} } catch (Exception $e) {}",
    "<?php",
];

/// A built declaration as its kind, name and whether it is conditional.
fn summary(decl: &BuiltDecl) -> (u8, String, bool) {
    match decl {
        BuiltDecl::Class {
            name, conditional, ..
        } => (0, name.to_string(), *conditional),
        BuiltDecl::Function {
            name, conditional, ..
        } => (1, name.to_string(), *conditional),
        BuiltDecl::GlobalConst { name, .. } => (2, name.to_string(), false),
    }
}

#[test]
fn the_one_walk_finds_what_the_five_walks_found() {
    let interner = Arc::new(Interner::new());
    for (i, source) in SOURCES.iter().enumerate() {
        for is_stub in [false, true] {
            let path = Path::new("/p/file.php");
            let parsed = parse_file(path, source.as_bytes(), is_stub, &interner)
                .unwrap_or_else(|_| panic!("source {i} parses"));
            let (functions, classes) = declared_names(&parsed.ast);
            assert_eq!(parsed.functions, functions, "functions of source {i}");
            assert_eq!(parsed.classes, classes, "classes of source {i}");
            assert_eq!(
                parsed.constants,
                declared_constants(&parsed.ast),
                "constants of source {i}"
            );
            let origin = Origin::File(Arc::from(path));
            let built = crate::registry::build_declarations(&parsed.ast, &origin);
            assert_eq!(
                parsed.built_decls.iter().map(summary).collect::<Vec<_>>(),
                built.iter().map(summary).collect::<Vec<_>>(),
                "built declarations of source {i}"
            );
            let mut fn_sites = DuplicateFunctionSites::default();
            fn_sites.index(&parsed.ast, path);
            let mut class_sites = DuplicateClassSites::default();
            class_sites.index(&parsed.ast, path, is_stub);
            let fns = |sites: &DuplicateFunctionSites| {
                let mut v: Vec<_> = sites
                    .iter()
                    .map(|(name, s)| {
                        (
                            name.clone(),
                            s.iter()
                                .map(|s| (s.span, s.param_hints.clone(), s.return_hint.clone()))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                v.sort_by(|a, b| a.0.cmp(&b.0));
                format!("{v:?}")
            };
            assert_eq!(
                fns(&parsed.duplicate_fn_sites),
                fns(&fn_sites),
                "function sites of source {i}"
            );
            let classes = |sites: &DuplicateClassSites| {
                let mut v: Vec<_> = sites
                    .iter()
                    .map(|(name, s)| {
                        (
                            name.clone(),
                            s.iter()
                                .map(|s| (s.span, s.kind, s.is_stub))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                v.sort_by(|a, b| a.0.cmp(&b.0));
                format!("{v:?}")
            };
            assert_eq!(
                classes(&parsed.duplicate_class_sites),
                classes(&class_sites),
                "class sites of source {i}"
            );
        }
    }
}
