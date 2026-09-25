//! `Project::check_banned`: the configured `banned_constructs` and
//! `banned_types` against every analysed file. A banned type is found in
//! the PHPDoc-refined types of functions, methods and properties, in class
//! constants' native types, and - for `float` - in float literals and
//! `(float)` casts.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use crate::banned::BannedConstruct;
use crate::registry::Registry;
use syntax::ast::*;
use syntax::token::{Position, Span};
use syntax::visit::{child_exprs, child_stmts, stmt_exprs};
use types::Type;

use super::Project;
use crate::diagnostic::{Diagnostic, DiagnosticBuilder};
use syntax::name_resolution::NameContext;

const SUPERGLOBALS: [&str; 9] = [
    "GLOBALS", "_SERVER", "_GET", "_POST", "_FILES", "_COOKIE", "_SESSION", "_REQUEST", "_ENV",
];

/// Whether `ty` mentions banned type `name` (a keyword from
/// `banned::BANNABLE_KEYWORD_TYPES`, or a class name).
fn type_mentions(ty: &Type, name: &str) -> bool {
    let direct = match (name, ty) {
        ("float", Type::Float | Type::FloatLiteral(_)) => true,
        ("int", Type::Int | Type::IntLiteral(_) | Type::IntRange(..)) => true,
        (
            "string",
            Type::String | Type::StringLiteral(_) | Type::ClassString(_) | Type::ClassKindString(_),
        ) => true,
        ("bool", Type::Bool | Type::True | Type::False) => true,
        ("array", Type::Array(..) | Type::ArrayShape(_)) => true,
        ("mixed", Type::Mixed(_)) => true,
        ("object", Type::ObjectWithoutClass) => true,
        ("callable", Type::Callable(_)) => true,
        ("iterable", Type::Object(n, _)) if n.eq_ignore_ascii_case("Traversable") => true,
        (class, Type::Object(n, _)) => n.trim_start_matches('\\').eq_ignore_ascii_case(class),
        _ => false,
    };
    direct || ty.any_child(|t| type_mentions(t, name))
}

struct Walker<'a> {
    constructs: &'a FxHashSet<BannedConstruct>,
    types: &'a [String],
    registry: &'a Registry,
    out: Vec<Diagnostic>,
}

impl<'a> Walker<'a> {
    fn ban(&mut self, construct: BannedConstruct, span: Span, what: String) {
        if self.constructs.contains(&construct) {
            let mut builder = DiagnosticBuilder::new(
                "<main>".to_string(),
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
            if type_mentions(ty, name) {
                self.out.push(
                    DiagnosticBuilder::new(
                        "<main>".to_string(),
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

    fn function_like(&mut self, params: &[Param], by_ref: bool, span: Span, name: &str) {
        for p in params {
            if p.by_ref {
                self.ban(
                    BannedConstruct::ByReferenceParameters,
                    p.span,
                    format!("By-reference parameter ${}", p.name),
                );
            }
            if let Some(default) = &p.default {
                self.expr(default);
            }
        }
        if by_ref {
            self.ban(
                BannedConstruct::ByReferenceReturns,
                span,
                format!("Returning by reference from {name}"),
            );
        }
    }

    fn signature_types(
        &mut self,
        sig: &crate::registry::FunctionSig,
        decl: &FunctionDecl,
        what: &str,
    ) {
        for (param, p) in sig.params.iter().zip(&decl.params) {
            self.ban_type(
                p.span,
                format!("Parameter ${} of {what}", p.name),
                &param.ty,
            );
        }
        self.ban_type(
            decl.return_type
                .as_ref()
                .map(|h| h.span)
                .unwrap_or(decl.name_span),
            format!("The return type of {what}"),
            &sig.return_type,
        );
    }

    fn class_like(
        &mut self,
        fqn: &str,
        methods: &[MethodDecl],
        properties: &[PropertyDecl],
        consts: &[ClassConst],
    ) {
        let info = self.registry.classes.get(fqn);
        for m in methods {
            let f = &m.function;
            self.function_like(
                &f.params,
                f.by_ref,
                f.name_span,
                &format!("{fqn}::{}()", f.name),
            );
            if let Some(sig) = info.and_then(|i| {
                i.methods
                    .get(f.name.as_ref())
                    .or_else(|| i.methods.get(f.name.to_ascii_lowercase().as_str()))
            }) {
                self.signature_types(sig, f, &format!("{fqn}::{}()", f.name));
            }
            self.stmts(&f.body);
        }
        for p in properties {
            if let Some(default) = &p.default {
                self.expr(default);
            }
            if let Some(prop) = info.and_then(|i| i.properties.get(p.name.as_ref())) {
                self.ban_type(p.span, format!("Property {fqn}::${}", p.name), &prop.ty);
            }
        }
        for c in consts {
            self.expr(&c.value);
            if let Some(native) = info
                .and_then(|i| i.consts.get(c.name.as_ref()))
                .and_then(|k| k.native_ty.as_ref())
            {
                self.ban_type(c.span, format!("Constant {fqn}::{}", c.name), native);
            }
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        let mut ctx = NameContext::default();
        self.stmts_in(stmts, &mut ctx);
    }

    fn stmts_in(&mut self, stmts: &[Stmt], ctx: &mut NameContext) {
        for stmt in stmts {
            self.stmt(stmt, ctx);
        }
    }

    fn stmt(&mut self, stmt: &Stmt, ctx: &mut NameContext) {
        match &stmt.kind {
            StmtKind::Namespace(name) => ctx.set_namespace(name),
            StmtKind::Use(imports) => ctx.add_uses(imports),
            StmtKind::FunctionDecl(f) => {
                let fqn = ctx.declared_name(&f.name);
                self.function_like(&f.params, f.by_ref, f.name_span, &format!("{fqn}()"));
                if let Some(sig) = self.registry.lookup_function(&fqn) {
                    let sig = sig.clone();
                    self.signature_types(&sig, f, &format!("{fqn}()"));
                }
                self.stmts(&f.body);
            }
            StmtKind::ClassDecl(c) => {
                let fqn = ctx.declared_name(&c.name);
                self.class_like(&fqn, &c.methods, &c.properties, &c.consts);
            }
            StmtKind::EnumDecl(e) => {
                let fqn = ctx.declared_name(&e.name);
                self.class_like(&fqn, &e.methods, &[], &e.consts);
            }
            _ => {
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
                    _ => {}
                }
                for e in stmt_exprs(stmt) {
                    self.expr(e);
                }
                for child in child_stmts(stmt) {
                    self.stmt(child, ctx);
                }
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

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Call(callee, args) => {
                if let ExprKind::Name(name) = &callee.kind {
                    let lower = name.trim_start_matches('\\').to_ascii_lowercase();
                    let construct = match lower.as_str() {
                        "eval" => Some(BannedConstruct::Eval),
                        "isset" => Some(BannedConstruct::Isset),
                        "empty" => Some(BannedConstruct::Empty),
                        "compact" => Some(BannedConstruct::Compact),
                        "extract" => Some(BannedConstruct::Extract),
                        "exit" | "die" => Some(BannedConstruct::Exit),
                        _ => None,
                    };
                    if let Some(construct) = construct {
                        // A bare `exit;` parses as a call whose span is
                        // just the name - reported as written.
                        let shown = if callee.span == e.span {
                            lower
                        } else {
                            format!("{lower}()")
                        };
                        self.ban(construct, e.span, shown);
                    }
                }
                self.arguments(args, e.span);
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
                        "<main>".to_string(),
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
                        "<main>".to_string(),
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
                body,
                by_ref,
                ..
            } => {
                self.function_like(params, *by_ref, e.span, "a closure");
                self.stmts(body);
            }
            ExprKind::ArrowFn {
                params,
                body,
                by_ref,
                ..
            } => {
                self.function_like(params, *by_ref, e.span, "an arrow function");
                self.expr(body);
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                self.arguments(args, e.span);
                for a in args {
                    self.expr(&a.value);
                }
                self.class_like(&decl.name, &decl.methods, &decl.properties, &decl.consts);
            }
            _ => {}
        }
        if let ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..) = &e.kind {
            self.ban(
                BannedConstruct::DynamicMemberAccess,
                e.span,
                "A dynamic member name".to_string(),
            );
        }
        for child in child_exprs(e) {
            self.expr(child);
        }
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
                out: Vec::new(),
            };
            walker.stmts(&file.ast);
            for span in &file.ignores.error_suppressions {
                walker.ban(
                    BannedConstruct::ErrorSuppression,
                    *span,
                    "The @ error-suppression operator".to_string(),
                );
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
