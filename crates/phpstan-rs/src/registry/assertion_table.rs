//! Applies `assertion_tables`' `[[assertions]]` (built-in entries plus
//! configured ones) to the matching declarations as if they carried the
//! `@phpstan-assert` tags, and serves `[[class_argument_returns]]`.

use std::sync::LazyLock;
use syntax::name::Name;

use crate::assertion_tables::{
    AssertWhen, AssertionEntry, ClassArgumentReturn, default_assertions,
    default_class_argument_returns,
};
use crate::registry::Registry;
use crate::registry::function_sig::{AssertInfo, AssertTarget};
use phpdoc::grammar::{self, TypeNode};

static DEFAULT_ASSERTIONS: LazyLock<Vec<AssertionEntry>> = LazyLock::new(default_assertions);
static DEFAULT_RETURNS: LazyLock<Vec<ClassArgumentReturn>> =
    LazyLock::new(default_class_argument_returns);

impl Registry {
    /// Adds configured `[[assertions]]`/`[[class_argument_returns]]`
    /// entries to the built-in ones.
    pub fn set_assertion_tables(
        &mut self,
        assertions: Vec<AssertionEntry>,
        returns: Vec<ClassArgumentReturn>,
    ) {
        let targets: Vec<(Option<String>, Option<String>)> = self
            .configured_assertions
            .iter()
            .chain(&assertions)
            .map(|entry| (entry.function.clone(), entry.class.clone()))
            .collect();
        for (function, class) in targets {
            if let Some(function) = function {
                self.record_changed_function(function.trim_start_matches('\\'));
            }
            if let Some(class) = class {
                self.record_changed_class(class.trim_start_matches('\\'));
            }
        }
        self.configured_assertions = assertions;
        self.configured_class_argument_returns = returns;
    }

    /// Every class-argument-return entry, built-in first.
    pub(crate) fn class_argument_returns(&self) -> impl Iterator<Item = &ClassArgumentReturn> {
        DEFAULT_RETURNS
            .iter()
            .chain(self.configured_class_argument_returns.iter())
    }

    /// Adds each assertion entry's `AssertInfo` to its declaration, once.
    pub(crate) fn apply_assertion_table(&mut self) {
        let entries: Vec<AssertionEntry> = DEFAULT_ASSERTIONS
            .iter()
            .chain(self.configured_assertions.iter())
            .cloned()
            .collect();
        for entry in entries {
            match (&entry.function, &entry.class, &entry.method) {
                (Some(function), _, _) => {
                    let function = function.trim_start_matches('\\');
                    if !self.scope.has_function(function) {
                        continue;
                    }
                    let Some((id, _, declaration)) = self.function(function) else {
                        continue;
                    };
                    let mut result = declaration.clone();
                    if add_assertion(&mut result, &entry) {
                        self.function_results.insert(id, result);
                    }
                }
                (None, Some(class), Some(method)) => {
                    let Some((id, canonical, _)) = self.class(class.trim_start_matches('\\'))
                    else {
                        continue;
                    };
                    if !self.scope.has_class(canonical) {
                        continue;
                    }
                    let Some(slot) = self.member_slot(canonical, method) else {
                        continue;
                    };
                    let method = crate::registry::MethodRef::new(id, slot);
                    let Some(declaration) = self.method_by_ref(method) else {
                        continue;
                    };
                    let mut result = declaration.clone();
                    if add_assertion(&mut result, &entry) {
                        self.set_method_result(method, result);
                    }
                }
                _ => {}
            }
        }
    }
}

fn add_assertion(sig: &mut crate::registry::FunctionReflection, entry: &AssertionEntry) -> bool {
    let parameter = entry.parameter.trim_start_matches('$');
    if !sig.params.iter().any(|p| p.name.as_ref() == parameter) {
        return false;
    }
    let (negated, written) = match entry.ty.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, entry.ty.as_str()),
    };
    let (equality, written) = match written.strip_prefix('=') {
        Some(rest) => (true, rest),
        None => (false, written),
    };
    let info = AssertInfo {
        negated,
        equality,
        ty: types::parse_type_string(written),
        target: AssertTarget::Param(Name::from(parameter)),
        case_refs: Vec::new(),
        written_ty: grammar::parse_type_prefix(written)
            .0
            .unwrap_or(TypeNode::Error)
            .into(),
    };
    let list = match entry.when {
        AssertWhen::Always => &mut sig.assert,
        AssertWhen::True => &mut sig.assert_if_true,
        AssertWhen::False => &mut sig.assert_if_false,
    };
    let already = list.iter().any(|existing| {
        existing.negated == info.negated
            && existing.target == info.target
            && existing.written_ty == info.written_ty
    });
    if already {
        false
    } else {
        list.push(info);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use syntax::{lexer::Lexer, parser::Parser};

    #[test]
    fn assertion_tables_overlay_function_and_method_signatures() {
        let tokens = Lexer::new(
            "<?php function configured_guard($value): void {} class Demo { public function guard($value): void {} }",
        )
        .tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        let mut registry = Registry::build(&program);
        let entry = |function, class, method| AssertionEntry {
            function,
            class,
            method,
            parameter: "value".into(),
            ty: "int".into(),
            when: AssertWhen::Always,
        };
        registry.set_assertion_tables(
            vec![
                entry(Some("configured_guard".into()), None, None),
                entry(None, Some("Demo".into()), Some("guard".into())),
            ],
            Vec::new(),
        );

        let function_declaration = registry.functions["configured_guard"].clone();
        let method_declaration = registry.classes["Demo"].methods["guard"].clone();
        registry.apply_assertion_table();

        assert!(registry.functions["configured_guard"].assert.is_empty());
        assert!(registry.classes["Demo"].methods["guard"].assert.is_empty());
        assert_eq!(
            registry
                .lookup_function("configured_guard")
                .unwrap()
                .assert
                .len(),
            1
        );
        assert_eq!(
            registry
                .lookup_own_method("Demo", "guard")
                .unwrap()
                .assert
                .len(),
            1
        );
        assert_eq!(registry.functions["configured_guard"], function_declaration);
        assert_eq!(
            registry.classes["Demo"].methods["guard"],
            method_declaration
        );
    }
}
