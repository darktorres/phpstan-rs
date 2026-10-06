use rustc_hash::FxHashMap;
use syntax::ast::Param;
use syntax::name::Name;

use crate::registry::name_context::{collect_class_name_case_refs, resolve_names_in_type};
use crate::registry::type_node::parse_type_node;
use phpdoc::DocType;
use phpdoc::docblock;
use types::{self, TemplateParam, Type};

use super::generics::mark_template_params;
use syntax::name_resolution::NameContext;

/// Which real, checkable thing a `@phpstan-assert`-family tag's own
/// `$target` text names - classified once here (registry-build time,
/// when a declaration's own parameter list is available to match a bare
/// name against) rather than carried as raw text into the checker.
/// Deliberately narrow: a method-call target (e.g. `$this->parseError()`)
/// or any array-index/nested-chain target isn't
/// classified into any of these and the whole tag is dropped - this
/// project's narrowing machinery has no way to key a call's own return
/// value the way it keys a variable/property, and guessing wrong here
/// risks a false assertion rather than just a missed one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AssertTarget {
    /// `$name` matching one of this declaration's own parameters.
    Param(Name),
    /// Bare `$this` - the assertion narrows the call's own receiver.
    ThisSelf,
    /// `$this->propName` - a single-level property access on the
    /// receiver.
    ThisProp(Name),
    /// `$name` shaped exactly like a bare parameter
    /// reference, but `name` doesn't match any of this declaration's own
    /// parameters - real PHPStan's own `parameter.notFound` ("Assert
    /// references unknown parameter $name."). Carries no narrowing
    /// effect (there's nothing real to narrow), but - unlike the old
    /// silent-drop behavior - is still surfaced to the checker so it can
    /// report the real authoring mistake this almost always is (a typo,
    /// or a stale tag left behind after a parameter rename).
    NotFound(Name),
    /// Any other target shape this project doesn't model at
    /// all (a method-call target, an array-index/nested-chain target,
    /// ...) - real PHPStan's own `assert.unknownExpr` ("Assert
    /// references unknown %s."). Carries the original raw target text
    /// verbatim, for that message. Same "no narrowing effect" note as
    /// `NotFound` above.
    Unknown(Name),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssertInfo {
    pub negated: bool,
    /// `=Type`: the target is identical to a value of `ty`.
    pub equality: bool,
    pub ty: Type,
    pub target: AssertTarget,
    // Shares the exact same type-resolution pipeline as every other
    // declared type on this same declaration, which already collects
    // `case_refs`.
    pub case_refs: Vec<(Name, Name)>,
    // The tag's type as written, for checks that need its parsed form
    // rather than the resolved `ty`.
    pub written_ty: DocType,
}

/// Classifies `tag.target`'s own raw text into a real `AssertTarget` -
/// always succeeds: a target this project doesn't model at all, or a
/// bare `$name` not matching any real parameter, still
/// produces a real `AssertTarget` variant (`Unknown`/`NotFound`) rather
/// than being silently dropped, so the checker can report real PHPStan's
/// own `assert.unknownExpr`/`parameter.notFound` for it. `param_names` is
/// this declaration's own parameter name list, checked so a target naming
/// a parameter that doesn't actually exist (a stale doc comment, or
/// simply a name this project failed to match for some other reason)
/// isn't silently misread as some other shape instead.
fn classify_assert_target(target: &str, params: &[Param]) -> AssertTarget {
    if target == "$this" {
        return AssertTarget::ThisSelf;
    }
    if let Some(prop) = target.strip_prefix("$this->")
        && !prop.is_empty()
        && prop.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return AssertTarget::ThisProp(Name::from(prop));
    }
    if let Some(name) = target.strip_prefix('$')
        && !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return if params.iter().any(|param| param.name.as_ref() == name) {
            AssertTarget::Param(Name::from(name))
        } else {
            AssertTarget::NotFound(Name::from(name))
        };
    }
    AssertTarget::Unknown(Name::from(target))
}

/// Resolves every `@phpstan-assert`-family tag on `doc` into a real
/// `AssertInfo` list - every tag produces one entry, an unrecognized
/// target never drops the tag (see `AssertTarget`'s own doc comment), so
/// callers that only care about applying real narrowing must match on
/// `target` themselves and skip `NotFound`/`Unknown`.
/// Shares the exact same type-resolution pipeline (`mark_template_params`
/// then `resolve_names_in_type`) every other declared type on this same
/// declaration already goes through.
pub(crate) fn resolve_assert_tags(
    tags: &[docblock::AssertTag],
    params: &[Param],
    template_params: &FxHashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> Vec<AssertInfo> {
    tags.iter()
        .map(|tag| {
            let target = classify_assert_target(&tag.target, params);
            let ty = parse_type_node(&tag.ty);
            let ty = mark_template_params(ty, template_params);
            let mut case_refs = Vec::new();
            collect_class_name_case_refs(&ty, ctx, &mut case_refs);
            let ty = resolve_names_in_type(ty, ctx);
            AssertInfo {
                negated: tag.negated,
                equality: tag.equality,
                ty,
                target,
                case_refs,
                written_ty: tag.ty.clone(),
            }
        })
        .collect()
}
