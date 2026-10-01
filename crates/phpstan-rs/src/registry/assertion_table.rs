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
        let mut tagged: Vec<Name> = Vec::new();
        for entry in entries {
            let sig = match (&entry.function, &entry.class, &entry.method) {
                (Some(function), _, _) => {
                    let function = function.trim_start_matches('\\');
                    if !self.scope.has_function(function) {
                        continue;
                    }
                    let Some(declared) = self
                        .functions
                        .get_with_name(function)
                        .map(|(name, _)| name.clone())
                    else {
                        continue;
                    };
                    tagged.push(Name::from(
                        declared.rsplit('\\').next().unwrap_or(&declared),
                    ));
                    self.functions.get_mut(&declared)
                }
                (None, Some(class), Some(method)) => {
                    let Some(canonical) = self
                        .lookup_class(class.trim_start_matches('\\'))
                        .map(|(c, _)| Name::from(c))
                    else {
                        continue;
                    };
                    if !self.scope.has_class(&canonical) {
                        continue;
                    }
                    self.classes.get_mut(&canonical).and_then(|info| {
                        info.methods
                            .iter_mut()
                            .find(|(name, _)| name.eq_ignore_ascii_case(method))
                            .map(|(name, sig)| {
                                tagged.push(name.clone());
                                sig
                            })
                    })
                }
                _ => None,
            };
            let Some(sig) = sig else {
                continue;
            };
            let parameter = entry.parameter.trim_start_matches('$');
            if !sig.params.iter().any(|p| p.name.as_ref() == parameter) {
                continue;
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
                    .unwrap_or(TypeNode::Error),
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
            if !already {
                list.push(info);
            }
        }
        self.names_with_assert_tags.extend(tagged);
    }
}
