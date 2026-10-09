use super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::visit::arena::{self, Visitor};

fn parse(src: &str) -> crate::parser::ParsedProgram {
    Parser::from_token_stream(Lexer::new(src).lex_token_stream())
        .parse_program()
        .expect("parses")
}

/// Every name reference in the file with its resolution, in walk order.
fn resolutions(src: &str) -> Vec<(String, ResolvedName)> {
    struct Collect(Vec<(String, ResolvedName)>);
    impl Collect {
        fn push(&mut self, name: &QualifiedName) {
            self.0
                .push((name.written.to_string(), name.resolved.clone()));
        }
    }
    impl Visitor for Collect {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            let Some(e) = arenas.expr(id) else {
                return;
            };
            match &e.kind {
                ExprKind::Name(n) => {
                    if let Some(name) = arenas.qualified_name(n) {
                        self.push(name);
                    }
                }
                ExprKind::ClassConstFetch(StaticTarget::Name(n), _)
                | ExprKind::StaticCall(StaticTarget::Name(n), _, _) => self.push(n),
                ExprKind::Instanceof(_, InstanceofTarget::Name(n)) => {
                    if let Some(name) = arenas.qualified_name(n) {
                        self.push(name);
                    }
                }
                _ => {}
            }
            arena::walk_expr(self, arenas, id);
        }
        fn visit_class_decl(&mut self, arenas: &NodeArenas, c: &ClassDecl) {
            if let Some(def) = arenas.class(c.def) {
                for n in def.extends(arenas).iter().chain(def.implements(arenas)) {
                    self.push(n);
                }
            }
            arena::walk_class_decl(self, arenas, c);
        }
        fn visit_attribute(&mut self, arenas: &NodeArenas, a: &Attribute) {
            self.push(&a.name);
            arena::walk_attribute(self, arenas, a);
        }
        fn visit_catch_clause(&mut self, arenas: &NodeArenas, c: &CatchClause) {
            for n in arenas.qualified_names(&c.types) {
                self.push(n);
            }
            arena::walk_catch_clause(self, arenas, c);
        }
        fn visit_type_hint(&mut self, arenas: &NodeArenas, h: &TypeHint) {
            if let TypeHintKind::Name(n) = &h.kind
                && let Some(name) = arenas.qualified_name(n)
            {
                self.push(name);
            }
        }
    }
    let program = parse(src);
    let mut c = Collect(Vec::new());
    arena::walk_stmts(&mut c, &program.arenas, program.statements());
    c.0
}

fn exact(s: &str) -> ResolvedName {
    ResolvedName::Exact(s.into())
}

fn find<'a>(all: &'a [(String, ResolvedName)], written: &str) -> &'a ResolvedName {
    &all.iter()
        .find(|(w, _)| w == written)
        .unwrap_or_else(|| panic!("{written} not found in {all:?}"))
        .1
}

#[test]
fn names_resolve_through_the_namespace_and_imports() {
    let all = resolutions(
        r#"<?php
        namespace App;
        use Lib\Thing;
        use Lib\Other as Alias;
        use function Lib\helper;
        use const Lib\LIMIT;
        #[Attr]
        class C extends Thing implements Alias {
            public function m(\Absolute $a, Local $b, int $c, self $d, integer $e): void {
                new Thing();
                helper();
                strlen('x');
                LIMIT;
                PHP_EOL;
                Alias::X;
                Sub\Deep::run();
                try {} catch (\Exception | Thing $e) {}
                $x instanceof Thing;
                parent::m();
            }
        }
        "#,
    );
    assert_eq!(find(&all, "Attr"), &exact("App\\Attr"));
    assert_eq!(find(&all, "Thing"), &exact("Lib\\Thing"));
    assert_eq!(find(&all, "Alias"), &exact("Lib\\Other"));
    assert_eq!(find(&all, "\\Absolute"), &exact("Absolute"));
    assert_eq!(find(&all, "Local"), &exact("App\\Local"));
    // A native keyword type isn't a name; `integer` is a class to PHP.
    assert!(all.iter().all(|(w, _)| w != "int" && w != "self"));
    assert_eq!(find(&all, "integer"), &exact("App\\integer"));
    assert_eq!(find(&all, "parent"), &ResolvedName::Unresolved);
    assert_eq!(find(&all, "helper"), &exact("Lib\\helper"));
    assert_eq!(
        find(&all, "strlen"),
        &ResolvedName::WithGlobalFallback("App\\strlen".into())
    );
    assert_eq!(find(&all, "LIMIT"), &exact("Lib\\LIMIT"));
    assert_eq!(
        find(&all, "PHP_EOL"),
        &ResolvedName::WithGlobalFallback("App\\PHP_EOL".into())
    );
    assert_eq!(find(&all, "Sub\\Deep"), &exact("App\\Sub\\Deep"));
    assert_eq!(find(&all, "\\Exception"), &exact("Exception"));
}

#[test]
fn a_later_namespace_resolves_what_follows_it_and_imports_do_not_leak() {
    let all = resolutions(
        r#"<?php
        namespace A { use X\Y; new Y(); }
        namespace B { new Y(); }
        "#,
    );
    let ys: Vec<&ResolvedName> = all
        .iter()
        .filter(|(w, _)| w == "Y")
        .map(|(_, r)| r)
        .collect();
    assert_eq!(ys, vec![&exact("X\\Y"), &exact("B\\Y")]);
}

#[test]
fn outside_any_namespace_a_function_has_no_fallback() {
    let all = resolutions("<?php strlen('x');");
    assert_eq!(find(&all, "strlen"), &exact("strlen"));
}

#[test]
fn declarations_carry_the_namespace_they_are_declared_in() {
    let program = parse(
        "<?php\nnamespace A;\nclass C {}\nconst K = 1;\nnamespace B {\n    enum E {}\n    if (true) {\n        function f() {}\n    }\n}\n",
    );
    let mut names = Vec::new();
    for_each_in_context_ids(
        &program.arenas,
        program.statements(),
        |_arenas, stmt, _| match &stmt.kind {
            StmtKind::ClassDecl(c) => names.push(Some(
                program.arenas.class_decl(c.clone()).namespaced_name.clone(),
            )),
            StmtKind::GlobalConst(consts) => names.extend(
                program
                    .arenas
                    .class_consts(consts)
                    .iter()
                    .map(|c| c.namespaced_name.clone()),
            ),
            StmtKind::If(IfStmt { then_branch, .. }) => {
                let StmtKind::Block(body) =
                    &program.arenas.stmt(*then_branch).expect("then branch").kind
                else {
                    panic!("expected a block");
                };
                let StmtKind::FunctionDecl(f) = &program
                    .arenas
                    .stmt(program.arenas.stmt_ids(body)[0])
                    .expect("body statement")
                    .kind
                else {
                    panic!("expected a function");
                };
                let f = program.arenas.function(*f).expect("function payload");
                names.push(f.namespaced_name.clone());
            }
            _ => {}
        },
    );
    let names: Vec<Option<&str>> = names.iter().map(Option::as_deref).collect();
    assert_eq!(
        names,
        [Some("A\\C"), Some("A\\K"), Some("B\\E"), Some("B\\f")]
    );
}

#[test]
fn the_context_at_a_position_has_only_what_precedes_it() {
    let src = "<?php\nnamespace A;\nuse X\\Y;\nnamespace B;\nuse Z\\Y;\n";
    let program = parse(src);
    let index = crate::span::LineIndex::new(src.as_bytes());
    let at = |line| {
        context_at_ids(
            &program.arenas,
            program.statements(),
            index.line_start(line).unwrap_or(u32::MAX),
        )
    };
    assert_eq!(at(3).resolve("Y"), "A\\Y");
    assert_eq!(at(4).resolve("Y"), "X\\Y");
    assert_eq!(at(6).resolve("Y"), "Z\\Y");
}

#[test]
fn resolves_to_is_resolve_equal_to_the_name() {
    let sources = [
        "<?php",
        "<?php namespace App\\Models; use Lib\\Thing; use Other\\Deep\\Name as Alias; use Vendor\\Pkg;",
        "<?php namespace App; use Lib\\Thing;",
    ];
    let names = [
        "Thing",
        "thing",
        "Alias",
        "\\Thing",
        "\\App\\Models\\Thing",
        "Pkg\\Sub\\Class_",
        "pkg\\Sub\\Class_",
        "namespace\\Thing",
        "Models\\Thing",
        "Missing",
        "",
    ];
    let candidates = [
        "Thing",
        "Lib\\Thing",
        "App\\Models\\Thing",
        "App\\Thing",
        "Other\\Deep\\Name",
        "Vendor\\Pkg\\Sub\\Class_",
        "Models\\Thing",
        "Missing",
        "App\\Models\\Missing",
        "App",
        "",
    ];
    for source in sources {
        let program = parse(source);
        let ctx = context_at_ids(&program.arenas, program.statements(), u32::MAX);
        for name in names {
            for fqn in candidates {
                assert_eq!(
                    ctx.resolves_to(name, fqn),
                    ctx.resolve(name) == fqn,
                    "{source}: {name} against {fqn}"
                );
            }
            let resolved = ctx.resolve(name);
            assert!(ctx.resolves_to(name, &resolved), "{source}: {name}");
        }
    }
}

#[test]
fn the_import_table_answers_as_the_table_of_owned_strings_did() {
    use crate::name::Interner;
    use rustc_hash::FxHashMap;
    // What the table was: (kind, name as `use_map_key` normalizes it) to
    // the path, a later import of a name replacing an earlier one.
    fn key(kind: UseKind, name: &str) -> (UseKind, String) {
        match kind {
            UseKind::Const => (kind, name.to_string()),
            UseKind::Class | UseKind::Function => (kind, name.to_ascii_lowercase()),
        }
    }
    let interner = Interner::global();
    let paths = [
        "Foo\\Bar",
        "Foo\\BAR",
        "foo\\bar",
        "Lib\\Thing\\Widget",
        "Lib\\Thing\\widget",
        "App",
        "app",
        "Vendor\\PHP\\Util\\FUNC_name",
        "Vendor\\Constants\\MAX_SIZE",
        "Vendor\\Constants\\max_size",
        "Z",
    ];
    let aliases = [
        None,
        None,
        Some("Baz"),
        Some("BAZ"),
        Some("w"),
        Some("W"),
        Some("Max"),
    ];
    let mut state = 0x1234_5678_9ABC_DEF0u64;
    let mut next = move |bound: usize| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 33) as usize) % bound
    };
    for _ in 0..400 {
        let mut table = UseMap::default();
        let mut reference: FxHashMap<(UseKind, String), String> = FxHashMap::default();
        for _ in 0..next(9) {
            let kind = [UseKind::Class, UseKind::Function, UseKind::Const][next(3)];
            let path = paths[next(paths.len())];
            let alias = aliases[next(aliases.len())];
            let path_name = interner.name(path);
            match alias {
                Some(alias) => {
                    table.insert(kind, &interner.name(alias), 0, path_name.clone());
                    reference.insert(key(kind, alias), path.to_string());
                }
                None => {
                    let from = path.rfind('\\').map_or(0, |at| at + 1);
                    table.insert(kind, &path_name, from, path_name.clone());
                    reference.insert(key(kind, &path[from..]), path.to_string());
                }
            }
        }
        for kind in [UseKind::Class, UseKind::Function, UseKind::Const] {
            for name in [
                "Bar",
                "BAR",
                "bar",
                "Widget",
                "WIDGET",
                "widget",
                "App",
                "APP",
                "app",
                "FUNC_name",
                "func_NAME",
                "MAX_SIZE",
                "max_size",
                "Max",
                "MAX",
                "Baz",
                "baz",
                "w",
                "W",
                "Z",
                "z",
                "Foo",
                "",
                "Nope",
            ] {
                assert_eq!(
                    table.get(kind, name).map(|n| n.as_str().to_string()),
                    reference.get(&key(kind, name)).cloned(),
                    "{kind:?} {name}"
                );
                assert_eq!(
                    table.contains(kind, name),
                    reference.contains_key(&key(kind, name))
                );
            }
        }
    }
    // A name of more than the stack buffer's length.
    let long = "A".repeat(200);
    let mut table = UseMap::default();
    table.insert(
        UseKind::Class,
        &interner.name(&long),
        0,
        interner.name("Long\\Path"),
    );
    assert!(table.contains(UseKind::Class, &long.to_ascii_lowercase()));
    assert!(table.contains(UseKind::Class, &long));
    assert!(!table.contains(UseKind::Const, &long.to_ascii_lowercase()));
}

#[test]
fn a_context_has_a_version_for_what_it_resolves_by() {
    let program = parse("<?php namespace A; use B\\C; use D\\E;");
    let mut seen = Vec::new();
    for_each_in_context_ids(&program.arenas, program.statements(), |_, _, ctx| {
        seen.push(ctx.version())
    });
    // The namespace and each `use` is a state of its own.
    assert_eq!(seen.len(), 3);
    assert!(seen[0] != 0 && seen[0] != seen[1] && seen[1] != seen[2]);
    // A copy resolves as the original does, and says so.
    let mut ctx = context_at_ids(&program.arenas, program.statements(), u32::MAX);
    let copy = ctx.clone();
    assert_eq!(ctx.version(), copy.version());
    ctx.set_namespace("A");
    assert_ne!(ctx.version(), copy.version());
    assert_eq!(NameContext::default().version(), 0);
}
