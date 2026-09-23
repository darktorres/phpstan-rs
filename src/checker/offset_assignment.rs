//! Writing through an offset, mirroring real PHPStan's
//! `OffsetAccessAssignmentRule`/`OffsetAccessAssignOpRule`
//! (`offsetAssign.dimType`) and `OffsetAccessValueAssignmentRule`
//! (`offsetAssign.valueType`), all level 3: a string given anything but
//! an int offset (or `[]`), and an `ArrayAccess` object given an offset
//! or value its `offsetSet()` doesn't accept. An array's invalid key type
//! is `offsetAccess.invalidOffset`'s concern, on writes as on reads.

use crate::ast::{Expr, ExprKind};
use crate::checker::assignment::offset_verdict;
use crate::checker::{Checker, Locals};
use crate::types::Type;

fn is_string_type(ty: &Type) -> bool {
    match ty {
        Type::String | Type::StringLiteral(_) | Type::ClassString(_) => true,
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
        let string_dim_error = is_string_type(&container)
            && match &dim {
                None => true,
                Some(d) => !matches!(d, Type::Mixed(_)) && !is_integer_type(d),
            };
        if string_dim_error {
            let message = match &dim {
                None => format!("Cannot assign new offset to {container}."),
                Some(d) => format!("Cannot assign offset {d} to {container}."),
            };
            self.report(
                label.to_string(),
                message,
                lhs.span,
                3,
                "offsetAssign.dimType",
            );
            return;
        }
        let Type::Object(class, _) = &container else {
            return;
        };
        if !self.registry.is_subclass_of(class, "ArrayAccess") {
            return;
        }
        let Some(offset_set) = self.registry.lookup_method(class, "offsetSet") else {
            return;
        };
        let [offset_param, value_param, ..] = offset_set.params.as_slice() else {
            return;
        };
        let offset_ty = dim.clone().unwrap_or(Type::Null);
        if !matches!(offset_ty, Type::Mixed(_))
            && !self.registry.is_assignable(&offset_ty, &offset_param.ty)
        {
            let message = match &dim {
                None => format!("Cannot assign new offset to {container}."),
                Some(d) => format!("Cannot assign offset {d} to {container}."),
            };
            self.report(
                label.to_string(),
                message,
                lhs.span,
                3,
                "offsetAssign.dimType",
            );
            return;
        }
        if !matches!(value, Type::Mixed(_)) && !self.registry.is_assignable(value, &value_param.ty)
        {
            self.report(
                label.to_string(),
                format!("{container} does not accept {value}."),
                lhs.span,
                3,
                "offsetAssign.valueType",
            );
        }
    }
}
