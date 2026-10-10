//! Writing through an offset, mirroring real PHPStan's
//! `OffsetAccessAssignmentRule`/`OffsetAccessAssignOpRule`
//! (`offsetAssign.dimType`) and `OffsetAccessValueAssignmentRule`
//! (`offsetAssign.valueType`), all level 3: a string given anything but
//! an int offset (or `[]`), and an `ArrayAccess` object given an offset
//! or value its `offsetSet()` doesn't accept. An array's invalid key type
//! is `offsetAccess.invalidOffset`'s concern, on writes as on reads.

use crate::checker::{Checker, Locals};
use syntax::ast::{ExprKind, ExprNode};
use types::Type;

impl<'a> Checker<'a> {
    /// `lhs` is the `$x[...]` write target; `value` is the assigned
    /// value's type; `is_assign_op` marks a compound assignment, which
    /// skips an offset the container provably doesn't have yet (a read
    /// error, reported elsewhere).
    pub(crate) fn check_offset_assignment(
        &mut self,
        lhs: &ExprNode,
        value: &Type,
        is_assign_op: bool,
        locals: &Locals,
    ) {
        let ExprKind::Index(base, idx) = &lhs.kind else {
            return;
        };
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("offset assignment belongs to its retained file owner");
        let arenas = syntax_owner.arenas();
        let base = arenas
            .expr(*base)
            .expect("offset assignment base belongs to its retained file owner");
        let container = self.infer_expr(base, locals);
        let dim = idx
            .as_ref()
            .and_then(|id| arenas.expr(*id))
            .map(|i| self.infer_expr(i, locals));
        if container.is_mixed() || (container.is_never() || container.is_null().yes()) {
            return;
        }
        if container.is_string().yes() {
            self.check_string_offset_write(lhs, &container, dim.as_ref(), value);
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
        report!(self, message, self.expr_span(lhs.id), level, identifier);
    }

    /// A write to a string offset PHP rejects or cuts short
    /// (`offsetAssign.valueType`): an empty value throws, a longer one
    /// writes only its first byte, and a negative offset writes nothing
    /// where it is before the start of the string, or before PHP 7.1, on
    /// every version the project runs on or on some.
    fn check_string_offset_write(
        &mut self,
        lhs: &ExprNode,
        container: &Type,
        dim: Option<&Type>,
        value: &Type,
    ) {
        let values = value.constant_strings();
        let message = if !values.is_empty() && values.iter().all(|v| v.as_bytes().is_empty()) {
            Some("Cannot assign an empty string to a string offset.".to_string())
        } else if !values.is_empty() && values.iter().all(|v| v.as_bytes().len() > 1) {
            Some("Only the first byte will be assigned to the string offset.".to_string())
        } else if let Some(k) = dim.and_then(Type::int_literal_value)
            && k < 0
        {
            let strings = container.constant_strings();
            let before_start =
                !strings.is_empty() && strings.iter().all(|s| (s.as_bytes().len() as i64) < -k);
            match self.negative_string_offsets() {
                types::NegativeStringOffsets::Absent => Some(format!("Illegal string offset {k}.")),
                _ if before_start => Some(format!("Illegal string offset {k}.")),
                types::NegativeStringOffsets::Either => {
                    Some(format!("Illegal string offset {k} before PHP 7.1."))
                }
                types::NegativeStringOffsets::FromEnd => None,
            }
        } else {
            None
        };
        if let Some(message) = message {
            report!(
                self,
                message,
                self.expr_span(lhs.id),
                3,
                "offsetAssign.valueType"
            );
        }
    }

    /// Why writing `value` at `dim` into one container type fails, if    /// Why writing `value` at `dim` into one container type fails, if
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
        let (class, _) = container.as_object()?;
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
