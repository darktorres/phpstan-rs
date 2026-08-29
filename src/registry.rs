use std::collections::{HashMap, HashSet};

use crate::ast::{ClassDecl, FunctionDecl, Stmt};
use crate::types::{self, Type};

#[derive(Debug, Clone)]
pub struct ParamSig {
    pub name: String,
    pub ty: Type,
    pub variadic: bool,
    pub has_default: bool,
}

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub params: Vec<ParamSig>,
    pub return_type: Type,
}

impl FunctionSig {
    fn from_decl(f: &FunctionDecl) -> FunctionSig {
        let params = f
            .params
            .iter()
            .map(|p| {
                let ty = p
                    .type_hint
                    .as_deref()
                    .map(types::parse_type_string)
                    .unwrap_or(Type::Mixed);
                ParamSig {
                    name: p.name.clone(),
                    ty,
                    variadic: p.variadic,
                    has_default: p.default.is_some(),
                }
            })
            .collect();
        let return_type = f
            .return_type
            .as_deref()
            .map(types::parse_type_string)
            .unwrap_or(Type::Mixed);
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
                let ty = p
                    .type_hint
                    .as_deref()
                    .map(types::parse_type_string)
                    .unwrap_or(Type::Mixed);
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
        if let Some(ctor) = c.methods.iter().find(|m| m.function.name == "__construct") {
            for p in &ctor.function.params {
                if p.promoted_visibility.is_none() {
                    continue;
                }
                let ty = p
                    .type_hint
                    .as_deref()
                    .map(types::parse_type_string)
                    .unwrap_or(Type::Mixed);
                properties.insert(
                    p.name.clone(),
                    PropertySig {
                        ty,
                        is_static: false,
                    },
                );
            }
        }

        ClassInfo {
            extends: c.extends.clone(),
            implements: c.implements.clone(),
            methods,
            properties,
            uses: c.uses.clone(),
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
    fn flatten_traits(&mut self) {
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

    fn index(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::ClassDecl(c) => {
                    self.classes.insert(c.name.clone(), ClassInfo::from_decl(c));
                }
                Stmt::FunctionDecl(f) => {
                    self.functions
                        .insert(f.name.clone(), FunctionSig::from_decl(f));
                }
                _ => {}
            }
        }
    }

    /// User-defined symbols always win: a stub only fills in a name that
    /// isn't already declared in the indexed source.
    pub fn merge_stubs(&mut self, stubs: Vec<(&str, FunctionSig)>) {
        for (name, sig) in stubs {
            self.functions.entry(name.to_string()).or_insert(sig);
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
            (Type::Array(a), Type::Array(b)) => self.is_assignable(a, b),
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
}
