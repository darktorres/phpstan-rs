//! The syntax a PHP version before the project's lowest does not have:
//! each construct is reported where the file spells it, when some version
//! the project runs on lacks it. PHP rejects the file there, or, for a
//! type keyword, reads it as a class name (`checker::type_declarations`).
//! What the tree does not keep - trailing commas, number spellings, a
//! heredoc's closing label, a constant's written visibility - the parse
//! records the first place of (`syntax::SyntaxMarks`).

use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::arena::{self, Visitor};
use syntax::{SyntaxMark, SyntaxMarks};
use types::php_version::PhpFeature;

use crate::checker::Checker;

/// The syntax that is spelled out and not kept in the tree, with its
/// feature, report subject and identifier.
const MARKS: &[(SyntaxMark, PhpFeature, &str, &str)] = &[
    (
        SyntaxMark::ClassConstVisibility,
        PhpFeature::ClassConstantVisibility,
        "Class constant visibility is",
        "classConstant.visibilityNotSupported",
    ),
    (
        SyntaxMark::TrailingCommaInGroupedUse,
        PhpFeature::TrailingCommaInGroupedUse,
        "A trailing comma in a grouped use is",
        "use.trailingCommaNotSupported",
    ),
    (
        SyntaxMark::TrailingCommaInArguments,
        PhpFeature::TrailingCommaInArguments,
        "A trailing comma in arguments is",
        "arguments.trailingCommaNotSupported",
    ),
    (
        SyntaxMark::FlexibleHeredoc,
        PhpFeature::FlexibleHeredoc,
        "A heredoc's closing label indented or followed on its line is",
        "heredoc.flexibleNotSupported",
    ),
    (
        SyntaxMark::NumericLiteralSeparator,
        PhpFeature::NumericLiteralSeparator,
        "A numeric literal separator is",
        "number.separatorNotSupported",
    ),
    (
        SyntaxMark::TrailingCommaInParameters,
        PhpFeature::TrailingCommaInParameters,
        "A trailing comma in parameters is",
        "parameters.trailingCommaNotSupported",
    ),
    (
        SyntaxMark::TrailingCommaInClosureUse,
        PhpFeature::TrailingCommaInClosureUse,
        "A trailing comma in a closure's use list is",
        "closureUse.trailingCommaNotSupported",
    ),
    (
        SyntaxMark::ExplicitOctal,
        PhpFeature::ExplicitOctal,
        "Explicit octal notation is",
        "number.explicitOctalNotSupported",
    ),
];

impl<'a> Checker<'a> {
    /// Reports the syntax of `program` and of `marks` that a PHP version
    /// the project runs on lacks.
    pub(crate) fn check_syntax_versions(
        &mut self,
        arenas: &NodeArenas,
        program: &[StmtId],
        marks: &SyntaxMarks,
    ) {
        // Every version the project runs on has every syntax gated here.
        if self
            .registry
            .php_supports_everywhere(PhpFeature::PipeOperator)
            != Some(false)
        {
            return;
        }
        for &(mark, feature, what, identifier) in MARKS {
            if let Some(span) = marks.first(mark) {
                self.report_unsupported_syntax(feature, what, span, identifier);
            }
        }
        arena::walk_stmts(&mut Gates { checker: self }, arenas, program);
    }

    /// Reports `what` (its subject and verb, "Enums are") at `span` when a
    /// version the project runs on lacks `feature`.
    fn report_unsupported_syntax(
        &mut self,
        feature: PhpFeature,
        what: &str,
        span: Span,
        identifier: &str,
    ) {
        if self.registry.php_supports_everywhere(feature) != Some(false) {
            return;
        }
        report_builder!(
            self,
            DiagnosticBuilder::new(
                format!(
                    "{what} supported only on PHP {} and later.",
                    feature.threshold()
                ),
                span,
                0,
                identifier.to_string(),
            )
            .non_ignorable(),
        );
    }
}

/// The walk for the syntax the tree keeps.
struct Gates<'c, 'a> {
    checker: &'c mut Checker<'a>,
}

impl Gates<'_, '_> {
    fn report(&mut self, feature: PhpFeature, what: &str, span: Span, identifier: &str) {
        self.checker
            .report_unsupported_syntax(feature, what, span, identifier);
    }

    /// `new` in an initializer, which PHP takes only from 8.1.
    fn initializer(&mut self, arenas: &NodeArenas, value: ExprId) {
        if let Some(span) = new_in(arenas, value) {
            self.report(
                PhpFeature::NewInInitializers,
                "New in initializers is",
                span,
                "new.inInitializerNotSupported",
            );
        }
    }

    /// Member access on `new` without its parentheses (`new A()->f()`).
    fn receiver(&mut self, arenas: &NodeArenas, receiver: ExprId) {
        if matches!(
            arenas.expr(receiver).map(|expr| &expr.kind),
            Some(ExprKind::New(..) | ExprKind::NewAnonymousClass(_))
        ) {
            self.report(
                PhpFeature::NewWithoutParentheses,
                "Member access on new without parentheses is",
                arenas.expr_span(receiver).unwrap_or_default(),
                "new.withoutParenthesesNotSupported",
            );
        }
    }
}

impl Visitor for Gates<'_, '_> {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else {
            return;
        };
        match &stmt.kind {
            StmtKind::StaticVar(vars) => {
                for value in arenas
                    .static_vars(vars.clone())
                    .iter()
                    .filter_map(|variable| variable.default)
                {
                    self.initializer(arenas, value);
                }
            }
            StmtKind::GlobalConst(consts) => {
                for c in arenas.class_consts(consts) {
                    self.initializer(arenas, c.value);
                }
            }
            StmtKind::ClassDecl(class)
                if arenas
                    .class(arenas.class_decl(class.clone()).def)
                    .is_some_and(|def| def.kind == ClassKind::Enum) =>
            {
                self.report(
                    PhpFeature::Enums,
                    "Enums are",
                    arenas.class_decl(class.clone()).name.span,
                    "enum.notSupported",
                );
            }
            _ => {}
        }
        arena::walk_stmt(self, arenas, id);
    }

    fn visit_class_def(&mut self, arenas: &NodeArenas, class: &ClassDef) {
        arena::walk_class_def(self, arenas, class);
    }

    fn visit_property_decl(&mut self, arenas: &NodeArenas, property: &PropertyDecl) {
        if let Some(hint) = &property.type_hint {
            self.report(
                PhpFeature::TypedProperties,
                "Typed properties are",
                hint.span,
                "property.typeNotSupported",
            );
        }
        arena::walk_property_decl(self, arenas, property);
    }

    fn visit_param(&mut self, arenas: &NodeArenas, param: &Param) {
        if let Some(default) = param.default {
            self.initializer(arenas, default);
        }
        arena::walk_param(self, arenas, param);
    }

    fn visit_attribute(&mut self, arenas: &NodeArenas, attribute: &Attribute) {
        for arg in arenas.args(attribute.args.clone()) {
            self.initializer(arenas, arg.value);
            self.visit_expr(arenas, arg.value);
        }
    }

    fn visit_catch_clause(&mut self, arenas: &NodeArenas, clause: &CatchClause) {
        if clause.types.len() > 1 {
            self.report(
                PhpFeature::MultiCatch,
                "Catching multiple exception types is",
                clause.span,
                "catch.multipleNotSupported",
            );
        }
        if clause.var.is_none() {
            self.report(
                PhpFeature::NoncapturingCatch,
                "Non-capturing catch is",
                clause.span,
                "catch.nonCapturingNotSupported",
            );
        }
        arena::walk_catch_clause(self, arenas, clause);
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else {
            return;
        };
        let span = arenas.expr_span(expr.id).unwrap_or_default();
        match &expr.kind {
            ExprKind::Destructure(items, syntax) => {
                let items = arenas.destructure_items(items);
                if *syntax == DestructureSyntax::Short {
                    self.report(
                        PhpFeature::ShortListSyntax,
                        "Destructuring with [] is",
                        span,
                        "list.shortSyntaxNotSupported",
                    );
                }
                if items.iter().any(|item| item.key.is_some()) {
                    self.report(
                        PhpFeature::KeyedList,
                        "Keys in a destructuring are",
                        span,
                        "list.keysNotSupported",
                    );
                }
                if items.iter().any(|item| item.by_ref) {
                    self.report(
                        PhpFeature::ListReferenceAssignment,
                        "Destructuring by reference is",
                        span,
                        "list.referenceNotSupported",
                    );
                }
            }
            ExprKind::ArrayLiteral(items)
                if arenas
                    .array_items(items.clone())
                    .iter()
                    .any(|item| item.spread) =>
            {
                self.report(
                    PhpFeature::SpreadInArrays,
                    "Spreading into an array is",
                    span,
                    "array.spreadNotSupported",
                );
            }
            ExprKind::CompoundAssign(BinOp::Coalesce, ..) => {
                self.report(
                    PhpFeature::NullCoalescingAssignment,
                    "The ??= operator is",
                    span,
                    "assignOp.nullCoalesceNotSupported",
                );
            }
            ExprKind::ArrowFn(_) => {
                self.report(
                    PhpFeature::ArrowFunctions,
                    "Arrow functions are",
                    span,
                    "arrowFunction.notSupported",
                );
            }
            ExprKind::Match { .. } => {
                self.report(
                    PhpFeature::MatchExpression,
                    "Match expressions are",
                    span,
                    "match.notSupported",
                );
            }
            ExprKind::Throw(_) => {
                self.report(
                    PhpFeature::ThrowExpression,
                    "Throw expression is",
                    span,
                    "throw.notSupported",
                );
            }
            ExprKind::ClassNameFetch(StaticTarget::Expr(_)) => {
                self.report(
                    PhpFeature::ClassNameOnObject,
                    "Fetching ::class on an object is",
                    span,
                    "classConstant.classOnObjectNotSupported",
                );
            }
            ExprKind::Binary(BinOp::Pipe, ..) => {
                self.report(
                    PhpFeature::PipeOperator,
                    "The pipe operator is",
                    span,
                    "pipe.notSupported",
                );
            }
            ExprKind::Clone(_, Some(_)) => {
                self.report(
                    PhpFeature::CloneWith,
                    "Clone with properties is",
                    span,
                    "clone.withNotSupported",
                );
            }
            _ => {}
        }
        match &expr.kind {
            ExprKind::PropertyAccess(receiver, _, operator)
            | ExprKind::MethodCall(receiver, _, _, operator)
            | ExprKind::DynamicPropertyAccess(receiver, _, operator)
            | ExprKind::DynamicMethodCall(receiver, _, _, operator) => {
                if *operator == ObjectOperator::NullsafeArrow {
                    self.report(
                        PhpFeature::NullsafeOperator,
                        "The nullsafe operator is",
                        span,
                        "nullsafe.notSupported",
                    );
                }
                self.receiver(arenas, *receiver);
            }
            ExprKind::Index(receiver, _) => self.receiver(arenas, *receiver),
            ExprKind::ClassConstFetch(StaticTarget::Expr(receiver), _)
            | ExprKind::StaticPropertyFetch(StaticTarget::Expr(receiver), _)
            | ExprKind::StaticCall(StaticTarget::Expr(receiver), _, _) => {
                self.receiver(arenas, *receiver)
            }
            _ => {}
        }
        arena::walk_expr(self, arenas, id);
    }
}

/// Where `value` makes an object with `new`, outside the closures in it.
fn new_in(arenas: &NodeArenas, value: ExprId) -> Option<Span> {
    struct Find(Option<Span>);
    impl Visitor for Find {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            if self.0.is_some() {
                return;
            }
            let Some(expr) = arenas.expr(id) else {
                return;
            };
            match &expr.kind {
                ExprKind::New(..) | ExprKind::NewAnonymousClass(_) => self.0 = arenas.expr_span(id),
                ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {}
                _ => arena::walk_expr(self, arenas, id),
            }
        }
    }
    let mut find = Find(None);
    find.visit_expr(arenas, value);
    find.0
}
