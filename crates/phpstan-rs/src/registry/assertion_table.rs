//! Applies `assertion_tables`' `[[assertions]]` (built-in entries plus
//! configured ones) to the matching declarations as if they carried the
//! `@phpstan-assert` tags, and serves `[[class_argument_returns]]`.

use std::sync::{Arc, LazyLock};

use crate::assertion_tables::{
    AssertWhen, AssertionEntry, ClassArgumentReturn, default_assertions,
    default_class_argument_returns,
};
use crate::registry::function_sig::{AssertInfo, AssertTarget};
use crate::registry::{NameContext, Registry, resolve_names_in_type};
use crate::types;

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
        let mut tagged: Vec<Arc<str>> = Vec::new();
        for entry in entries {
            let sig = match (&entry.function, &entry.class, &entry.method) {
                (Some(function), _, _) => self
                    .functions
                    .iter_mut()
                    .find(|(name, _)| name.eq_ignore_ascii_case(function.trim_start_matches('\\')))
                    .map(|(name, sig)| {
                        let bare = name.rsplit('\\').next().unwrap_or(name);
                        tagged.push(Arc::from(bare));
                        sig
                    }),
                (None, Some(class), Some(method)) => {
                    let Some(canonical) = self
                        .lookup_class(class.trim_start_matches('\\'))
                        .map(|(c, _)| Arc::<str>::from(c))
                    else {
                        continue;
                    };
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
            let info = AssertInfo {
                negated,
                ty: resolve_names_in_type(
                    types::parse_type_string(written),
                    &NameContext::default(),
                ),
                target: AssertTarget::Param(Arc::from(parameter)),
                case_refs: Vec::new(),
                raw_ty: Arc::from(written),
            };
            let list = match entry.when {
                AssertWhen::Always => &mut sig.assert,
                AssertWhen::True => &mut sig.assert_if_true,
                AssertWhen::False => &mut sig.assert_if_false,
            };
            let already = list.iter().any(|existing| {
                existing.negated == info.negated
                    && existing.target == info.target
                    && existing.raw_ty == info.raw_ty
            });
            if !already {
                list.push(info);
            }
        }
        self.names_with_assert_tags.extend(tagged);
    }
}
