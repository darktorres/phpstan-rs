//! Writing through an offset, mirroring real PHPStan's
//! `OffsetAccessAssignmentRule`/`OffsetAccessAssignOpRule`
//! (`offsetAssign.dimType`) and `OffsetAccessValueAssignmentRule`
//! (`offsetAssign.valueType`), all level 3: a string given anything but
//! an int offset (or `[]`), and an `ArrayAccess` object given an offset
//! or value its `offsetSet()` doesn't accept. An array's invalid key type
//! is `offsetAccess.invalidOffset`'s concern, on writes as on reads.

use crate::checker::assignment::offset_verdict;
use crate::checker::{Checker, Locals};
use syntax::ast::{Expr, ExprKind};
use types::Type;

fn is_string_type(ty: &Type) -> bool {
    match ty {
        Type::String | Type::StringLiteral(_) | Type::ClassString(_) | Type::ClassKindString(_) => {
            true
        }
        Type::Intersection(members) => {
            members
                .iter()
                .any(|m| matches!(m, Type::String | Type::StringLiteral(_)))
                && members.iter().all(|m| {
                    matches!(
                        m,
                        Type::String | Type::StringLiteral(_) | Type::Accessory(_)
                    )
                })
        }
        _ => false,
    }
}

fn is_integer_type(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::IntLiteral(_) | Type::IntRange(..) => true,
        Type::Union(members) => members.iter().all(is_integer_type),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    /// `lhs` is the `$x[...]` write target; `value` is the assigned
    /// value's type; `is_assign_op` marks a compound assignment, which
    /// skips an offset the container provably doesn't have yet (a read
    /// error, reported elsewhere).
    pub(crate) fn check_offset_assignment(
        &mut self,
        lhs: &Expr,
        value: &Type,
        is_assign_op: bool,
        locals: &Locals,
        label: &str,
    ) {
        let ExprKind::Index(base, idx) = &lhs.kind else {
            return;
        };
        let container = self.infer_expr(base, locals);
        let dim = idx.as_deref().map(|i| self.infer_expr(i, locals));
        if matches!(container, Type::Mixed(_) | Type::Null | Type::Never) {
            return;
        }
        if is_assign_op
            && let Some(dim) = &dim
            && offset_verdict(&container, dim) == Some(false)
        {
            return;
        }
        let members: Vec<&Type> = match &container {
            Type::Union(members) => members
                .iter()
                .filter(|m| !matches!(m, Type::Null))
                .collect(),
            other => vec![other],
        };
        let verdicts: Vec<Option<&'static str>> = members
            .iter()
            .map(|m| self.offset_write_error(m, dim.as_ref(), value))
            .collect();
        let Some(identifier) = verdicts.iter().flatten().next().copied() else {
            return;
        };
        let level = if verdicts.iter().all(Option::is_some) {
            3
        } else {
            7
        };
        let message = if identifier == "offsetAssign.valueType" {
            format!("{container} does not accept {value}.")
        } else {
            match &dim {
                None => format!("Cannot assign new offset to {container}."),
                Some(d) => format!("Cannot assign offset {d} to {container}."),
            }
        };
        self.report(label.to_string(), message, lhs.span, level, identifier);
    }

    /// Why writing `value` at `dim` into one container type fails, if
    /// it does: `offsetAssign.dimType` or `offsetAssign.valueType`.
    fn offset_write_error(
        &self,
        container: &Type,
        dim: Option<&Type>,
        value: &Type,
    ) -> Option<&'static str> {
        if is_string_type(container) {
            let bad = match dim {
                None => true,
                Some(d) => !matches!(d, Type::Mixed(_)) && !is_integer_type(d),
            };
            return bad.then_some("offsetAssign.dimType");
        }
        let Type::Object(class, _) = container else {
            return None;
        };
        if !self.registry.is_subclass_of(class, "ArrayAccess") {
            return None;
        }
        let offset_set = self.registry.lookup_method(class, "offsetSet")?;
        let [offset_param, value_param, ..] = offset_set.params.as_slice() else {
            return None;
        };
        let offset_ty = dim.cloned().unwrap_or(Type::Null);
        if !matches!(offset_ty, Type::Mixed(_))
            && !self.registry.accepts(&offset_param.ty, &offset_ty).yes()
        {
            return Some("offsetAssign.dimType");
        }
        (!matches!(value, Type::Mixed(_)) && !self.registry.accepts(&value_param.ty, value).yes())
            .then_some("offsetAssign.valueType")
    }
}
