use std::collections::{HashMap, HashSet};

use crate::ast::{ClassDecl, EnumDecl, FunctionDecl, Stmt};
use crate::docblock::{self, DocBlock};
use crate::types::{self, Type};

#[derive(Debug, Clone)]
pub struct ParamSig {
    pub name: String,
    pub ty: Type,
    pub variadic: bool,
    pub has_default: bool,
    pub by_ref: bool,
}

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub params: Vec<ParamSig>,
    pub return_type: Type,
}

/// Resolves a param's/property's type: a PHPDoc type (when present) wins
/// over the native hint, since it's usually the more precise of the two
/// (an untyped native param described only in `@param`, or a native `array`
/// refined to `User[]`) - falls back to the native hint, then `Mixed`.
fn resolve_type(doc_type: Option<&str>, native_hint: Option<&str>) -> Type {
    doc_type
        .or(native_hint)
        .map(types::parse_type_string)
        .unwrap_or(Type::Mixed)
}

/// A class constant's value is almost always a literal in real code - this
/// covers the common cases the same way `checker.rs`'s own literal
/// inference does, without pulling in the full expression-inference engine
/// (which lives in `checker.rs` and depends on `Registry` itself, so it
/// can't be called from here). Anything more complex than a bare literal
/// (an array, a concatenation, another constant) falls back to `Mixed` -
/// undefined-constant detection only needs the name to be known, not a
/// precise type.
fn infer_const_type(value: &crate::ast::Expr) -> Type {
    use crate::ast::{Expr, Literal};
    match value {
        Expr::Literal(Literal::Int(n)) => Type::IntLiteral(*n),
        Expr::Literal(Literal::Float(_)) => Type::Float,
        Expr::Literal(Literal::String(s)) => Type::StringLiteral(s.clone()),
        Expr::Literal(Literal::Bool(true)) => Type::True,
        Expr::Literal(Literal::Bool(false)) => Type::False,
        Expr::Literal(Literal::Null) => Type::Null,
        _ => Type::Mixed,
    }
}

/// Rewrites every `Object(name)` in a type tree whose name is one of this
/// function's own declared `@template` parameters into `TemplateParam(name)`.
/// `parse_type_string` has no way to know "T" is a template rather than an
/// ordinary (if oddly-named) class, so that reclassification happens here,
/// where the declaring function's `@template` list is known.
fn mark_template_params(ty: Type, template_names: &HashSet<String>) -> Type {
    match ty {
        Type::Object(name) if template_names.contains(&name) => Type::TemplateParam(name),
        Type::Array(inner) => Type::Array(Box::new(mark_template_params(*inner, template_names))),
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|m| mark_template_params(m, template_names))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .into_iter()
                .map(|m| mark_template_params(m, template_names))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, mark_template_params(t, template_names), opt))
                .collect(),
        ),
        other => other,
    }
}

impl FunctionSig {
    fn from_decl(f: &FunctionDecl) -> FunctionSig {
        let doc: Option<DocBlock> = f.doc_comment.as_deref().map(docblock::parse);
        let template_names: HashSet<String> = doc
            .as_ref()
            .map(|d| d.templates.iter().cloned().collect())
            .unwrap_or_default();

        let params = f
            .params
            .iter()
            .map(|p| {
                let doc_ty = doc.as_ref().and_then(|d| d.params.get(&p.name));
                let ty = resolve_type(doc_ty.map(String::as_str), p.type_hint.as_deref());
                let ty = mark_template_params(ty, &template_names);
                ParamSig {
                    name: p.name.clone(),
                    ty,
                    variadic: p.variadic,
                    has_default: p.default.is_some(),
                    by_ref: p.by_ref,
                }
            })
            .collect();

        let doc_return = doc.as_ref().and_then(|d| d.return_type.as_deref());
        let return_type = resolve_type(doc_return, f.return_type.as_deref());
        let return_type = mark_template_params(return_type, &template_names);

        FunctionSig {
            params,
            return_type,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PropertySig {
    pub ty: Type,
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub methods: HashMap<String, FunctionSig>,
    pub properties: HashMap<String, PropertySig>,
    pub consts: HashMap<String, Type>,
    pub uses: Vec<String>,
}

impl ClassInfo {
    fn from_decl(c: &ClassDecl) -> ClassInfo {
        let methods = c
            .methods
            .iter()
            .map(|m| (m.function.name.clone(), FunctionSig::from_decl(&m.function)))
            .collect();

        let mut properties: HashMap<String, PropertySig> = c
            .properties
            .iter()
            .map(|p| {
                let doc_ty = p.doc_comment.as_deref().map(docblock::parse);
                let ty = resolve_type(
                    doc_ty.as_ref().and_then(|d| d.var_type.as_deref()),
                    p.type_hint.as_deref(),
                );
                (
                    p.name.clone(),
                    PropertySig {
                        ty,
                        is_static: p.is_static,
                    },
                )
            })
            .collect();

        // Constructor property promotion: `public function __construct(
        // private int $id) {}` declares `$id` as a property, not just a
        // parameter - the parser records this via Param::promoted_visibility.
        // A `@param` on the constructor's own docblock describes the same
        // promoted property, so it gets the same PHPDoc-wins treatment.
        if let Some(ctor) = c.methods.iter().find(|m| m.function.name == "__construct") {
            let ctor_doc = ctor.function.doc_comment.as_deref().map(docblock::parse);
            for p in &ctor.function.params {
                if p.promoted_visibility.is_none() {
                    continue;
                }
                let doc_ty = ctor_doc.as_ref().and_then(|d| d.params.get(&p.name));
                let ty = resolve_type(doc_ty.map(String::as_str), p.type_hint.as_deref());
                properties.insert(
                    p.name.clone(),
                    PropertySig {
                        ty,
                        is_static: false,
                    },
                );
            }
        }

        let consts = c
            .consts
            .iter()
            .map(|cc| (cc.name.clone(), infer_const_type(&cc.value)))
            .collect();

        ClassInfo {
            extends: c.extends.clone(),
            implements: c.implements.clone(),
            methods,
            properties,
            consts,
            uses: c.uses.clone(),
        }
    }

    /// An enum participates in the class hierarchy the same way a class
    /// does (methods, `implements`, `use`) - it just can't extend anything
    /// or declare instance properties. Cases aren't registered as anything
    /// yet: resolving `Suit::Hearts`'s type would need static-access type
    /// inference, which nothing in the checker does for *any* class today,
    /// not just enums - a separate, pre-existing gap this doesn't attempt
    /// to close.
    fn from_enum_decl(e: &EnumDecl) -> ClassInfo {
        let methods = e
            .methods
            .iter()
            .map(|m| (m.function.name.clone(), FunctionSig::from_decl(&m.function)))
            .collect();
        let consts = e
            .consts
            .iter()
            .map(|cc| (cc.name.clone(), infer_const_type(&cc.value)))
            .collect();

        ClassInfo {
            extends: Vec::new(),
            implements: e.implements.clone(),
            methods,
            properties: HashMap::new(),
            consts,
            uses: e.uses.clone(),
        }
    }
}

/// A symbol table built by indexing already-parsed source, standing in for
/// the reflection a real PHP runtime would normally provide (see the
/// discussion this project started from: no live interpreter means no
/// `ReflectionClass`, so class/function signatures come from parsing
/// everything up front instead). `merge_stubs` layers in curated built-in
/// signatures (`stubs.rs`) the same way php-src stubs would sit alongside
/// project code in a real implementation.
#[derive(Debug, Default)]
pub struct Registry {
    pub classes: HashMap<String, ClassInfo>,
    pub functions: HashMap<String, FunctionSig>,
}

impl Registry {
    pub fn build(program: &[Stmt]) -> Registry {
        let mut registry = Registry::default();
        registry.index(program);
        registry.flatten_traits();
        registry
    }

    /// A `use TraitName;` can reference a trait declared later in the same
    /// file (PHP resolves this at compile time, not top-to-bottom), so
    /// flattening has to be a second pass over the fully-indexed classes -
    /// a using class's own methods/properties win over a trait's.
    ///
    /// `pub(crate)` (not private) so `Project::update_file` can re-run
    /// just this pass after patching one file's contributions in place,
    /// without needing to go through `build`'s full from-scratch `index`.
    pub(crate) fn flatten_traits(&mut self) {
        type TraitAddition = (
            String,
            Vec<(String, FunctionSig)>,
            Vec<(String, PropertySig)>,
        );

        let class_names: Vec<String> = self.classes.keys().cloned().collect();
        let mut additions: Vec<TraitAddition> = Vec::new();

        for class_name in &class_names {
            let uses = self.classes[class_name].uses.clone();
            if uses.is_empty() {
                continue;
            }
            let mut methods = Vec::new();
            let mut properties = Vec::new();
            for trait_name in &uses {
                if let Some(trait_info) = self.classes.get(trait_name) {
                    methods.extend(
                        trait_info
                            .methods
                            .iter()
                            .map(|(n, s)| (n.clone(), s.clone())),
                    );
                    properties.extend(
                        trait_info
                            .properties
                            .iter()
                            .map(|(n, s)| (n.clone(), s.clone())),
                    );
                }
            }
            additions.push((class_name.clone(), methods, properties));
        }

        for (class_name, methods, properties) in additions {
            if let Some(info) = self.classes.get_mut(&class_name) {
                for (name, sig) in methods {
                    info.methods.entry(name).or_insert(sig);
                }
                for (name, sig) in properties {
                    info.properties.entry(name).or_insert(sig);
                }
            }
        }
    }

    /// `pub(crate)` for the same reason as `flatten_traits` - lets
    /// `Project` index one file's statements into an already-built
    /// registry (initial load, or after a re-parse) without re-running
    /// `build`'s from-scratch pass over every other file.
    pub(crate) fn index(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::ClassDecl(c) => {
                    self.classes.insert(c.name.clone(), ClassInfo::from_decl(c));
                }
                Stmt::EnumDecl(e) => {
                    self.classes
                        .insert(e.name.clone(), ClassInfo::from_enum_decl(e));
                }
                Stmt::FunctionDecl(f) => {
                    self.functions
                        .insert(f.name.clone(), FunctionSig::from_decl(f));
                }
                _ => {}
            }
        }
    }

    /// Surgically removes one file's prior contributions before
    /// re-indexing its new parse - `Project::update_file`'s whole reason
    /// for existing. Names are the only identity a registry entry has
    /// (namespaces aren't modeled anywhere in this project), so two files
    /// declaring the same top-level name would collide here exactly as
    /// they would everywhere else in this registry - a pre-existing,
    /// accepted simplification, not something incrementality introduces.
    pub(crate) fn remove_declarations(&mut self, functions: &[String], classes: &[String]) {
        for name in functions {
            self.functions.remove(name);
        }
        for name in classes {
            self.classes.remove(name);
        }
    }

    /// User-defined symbols always win: a stub only fills in a name that
    /// isn't already declared in the indexed source.
    pub fn merge_stubs<S: Into<String>>(&mut self, stubs: Vec<(S, FunctionSig)>) {
        for (name, sig) in stubs {
            self.functions.entry(name.into()).or_insert(sig);
        }
    }

    /// Same "user-defined wins" rule as `merge_stubs`, for classes -
    /// without this, built-in classes (`Exception`, `ArrayObject`, ...)
    /// would never be known to the registry at all, since stub sources
    /// were previously only mined for function signatures.
    pub fn merge_stub_classes<S: Into<String>>(&mut self, stubs: Vec<(S, ClassInfo)>) {
        for (name, info) in stubs {
            self.classes.entry(name.into()).or_insert(info);
        }
    }

    /// Does `class` derive from or implement `of`, directly or transitively?
    /// Walks `extends` and `implements` together as one graph, which also
    /// covers an interface extending several other interfaces.
    pub fn is_subclass_of(&self, class: &str, of: &str) -> bool {
        if class == of {
            return true;
        }
        let mut seen = HashSet::new();
        let mut stack = vec![class.to_string()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            for parent in info.extends.iter().chain(info.implements.iter()) {
                if parent == of {
                    return true;
                }
                stack.push(parent.clone());
            }
        }
        false
    }

    /// Looks up a method, walking up the (single-parent) extends chain if
    /// not declared directly on `class`. Traits aren't modeled yet.
    pub fn lookup_method(&self, class: &str, method: &str) -> Option<&FunctionSig> {
        let mut current = class;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(current.to_string()) {
                return None;
            }
            let info = self.classes.get(current)?;
            if let Some(sig) = info.methods.get(method) {
                return Some(sig);
            }
            current = info.extends.first()?;
        }
    }

    /// Looks up a property, walking up the extends chain if not declared
    /// directly on `class` (covers inherited properties, including ones
    /// introduced via constructor promotion).
    pub fn lookup_property(&self, class: &str, prop: &str) -> Option<&PropertySig> {
        let mut current = class;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(current.to_string()) {
                return None;
            }
            let info = self.classes.get(current)?;
            if let Some(sig) = info.properties.get(prop) {
                return Some(sig);
            }
            current = info.extends.first()?;
        }
    }

    /// Looks up a class constant, walking up the extends chain if not
    /// declared directly on `class` - mirrors `lookup_property`.
    pub fn lookup_const(&self, class: &str, const_name: &str) -> Option<&Type> {
        let mut current = class;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(current.to_string()) {
                return None;
            }
            let info = self.classes.get(current)?;
            if let Some(ty) = info.consts.get(const_name) {
                return Some(ty);
            }
            current = info.extends.first()?;
        }
    }

    pub fn lookup_function(&self, name: &str) -> Option<&FunctionSig> {
        self.functions.get(name)
    }

    /// Type-compatibility check that consults the class hierarchy for
    /// `Object` types; every other case defers to `types::is_subtype`,
    /// which has no notion of class context.
    pub fn is_assignable(&self, sub: &Type, sup: &Type) -> bool {
        match (sub, sup) {
            (Type::Object(a), Type::Object(b)) => a == b || self.is_subclass_of(a, b),
            (Type::Union(members), _) => members.iter().all(|m| self.is_assignable(m, sup)),
            (_, Type::Union(members)) => members.iter().any(|m| self.is_assignable(sub, m)),
            (Type::Intersection(members), _) => members.iter().any(|m| self.is_assignable(m, sup)),
            (_, Type::Intersection(members)) => members.iter().all(|m| self.is_assignable(sub, m)),
            // Deliberately more lenient here than types::is_subtype's pure
            // structural relation (which correctly treats Array(Mixed) as
            // NOT a subtype of Array(Int)): an array literal's element type
            // is never inferred precisely by this checker, so it always
            // shows up as Array(Mixed) meaning "unknown", not "really can
            // hold anything" - without this, e.g. a PHPDoc `int[]` param
            // would false-positive against every `[1, 2, 3]` literal.
            (Type::Array(a), Type::Array(b)) => {
                matches!(**a, Type::Mixed) || self.is_assignable(a, b)
            }
            // Same leniency, same reason, for an array of unknown structure
            // (an array literal, most commonly) checked against a specific
            // expected shape: e.g. `return ["count" => ..., "first" => ...]`
            // against `@return array{count: int, first: string}` shouldn't
            // false-positive just because literal keys/values aren't
            // inferred. A genuinely known, homogeneous Array(V) still
            // correctly fails to satisfy an arbitrary shape.
            (Type::Array(a), Type::ArrayShape(_)) => matches!(**a, Type::Mixed),
            _ => types::is_subtype(sub, sup),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn build_registry(src: &str) -> Registry {
        let tokens = Lexer::new(src).tokenize();
        let program = Parser::new(tokens).parse_program();
        Registry::build(&program)
    }

    #[test]
    fn class_consts_are_registered_and_typed_from_their_literal_value() {
        let r = build_registry(
            r#"<?php
            class Config {
                const NAME = "app";
                const MAX = 10;
            }"#,
        );
        assert_eq!(
            r.lookup_const("Config", "NAME"),
            Some(&Type::StringLiteral("app".to_string()))
        );
        assert_eq!(r.lookup_const("Config", "MAX"), Some(&Type::IntLiteral(10)));
        assert_eq!(r.lookup_const("Config", "MISSING"), None);
    }

    #[test]
    fn class_consts_are_inherited_through_extends() {
        let r = build_registry(
            r#"<?php
            class Base { const KIND = "base"; }
            class Derived extends Base {}"#,
        );
        assert_eq!(
            r.lookup_const("Derived", "KIND"),
            Some(&Type::StringLiteral("base".to_string()))
        );
    }

    #[test]
    fn enum_consts_are_registered_too() {
        let r = build_registry(
            r#"<?php
            enum Suit {
                case Hearts;
                const KIND = "hearts";
            }"#,
        );
        assert_eq!(
            r.lookup_const("Suit", "KIND"),
            Some(&Type::StringLiteral("hearts".to_string()))
        );
    }

    #[test]
    fn transitive_class_inheritance() {
        let r = build_registry(
            r#"<?php
            interface Shape {}
            abstract class BaseShape implements Shape {}
            class Circle extends BaseShape {}
        "#,
        );
        assert!(r.is_subclass_of("Circle", "BaseShape"));
        assert!(r.is_subclass_of("Circle", "Shape"));
        assert!(!r.is_subclass_of("BaseShape", "Circle"));
    }

    #[test]
    fn interface_extending_multiple_interfaces() {
        let r = build_registry(
            r#"<?php
            interface A {}
            interface B {}
            interface C extends A, B {}
            class Impl implements C {}
        "#,
        );
        assert!(r.is_subclass_of("Impl", "A"));
        assert!(r.is_subclass_of("Impl", "B"));
    }

    #[test]
    fn method_lookup_walks_up_extends_chain() {
        let r = build_registry(
            r#"<?php
            class Base { public function greet(): string { return "hi"; } }
            class Sub extends Base {}
        "#,
        );
        let sig = r.lookup_method("Sub", "greet").expect("inherited method");
        assert_eq!(sig.return_type.to_string(), "string");
    }

    #[test]
    fn merge_stubs_does_not_override_user_functions() {
        let mut r = build_registry(
            r#"<?php
            function strlen(string $s): int { return 0; }
        "#,
        );
        r.merge_stubs(vec![(
            "strlen",
            FunctionSig {
                params: vec![],
                return_type: Type::Bool,
            },
        )]);
        assert_eq!(r.functions["strlen"].return_type, Type::Int);
    }

    #[test]
    fn is_assignable_uses_class_hierarchy() {
        let r = build_registry(
            r#"<?php
            interface Shape {}
            class Circle implements Shape {}
        "#,
        );
        assert!(r.is_assignable(
            &Type::Object("Circle".to_string()),
            &Type::Object("Shape".to_string())
        ));
        assert!(!r.is_assignable(
            &Type::Object("Shape".to_string()),
            &Type::Object("Circle".to_string())
        ));
    }

    #[test]
    fn is_assignable_treats_unknown_array_element_type_as_compatible() {
        let r = Registry::default();
        let unknown_elements = Type::Array(Box::new(Type::Mixed));
        let ints = Type::Array(Box::new(Type::Int));
        // The lenient direction that types::is_subtype deliberately does
        // NOT take (see its own array-covariance test) - is_assignable is
        // the practical, checker-facing layer where "we don't know" must
        // not read as "definitely incompatible".
        assert!(r.is_assignable(&unknown_elements, &ints));
        // A genuinely known, different element type is still caught.
        let strings = Type::Array(Box::new(Type::String));
        assert!(!r.is_assignable(&strings, &ints));
    }

    #[test]
    fn is_assignable_treats_unknown_array_as_compatible_with_a_shape() {
        let r = Registry::default();
        let unknown = Type::Array(Box::new(Type::Mixed));
        let shape = Type::ArrayShape(vec![("name".to_string(), Type::String, false)]);
        assert!(r.is_assignable(&unknown, &shape));
        // A genuinely known, homogeneous array still doesn't satisfy an
        // arbitrary shape.
        let ints = Type::Array(Box::new(Type::Int));
        assert!(!r.is_assignable(&ints, &shape));
    }

    #[test]
    fn declared_property_type_is_tracked() {
        let r = build_registry(
            r#"<?php
            class Box { public int $value = 0; }
        "#,
        );
        assert_eq!(r.lookup_property("Box", "value").unwrap().ty, Type::Int);
    }

    #[test]
    fn constructor_promotion_registers_a_property() {
        let r = build_registry(
            r#"<?php
            class Point {
                public function __construct(private int $x, public string $label) {}
            }
        "#,
        );
        assert_eq!(r.lookup_property("Point", "x").unwrap().ty, Type::Int);
        assert_eq!(
            r.lookup_property("Point", "label").unwrap().ty,
            Type::String
        );
    }

    #[test]
    fn trait_methods_and_properties_are_flattened_in() {
        let r = build_registry(
            r#"<?php
            trait Greets {
                public string $greeting = "hi";
                public function greet(): string { return "hi"; }
            }
            class Person {
                use Greets;
            }
        "#,
        );
        assert_eq!(
            r.lookup_method("Person", "greet").unwrap().return_type,
            Type::String
        );
        assert_eq!(
            r.lookup_property("Person", "greeting").unwrap().ty,
            Type::String
        );
    }

    #[test]
    fn trait_used_before_its_declaration_still_resolves() {
        // PHP resolves `use` at compile time, not top-to-bottom - a trait
        // declared later in the file must still flatten in correctly.
        let r = build_registry(
            r#"<?php
            class Person {
                use Greets;
            }
            trait Greets {
                public function greet(): string { return "hi"; }
            }
        "#,
        );
        assert!(r.lookup_method("Person", "greet").is_some());
    }

    #[test]
    fn class_method_overrides_trait_method_of_same_name() {
        let r = build_registry(
            r#"<?php
            trait Greets {
                public function greet(): string { return "generic"; }
            }
            class Person {
                use Greets;
                public function greet(): int { return 1; }
            }
        "#,
        );
        assert_eq!(
            r.lookup_method("Person", "greet").unwrap().return_type,
            Type::Int
        );
    }

    #[test]
    fn property_lookup_walks_up_extends_chain() {
        let r = build_registry(
            r#"<?php
            class Base { public string $name = ""; }
            class Sub extends Base {}
        "#,
        );
        assert_eq!(r.lookup_property("Sub", "name").unwrap().ty, Type::String);
    }

    #[test]
    fn phpdoc_types_an_otherwise_untyped_function() {
        let r = build_registry(
            r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double($x) { return $x * 2; }
        "#,
        );
        let sig = &r.functions["double"];
        assert_eq!(sig.params[0].ty, Type::Int);
        assert_eq!(sig.return_type, Type::Int);
    }

    #[test]
    fn phpdoc_wins_over_a_conflicting_native_hint() {
        let r = build_registry(
            r#"<?php
            /**
             * @param int[] $items
             */
            function f(array $items) {}
        "#,
        );
        assert_eq!(
            r.functions["f"].params[0].ty,
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn phpdoc_types_an_otherwise_untyped_property() {
        let r = build_registry(
            r#"<?php
            class Box {
                /** @var int */
                public $value;
            }
        "#,
        );
        assert_eq!(r.lookup_property("Box", "value").unwrap().ty, Type::Int);
    }

    #[test]
    fn phpdoc_types_a_promoted_constructor_property_via_param_tag() {
        let r = build_registry(
            r#"<?php
            class Point {
                /**
                 * @param int[] $coords
                 */
                public function __construct(public $coords) {}
            }
        "#,
        );
        assert_eq!(
            r.lookup_property("Point", "coords").unwrap().ty,
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn missing_doc_comment_falls_back_to_native_hint() {
        let r = build_registry(r#"<?php function f(string $s): string { return $s; }"#);
        assert_eq!(r.functions["f"].params[0].ty, Type::String);
        assert_eq!(r.functions["f"].return_type, Type::String);
    }

    #[test]
    fn enum_is_registered_with_its_methods_and_implements() {
        let r = build_registry(
            r#"<?php
            interface HasLabel {}
            enum Suit: string implements HasLabel {
                case Hearts = 'H';
                public function label(): string { return "Hearts"; }
            }
        "#,
        );
        assert!(r.is_subclass_of("Suit", "HasLabel"));
        assert_eq!(
            r.lookup_method("Suit", "label").unwrap().return_type,
            Type::String
        );
    }
}
