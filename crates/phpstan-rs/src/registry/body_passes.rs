//! Walks a body once for several passes. A pass says, at each node, which
//! children it wants; a child is visited when some pass wants it, and each
//! pass sees only the nodes it wants, in the order a walk of its own would
//! give.

use smallvec::SmallVec;
use syntax::ast::*;
use syntax::visit::arena::{self, Visitor};

/// The children of a node a pass wants visited.
pub(crate) enum Wants {
    /// None: the pass handles the node and what is below it itself.
    None,
    /// Every child the walk visits.
    All,
    /// Only these expressions, which are among the children the walk
    /// visits, in the order it visits them.
    Exprs(SmallVec<[ExprId; 4]>),
}

impl Wants {
    fn includes(&self, id: ExprId) -> bool {
        match self {
            Wants::None => false,
            Wants::All => true,
            Wants::Exprs(wanted) => wanted.contains(&id),
        }
    }

    fn is_none(&self) -> bool {
        match self {
            Wants::None => true,
            Wants::All => false,
            Wants::Exprs(wanted) => wanted.is_empty(),
        }
    }
}

/// One pass over a body.
pub(crate) trait Pass {
    fn stmt(&mut self, _arenas: &NodeArenas, _id: StmtId) -> Wants {
        Wants::All
    }

    fn expr(&mut self, _arenas: &NodeArenas, _id: ExprId) -> Wants {
        Wants::All
    }

    /// The children of the expression `id` have been visited.
    fn leave_expr(&mut self, _id: ExprId) {}

    fn catch_clause(&mut self, _clause: &CatchClause) {}
}

/// What each pass wants of the children of the node being walked.
struct Frame {
    first: Wants,
    second: Wants,
}

/// Walks a body for `first` and `second`. A pass that is `None` is left
/// out.
pub(crate) struct Fanout<'p, A: Pass, B: Pass> {
    first: Option<&'p mut A>,
    second: Option<&'p mut B>,
    frame: Frame,
}

impl<'p, A: Pass, B: Pass> Fanout<'p, A, B> {
    pub(crate) fn new(first: Option<&'p mut A>, second: Option<&'p mut B>) -> Self {
        let wants = |present: bool| if present { Wants::All } else { Wants::None };
        let frame = Frame {
            first: wants(first.is_some()),
            second: wants(second.is_some()),
        };
        Fanout {
            first,
            second,
            frame,
        }
    }

    pub(crate) fn walk(&mut self, arenas: &NodeArenas, body: &[StmtId]) {
        arena::walk_stmts(self, arenas, body);
    }

    /// The passes that want the expression `id`, a child of the node being
    /// walked.
    fn wanted_expr(&self, id: ExprId) -> (bool, bool) {
        (
            self.frame.first.includes(id),
            self.frame.second.includes(id),
        )
    }

    /// The passes that want every child of the node being walked.
    fn wanted_all(&self) -> (bool, bool) {
        (
            matches!(self.frame.first, Wants::All),
            matches!(self.frame.second, Wants::All),
        )
    }

    fn descend(&mut self, wants: Frame, walk: impl FnOnce(&mut Self)) {
        if wants.first.is_none() && wants.second.is_none() {
            return;
        }
        let outer = std::mem::replace(&mut self.frame, wants);
        walk(self);
        self.frame = outer;
    }
}

impl<A: Pass, B: Pass> Visitor for Fanout<'_, A, B> {
    fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}

    fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}

    fn visit_catch_clause(&mut self, arenas: &NodeArenas, clause: &CatchClause) {
        let (first, second) = self.wanted_all();
        if first && let Some(pass) = self.first.as_mut() {
            pass.catch_clause(clause);
        }
        if second && let Some(pass) = self.second.as_mut() {
            pass.catch_clause(clause);
        }
        arena::walk_stmts(self, arenas, arenas.stmt_ids(&clause.body));
    }

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let (first, second) = self.wanted_all();
        let wants = Frame {
            first: match self.first.as_mut() {
                Some(pass) if first => pass.stmt(arenas, id),
                _ => Wants::None,
            },
            second: match self.second.as_mut() {
                Some(pass) if second => pass.stmt(arenas, id),
                _ => Wants::None,
            },
        };
        self.descend(wants, |this| arena::walk_stmt(this, arenas, id));
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let (first, second) = self.wanted_expr(id);
        let wants = Frame {
            first: match self.first.as_mut() {
                Some(pass) if first => pass.expr(arenas, id),
                _ => Wants::None,
            },
            second: match self.second.as_mut() {
                Some(pass) if second => pass.expr(arenas, id),
                _ => Wants::None,
            },
        };
        syntax::stack::guard(|| {
            self.descend(wants, |this| arena::walk_expr(this, arenas, id));
        });
        if first && let Some(pass) = self.first.as_mut() {
            pass.leave_expr(id);
        }
        if second && let Some(pass) = self.second.as_mut() {
            pass.leave_expr(id);
        }
    }
}

/// What a body does with its parameters and what it returns, from one walk.
/// `yields` is whether `body` has a `yield`.
pub(crate) fn summarize(
    arenas: &NodeArenas,
    params: &[Param],
    body: &[StmtId],
    yields: bool,
) -> (
    Option<Box<crate::registry::retention::ParamUses>>,
    Option<Box<crate::registry::fresh::FreshReturns>>,
) {
    use crate::registry::fresh::Returns;
    use crate::registry::retention::Uses;

    let mut uses = Uses::new(params, yields);
    let mut returns = (!yields).then(|| Returns::new(params));
    Fanout::new(Some(&mut uses), returns.as_mut()).walk(arenas, body);
    let parameters = uses.finish(params);
    let fresh = returns.and_then(Returns::finish);
    #[cfg(debug_assertions)]
    {
        use crate::registry::fresh::FreshReturns;
        use crate::registry::retention::ParamUses;
        debug_assert_eq!(
            format!("{parameters:?}"),
            format!("{:?}", ParamUses::reference(arenas, params, body, yields))
        );
        debug_assert_eq!(
            format!("{fresh:?}"),
            format!(
                "{:?}",
                FreshReturns::reference(arenas, params, body, yields)
            )
        );
    }
    (parameters, fresh)
}
