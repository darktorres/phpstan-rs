//! Parsing of namespaces, use/group-use imports, and qualified/keyword-as-name edge cases.

use super::super::*;
use super::helpers::*;

#[test]
fn a_class_named_enum_is_accepted_as_a_parameter_type_hint() {
    // `enum` is only reserved as a declaration keyword at statement
    // position - as a type hint it's an ordinary (if unusually named)
    // class reference (real shape: vendored phpstan-src's own
    // nsrt/bug-4213.php).
    //
    // `take_name_segment` must keep the identifier's own declared case,
    // via the `Lexer::pending_keyword_raw`/`Parser::keyword_raw`
    // out-of-band channel - reconstructing it from `Keyword::as_str()`'s
    // canonical lowercase spelling would corrupt the source's real
    // capitalization.
    let program = parse("<?php function f(Enum ...$enums): void {}");
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected a FunctionDecl, got {:?}", program[0].kind);
    };
    assert_eq!(
        f.params[0]
            .type_hint
            .as_ref()
            .map(TypeHint::to_string)
            .as_deref(),
        Some("Enum")
    );
    assert!(f.params[0].variadic);
}

#[test]
fn qualified_name_with_keyword_segment_parses() {
    // `Enum` here is a namespace segment, not the `enum` declaration
    // keyword - PHP's keywords are mostly "semi-reserved". Its real
    // declared case must be kept throughout the qualified name (see the
    // sibling test above), not just for a bare, unqualified reference.
    let program = parse("<?php use Piwigo\\Common\\Enum\\AlbumSortOrder;");
    let StmtKind::Use(imports) = &program[0].kind else {
        panic!("expected Use");
    };
    assert_eq!(imports[0].path, "Piwigo\\Common\\Enum\\AlbumSortOrder");
}

#[test]
fn keyword_as_method_and_static_member_name_parses() {
    let program = parse(
        r#"<?php
            $x = $obj->match();
            $y = Foo::new();
            $z = Foo::list();
        "#,
    );
    assert_eq!(program.len(), 3);
}

#[test]
fn keyword_as_named_argument_name_parses() {
    let program = parse("<?php f(namespace: 'x', default: 1);");
    assert!(matches!(&program[0].kind, StmtKind::Expression(_)));
}

#[test]
fn braced_namespace_declarations_splice_their_body_in() {
    let program = parse(
        r#"<?php
            namespace Foo\Bar {
                class A {}
                function f() {}
            }
            namespace {
                class B {}
            }
        "#,
    );
    let class_names: Vec<&str> = program
        .iter()
        .filter_map(|s| match &s.kind {
            StmtKind::ClassDecl(c) => c.name.as_deref(),
            _ => None,
        })
        .collect();
    assert_eq!(class_names, vec!["A", "B"]);
    assert!(
        program
            .iter()
            .any(|s| matches!(&s.kind, StmtKind::FunctionDecl(f) if f.name == "f"))
    );
    assert!(matches!(&program[0].kind, StmtKind::Namespace(n) if n == "Foo\\Bar"));
}

#[test]
fn namespace_relative_name_parses_as_expression_and_as_type() {
    let program = parse(
        r#"<?php
            function f($a = namespace\unknown) {}
            function g(namespace\integer $x) {}
        "#,
    );
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    assert!(matches!(
        &f.params[0].default,
        Some(Expr { kind: ExprKind::Name(n), .. }) if n == "namespace\\unknown"
    ));

    let StmtKind::FunctionDecl(g) = &program[1].kind else {
        panic!("expected FunctionDecl");
    };
    assert_eq!(
        g.params[0]
            .type_hint
            .as_ref()
            .map(TypeHint::to_string)
            .as_deref(),
        Some("namespace\\integer")
    );
}

#[test]
fn group_use_with_trailing_comma_parses() {
    parse(
        r#"<?php
            use Foo\{
                const FOO_CONST,
                function foo_func,
                FooClass as BarClass,
            };
        "#,
    );
}

#[test]
fn use_with_comma_separated_list_parses() {
    let program = parse(r#"<?php use function foo\bar\baz, foo\bar\qux;"#);
    let StmtKind::Use(imports) = &program[0].kind else {
        panic!("expected Use");
    };
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].path, "foo\\bar\\baz");
    assert_eq!(imports[1].path, "foo\\bar\\qux");
}

#[test]
fn use_group_expands_each_member_with_the_shared_prefix() {
    let program = parse(r#"<?php use Foo\Bar\{Baz, Qux as Quux};"#);
    let StmtKind::Use(imports) = &program[0].kind else {
        panic!("expected Use");
    };
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].path, "Foo\\Bar\\Baz");
    assert_eq!(imports[0].alias, None);
    assert_eq!(imports[1].path, "Foo\\Bar\\Qux");
    assert_eq!(imports[1].alias, Some("Quux".into()));
}

#[test]
fn readonly_abstract_final_usable_as_plain_names() {
    let program = parse(
        r#"<?php
            function readonly() {}
            readonly();
            abstract();
            final();
        "#,
    );
    assert!(matches!(&program[0].kind, StmtKind::FunctionDecl(f) if f.name == "readonly"));
    for stmt in &program[1..] {
        assert!(matches!(
            &stmt.kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::Call(_, _),
                ..
            })
        ));
    }
}

#[test]
fn enum_usable_as_plain_function_and_constant_name() {
    let program = parse(
        r#"<?php
            function enum() { return 1; }
            var_dump(enum());
            var_dump(enum);
            enum Suit { case Hearts; }
        "#,
    );
    assert!(matches!(&program[0].kind, StmtKind::FunctionDecl(f) if f.name == "enum"));
    assert!(matches!(&program[3].kind, StmtKind::EnumDecl(e) if e.name == "Suit"));
}

#[test]
fn fn_usable_as_namespace_segment_when_not_a_signature() {
    let program = parse(r#"<?php fn\test();"#);
    assert!(
        matches!(&program[0].kind, StmtKind::Expression(Expr { kind: ExprKind::Call(callee, _), .. })
            if matches!(&callee.kind, ExprKind::Name(n) if n == "fn\\test"))
    );
}

#[test]
fn namespace_relative_call_statement_is_not_a_declaration() {
    let program = parse(r#"<?php namespace\foo();"#);
    assert!(matches!(
        &program[0].kind,
        StmtKind::Expression(Expr { kind: ExprKind::Call(callee, _), .. })
            if matches!(&callee.kind, ExprKind::Name(n) if n == "namespace\\foo")
    ));
}
