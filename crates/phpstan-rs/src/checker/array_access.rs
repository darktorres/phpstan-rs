//! `$o[…]` on an `ArrayAccess` object is a call of one of its methods
//! (`offsetGet`, `offsetSet`, `offsetExists`, `offsetUnset`), and
//! `count($o)` on a `Countable` one a call of `count`, with what an
//! explicit call of it does: it may throw, it may change the object's
//! properties, and it isn't pure. An object that is one only through a
//! subclass may run any code there.

use crate::checker::{Checker, Locals, purge_stale_narrowing_for};
use syntax::ast::{Expr, ExprKind};
use syntax::span::Span;
use types::Type;

impl<'a> Checker<'a> {
    /// The members of `ty` whose offsets are method calls: `true` for one
    /// that is `ArrayAccess`, `false` for one that may be through a
    /// subclass (an interface or a non-final class).
    fn array_access_members(&self, ty: &Type) -> Vec<(Type, bool)> {
        self.members_implementing(ty, "ArrayAccess")
    }

    /// The members of `ty` that are `interface` objects (`true`), or may
    /// be through a subclass (`false`).
    fn members_implementing(&self, ty: &Type, interface: &str) -> Vec<(Type, bool)> {
        ty.union_members()
            .iter()
            .filter_map(|member| {
                let (class, _) = member.as_object()?;
                if self.registry.is_subclass_of(class, interface)
                    || class.eq_ignore_ascii_case(interface)
                {
                    return Some((member.clone(), true));
                }
                let is_final = self
                    .registry
                    .classes
                    .get(class.as_ref())
                    .is_some_and(|info| info.is_final());
                (!is_final).then(|| (member.clone(), false))
            })
            .collect()
    }

    /// The effects of `base[…]` calling `method` on each of `base_ty`'s
    /// `ArrayAccess` members, at the expression `span` that makes the
    /// call: its throw point, the facts about `base` it may change, and
    /// an impure point.
    pub(crate) fn implicit_array_access_call(
        &mut self,
        base: &Expr,
        base_ty: &Type,
        method: &str,
        span: Span,
        locals: &mut Locals,
    ) {
        self.implicit_method_call(base, base_ty, "ArrayAccess", method, span, locals);
    }

    /// The effects of PHP calling `method` of `interface` on each member of
    /// `base_ty` that is (or may be) one: `$o[…]` on an `ArrayAccess`
    /// object, `count($o)` on a `Countable` one.
    pub(crate) fn implicit_method_call(
        &mut self,
        base: &Expr,
        base_ty: &Type,
        interface: &str,
        method: &str,
        span: Span,
        locals: &mut Locals,
    ) {
        use crate::registry::PropertyWrites;
        let members = self.members_implementing(base_ty, interface);
        if members.is_empty() {
            return;
        }
        let mut writes = PropertyWrites::Nothing;
        for (member, exact) in &members {
            let resolved = exact
                .then(|| self.find_method_receiver(member, method))
                .flatten();
            match &resolved {
                Some((class, _, owner, sig)) => {
                    let dynamic = self.dynamic_method_throw_type(class, method, &[]);
                    self.record_call_throw_point(dynamic, Some(sig), span);
                    let member_writes = if crate::registry::is_non_overridable(
                        self.registry,
                        class,
                        owner,
                        method,
                        sig,
                    ) {
                        self.registry.method_property_writes(owner, method)
                    } else {
                        PropertyWrites::Anything
                    };
                    writes = writes.max(member_writes);
                }
                None => {
                    self.record_call_throw_point(None, None, span);
                    writes = PropertyWrites::Anything;
                }
            }
        }
        if writes > PropertyWrites::Nothing
            && let Some(key) = locals.key_of(base)
            && locals.has_key_depending_on(key)
        {
            purge_stale_narrowing_for(locals, key);
        }
        if !self.implicit_calls.contains(&span) {
            self.implicit_calls.push(span);
        }
    }

    /// The implicit calls of writing to `lhs` (`$o[k] = v`, `$o[k] .= v`,
    /// `$o[k]++`), made by the expression at `span`: `offsetSet` on the
    /// written container, and `offsetGet` on each container a nested write
    /// reads on its way. A nested write into what an `ArrayAccess`
    /// object's `offsetGet` returned by value changes a copy, and is
    /// reported (`offsetAssign.indirectModification`).
    pub(crate) fn implicit_array_access_write(
        &mut self,
        lhs: &Expr,
        span: Span,
        locals: &mut Locals,
    ) {
        let ExprKind::Index(base, _) = &lhs.kind else {
            return;
        };
        let base_ty = self.infer_expr(base, locals);
        self.implicit_array_access_call(base, &base_ty, "offsetSet", span, locals);
        let mut link: &Expr = base;
        while let ExprKind::Index(inner, _) = &link.kind {
            let inner_ty = self.infer_expr(inner, locals);
            self.implicit_array_access_call(inner, &inner_ty, "offsetGet", span, locals);
            let by_value = self
                .array_access_members(&inner_ty)
                .into_iter()
                .filter(|(_, exact)| *exact)
                .filter_map(|(member, _)| {
                    let (class, _, _, sig) = self.find_method_receiver(&member, "offsetGet")?;
                    (!sig.returns_ref()).then_some(class)
                })
                .next();
            if let Some(class) = by_value {
                self.report(
                    format!(
                        "Indirect modification of overloaded element of {class} has no effect."
                    ),
                    link.span,
                    3,
                    "offsetAssign.indirectModification",
                );
            }
            link = inner;
        }
    }

    /// The implicit calls of probing `operand` (`isset`, `empty`, `??`,
    /// `unset`), made by the expression at `span`: `root` on the probed
    /// container (`offsetExists`, then `offsetGet` for `empty` and `??`;
    /// `offsetUnset`), and `offsetGet` on each container on the way.
    pub(crate) fn implicit_array_access_probe(
        &mut self,
        operand: &Expr,
        root: &[&str],
        span: Span,
        locals: &mut Locals,
    ) {
        let ExprKind::Index(base, _) = &operand.kind else {
            return;
        };
        let base_ty = self.infer_expr(base, locals);
        for method in root {
            self.implicit_array_access_call(base, &base_ty, method, span, locals);
        }
        let mut link: &Expr = base;
        while let ExprKind::Index(inner, _) = &link.kind {
            let inner_ty = self.infer_expr(inner, locals);
            self.implicit_array_access_call(inner, &inner_ty, "offsetGet", span, locals);
            link = inner;
        }
    }

    /// `offsetAccess.invalidOffset` for an `ArrayAccess` member whose
    /// `method` doesn't accept `dim` as its offset: level 3 when every
    /// such member refuses it, 7 when only some do.
    pub(crate) fn check_array_access_offset(
        &mut self,
        base_ty: &Type,
        method: &str,
        dim: &Type,
        span: Span,
    ) {
        if dim.is_mixed() {
            return;
        }
        let verdicts: Vec<bool> = self
            .array_access_members(base_ty)
            .into_iter()
            .filter(|(_, exact)| *exact)
            .filter_map(|(member, _)| {
                let (_, _, _, sig) = self.find_method_receiver(&member, method)?;
                let offset = sig.params().first()?;
                Some(!self.accepts(offset.ty(), dim).yes())
            })
            .collect();
        if !verdicts.contains(&true) {
            return;
        }
        let level = if verdicts.iter().all(|refused| *refused) {
            3
        } else {
            7
        };
        self.report(
            format!("Cannot access offset {dim} on {base_ty}."),
            span,
            level,
            "offsetAccess.invalidOffset",
        );
    }
}
