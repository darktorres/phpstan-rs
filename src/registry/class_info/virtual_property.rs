use std::collections::HashMap;

use crate::ast::{Expr, ExprKind, PropertyHookBody, PropertyHooks, Stmt, StmtKind};
use crate::ast_walk::{expr_children, stmt_children};
use crate::registry::function_sig::{mark_template_params, resolve_type};
use crate::registry::name_context::{
    NameContext, collect_class_name_case_refs, resolve_names_in_type,
};
use crate::types::{TemplateParam, Type};

/// PHP 8.4 property hooks: an explicit `set(Type $value) { ... }` hook can
/// accept a wider type than the property's own backing/read type -
/// `public array $x { set(array $value) { $this->x = sanitize($value); } }`
/// does its own coercion inside the hook body before the real assignment
/// happens. A property write is checked against this parameter's native
/// type when present - the hook's own doc comment (if any) isn't captured
/// by the parser at all, so only the native type hint is available. No
/// hook, or an implicit `set { ... }` with no parameter list, returns
/// `None` - the property's type stays the write sink, unchanged.
pub(crate) fn property_hook_set_type(
    hooks: Option<&PropertyHooks>,
    template_params: &HashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> (Option<Type>, Vec<(String, String)>) {
    let Some((Some(param), _)) = hooks.and_then(|h| h.set.as_ref()) else {
        return (None, Vec::new());
    };
    let ty = resolve_type(None, param.type_hint.as_deref());
    let ty = mark_template_params(ty, template_params);
    let mut case_refs = Vec::new();
    collect_class_name_case_refs(&ty, ctx, &mut case_refs);
    (Some(resolve_names_in_type(ty, ctx)), case_refs)
}

/// Whether either hook of `hooks` references `$this->prop_name` anywhere
/// in its body - see `PropertySig::is_virtual`'s doc comment for the
/// full, empirically-verified rule this mirrors. `get` and `set` are
/// checked independently and OR'd together: real PHP makes the whole
/// property non-virtual the moment either hook references the backing
/// storage, even if the other never does.
pub(crate) fn hooks_reference_backing_storage(hooks: &PropertyHooks, prop_name: &str) -> bool {
    hooks
        .get
        .as_ref()
        .is_some_and(|body| hook_body_references_property(body, prop_name))
        || hooks
            .set
            .as_ref()
            .is_some_and(|(_, body)| hook_body_references_property(body, prop_name))
}

/// `pub(crate)`: also reused by `checker::oop::check_property_hooks` to
/// test each hook individually (`propertyGetHook.noRead`/
/// `propertySetHook.noAssign`), not just the OR'd-together whole-property
/// question `hooks_reference_backing_storage` above answers.
pub(crate) fn hook_body_references_property(body: &PropertyHookBody, prop_name: &str) -> bool {
    match body {
        PropertyHookBody::Arrow(e) => expr_references_property(e, prop_name),
        PropertyHookBody::Block(stmts) => {
            stmts.iter().any(|s| stmt_references_property(s, prop_name))
        }
        // An abstract hook can't reference `$this->prop` at all - no
        // real body exists to reference anything.
        PropertyHookBody::Abstract => false,
    }
}

/// Deliberately checks every occurrence of `$this->prop_name` - a read, a
/// bare unused statement, an `isset()`/`unset()` argument, or a write
/// target - uniformly, since real PHP's virtual-property algorithm
/// doesn't distinguish between them either (a bare `$this->x;` statement
/// with no use at all still makes the property non-virtual).
/// `expr_children`/`stmt_children` already stop at a nested closure/
/// arrow-fn/anonymous-class boundary (see their module doc comment in
/// `ast_walk.rs`) - exactly the scope boundary real PHP's algorithm
/// respects too (a `$this->propName` read inside a nested closure/
/// arrow-fn does NOT count).
fn stmt_references_property(stmt: &Stmt, prop_name: &str) -> bool {
    stmt_expr_fields(stmt)
        .into_iter()
        .any(|e| expr_references_property(e, prop_name))
        || stmt_children(stmt)
            .into_iter()
            .any(|s| stmt_references_property(s, prop_name))
}

fn expr_references_property(e: &Expr, prop_name: &str) -> bool {
    if let ExprKind::PropertyAccess(base, name, _) = &e.kind
        && **name == *prop_name
        && matches!(&base.kind, ExprKind::Variable(v) if **v == *"this")
    {
        return true;
    }

    expr_children(e)
        .into_iter()
        .any(|c| expr_references_property(c, prop_name))
}

/// This statement's own direct `Expr` fields (not nested statements -
/// `stmt_children` already covers those) - mirrors `checker::unused::
/// walk_stmt_for_members`'s identical enumeration, kept as its own copy
/// here since that one is coupled to a different walk's own accumulator/
/// context parameters.
fn stmt_expr_fields(stmt: &Stmt) -> Vec<&Expr> {
    match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => vec![e],
        StmtKind::Return(Some(e)) => vec![e],
        StmtKind::Echo(exprs) => exprs.iter().collect(),
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => vec![cond],
        StmtKind::For {
            init, cond, step, ..
        } => init.iter().chain(cond).chain(step).collect(),
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            let mut v = vec![subject];
            v.extend(key.iter());
            v.push(value);
            v
        }
        StmtKind::GlobalConst(consts) => consts.iter().map(|c| &c.value).collect(),
        StmtKind::StaticVar(vars) => vars.iter().filter_map(|(_, d)| d.as_ref()).collect(),
        StmtKind::Switch { subject, cases } => {
            let mut v = vec![subject];
            v.extend(cases.iter().filter_map(|c| c.test.as_ref()));
            v
        }
        _ => vec![],
    }
}
