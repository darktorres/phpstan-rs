//! Parsing of top-level and class-member declarations: classes, interfaces, traits, enums, functions, properties, constants, and their modifiers.

use super::super::*;
use super::helpers::*;

#[test]
fn top_level_const_declaration_parses() {
    let program = parse("<?php const FOO = 1, BAR = 2;");
    let StmtKind::GlobalConst(consts) = &program[0].kind else {
        panic!("expected GlobalConst");
    };
    assert_eq!(consts.len(), 2);
    assert_eq!(consts[0].name, "FOO");
    assert_eq!(consts[1].name, "BAR");
}

#[test]
fn true_and_false_are_valid_return_types() {
    // PHP 8.2+ stand-alone true/false types, as used throughout
    // php-src's own stub files (e.g. `string|false`).
    let program = parse("<?php function f(): string|false { return false; }");
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    assert_eq!(f.return_type.as_deref(), Some("string|false"));
}

#[test]
fn pure_enum_parses_with_unbacked_cases() {
    let program = parse(
        r#"<?php
            enum SortDirection {
                case Ascending;
                case Descending;
            }
        "#,
    );
    let StmtKind::EnumDecl(e) = &program[0].kind else {
        panic!("expected EnumDecl");
    };
    assert_eq!(e.name, "SortDirection");
    assert!(e.backing_type.is_none());
    assert_eq!(e.cases.len(), 2);
    assert_eq!(e.cases[0].name, "Ascending");
    assert!(e.cases[0].value.is_none());
}

#[test]
fn backed_enum_with_interface_and_method_parses() {
    let program = parse(
        r#"<?php
            enum Suit: string implements \JsonSerializable
            {
                case Hearts = 'H';

                public function label(): string
                {
                    return match ($this) {
                        Suit::Hearts => 'Hearts',
                    };
                }
            }
        "#,
    );
    let StmtKind::EnumDecl(e) = &program[0].kind else {
        panic!("expected EnumDecl");
    };
    assert_eq!(e.backing_type.as_deref(), Some("string"));
    assert_eq!(e.implements, vec!["\\JsonSerializable".to_string()]);
    assert!(matches!(
        &e.cases[0].value,
        Some(Expr {
            kind: ExprKind::Literal(_),
            ..
        })
    ));
    assert_eq!(e.methods.len(), 1);
    assert_eq!(e.methods[0].function.name, "label");
}

#[test]
fn readonly_class_parses() {
    let program = parse("<?php readonly class Point { public int $x; }");
    assert!(matches!(&program[0].kind, StmtKind::ClassDecl(_)));
}

#[test]
fn class_interface_and_trait_declarations_are_tagged_with_their_own_kind() {
    let program = parse("<?php class C {} interface I {} trait T {}");
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.kind, ClassKind::Class);
    let StmtKind::ClassDecl(i) = &program[1].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(i.kind, ClassKind::Interface);
    let StmtKind::ClassDecl(t) = &program[2].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(t.kind, ClassKind::Trait);
}

#[test]
fn a_final_method_is_recorded_as_final_and_a_non_final_one_is_not() {
    let program = parse(
        "<?php class C { final public function locked(): void {} public function open(): void {} }",
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let locked = c
        .methods
        .iter()
        .find(|m| m.function.name == "locked")
        .unwrap();
    assert!(locked.is_final);
    let open = c
        .methods
        .iter()
        .find(|m| m.function.name == "open")
        .unwrap();
    assert!(!open.is_final);
}

#[test]
fn a_readonly_property_modifier_is_recorded_per_property() {
    let program = parse("<?php class C { public readonly int $x; public int $y; }");
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let x = c.properties.iter().find(|p| p.name == "x").unwrap();
    assert!(x.is_readonly);
    let y = c.properties.iter().find(|p| p.name == "y").unwrap();
    assert!(!y.is_readonly);
}

#[test]
fn readonly_class_shorthand_marks_every_property_readonly() {
    let program = parse("<?php readonly class C { public int $x; public int $y; }");
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert!(c.properties.iter().all(|p| p.is_readonly));
}

#[test]
fn a_promoted_constructor_parameter_captures_its_own_readonly_modifier() {
    let program =
        parse("<?php class C { public function __construct(public readonly int $id) {} }");
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let ctor = c
        .methods
        .iter()
        .find(|m| m.function.name == "__construct")
        .unwrap();
    assert!(ctor.function.params[0].promoted_readonly);
}

#[test]
fn a_trait_as_alias_clause_is_captured_in_the_ast() {
    let program = parse(
        r#"<?php
            class C {
                use A, B {
                    A::hello insteadof B;
                    B::hello as helloB;
                }
            }
            "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.trait_aliases.len(), 1);
    assert_eq!(c.trait_aliases[0].trait_name.as_deref(), Some("B"));
    assert_eq!(c.trait_aliases[0].method, "hello");
    assert_eq!(c.trait_aliases[0].alias, "helloB");
}

#[test]
fn a_bare_as_alias_clause_with_no_trait_qualifier_is_captured() {
    let program = parse(
        r#"<?php
            class C {
                use X {
                    greet as greetAlias;
                }
            }
            "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.trait_aliases.len(), 1);
    assert_eq!(c.trait_aliases[0].trait_name, None);
    assert_eq!(c.trait_aliases[0].method, "greet");
    assert_eq!(c.trait_aliases[0].alias, "greetAlias");
}

#[test]
fn an_as_clause_that_only_changes_visibility_captures_no_alias() {
    let program = parse(
        r#"<?php
            class C {
                use X {
                    greet as protected;
                }
            }
            "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert!(c.trait_aliases.is_empty());
    assert_eq!(c.resolved_trait_conflicts, vec!["greet".to_string()]);
}

#[test]
fn an_insteadof_clause_records_its_method_as_a_resolved_conflict() {
    let program = parse(
        r#"<?php
            class C {
                use A, B {
                    A::hello insteadof B;
                }
            }
            "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.resolved_trait_conflicts, vec!["hello".to_string()]);
}

#[test]
fn an_insteadof_clause_with_a_fully_qualified_trait_name_is_still_parsed() {
    // A trait name before `::` in a trait-adaptation clause can be
    // fully qualified (`\Some\Trait::method insteadof ...`) - found
    // via bulk-running this project's checker against the vendored
    // phpstan-src corpus (nsrt/traits/traits.php uses exactly this
    // shape and previously failed to parse at all).
    let program = parse(
        r#"<?php
            class C {
                use \NsA\A, \NsB\B {
                    \NsA\A::hello insteadof \NsB\B;
                }
            }
            "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.resolved_trait_conflicts, vec!["hello".to_string()]);
}

#[test]
fn a_named_function_declaration_is_unaffected_by_the_anonymous_statement_lookahead() {
    let program = parse("<?php function foo(int $x): int { return $x; }");
    assert_eq!(program.len(), 1);
    assert!(matches!(program[0].kind, StmtKind::FunctionDecl(_)));
}

#[test]
fn a_named_by_ref_function_declaration_is_unaffected_by_the_anonymous_statement_lookahead() {
    let program = parse("<?php function &bar(): int { static $x = 1; return $x; }");
    assert_eq!(program.len(), 1);
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected a FunctionDecl, got {:?}", program[0].kind);
    };
    assert_eq!(f.name, "bar");
}

#[test]
fn anonymous_class_parses_with_interface_and_constructor_args() {
    let program = parse(
        r#"<?php
            $x = new readonly class(1) implements Countable {
                public function __construct(private int $n) {}
                public function count(): int { return $this->n; }
            };
        "#,
    );
    let StmtKind::Expression(Expr {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected an assignment");
    };
    let ExprKind::NewAnonymousClass(decl, args) = &rhs.kind else {
        panic!("expected NewAnonymousClass");
    };
    assert_eq!(args.len(), 1);
    assert_eq!(decl.implements, vec!["Countable".to_string()]);
    assert_eq!(decl.methods.len(), 2);
}

#[test]
fn asymmetric_visibility_property_parses() {
    let program = parse(
        r#"<?php
            class Foo {
                public private(set) string $name = 'a';
                private(set) int $count = 0;
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.properties.len(), 2);
    assert_eq!(c.properties[0].name, "name");
    assert_eq!(c.properties[0].visibility, Visibility::Public);
    assert_eq!(c.properties[1].name, "count");
    assert_eq!(c.properties[1].visibility, Visibility::Public);
}

#[test]
fn property_hooks_parse() {
    let program = parse(
        r#"<?php
            class Foo {
                public string $upper {
                    get => strtoupper($this->raw);
                    set(string $value) {
                        $this->raw = strtolower($value);
                    }
                }
                public int $bare {
                    get;
                    set;
                }
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.properties.len(), 2);

    let hooks = c.properties[0].hooks.as_ref().expect("expected hooks");
    assert!(matches!(hooks.get, Some(PropertyHookBody::Arrow(_))));
    let (param, body) = hooks.set.as_ref().expect("expected set hook");
    assert_eq!(param.as_ref().unwrap().name, "value");
    assert!(matches!(body, PropertyHookBody::Block(_)));

    let bare_hooks = c.properties[1].hooks.as_ref().expect("expected hooks");
    assert!(bare_hooks.get.is_none());
    assert!(bare_hooks.set.is_none());
}

#[test]
fn dnf_type_hint_parses() {
    let program = parse(
        r#"<?php
            class Test {
                public (A&B)|X $prop;
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.properties[0].type_hint.as_deref(), Some("(A&B)|X"));
}

#[test]
fn by_reference_property_hook_parses() {
    let program = parse(
        r#"<?php
            class Foo {
                public $prop {
                    &get => $this->_prop;
                }
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let hooks = c.properties[0].hooks.as_ref().expect("expected hooks");
    assert!(matches!(hooks.get, Some(PropertyHookBody::Arrow(_))));
}

#[test]
fn public_set_and_final_promoted_params_parse() {
    let program = parse(
        r#"<?php
            class Foo {
                public function __construct(
                    public public(set) string $a,
                    final $b,
                ) {}
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let ctor = &c.methods[0].function;
    assert_eq!(ctor.params[0].promoted_visibility, Some(Visibility::Public));
    assert_eq!(ctor.params[1].name, "b");
}

#[test]
fn legacy_var_property_declaration_parses() {
    let program = parse(
        r#"<?php
            class Dnf {
                var A|(B&C) $a;
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.properties[0].name, "a");
    assert_eq!(c.properties[0].visibility, Visibility::Public);
    assert_eq!(c.properties[0].type_hint.as_deref(), Some("A|(B&C)"));
}

#[test]
fn hooks_on_promoted_constructor_property_parse() {
    let program = parse(
        r#"<?php
            class Test {
                public function __construct(
                    public $prop {
                        get { echo "get\n"; }
                        set { echo "set($value)\n"; }
                    },
                ) {}
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let ctor = &c.methods[0].function;
    let hooks = ctor.params[0].hooks.as_ref().expect("expected hooks");
    assert!(matches!(hooks.get, Some(PropertyHookBody::Block(_))));
    assert!(hooks.set.is_some());
}

#[test]
fn readonly_class_still_parses_as_a_class() {
    let program = parse(r#"<?php readonly class Foo {}"#);
    assert!(matches!(&program[0].kind, StmtKind::ClassDecl(c) if c.name == "Foo"));
}
