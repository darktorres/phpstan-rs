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
use syntax::visit::{
    Visitor, walk_catch_clause, walk_class_def, walk_expr, walk_param, walk_property_decl,
    walk_stmt, walk_stmts,
};
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
    pub(crate) fn check_syntax_versions(&mut self, program: &[Stmt], marks: &SyntaxMarks) {
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
        walk_stmts(&mut Gates { checker: self }, program);
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
        self.report_builder(
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
    fn initializer(&mut self, value: &Expr) {
        if let Some(span) = new_in(value) {
            self.report(
                PhpFeature::NewInInitializers,
                "New in initializers is",
                span,
                "new.inInitializerNotSupported",
            );
        }
    }

    /// Member access on `new` without its parentheses (`new A()->f()`).
    fn receiver(&mut self, receiver: &Expr) {
        if matches!(
            receiver.kind,
            ExprKind::New(..) | ExprKind::NewAnonymousClass(_)
        ) {
            self.report(
                PhpFeature::NewWithoutParentheses,
                "Member access on new without parentheses is",
                receiver.span,
                "new.withoutParenthesesNotSupported",
            );
        }
    }
}

impl<'ast> Visitor<'ast> for Gates<'_, '_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::StaticVar(vars) => {
                for value in vars.iter().filter_map(|(_, value)| value.as_ref()) {
                    self.initializer(value);
                }
            }
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.initializer(&c.value);
                }
            }
            StmtKind::ClassDecl(class) if class.def.kind == ClassKind::Enum => {
                self.report(
                    PhpFeature::Enums,
                    "Enums are",
                    class.name.span,
                    "enum.notSupported",
                );
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_class_def(&mut self, class: &'ast ClassDef) {
        walk_class_def(self, class);
    }

    fn visit_property_decl(&mut self, property: &'ast PropertyDecl) {
        if let Some(hint) = &property.type_hint {
            self.report(
                PhpFeature::TypedProperties,
                "Typed properties are",
                hint.span,
                "property.typeNotSupported",
            );
        }
        walk_property_decl(self, property);
    }

    fn visit_param(&mut self, param: &'ast Param) {
        if let Some(default) = &param.default {
            self.initializer(default);
        }
        walk_param(self, param);
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        for arg in &attribute.args {
            self.initializer(&arg.value);
            self.visit_expr(&arg.value);
        }
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
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
        walk_catch_clause(self, clause);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        let span = expr.span;
        match &expr.kind {
            ExprKind::Destructure(items, syntax) => {
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
            ExprKind::ArrayLiteral(items) if items.iter().any(|item| item.spread) => {
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
                self.receiver(receiver);
            }
            ExprKind::Index(receiver, _) => self.receiver(receiver),
            ExprKind::ClassConstFetch(StaticTarget::Expr(receiver), _)
            | ExprKind::StaticPropertyFetch(StaticTarget::Expr(receiver), _)
            | ExprKind::StaticCall(StaticTarget::Expr(receiver), _, _) => self.receiver(receiver),
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// Where `value` makes an object with `new`, outside the closures in it.
fn new_in(value: &Expr) -> Option<Span> {
    struct Find(Option<Span>);
    impl<'ast> Visitor<'ast> for Find {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if self.0.is_some() {
                return;
            }
            match &expr.kind {
                ExprKind::New(..) | ExprKind::NewAnonymousClass(_) => self.0 = Some(expr.span),
                ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {}
                _ => walk_expr(self, expr),
            }
        }
    }
    let mut find = Find(None);
    find.visit_expr(value);
    find.0
}
