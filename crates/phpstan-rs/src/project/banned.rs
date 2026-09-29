//! `Project::check_banned`: the configured `banned_constructs` and
//! `banned_types` against every analysed file. A banned type is found in
//! the PHPDoc-refined types of functions, methods and properties, in class
//! constants' native types, in closures' and arrow functions' native
//! signatures, and - for `float` - in float literals and `(float)` casts.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use crate::banned::BannedConstruct;
use crate::registry::Registry;
use crate::registry::{class_decl_fqn, class_decl_name};
use syntax::ast::*;
use syntax::token::{Position, Span};
use syntax::visit::{
    Visitor, walk_attribute, walk_class_const, walk_expr, walk_method_decl, walk_param,
    walk_property_decl, walk_stmt,
};
use types::Type;

use super::Project;
use diagnostics::{Diagnostic, DiagnosticBuilder};

const SUPERGLOBALS: [&str; 9] = [
    "GLOBALS", "_SERVER", "_GET", "_POST", "_FILES", "_COOKIE", "_SESSION", "_REQUEST", "_ENV",
];

struct Walker<'a> {
    constructs: &'a FxHashSet<BannedConstruct>,
    types: &'a [String],
    registry: &'a Registry,
    /// The classes whose declarations enclose the walk, innermost last.
    classes: Vec<String>,
    out: Vec<Diagnostic>,
}

impl<'a> Walker<'a> {
    fn ban(&mut self, construct: BannedConstruct, span: Span, what: String) {
        if self.constructs.contains(&construct) {
            let mut builder = DiagnosticBuilder::new(
                format!("{what} is banned by this project's configuration."),
                span,
                0,
                construct.identifier(),
            );
            if construct == BannedConstruct::IgnoreComments {
                builder = builder.non_ignorable();
            }
            self.out.push(builder.build());
        }
    }

    fn ban_type(&mut self, span: Span, subject: String, ty: &Type) {
        for name in self.types {
            if ty.mentions(name) {
                self.out.push(
                    DiagnosticBuilder::new(
                        format!("{subject} uses banned type {name}."),
                        span,
                        0,
                        "banned.type",
                    )
                    .build(),
                );
            }
        }
    }

    fn float_banned(&self) -> bool {
        self.types.iter().any(|t| t == "float")
    }

    fn by_ref_return(&mut self, span: Span, name: &str) {
        self.ban(
            BannedConstruct::ByReferenceReturns,
            span,
            format!("Returning by reference from {name}"),
        );
    }

    fn signature_types(
        &mut self,
        sig: &crate::registry::FunctionReflection,
        decl: &FunctionDecl,
        what: &str,
    ) {
        for (param, p) in sig.params().iter().zip(&decl.params) {
            self.ban_type(
                p.span,
                format!("Parameter ${} of {what}", p.name),
                param.ty(),
            );
        }
        self.ban_type(
            decl.return_type
                .as_ref()
                .map(|h| h.span)
                .unwrap_or(decl.name_span),
            format!("The return type of {what}"),
            sig.return_type(),
        );
    }

    /// A closure's or arrow function's native signature, which has no
    /// reflection to refine it.
    fn hint_types(&mut self, params: &[Param], return_type: Option<&TypeHint>, what: &str) {
        for p in params {
            if let Some(hint) = &p.type_hint {
                self.ban_hint(p.span, format!("Parameter ${} of {what}", p.name), hint);
            }
        }
        if let Some(hint) = return_type {
            self.ban_hint(hint.span, format!("The return type of {what}"), hint);
        }
    }

    fn ban_hint(&mut self, span: Span, subject: String, hint: &TypeHint) {
        for name in self.types {
            if hint_mentions(hint, name) {
                self.out.push(
                    DiagnosticBuilder::new(
                        format!("{subject} uses banned type {name}."),
                        span,
                        0,
                        "banned.type",
                    )
                    .build(),
                );
            }
        }
    }

    fn arguments(&mut self, args: &[Arg], span: Span) {
        if args.iter().any(|a| a.name.is_some()) {
            self.ban(
                BannedConstruct::NamedArguments,
                span,
                "A named argument".to_string(),
            );
        }
    }

    /// The class whose members are being walked.
    fn class(&self) -> Option<&str> {
        self.classes.last().map(String::as_str)
    }

    /// The constructs `e` itself is; its children are walked after.
    fn expr_itself(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Call(callee, args) => {
                if let ExprKind::Name(name) = &callee.kind {
                    let lower = name.trim_start_matches('\\').to_ascii_lowercase();
                    let construct = match lower.as_str() {
                        "compact" => Some(BannedConstruct::Compact),
                        "extract" => Some(BannedConstruct::Extract),
                        _ => None,
                    };
                    if let Some(construct) = construct {
                        self.ban(construct, e.span, format!("{lower}()"));
                    }
                }
                self.arguments(args, e.span);
            }
            ExprKind::Eval(_) => self.ban(BannedConstruct::Eval, e.span, "eval()".to_string()),
            // Reported at the `@` itself.
            ExprKind::ErrorSuppress(_) => {
                let at = Span {
                    start: e.span.start,
                    end: syntax::token::Position {
                        line: e.span.start.line,
                        col: e.span.start.col + 1,
                    },
                };
                self.ban(
                    BannedConstruct::ErrorSuppression,
                    at,
                    "The @ error-suppression operator".to_string(),
                );
            }
            ExprKind::Isset(_) => self.ban(BannedConstruct::Isset, e.span, "isset()".to_string()),
            ExprKind::Empty(_) => self.ban(BannedConstruct::Empty, e.span, "empty()".to_string()),
            ExprKind::Exit(kind, args) => {
                // Reported as written: bare `exit` or `exit()`.
                let keyword = kind.as_str();
                let shown = if args.is_none() {
                    keyword.to_string()
                } else {
                    format!("{keyword}()")
                };
                self.ban(BannedConstruct::Exit, e.span, shown);
            }
            ExprKind::New(_, args)
            | ExprKind::MethodCall(_, _, args, _)
            | ExprKind::StaticCall(_, _, args)
            | ExprKind::DynamicMethodCall(_, _, args)
            | ExprKind::DynamicStaticCall(_, _, args) => {
                self.arguments(args, e.span);
            }
            ExprKind::AssignRef(target, _) => {
                self.ban(
                    BannedConstruct::AssignByReference,
                    e.span,
                    "Assignment by reference".to_string(),
                );
                if matches!(target.kind, ExprKind::Index(_, None)) {
                    self.ban(
                        BannedConstruct::ArrayAppend,
                        target.span,
                        "Appending with []".to_string(),
                    );
                }
            }
            ExprKind::Assign(target, _) | ExprKind::CompoundAssign(_, target, _)
                if matches!(target.kind, ExprKind::Index(_, None)) =>
            {
                self.ban(
                    BannedConstruct::ArrayAppend,
                    target.span,
                    "Appending with []".to_string(),
                );
            }
            ExprKind::Ternary(_, None, _) => self.ban(
                BannedConstruct::ShortTernary,
                e.span,
                "The short ternary ?:".to_string(),
            ),
            ExprKind::Binary(op @ (BinOp::Eq | BinOp::NotEq), _, _) => {
                let symbol = if matches!(op, BinOp::Eq) { "==" } else { "!=" };
                self.ban(
                    BannedConstruct::LooseComparison,
                    e.span,
                    format!("Loose comparison {symbol}"),
                );
            }
            ExprKind::ShellExec(_) => self.ban(
                BannedConstruct::Backticks,
                e.span,
                "A backtick shell command".to_string(),
            ),
            ExprKind::Variable(name) if SUPERGLOBALS.contains(&name.as_ref()) => {
                self.ban(BannedConstruct::Superglobals, e.span, format!("${name}"));
            }
            ExprKind::DynamicVariable(_) => self.ban(
                BannedConstruct::VariableVariables,
                e.span,
                "A variable variable".to_string(),
            ),
            ExprKind::DynamicPropertyAccess(..)
            | ExprKind::DynamicClassConstFetch(..)
            | ExprKind::DynamicStaticPropertyFetch(..) => self.ban(
                BannedConstruct::DynamicMemberAccess,
                e.span,
                "A dynamic member name".to_string(),
            ),
            ExprKind::Literal(Literal::Float(f)) if self.float_banned() => {
                self.out.push(
                    DiagnosticBuilder::new(
                        format!("Float literal {f} uses banned type float."),
                        e.span,
                        0,
                        "banned.type",
                    )
                    .build(),
                );
            }
            ExprKind::Cast(cast, _) if cast.kind() == CastKind::Float && self.float_banned() => {
                self.out.push(
                    DiagnosticBuilder::new(
                        "A (float) cast uses banned type float.".to_string(),
                        e.span,
                        0,
                        "banned.type",
                    )
                    .build(),
                );
            }
            ExprKind::Closure {
                params,
                by_ref,
                return_type,
                ..
            } => {
                if *by_ref {
                    self.by_ref_return(e.span, "a closure");
                }
                self.hint_types(params, return_type.as_deref(), "a closure");
            }
            ExprKind::ArrowFn {
                params,
                by_ref,
                return_type,
                ..
            } => {
                if *by_ref {
                    self.by_ref_return(e.span, "an arrow function");
                }
                self.hint_types(params, return_type.as_deref(), "an arrow function");
            }
            ExprKind::NewAnonymousClass(_, args) => self.arguments(args, e.span),
            _ => {}
        }
        if let ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..) = &e.kind {
            self.ban(
                BannedConstruct::DynamicMemberAccess,
                e.span,
                "A dynamic member name".to_string(),
            );
        }
    }
}

/// Whether a native type hint names the banned type `name` (lowercase,
/// without a leading backslash).
fn hint_mentions(hint: &TypeHint, name: &str) -> bool {
    match &hint.kind {
        TypeHintKind::Keyword(keyword, _) => keyword.as_str() == name,
        TypeHintKind::Name(written) => written.trim_start_matches('\\').eq_ignore_ascii_case(name),
        TypeHintKind::Nullable(inner) => hint_mentions(inner, name),
        TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => {
            members.iter().any(|m| hint_mentions(m, name))
        }
    }
}

impl<'ast> Visitor<'ast> for Walker<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Switch { .. } => {
                self.ban(BannedConstruct::Switch, stmt.span, "switch".to_string())
            }
            StmtKind::Goto(label) => {
                self.ban(BannedConstruct::Goto, stmt.span, format!("goto {label}"))
            }
            StmtKind::Global(names) => self.ban(
                BannedConstruct::Global,
                stmt.span,
                format!(
                    "global {}",
                    names
                        .iter()
                        .map(|v| match v {
                            GlobalVar::Name(n) => format!("${n}"),
                            GlobalVar::Dynamic(e) => {
                                format!("${{{}}}", syntax::printer::print_expr(e))
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
            StmtKind::FunctionDecl(f) => {
                let fqn = f.fq_name().to_string();
                let what = format!("{fqn}()");
                if f.by_ref {
                    self.by_ref_return(f.name_span, &what);
                }
                if let Some(sig) = self.registry.lookup_function(&fqn) {
                    self.signature_types(sig, f, &what);
                }
            }
            StmtKind::ClassDecl(c) => {
                let fqn = class_decl_fqn(c).to_string();
                self.classes.push(fqn);
                walk_stmt(self, stmt);
                self.classes.pop();
                return;
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        self.expr_itself(e);
        if let ExprKind::NewAnonymousClass(decl, _) = &e.kind {
            self.classes.push(class_decl_name(decl).to_string());
            walk_expr(self, e);
            self.classes.pop();
        } else {
            walk_expr(self, e);
        }
    }

    fn visit_method_decl(&mut self, m: &'ast MethodDecl) {
        if let Some(fqn) = self.class().map(str::to_string) {
            let f = &m.function;
            let what = format!("{fqn}::{}()", f.name);
            if f.by_ref {
                self.by_ref_return(f.name_span, &what);
            }
            if let Some(sig) = self.registry.lookup_method(&fqn, &f.name) {
                self.signature_types(sig, f, &what);
            }
        }
        walk_method_decl(self, m);
    }

    fn visit_property_decl(&mut self, p: &'ast PropertyDecl) {
        if let Some(fqn) = self.class().map(str::to_string) {
            if let Some(prop) = self.registry.lookup_property(&fqn, &p.name) {
                self.ban_type(p.span, format!("Property {fqn}::${}", p.name), prop.ty());
            }
            if p.hooks.as_ref().is_some_and(|h| h.get_by_ref) {
                self.by_ref_return(p.span, &format!("{fqn}::${}'s get hook", p.name));
            }
        }
        walk_property_decl(self, p);
    }

    fn visit_class_const(&mut self, c: &'ast ClassConst) {
        if let Some(fqn) = self.class().map(str::to_string)
            && let Some(native) = self
                .registry
                .classes
                .get(fqn.as_str())
                .and_then(|i| i.consts().get(c.name.as_ref()))
                .and_then(|k| k.native_ty().as_ref())
        {
            let native = native.clone();
            self.ban_type(c.span, format!("Constant {fqn}::{}", c.name), &native);
        }
        walk_class_const(self, c);
    }

    fn visit_param(&mut self, p: &'ast Param) {
        if p.by_ref {
            self.ban(
                BannedConstruct::ByReferenceParameters,
                p.span,
                format!("By-reference parameter ${}", p.name),
            );
        }
        walk_param(self, p);
    }

    fn visit_attribute(&mut self, a: &'ast Attribute) {
        self.arguments(&a.args, a.span);
        walk_attribute(self, a);
    }
}

impl Project {
    /// Every banned construct/type in the analysed files, keyed by file.
    pub fn check_banned(
        &self,
        constructs: &[BannedConstruct],
        types: &[String],
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut out = FxHashMap::default();
        if constructs.is_empty() && types.is_empty() {
            return out;
        }
        let constructs: FxHashSet<BannedConstruct> = constructs.iter().copied().collect();
        let types: Vec<String> = types
            .iter()
            .map(|t| t.trim_start_matches('\\').to_ascii_lowercase())
            .collect();
        for (path, file) in &self.files {
            if !file.analyzed {
                continue;
            }
            let mut walker = Walker {
                constructs: &constructs,
                types: &types,
                registry: &self.registry,
                classes: Vec::new(),
                out: Vec::new(),
            };
            for stmt in &file.ast {
                walker.visit_stmt(stmt);
            }
            let mut ignore_lines: Vec<u32> = file
                .ignores
                .lines
                .iter()
                .chain(file.ignores.identifiers.keys())
                .copied()
                .collect();
            ignore_lines.sort_unstable();
            ignore_lines.dedup();
            for line in ignore_lines {
                let at = Position { line, col: 1 };
                walker.ban(
                    BannedConstruct::IgnoreComments,
                    Span { start: at, end: at },
                    "A @phpstan-ignore comment suppressing this line".to_string(),
                );
            }
            if !walker.out.is_empty() {
                let mut found = walker.out;
                found.sort_by_key(|d| (d.span.start.line, d.span.start.col));
                out.insert(path.clone(), found);
            }
        }
        out
    }
}
