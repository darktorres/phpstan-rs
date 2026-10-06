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
    assert_eq!(
        f.return_type.as_ref().map(TypeHint::to_string).as_deref(),
        Some("string|false")
    );
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
    let StmtKind::ClassDecl(e) = &program[0].kind else {
        panic!("expected an enum");
    };
    assert_eq!(e.def.kind, ClassKind::Enum);
    assert_eq!(Some(&*e.name), Some("SortDirection"));
    assert!(e.def.backing_type.is_none());
    assert_eq!(e.def.enum_cases.len(), 2);
    assert_eq!(e.def.enum_cases[0].name, "Ascending");
    assert!(e.def.enum_cases[0].value.is_none());
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
    let StmtKind::ClassDecl(e) = &program[0].kind else {
        panic!("expected an enum");
    };
    assert_eq!(e.def.kind, ClassKind::Enum);
    assert_eq!(
        e.def
            .backing_type
            .as_ref()
            .map(TypeHint::to_string)
            .as_deref(),
        Some("string")
    );
    assert_eq!(
        e.def
            .implements
            .iter()
            .map(|n| &*n.written)
            .collect::<Vec<_>>(),
        vec!["\\JsonSerializable"]
    );
    assert!(matches!(
        &e.def.enum_cases[0].value,
        Some(ExprNode {
            kind: ExprKind::Literal(_),
            ..
        })
    ));
    assert_eq!(e.def.methods.len(), 1);
    assert_eq!(e.def.methods[0].function.name, "label");
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
    assert_eq!(c.def.kind, ClassKind::Class);
    let StmtKind::ClassDecl(i) = &program[1].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(i.def.kind, ClassKind::Interface);
    let StmtKind::ClassDecl(t) = &program[2].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(t.def.kind, ClassKind::Trait);
}

#[test]
fn a_class_named_the_same_as_a_keyword_keeps_its_own_declared_case() {
    // Real vendored `marc-mabe/php-enum` declares a class literally
    // named `Enum` (case-sensitive match against the `enum` keyword's
    // own canonical spelling) - `take_name_segment`'s own
    // keyword-recovery path must not corrupt it to `enum`.
    let program = parse("<?php class Enum { public static function foo(): void {} }");
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(Some(&*c.name), Some("Enum"));
}

#[test]
fn a_qualified_name_segment_matching_a_keyword_keeps_its_own_case() {
    let program = parse("<?php namespace MabeEnum; class Foo extends \\MabeEnum\\Enum {}");
    let StmtKind::ClassDecl(c) = &program[1].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.def.extends.len(), 1);
    assert!(
        c.def.extends[0].ends_with("MabeEnum\\Enum"),
        "expected the qualified name's own Enum segment to keep its case, got: {:?}",
        c.def.extends[0]
    );
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
        .def
        .methods
        .iter()
        .find(|m| m.function.name == "locked")
        .unwrap();
    assert!(locked.is_final);
    let open = c
        .def
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
    let x = c.def.properties.iter().find(|p| p.name == "x").unwrap();
    assert!(x.is_readonly);
    let y = c.def.properties.iter().find(|p| p.name == "y").unwrap();
    assert!(!y.is_readonly);
}

#[test]
fn readonly_class_shorthand_marks_every_property_readonly() {
    let program = parse("<?php readonly class C { public int $x; public int $y; }");
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert!(c.def.properties.iter().all(|p| p.is_readonly));
}

#[test]
fn a_promoted_constructor_parameter_captures_its_own_readonly_modifier() {
    let program =
        parse("<?php class C { public function __construct(public readonly int $id) {} }");
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let ctor = c
        .def
        .methods
        .iter()
        .find(|m| m.function.name == "__construct")
        .unwrap();
    assert!(
        ctor.function.params[0]
            .promotion
            .is_some_and(|promotion| promotion.readonly)
    );
}

#[test]
fn each_trait_use_statement_keeps_its_own_traits_doc_comment_and_adaptations() {
    let program = parse(
        r#"<?php
            class C {
                /** @use A<int> */
                use A, B {
                    A::hello insteadof B;
                }
                use D;
            }
            "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.def.trait_uses.len(), 2);
    fn names(u: &TraitUse) -> Vec<&str> {
        u.traits.iter().map(|t| &*t.written).collect()
    }
    assert_eq!(names(&c.def.trait_uses[0]), ["A", "B"]);
    assert_eq!(
        c.def.trait_uses[0].doc_comment.as_deref(),
        Some("/** @use A<int> */")
    );
    assert_eq!(c.def.trait_uses[0].adaptations.precedences.len(), 1);
    assert_eq!(names(&c.def.trait_uses[1]), ["D"]);
    assert!(c.def.trait_uses[1].doc_comment.is_none());
    assert!(c.def.trait_uses[1].adaptations.precedences.is_empty());
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
    assert_eq!(c.def.trait_uses[0].adaptations.aliases.len(), 1);
    let trait_name = c.def.trait_uses[0].adaptations.aliases[0]
        .trait_name
        .as_ref()
        .unwrap();
    assert_eq!(&*trait_name.written, "B");
    assert_eq!(c.def.trait_uses[0].adaptations.aliases[0].method, "hello");
    assert_eq!(
        c.def.trait_uses[0].adaptations.aliases[0].alias,
        Some("helloB".into())
    );
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
    assert_eq!(c.def.trait_uses[0].adaptations.aliases.len(), 1);
    assert!(
        c.def.trait_uses[0].adaptations.aliases[0]
            .trait_name
            .is_none()
    );
    assert_eq!(c.def.trait_uses[0].adaptations.aliases[0].method, "greet");
    assert_eq!(
        c.def.trait_uses[0].adaptations.aliases[0].alias,
        Some("greetAlias".into())
    );
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
    assert_eq!(c.def.trait_uses[0].adaptations.aliases.len(), 1);
    assert!(matches!(
        c.def.trait_uses[0].adaptations.aliases[0].visibility,
        Some(Visibility::Protected)
    ));
    assert!(c.def.trait_uses[0].adaptations.aliases[0].alias.is_none());
    assert_eq!(
        c.def
            .resolved_trait_conflicts()
            .map(|n| n.as_str())
            .collect::<Vec<_>>(),
        ["greet"]
    );
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
    assert_eq!(c.def.trait_uses[0].adaptations.precedences.len(), 1);
    assert_eq!(
        &*c.def.trait_uses[0].adaptations.precedences[0]
            .trait_name
            .written,
        "A"
    );
    assert_eq!(
        c.def.trait_uses[0].adaptations.precedences[0].method,
        "hello"
    );
    let instead_of: Vec<&str> = c.def.trait_uses[0].adaptations.precedences[0]
        .instead_of
        .iter()
        .map(|q| &*q.written)
        .collect();
    assert_eq!(instead_of, ["B"]);
    assert_eq!(
        c.def
            .resolved_trait_conflicts()
            .map(|n| n.as_str())
            .collect::<Vec<_>>(),
        ["hello"]
    );
}

#[test]
fn an_insteadof_clause_with_a_fully_qualified_trait_name_is_still_parsed() {
    // A trait name before `::` in a trait-adaptation clause can be
    // fully qualified (`\Some\Trait::method insteadof ...`) - real
    // shape: vendored phpstan-src's own nsrt/traits/traits.php.
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
    assert_eq!(
        c.def
            .resolved_trait_conflicts()
            .map(|n| n.as_str())
            .collect::<Vec<_>>(),
        ["hello"]
    );
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
    let StmtKind::Expression(ExprNode {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected an assignment");
    };
    let ExprKind::NewAnonymousClass(class) = &rhs.kind else {
        panic!("expected NewAnonymousClass");
    };
    let decl = &class.def;
    assert_eq!(class.args.len(), 1);
    assert_eq!(
        decl.implements
            .iter()
            .map(|n| &*n.written)
            .collect::<Vec<_>>(),
        vec!["Countable"]
    );
    assert_eq!(decl.methods.len(), 2);
}

#[test]
fn anonymous_class_with_a_leading_attribute_group_parses() {
    // Real PHP allows `#[Attr]` between `new` and `class` for an
    // anonymous class (the PHP manual's own
    // `new #[ExampleAttribute] class () {}` example; real shape:
    // Piwigo's tests/Unit/Template/RendererTest.php).
    let program = parse(
        r#"<?php
            $x = new #[TemplateAttr('a.latte')] class implements Countable {
                public function count(): int { return 0; }
            };
        "#,
    );
    let StmtKind::Expression(ExprNode {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected an assignment");
    };
    let ExprKind::NewAnonymousClass(class) = &rhs.kind else {
        panic!("expected NewAnonymousClass");
    };
    let decl = &class.def;
    assert_eq!(decl.attrs.len(), 1);
    assert_eq!(decl.attrs[0].name, "TemplateAttr");
    assert_eq!(
        decl.implements
            .iter()
            .map(|n| &*n.written)
            .collect::<Vec<_>>(),
        vec!["Countable"]
    );
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
    assert_eq!(c.def.properties.len(), 2);
    assert_eq!(c.def.properties[0].name, "name");
    assert_eq!(c.def.properties[0].visibility, Visibility::Public);
    assert_eq!(c.def.properties[1].name, "count");
    assert_eq!(c.def.properties[1].visibility, Visibility::Public);
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
    assert_eq!(c.def.properties.len(), 2);

    let hooks = c.def.properties[0].hooks.as_ref().expect("expected hooks");
    assert!(matches!(
        hooks.get.as_ref().map(|hook| &hook.body),
        Some(PropertyHookBody::Arrow(_))
    ));
    let set_hook = hooks.set.as_ref().expect("expected set hook");
    let (param, body) = (&set_hook.param, &set_hook.body);
    assert_eq!(param.as_ref().unwrap().name, "value");
    assert!(matches!(body, PropertyHookBody::Block(_)));

    // A bodyless hook (`get;`/`set;`) parses to
    // `Some(PropertyHookBody::Abstract)`/`Some((None, PropertyHookBody::
    // Abstract))`, not `None` - distinguishing "an abstract hook was
    // declared with no body" from "no hook at all", which `None` alone
    // couldn't tell apart.
    let bare_hooks = c.def.properties[1].hooks.as_ref().expect("expected hooks");
    assert!(matches!(
        bare_hooks.get.as_ref().map(|hook| &hook.body),
        Some(PropertyHookBody::Abstract)
    ));
    let set_hook = bare_hooks.set.as_ref().expect("expected set hook");
    let (set_param, set_body) = (&set_hook.param, &set_hook.body);
    assert!(set_param.is_none());
    assert!(matches!(set_body, PropertyHookBody::Abstract));
}

#[test]
fn an_attribute_on_a_property_hook_parses() {
    let program = parse(
        r#"<?php
            class C {
                public int $x {
                    #[Foo]
                    get => 1;
                    #[Bar]
                    set(int $v) { $this->x = $v; }
                }
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let hooks = c.def.properties[0].hooks.as_ref().expect("expected hooks");
    assert_eq!(hooks.get.as_ref().unwrap().attrs.len(), 1);
    assert_eq!(hooks.get.as_ref().unwrap().attrs[0].name, "Foo");
    assert_eq!(hooks.set.as_ref().unwrap().attrs.len(), 1);
    assert_eq!(hooks.set.as_ref().unwrap().attrs[0].name, "Bar");
}

#[test]
fn property_hook_declared_abstract_with_no_body_is_distinct_from_no_hook_at_all() {
    let program = parse(
        r#"<?php
            abstract class C {
                abstract public int $x { get; }
                public int $y = 1;
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let x_hooks = c.def.properties[0]
        .hooks
        .as_ref()
        .expect("expected hooks on $x");
    assert!(matches!(
        x_hooks.get.as_ref().map(|hook| &hook.body),
        Some(PropertyHookBody::Abstract)
    ));
    assert!(x_hooks.set.is_none());
    assert!(c.def.properties[1].hooks.is_none());
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
    assert_eq!(
        c.def.properties[0]
            .type_hint
            .as_ref()
            .map(TypeHint::to_string)
            .as_deref(),
        Some("(A&B)|X")
    );
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
    let hooks = c.def.properties[0].hooks.as_ref().expect("expected hooks");
    assert!(matches!(
        hooks.get.as_ref().map(|hook| &hook.body),
        Some(PropertyHookBody::Arrow(_))
    ));
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
    let ctor = &c.def.methods[0].function;
    assert_eq!(
        ctor.params[0]
            .promotion
            .map(|promotion| promotion.visibility),
        Some(Visibility::Public)
    );
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
    assert_eq!(c.def.properties[0].name, "a");
    assert_eq!(c.def.properties[0].visibility, Visibility::Public);
    assert_eq!(
        c.def.properties[0]
            .type_hint
            .as_ref()
            .map(TypeHint::to_string)
            .as_deref(),
        Some("A|(B&C)")
    );
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
    let ctor = &c.def.methods[0].function;
    let hooks = ctor.params[0].hooks.as_ref().expect("expected hooks");
    assert!(matches!(
        hooks.get.as_ref().map(|hook| &hook.body),
        Some(PropertyHookBody::Block(_))
    ));
    assert!(hooks.set.is_some());
}

#[test]
fn readonly_class_still_parses_as_a_class() {
    let program = parse(r#"<?php readonly class Foo {}"#);
    assert!(matches!(&program[0].kind, StmtKind::ClassDecl(c) if Some(&*c.name) == Some("Foo")));
}

// These exercise the real `Attribute { name, args }` node parsed onto
// every declaration site that can carry one.

#[test]
fn class_attribute_with_a_named_argument_parses() {
    let program = parse(r#"<?php #[Entity(table: 'users')] class User {}"#);
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.def.attrs.len(), 1);
    assert_eq!(c.def.attrs[0].name, "Entity");
    assert_eq!(c.def.attrs[0].args.len(), 1);
    assert_eq!(c.def.attrs[0].args[0].name.as_deref(), Some("table"));
    assert!(matches!(
        &c.def.attrs[0].args[0].value.kind,
        ExprKind::Literal(Literal::String(s)) if *s == "users"
    ));
}

#[test]
fn stacked_attribute_groups_on_a_function_combine_in_source_order() {
    // Real PHP allows several `#[...]` groups back-to-back on one
    // declaration - equivalent to one group with all the same names,
    // comma-separated.
    let program = parse("<?php #[Pure] #[Deprecated('use bar() instead')] function f() {}");
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    assert_eq!(f.attrs.len(), 2);
    assert_eq!(f.attrs[0].name, "Pure");
    assert_eq!(f.attrs[1].name, "Deprecated");
}

#[test]
fn a_comma_separated_attribute_group_produces_one_attribute_per_name() {
    let program = parse("<?php #[Foo, Bar(1, 2)] function f() {}");
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected FunctionDecl");
    };
    assert_eq!(f.attrs.len(), 2);
    assert_eq!(f.attrs[0].name, "Foo");
    assert!(f.attrs[0].args.is_empty());
    assert_eq!(f.attrs[1].name, "Bar");
    assert_eq!(f.attrs[1].args.len(), 2);
}

#[test]
fn method_property_const_and_param_attributes_all_parse() {
    let program = parse(
        r#"<?php
            class Widget {
                #[Column]
                private int $id;

                #[Since('1.0')]
                const VERSION = 1;

                #[Route('/widgets')]
                public function index(#[SensitiveParameter] string $secret): void {}
            }
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.def.properties[0].attrs.len(), 1);
    assert_eq!(c.def.properties[0].attrs[0].name, "Column");
    assert_eq!(c.def.consts[0].attrs.len(), 1);
    assert_eq!(c.def.consts[0].attrs[0].name, "Since");
    let method = &c.def.methods[0];
    assert_eq!(method.function.attrs.len(), 1);
    assert_eq!(method.function.attrs[0].name, "Route");
    assert_eq!(method.function.params[0].attrs.len(), 1);
    assert_eq!(
        method.function.params[0].attrs[0].name,
        "SensitiveParameter"
    );
}

#[test]
fn enum_case_attribute_parses() {
    let program = parse(
        r#"<?php
            enum Suit: string {
                #[Label('Hearts')]
                case Hearts = 'H';
            }
        "#,
    );
    let StmtKind::ClassDecl(e) = &program[0].kind else {
        panic!("expected an enum");
    };
    assert_eq!(e.def.kind, ClassKind::Enum);
    assert_eq!(e.def.enum_cases[0].attrs.len(), 1);
    assert_eq!(e.def.enum_cases[0].attrs[0].name, "Label");
}

#[test]
fn a_doc_comment_before_an_attribute_group_still_attaches_to_the_declaration() {
    // Regression guard: `self.doc_comments` is keyed by token position, so
    // a real `#[...]` group now sitting between a doc comment and the
    // declaration it precedes must not make the doc comment invisible to
    // `take_doc_comment()`.
    let program = parse(
        r#"<?php
            /** Handles a single widget. */
            #[Entity]
            class Widget {}
        "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert_eq!(c.def.attrs.len(), 1);
    assert_eq!(
        c.def.doc_comment.as_deref(),
        Some("/** Handles a single widget. */")
    );
}

#[test]
fn a_declaration_with_no_attributes_has_an_empty_attrs_list() {
    // False-positive guard: ordinary declarations with nothing preceding
    // them must not somehow pick up a stray attribute.
    let program = parse("<?php class Plain { public int $x; function f() {} const C = 1; }");
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert!(c.def.attrs.is_empty());
    assert!(c.def.properties[0].attrs.is_empty());
    assert!(c.def.methods[0].function.attrs.is_empty());
    assert!(c.def.consts[0].attrs.is_empty());
}

#[test]
fn a_promoted_parameter_keeps_its_set_visibility() {
    let program = parse(
        r#"<?php
            class C {
                public function __construct(public private(set) string $a, protected(set) int $b) {}
            }
            "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    let params = &c.def.methods[0].function.params;
    assert!(matches!(
        params[0].promotion.map(|promotion| promotion.visibility),
        Some(Visibility::Public)
    ));
    assert!(matches!(
        params[0]
            .promotion
            .and_then(|promotion| promotion.set_visibility),
        Some(Visibility::Private)
    ));
    assert!(matches!(
        params[1].promotion.map(|promotion| promotion.visibility),
        Some(Visibility::Public)
    ));
    assert!(matches!(
        params[1]
            .promotion
            .and_then(|promotion| promotion.set_visibility),
        Some(Visibility::Protected)
    ));
}

#[test]
fn a_by_reference_get_hook_is_recorded() {
    let program = parse(
        r#"<?php
            class C {
                public array $items { &get => $this->items; }
            }
            "#,
    );
    let StmtKind::ClassDecl(c) = &program[0].kind else {
        panic!("expected ClassDecl");
    };
    assert!(
        c.def.properties[0]
            .hooks
            .as_ref()
            .unwrap()
            .get
            .as_ref()
            .unwrap()
            .by_ref
    );
}
