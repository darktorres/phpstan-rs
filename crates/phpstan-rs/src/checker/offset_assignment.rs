//! Writing through an offset, mirroring real PHPStan's
//! `OffsetAccessAssignmentRule`/`OffsetAccessAssignOpRule`
//! (`offsetAssign.dimType`) and `OffsetAccessValueAssignmentRule`
//! (`offsetAssign.valueType`), all level 3: a string given anything but
//! an int offset (or `[]`), and an `ArrayAccess` object given an offset
//! or value its `offsetSet()` doesn't accept. An array's invalid key type
//! is `offsetAccess.invalidOffset`'s concern, on writes as on reads.

use crate::checker::{Checker, Locals};
use syntax::ast::{Expr, ExprKind};
use types::Type;

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
    ) {
        let ExprKind::Index(base, idx) = &lhs.kind else {
            return;
        };
        let container = self.infer_expr(base, locals);
        let dim = idx.as_deref().map(|i| self.infer_expr(i, locals));
        if container.is_mixed() || (container.is_never() || container.is_null().yes()) {
            return;
        }
        if is_assign_op
            && let Some(dim) = &dim
            && container.has_offset_value_type(dim).no()
        {
            return;
        }
        let members: Vec<&Type> = container
            .union_members()
            .iter()
            .filter(|m| !m.is_null().yes())
            .collect();
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
        self.report(message, lhs.span, level, identifier);
    }

    /// Why writing `value` at `dim` into one container type fails, if
    /// it does: `offsetAssign.dimType` or `offsetAssign.valueType`.
    fn offset_write_error(
        &self,
        container: &Type,
        dim: Option<&Type>,
        value: &Type,
    ) -> Option<&'static str> {
        if container.is_string().yes() {
            let bad = match dim {
                None => true,
                Some(d) => !d.is_mixed() && !d.is_int().yes(),
            };
            return bad.then_some("offsetAssign.dimType");
        }
        let (class, _) = container.object_class_and_args()?;
        if !self.registry.is_subclass_of(class, "ArrayAccess") {
            return None;
        }
        let offset_set = self.registry.lookup_method(class, "offsetSet")?;
        let [offset_param, value_param, ..] = offset_set.params().as_slice() else {
            return None;
        };
        let offset_ty = dim.cloned().unwrap_or(Type::null());
        if !offset_ty.is_mixed() && !self.accepts(offset_param.ty(), &offset_ty).yes() {
            return Some("offsetAssign.dimType");
        }
        (!value.is_mixed() && !self.accepts(value_param.ty(), value).yes())
            .then_some("offsetAssign.valueType")
    }
}
