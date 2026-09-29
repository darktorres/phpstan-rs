use rustc_hash::FxHashMap;
use syntax::name::Name;

use crate::registry::function_sig::declared_type;
use syntax::ast::{Expr, ExprKind, PropertyHookBody, PropertyHooks};
use syntax::name_resolution::NameContext;
use syntax::visit::{any_expr_in_body, any_expr_in_scope};
use types::{TemplateParam, Type};

/// PHP 8.4 property hooks: an explicit `set(Type $value) { ... }` hook can
/// accept a wider type than the property's own backing/read type -
/// `public array $x { set(array $value) { $this->x = sanitize($value); } }`
/// does its own coercion inside the hook body before the real assignment
/// happens. A property write is checked against this parameter's native
/// type when present - the hook's own doc comment (if any) isn't captured
/// by the parser at all, so only the native type hint is available. No
/// hook, or an implicit `set { ... }` with no parameter list, returns
/// `None` - the property's type stays the write sink, unchanged.
#[allow(clippy::type_complexity)]
pub(crate) fn property_hook_set_type(
    hooks: Option<&PropertyHooks>,
    template_params: &FxHashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> (Option<Type>, Vec<(Name, Name)>) {
    let Some(param) = hooks.and_then(|h| h.set.as_ref()?.param.as_ref()) else {
        return (None, Vec::new());
    };
    let declared = declared_type(None, param.type_hint.as_ref(), template_params, ctx);
    (Some(declared.ty), declared.case_refs)
}

/// Whether either hook of `hooks` references `$this->prop_name` anywhere
/// in its body - see `PropertyReflection::is_virtual`'s doc comment for the
/// full, empirically-verified rule this mirrors. `get` and `set` are
/// checked independently and OR'd together: real PHP makes the whole
/// property non-virtual the moment either hook references the backing
/// storage, even if the other never does.
pub(crate) fn hooks_reference_backing_storage(hooks: &PropertyHooks, prop_name: &str) -> bool {
    hooks
        .iter()
        .any(|hook| hook_body_references_property(&hook.body, prop_name))
}

/// `pub(crate)`: also reused by `checker::oop::check_property_hooks` to
/// test each hook individually (`propertyGetHook.noRead`/
/// `propertySetHook.noAssign`), not just the OR'd-together whole-property
/// question `hooks_reference_backing_storage` above answers.
pub(crate) fn hook_body_references_property(body: &PropertyHookBody, prop_name: &str) -> bool {
    // Every occurrence of `$this->prop_name` counts - a read, a bare
    // statement, an `isset()`/`unset()` operand, or a write target - as in
    // PHP's own virtual-property rule; one inside a nested closure, arrow
    // function or anonymous class does not.
    any_hook_expr(body, |e| {
        matches!(&e.kind, ExprKind::PropertyAccess(base, name, _)
            if **name == *prop_name
                && matches!(&base.kind, ExprKind::Variable(v) if **v == *"this"))
    })
}

/// Whether `body` calls the parent's hook of `prop_name`
/// (`parent::$prop_name::get()`/`::set()`), which uses the backing value
/// when the parent declares no such hook.
pub(crate) fn hook_body_calls_parent_hook(body: &PropertyHookBody, prop_name: &str) -> bool {
    any_hook_expr(body, |e| {
        matches!(&e.kind, ExprKind::StaticCall(target, _, _)
            if target.parent_hook_property() == Some(prop_name))
    })
}

/// Whether `pred` holds for an expression in the hook's own body.
fn any_hook_expr(body: &PropertyHookBody, pred: impl FnMut(&Expr) -> bool) -> bool {
    match body {
        PropertyHookBody::Arrow(e) => any_expr_in_scope(e, pred),
        PropertyHookBody::Block(stmts) => any_expr_in_body(stmts, pred),
        PropertyHookBody::Abstract => false,
    }
}
