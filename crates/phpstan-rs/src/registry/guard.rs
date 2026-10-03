//! What a conditional declaration is guarded by, and whether the PHP
//! versions a project runs on let that guard hold.
//!
//! A polyfill declares `Stringable` only `if (\PHP_VERSION_ID < 80000)`; a
//! project that needs PHP 8.0 never runs it. The guard is kept symbolic with
//! the declaration, so a change of the configured versions evaluates it again
//! without a parse.

use syntax::ast::{BinOp, Expr, ExprKind, Literal, UnaryOp};
use syntax::declarations::GuardTerm;
use types::PhpVersions;

/// A comparison a guard makes of `PHP_VERSION_ID`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Compare {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl Compare {
    /// The operator of `n <op> PHP_VERSION_ID` as one of `PHP_VERSION_ID
    /// <op> n`.
    fn flipped(self) -> Compare {
        match self {
            Compare::Lt => Compare::Gt,
            Compare::Le => Compare::Ge,
            Compare::Gt => Compare::Lt,
            Compare::Ge => Compare::Le,
            Compare::Eq | Compare::Ne => self,
        }
    }

    fn of_binary(op: BinOp) -> Option<Compare> {
        Some(match op {
            BinOp::Lt => Compare::Lt,
            BinOp::LtEq => Compare::Le,
            BinOp::Gt => Compare::Gt,
            BinOp::GtEq => Compare::Ge,
            BinOp::Eq | BinOp::Identical => Compare::Eq,
            BinOp::NotEq | BinOp::NotIdentical => Compare::Ne,
            _ => return None,
        })
    }

    /// The operator `version_compare` takes as text.
    fn of_text(text: &[u8]) -> Option<Compare> {
        Some(match text {
            b"<" | b"lt" => Compare::Lt,
            b"<=" | b"le" => Compare::Le,
            b">" | b"gt" => Compare::Gt,
            b">=" | b"ge" => Compare::Ge,
            b"==" | b"eq" => Compare::Eq,
            b"!=" | b"<>" | b"ne" => Compare::Ne,
            _ => return None,
        })
    }
}

/// A declaration's guard, as the condition it is nested under says it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GuardExpr {
    /// Nothing known: a loop, a `function_exists()` test, any other code -
    /// `if (false)` too, which is how a package declares a class for the
    /// tools that read its source and never for PHP to run.
    Opaque,
    /// `PHP_VERSION_ID <op> n`.
    VersionId(Compare, u32),
    Not(Box<GuardExpr>),
    And(Vec<GuardExpr>),
    Or(Vec<GuardExpr>),
}

/// Whether a guard holds on the versions a project runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Guard {
    /// On none of them: the declaration never runs.
    Never,
    /// On all of them: it runs.
    Always,
    Maybe,
}

impl Guard {
    fn negated(self) -> Guard {
        match self {
            Guard::Never => Guard::Always,
            Guard::Always => Guard::Never,
            Guard::Maybe => Guard::Maybe,
        }
    }
}

impl GuardExpr {
    /// The guard `terms` make together, `None` for no terms.
    pub(crate) fn of_terms(terms: &[GuardTerm<'_>]) -> Option<GuardExpr> {
        if terms.is_empty() {
            return None;
        }
        let mut all: Vec<GuardExpr> = terms
            .iter()
            .map(|term| match *term {
                GuardTerm::Opaque => GuardExpr::Opaque,
                GuardTerm::Condition { expr, negated } => {
                    let guard = GuardExpr::of_condition(expr);
                    if negated {
                        GuardExpr::Not(Box::new(guard))
                    } else {
                        guard
                    }
                }
            })
            .collect();
        Some(if all.len() == 1 {
            all.remove(0)
        } else {
            GuardExpr::And(all)
        })
    }

    /// What `expr`, an `if` condition, says about the versions.
    fn of_condition(expr: &Expr) -> GuardExpr {
        match &expr.kind {
            ExprKind::Unary(UnaryOp::Not, inner) => {
                GuardExpr::Not(Box::new(GuardExpr::of_condition(inner)))
            }
            ExprKind::Binary(BinOp::And | BinOp::LogicalAnd, left, right) => GuardExpr::And(vec![
                GuardExpr::of_condition(left),
                GuardExpr::of_condition(right),
            ]),
            ExprKind::Binary(BinOp::Or | BinOp::LogicalOr, left, right) => GuardExpr::Or(vec![
                GuardExpr::of_condition(left),
                GuardExpr::of_condition(right),
            ]),
            ExprKind::Binary(op, left, right) => Compare::of_binary(*op)
                .and_then(
                    |compare| match (is_constant(left, "PHP_VERSION_ID"), int(left)) {
                        (true, _) => int(right).map(|n| (compare, n)),
                        (_, Some(n)) if is_constant(right, "PHP_VERSION_ID") => {
                            Some((compare.flipped(), n))
                        }
                        _ => None,
                    },
                )
                .and_then(|(compare, n)| u32::try_from(n).ok().map(|n| (compare, n)))
                .map_or(GuardExpr::Opaque, |(compare, n)| {
                    GuardExpr::VersionId(compare, n)
                }),
            ExprKind::Call(callee, args) => version_compare(callee, args),
            _ => GuardExpr::Opaque,
        }
    }

    /// Whether this guard holds on `versions`; with none configured, nothing
    /// about the versions is known.
    pub(crate) fn on(&self, versions: Option<PhpVersions>) -> Guard {
        match self {
            GuardExpr::Opaque => Guard::Maybe,
            GuardExpr::VersionId(compare, n) => versions.map_or(Guard::Maybe, |versions| {
                // The ids a version runs as: its first release to its
                // last patch.
                let (lo, hi) = (versions.min.id(), versions.max.id() + 99);
                let (n, outside) = (*n, n < &lo || n > &hi);
                match compare {
                    Compare::Lt if hi < n => Guard::Always,
                    Compare::Lt if lo >= n => Guard::Never,
                    Compare::Le if hi <= n => Guard::Always,
                    Compare::Le if lo > n => Guard::Never,
                    Compare::Gt if lo > n => Guard::Always,
                    Compare::Gt if hi <= n => Guard::Never,
                    Compare::Ge if lo >= n => Guard::Always,
                    Compare::Ge if hi < n => Guard::Never,
                    Compare::Eq if outside => Guard::Never,
                    Compare::Ne if outside => Guard::Always,
                    _ => Guard::Maybe,
                }
            }),
            GuardExpr::Not(inner) => inner.on(versions).negated(),
            GuardExpr::And(all) => {
                let holds: Vec<Guard> = all.iter().map(|g| g.on(versions)).collect();
                if holds.contains(&Guard::Never) {
                    Guard::Never
                } else if holds.iter().all(|g| *g == Guard::Always) {
                    Guard::Always
                } else {
                    Guard::Maybe
                }
            }
            GuardExpr::Or(any) => {
                let holds: Vec<Guard> = any.iter().map(|g| g.on(versions)).collect();
                if holds.contains(&Guard::Always) {
                    Guard::Always
                } else if holds.iter().all(|g| *g == Guard::Never) {
                    Guard::Never
                } else {
                    Guard::Maybe
                }
            }
        }
    }
}

/// Whether `expr` is the global constant `name`.
fn is_constant(expr: &Expr, name: &str) -> bool {
    let ExprKind::Name(written) = &expr.kind else {
        return false;
    };
    let (primary, fallback) = written.candidates();
    fallback.unwrap_or(primary) == name
}

fn int(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Literal(Literal::Int(n)) => Some(*n),
        _ => None,
    }
}

/// `version_compare(PHP_VERSION, 'x.y.z', '<op>')`, as the comparison of
/// `PHP_VERSION_ID` with the version's id. A call that has no operator
/// returns a number, not a condition.
fn version_compare(callee: &Expr, args: &[syntax::ast::Arg]) -> GuardExpr {
    let ExprKind::Name(name) = &callee.kind else {
        return GuardExpr::Opaque;
    };
    let (primary, fallback) = name.candidates();
    if !fallback
        .unwrap_or(primary)
        .eq_ignore_ascii_case("version_compare")
        || args.len() != 3
        || args.iter().any(|a| a.spread || a.name.is_some())
    {
        return GuardExpr::Opaque;
    }
    let (Some(version), Some(compare)) = (text(&args[1].value), text(&args[2].value)) else {
        return GuardExpr::Opaque;
    };
    if !is_constant(&args[0].value, "PHP_VERSION") {
        return GuardExpr::Opaque;
    }
    match (version_id(version), Compare::of_text(compare)) {
        (Some(id), Some(compare)) => GuardExpr::VersionId(compare, id),
        _ => GuardExpr::Opaque,
    }
}

fn text(expr: &Expr) -> Option<&[u8]> {
    match &expr.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.as_bytes()),
        _ => None,
    }
}

/// The `PHP_VERSION_ID` of a plain `major[.minor[.patch]]` version text.
fn version_id(version: &[u8]) -> Option<u32> {
    let version = std::str::from_utf8(version).ok()?;
    let mut parts = version.split('.');
    let mut next = |scale: u32| match parts.next() {
        Some(part) => part
            .parse::<u32>()
            .ok()
            .filter(|n| *n < 100)
            .map(|n| n * scale),
        None => Some(0),
    };
    let id = next(10_000)? + next(100)? + next(1)?;
    parts.next().is_none().then_some(id)
}
