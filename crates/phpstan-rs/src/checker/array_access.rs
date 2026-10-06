//! `$o[…]` on an `ArrayAccess` object is a call of one of its methods
//! (`offsetGet`, `offsetSet`, `offsetExists`, `offsetUnset`), and
//! `count($o)` on a `Countable` one a call of `count`, with what an
//! explicit call of it does: it may throw, it may change the object's
//! properties, and it isn't pure. An object that is one only through a
//! subclass may run any code there. A property PHP runs `__get`, `__set`,
//! `__isset` or `__unset` for is accessed the same way, along the same
//! paths.

use crate::checker::call_effects::is_this;
use crate::checker::{Checker, Locals};
use crate::registry::summaries::{Access, Operation};
use syntax::ast::{DestructureItem, Expr, ExprKind};
use syntax::span::Span;
use types::Type;

/// The `ArrayAccess` method PHP calls for `access` to an element.
fn element_method(access: Access) -> &'static str {
    match access {
        Access::Read => "offsetGet",
        Access::Write => "offsetSet",
        Access::Isset => "offsetExists",
        Access::Unset => "offsetUnset",
    }
}

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
                    .class_by_name(class.as_ref())
                    .is_some_and(|info| info.is_final());
                (!is_final).then(|| (member.clone(), false))
            })
            .collect()
    }

    /// The effects of an offset access, `access`, on each of `base_ty`'s
    /// `ArrayAccess` members, at the expression `span` that makes the call:
    /// its throw point, the facts it may change, and an impure point;
    /// `this` when the container is the scope's own `$this`.
    pub(crate) fn implicit_array_access_call(
        &mut self,
        base_ty: &Type,
        this: bool,
        access: Access,
        span: Span,
        locals: &mut Locals,
    ) {
        let method = element_method(access);
        self.implicit_call_points(base_ty, "ArrayAccess", method, span);
        self.forget_what_an_operation_may_change(base_ty, this, Operation::Element(access), locals);
    }

    /// The effects of PHP calling `method` of `interface` on each member of
    /// `base_ty` that is (or may be) one: `count($o)` on a `Countable`
    /// object.
    pub(crate) fn implicit_method_call(
        &mut self,
        base_ty: &Type,
        interface: &str,
        method: &str,
        span: Span,
        locals: &mut Locals,
    ) {
        if !self.implicit_call_points(base_ty, interface, method, span) {
            return;
        }
        let mut effect = crate::registry::Effect::nothing();
        for (member, _) in &self.members_implementing(base_ty, interface) {
            effect.join(&self.method_call_effect(member, method));
        }
        self.forget_effect(&effect, false, locals);
    }

    /// The throw point and the impure point of PHP calling `method` of
    /// `interface` on each member of `base_ty` that is (or may be) one, at
    /// the expression `span`; whether there is one.
    fn implicit_call_points(
        &mut self,
        base_ty: &Type,
        interface: &str,
        method: &str,
        span: Span,
    ) -> bool {
        let members = self.members_implementing(base_ty, interface);
        if members.is_empty() {
            return false;
        }
        for (member, exact) in &members {
            let resolved = exact
                .then(|| self.find_method_receiver(member, method))
                .flatten();
            match &resolved {
                Some((class, _, _, sig)) => {
                    let dynamic = self.dynamic_method_throw_type(class, method, &[]);
                    self.record_call_throw_point(dynamic, Some(sig), span);
                }
                None => self.record_call_throw_point(None, None, span),
            }
        }
        if !self.implicit_calls.contains(&span) {
            self.implicit_calls.push(span);
        }
        true
    }

    /// `access` to the property or the element `operand` names, by the
    /// expression at `span`: a magic method of the object, or an
    /// `ArrayAccess` method (`implicit_array_access_call`). A computed
    /// property name becomes a string.
    fn implicit_member_access(
        &mut self,
        operand: &Expr,
        accesses: &[Access],
        span: Span,
        locals: &mut Locals,
    ) {
        let (base, prop) = match &operand.kind {
            ExprKind::Index(base, _) => {
                let base_ty = self.infer_expr(base, locals);
                for access in accesses {
                    self.implicit_array_access_call(&base_ty, is_this(base), *access, span, locals);
                }
                return;
            }
            ExprKind::PropertyAccess(base, prop, _) => (base, Some(prop.name.clone())),
            ExprKind::DynamicPropertyAccess(base, name, _) => {
                self.forget_what_an_operation_on_may_change(name, Operation::String, locals);
                (base, None)
            }
            _ => return,
        };
        let base_ty = self.infer_expr(base, locals);
        for access in accesses {
            self.forget_what_an_operation_may_change(
                &base_ty,
                is_this(base),
                Operation::Property(prop.clone(), *access),
                locals,
            );
        }
    }

    /// The implicit calls of writing to `lhs` (`$o[k] = v`, `$o->p = v`,
    /// `$o[k] .= v`, `$o->p++`), made by the expression at `span`:
    /// `offsetSet` or `__set` on the written container, and `offsetGet` on
    /// each container a nested write reads on its way; the reads of a
    /// property on the way, and of what is written for a compound write,
    /// are checked as reads. A nested write into what an `ArrayAccess`
    /// object's `offsetGet` returned by value changes a copy, and is
    /// reported (`offsetAssign.indirectModification`).
    pub(crate) fn implicit_array_access_write(
        &mut self,
        lhs: &Expr,
        span: Span,
        locals: &mut Locals,
    ) {
        self.implicit_member_fetch(lhs, Access::Write, span, locals);
    }

    /// The implicit calls of taking a reference to `target` (`$r = &$o[k]`,
    /// `$o->p = &$r`, `[&$o[k]] = $a`, `foreach ($a as &$o->p)`), made by
    /// the expression at `span`: the property or the element is read
    /// (`__get`, `offsetGet`), and so is each container on the way, as for
    /// a write.
    pub(crate) fn implicit_reference(&mut self, target: &Expr, span: Span, locals: &mut Locals) {
        self.implicit_member_fetch(target, Access::Read, span, locals);
    }

    /// `access` to the property or the element `lhs` names, and a read of
    /// each element on the way to it (`implicit_array_access_write`).
    fn implicit_member_fetch(
        &mut self,
        lhs: &Expr,
        access: Access,
        span: Span,
        locals: &mut Locals,
    ) {
        self.implicit_member_access(lhs, &[access], span, locals);
        let ExprKind::Index(base, _) = &lhs.kind else {
            return;
        };
        let mut link: &Expr = base;
        while let ExprKind::Index(inner, _) = &link.kind {
            let inner_ty = self.infer_expr(inner, locals);
            self.implicit_array_access_call(&inner_ty, is_this(inner), Access::Read, span, locals);
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

    /// The implicit calls of destructuring a value of type `source_ty` into
    /// `items`, made by the expression at `span`: `offsetGet` on the value,
    /// and on each element a nested pattern destructures, and a write to
    /// each target (`implicit_array_access_write`), or a reference taken to
    /// it (`implicit_reference`).
    pub(crate) fn implicit_destructure(
        &mut self,
        items: &[DestructureItem],
        source_ty: &Type,
        span: Span,
        locals: &mut Locals,
    ) {
        self.implicit_array_access_call(source_ty, false, Access::Read, span, locals);
        let element = source_ty.array_value_type().unwrap_or_else(Type::mixed);
        for item in items {
            let Some(target) = &item.target else {
                continue;
            };
            match &target.kind {
                ExprKind::Destructure(nested, _) => {
                    self.implicit_destructure(nested, &element, span, locals);
                }
                _ if item.by_ref => self.implicit_reference(target, span, locals),
                _ => self.implicit_array_access_write(target, span, locals),
            }
        }
    }

    /// The implicit calls of probing `operand` (`isset`, `empty`, `??`,
    /// `unset`), made by the expression at `span`: `root` on the probed
    /// element or property (a test, then a read for `empty` and `??`; an
    /// unset), and on each container on the way, which the probe reads: a
    /// test and then a read, or only a read for an unset.
    pub(crate) fn implicit_array_access_probe(
        &mut self,
        operand: &Expr,
        root: &[Access],
        span: Span,
        locals: &mut Locals,
    ) {
        let links: &[Access] = if root.contains(&Access::Unset) {
            &[Access::Read]
        } else {
            &[Access::Isset, Access::Read]
        };
        self.implicit_member_access(operand, root, span, locals);
        let mut link: &Expr = operand;
        while let ExprKind::Index(inner, _)
        | ExprKind::PropertyAccess(inner, _, _)
        | ExprKind::DynamicPropertyAccess(inner, _, _) = &link.kind
        {
            self.implicit_member_access(inner, links, span, locals);
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
