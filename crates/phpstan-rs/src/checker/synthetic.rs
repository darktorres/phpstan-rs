use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use syntax::ast::{ExprId, ExprKind, ExprNode};
use syntax::span::Span;

/// The upper portion of the file-local expression ID space is reserved for
/// short-lived checker nodes. Parsed files cannot realistically approach
/// this boundary; keeping the range explicit also lets cache and native-type
/// readers reject these IDs before consulting the parsed-file owner.
const SYNTHETIC_EXPR_ID_START: u32 = 0xf000_0000;
const SYNTHETIC_EXPR_ID_END: u32 = u32::MAX;

pub(crate) struct SyntheticExpr {
    node: ExprNode,
}

impl Deref for SyntheticExpr {
    type Target = ExprNode;

    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

/// Reusable storage owned by one checker and reset whenever it starts a new
/// file. Bump allocation keeps recursive checker-created expressions stable
/// even when another synthetic node is allocated during their evaluation.
#[derive(Default)]
pub(super) struct SyntheticNodeScratch {
    arena: Rc<bumpalo::Bump>,
    next_expr: Cell<u32>,
    spans: RefCell<Vec<Span>>,
}

impl SyntheticNodeScratch {
    pub(super) fn alloc_expr(&self, kind: ExprKind, span: Span) -> &SyntheticExpr {
        self.alloc_expr_in(&self.arena, kind, span)
    }

    pub(super) fn arena_handle(&self) -> Rc<bumpalo::Bump> {
        Rc::clone(&self.arena)
    }

    pub(super) fn alloc_expr_in<'a>(
        &self,
        arena: &'a bumpalo::Bump,
        kind: ExprKind,
        span: Span,
    ) -> &'a SyntheticExpr {
        let offset = self.next_expr.get();
        let raw = SYNTHETIC_EXPR_ID_START
            .checked_add(offset)
            .filter(|id| *id <= SYNTHETIC_EXPR_ID_END)
            .expect("checker synthetic expression ID space exhausted");
        self.next_expr.set(offset + 1);
        let mut spans = self.spans.borrow_mut();
        assert_eq!(spans.len(), offset as usize);
        spans.push(span);
        arena.alloc(SyntheticExpr {
            node: ExprNode {
                id: ExprId::from_raw(la_arena::RawIdx::from_u32(raw)),
                kind,
            },
        })
    }

    pub(super) fn clear(&mut self) {
        Rc::get_mut(&mut self.arena)
            .expect("no synthetic-node arena handle survives a file check")
            .reset();
        self.next_expr.set(0);
        self.spans.get_mut().clear();
    }

    pub(super) fn is_synthetic_id(id: ExprId) -> bool {
        let raw = u32::from(id.into_raw());
        (SYNTHETIC_EXPR_ID_START..=SYNTHETIC_EXPR_ID_END).contains(&raw)
    }

    pub(super) fn span(&self, id: ExprId) -> Option<Span> {
        let raw = u32::from(id.into_raw());
        let offset = raw.checked_sub(SYNTHETIC_EXPR_ID_START)?;
        self.spans.borrow().get(offset as usize).copied()
    }

    #[cfg(test)]
    fn owns(&self, id: ExprId) -> bool {
        let raw = u32::from(id.into_raw());
        raw >= SYNTHETIC_EXPR_ID_START && raw - SYNTHETIC_EXPR_ID_START < self.next_expr.get()
    }

    #[cfg(test)]
    fn allocated_bytes(&self) -> usize {
        self.arena.allocated_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span::new(0, 1)
    }

    #[test]
    fn synthetic_ids_are_unique_and_owned_by_the_current_file_scratch() {
        let mut scratch = SyntheticNodeScratch::default();
        let first = scratch
            .alloc_expr(ExprKind::Literal(syntax::ast::Literal::Bool(true)), span())
            .id;
        let second = scratch
            .alloc_expr(ExprKind::Literal(syntax::ast::Literal::Bool(false)), span())
            .id;

        assert_ne!(first, second);
        assert!(SyntheticNodeScratch::is_synthetic_id(first));
        assert!(scratch.owns(first));
        assert!(scratch.owns(second));

        scratch.clear();

        assert!(!scratch.owns(first));
        let next_file = scratch
            .alloc_expr(ExprKind::Literal(syntax::ast::Literal::Null), span())
            .id;
        assert_eq!(next_file, first);
        assert!(scratch.owns(next_file));
    }

    #[test]
    fn clearing_synthetic_nodes_keeps_bump_storage_available() {
        let mut scratch = SyntheticNodeScratch::default();
        for _ in 0..128 {
            scratch.alloc_expr(ExprKind::Literal(syntax::ast::Literal::Null), span());
        }
        let warmed = scratch.allocated_bytes();
        assert!(warmed > 0);

        scratch.clear();
        for _ in 0..128 {
            scratch.alloc_expr(ExprKind::Literal(syntax::ast::Literal::Null), span());
        }

        // `Bump::reset` can reserve a differently sized chunk on reuse;
        // the arena may grow, so its byte count is not an exact reuse
        // invariant. It must retain at least the storage already warmed.
        assert!(scratch.allocated_bytes() >= warmed);
    }
}
