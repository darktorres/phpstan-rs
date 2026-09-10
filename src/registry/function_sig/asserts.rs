use std::collections::HashMap;

use crate::docblock;
use crate::registry::name_context::{NameContext, resolve_names_in_type};
use crate::types::{self, TemplateParam, Type};

use super::generics::mark_template_params;

/// Which real, checkable thing a `@phpstan-assert`-family tag's own
/// `$target` text names - classified once here (registry-build time,
/// when a declaration's own parameter list is available to match a bare
/// name against) rather than carried as raw text into the checker.
/// Deliberately narrow: a method-call target (`$this->parseError()`,
/// a real corpus shape) or any array-index/nested-chain target isn't
/// classified into any of these and the whole tag is dropped - this
/// project's narrowing machinery has no way to key a call's own return
/// value the way it keys a variable/property, and guessing wrong here
/// risks a false assertion rather than just a missed one.
#[derive(Debug, Clone, PartialEq)]
pub enum AssertTarget {
    /// `$name` matching one of this declaration's own parameters.
    Param(String),
    /// Bare `$this` - the assertion narrows the call's own receiver.
    ThisSelf,
    /// `$this->propName` - a single-level property access on the
    /// receiver.
    ThisProp(String),
}

#[derive(Debug, Clone)]
pub struct AssertInfo {
    pub negated: bool,
    pub ty: Type,
    pub target: AssertTarget,
}

/// Classifies `tag.target`'s own raw text into a real `AssertTarget`,
/// `None` for anything this project doesn't model (see `AssertTarget`'s
/// own doc comment) - `param_names` is this declaration's own parameter
/// name list, checked so a target naming a parameter that doesn't
/// actually exist (a stale doc comment, or simply a name this project
/// failed to match for some other reason) isn't silently misread as
/// some other shape instead.
fn classify_assert_target(target: &str, param_names: &[&str]) -> Option<AssertTarget> {
    if target == "$this" {
        return Some(AssertTarget::ThisSelf);
    }
    if let Some(prop) = target.strip_prefix("$this->")
        && !prop.is_empty()
        && prop.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return Some(AssertTarget::ThisProp(prop.to_string()));
    }
    let name = target.strip_prefix('$')?;
    if param_names.contains(&name) {
        return Some(AssertTarget::Param(name.to_string()));
    }
    None
}

/// Resolves every `@phpstan-assert`-family tag on `doc` into a real
/// `AssertInfo` list, dropping any tag whose own target doesn't classify
/// (`classify_assert_target`) or whose own type text doesn't parse into
/// anything meaningful. Shares the exact same type-resolution pipeline
/// (`mark_template_params` then `resolve_names_in_type`) every other
/// declared type on this same declaration already goes through.
pub(crate) fn resolve_assert_tags(
    tags: &[docblock::AssertTag],
    param_names: &[&str],
    template_params: &HashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> Vec<AssertInfo> {
    tags.iter()
        .filter_map(|tag| {
            let target = classify_assert_target(&tag.target, param_names)?;
            let ty = types::parse_type_string(&tag.ty);
            let ty = mark_template_params(ty, template_params);
            let ty = resolve_names_in_type(ty, ctx);
            Some(AssertInfo {
                negated: tag.negated,
                ty,
                target,
            })
        })
        .collect()
}
