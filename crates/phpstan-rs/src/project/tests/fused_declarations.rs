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

/// The global constants one file's parse tree declares, by the names the
/// registry holds them under.
fn declared_constants(stmts: &[syntax::ast::Stmt]) -> Vec<syntax::name::Name> {
    let mut constants = Vec::new();
    syntax::declarations::for_each_declaration(stmts, |declaration, _, _| {
        if let syntax::declarations::Declared::Constant(_)
        | syntax::declarations::Declared::Define { .. } = declaration.declared
        {
            constants.push(syntax::name::Name::from(
                declaration.name().trim_start_matches('\\'),
            ));
        }
    });
    constants
}

#[test]
fn the_one_walk_finds_what_the_five_walks_found() {
    for (i, source) in SOURCES.iter().enumerate() {
        for is_stub in [false, true] {
            let path = Path::new("/p/file.php");
            let parsed = parse_file(path, source.as_bytes(), is_stub)
                .unwrap_or_else(|_| panic!("source {i} parses"));
            let (functions, classes) = declared_names(parsed.ast.as_deref().unwrap());
            assert_eq!(parsed.functions, functions, "functions of source {i}");
            assert_eq!(parsed.classes, classes, "classes of source {i}");
            assert_eq!(
                parsed.constants,
                declared_constants(parsed.ast.as_deref().unwrap()),
                "constants of source {i}"
            );
            let origin = Origin::File(Arc::from(path));
            let built =
                crate::registry::build_declarations(parsed.ast.as_deref().unwrap(), &origin);
            assert_eq!(
                parsed.built_decls.iter().map(summary).collect::<Vec<_>>(),
                built.iter().map(summary).collect::<Vec<_>>(),
                "built declarations of source {i}"
            );
            let mut fn_sites = DuplicateFunctionSites::default();
            fn_sites.index(parsed.ast.as_deref().unwrap(), path);
            let mut class_sites = DuplicateClassSites::default();
            class_sites.index(parsed.ast.as_deref().unwrap(), path, is_stub);
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

#[test]
fn the_unconditional_classes_are_those_the_declaration_walk_finds() {
    use syntax::ast::ClassKind;
    use syntax::declarations::{Declared, for_each_declaration};
    let mut sources: Vec<&str> = SOURCES.to_vec();
    sources.extend([
        "<?php { class InBlock {} { class Deeper {} } } declare(ticks=1) { class InDeclare {} } class Top {}",
        "<?php namespace N { class A {} } namespace M { class B {} enum E {} }",
        "<?php if (true) { class Cond {} } else { class Other {} } while (0) { class InLoop {} } class Plain {}",
        "<?php function f() { class InFn {} } $x = function () { class InClosure {} }; $o = new class {}; class Last {}",
        "<?php foreach ([] as $i) { class InForeach {} } class Kept {} interface I {} trait T {}",
    ]);
    for (i, source) in sources.iter().enumerate() {
        let parsed = syntax::parse(source.as_bytes()).expect("parses");
        let mut by_visitor: Vec<String> = Vec::new();
        for_each_declaration(&parsed.stmts, |declaration, _, _| {
            if let Declared::Class(c) = declaration.declared
                && c.def.kind != ClassKind::Enum
                && !declaration.conditional
            {
                by_visitor.push(declaration.name());
            }
        });
        let mut found = Vec::new();
        super::super::class_shape::collect_classes(&parsed.stmts, &mut found);
        let by_walk: Vec<String> = found.into_iter().map(|(name, _)| name).collect();
        assert_eq!(by_walk, by_visitor, "source {i}");
    }
}

#[test]
fn leaving_out_the_walks_a_file_cannot_need_gives_the_tree_the_walks_gave() {
    let sources = [
        "<?php class A { public function m() {} }",
        "<?php $o = new class { public function m() {} }; $p = new class {};",
        "<?php $a = fn() => new class {}; function f() { return new class extends B {}; }",
        "<?php use JetBrains\\PhpStorm\\Internal\\LanguageLevelTypeAware; class C { #[LanguageLevelTypeAware(['8.0' => 'int'], default: 'string')] public $p; }",
        "<?php use JetBrains\\PhpStorm\\Internal\\PhpStormStubsElementAvailable as Avail; function g(#[Avail(from: '5.3', to: '7.4')] $a, #[Avail('8.0')] $b) {}",
        "<?php #[\\JetBrains\\PhpStorm\\Internal\\PhpStormStubsElementAvailable('8.0')] function h() {} #[\\JetBrains\\PhpStorm\\Internal\\PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] function h() {}",
        "<?php namespace N; use JetBrains\\PhpStorm\\Internal as I; #[I\\PhpVersionAware(['8.1' => 'int'], default: 'string')] function k(): int|string { return 1; }",
        "<?php #[Other] function plain() {} $x = new class {};",
        "<?php",
    ];
    for (i, source) in sources.iter().enumerate() {
        let path = Path::new("/p/file.php");
        let parsed =
            syntax::parse_watching(source.as_bytes(), None, Some(&WATCHED)).expect("parses");
        let facts = parsed.facts;
        // What the walks give with nothing left out.
        let mut every = syntax::parse(source.as_bytes()).expect("parses").stmts;
        crate::stub_attributes::apply_stub_attributes(&mut every);
        crate::registry::name_anonymous_classes(
            &mut every,
            &path.display().to_string(),
            &syntax::span::LineIndex::new(source.as_bytes()),
        );
        let Parsed {
            ast: gated,
            facts: gated_facts,
            ..
        } = parse_with_ignores(None, path, *source, None, false).expect("parses");
        assert_eq!(gated_facts, facts, "facts of source {i}");
        assert_eq!(format!("{gated:?}"), format!("{every:?}"), "source {i}");
        // The anonymous classes' registry entries.
        let origin = Origin::File(Arc::from(path));
        let all = crate::registry::anonymous_class_decls(&gated, &origin);
        assert_eq!(
            !all.is_empty(),
            facts.has_anonymous_class,
            "anonymous classes of source {i}"
        );
    }
}

#[test]
fn what_a_file_names_is_found_among_its_identifiers() {
    let facts = |source: &str| {
        syntax::parse_watching(source.as_bytes(), None, Some(&WATCHED))
            .unwrap()
            .facts
    };
    let rebind =
        |source: &str| facts(source).mentions & crate::checker::unused::MENTIONS_REBIND != 0;
    let stub_attribute =
        |source: &str| facts(source).mentions & crate::stub_attributes::MENTIONS != 0;
    let guard =
        |source: &str| facts(source).mentions & crate::checker::includes::MENTIONS_GUARD != 0;
    assert_eq!(facts("<?php class A {}"), syntax::FileFacts::default());
    assert!(facts("<?php $f = new class {};").has_anonymous_class);
    assert!(facts("<?php $f = new #[A] class {};").has_anonymous_class);
    assert!(!facts("<?php $f = new Foo(); class Bar {}").has_anonymous_class);
    for source in [
        "<?php Closure::bind($f, null, A::class);",
        "<?php $f->bindTo($o);",
        "<?php $f->BINDTO($o);",
        "<?php $f->call($o);",
    ] {
        assert!(rebind(source), "{source}");
    }
    assert!(!rebind("<?php $f->bound($o); $g->called();"));
    // A variable of the same spelling names nothing.
    assert!(!rebind("<?php $bind = 1; $call = 2;"));
    for attribute in [
        "<?php use X\\PhpVersionAware; ",
        "<?php #[\\X\\LanguageLevelTypeAware([])] function f() {}",
        "<?php use X\\PhpStormStubsElementAvailable as Y;",
    ] {
        assert!(stub_attribute(attribute), "{attribute}");
    }
    assert!(!stub_attribute("<?php #[Other] function f() {}"));
    for source in [
        "<?php if (file_exists($p)) {}",
        "<?php if (IS_READABLE($p)) {}",
        "<?php set_include_path('/x');",
        "<?php ini_set('include_path', '/x');",
    ] {
        assert!(guard(source), "{source}");
    }
    assert!(!guard("<?php echo 1;"));
}
