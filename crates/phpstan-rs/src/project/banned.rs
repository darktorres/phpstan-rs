//! `Project::check_banned`: the configured `banned_constructs` and
//! `banned_types` against every analysed file. A banned type is found in
//! the PHPDoc-refined types of functions, methods and properties, in class
//! constants' native types, and - for `float` - in float literals and
//! `(float)` casts.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use crate::banned::BannedConstruct;
use crate::registry::Registry;
use crate::registry::class_decl_name;
use syntax::ast::*;
use syntax::token::{Position, Span};
use syntax::visit::{Visitor, any_child_expr, walk_stmt};
use types::Type;

use super::Project;
use diagnostics::{Diagnostic, DiagnosticBuilder};
use syntax::name_resolution::NameContext;

const SUPERGLOBALS: [&str; 9] = [
    "GLOBALS", "_SERVER", "_GET", "_POST", "_FILES", "_COOKIE", "_SESSION", "_REQUEST", "_ENV",
];

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
                i.methods()
                    .get(f.name.as_ref())
                    .or_else(|| i.methods().get(f.name.to_ascii_lowercase().as_str()))
            }) {
                self.signature_types(sig, f, &format!("{fqn}::{}()", f.name));
            }
            self.stmts(&f.body);
        }
        for p in properties {
            if let Some(default) = &p.default {
                self.expr(default);
            }
            if let Some(prop) = info.and_then(|i| i.properties().get(p.name.as_ref())) {
                self.ban_type(p.span, format!("Property {fqn}::${}", p.name), prop.ty());
            }
        }
        for c in consts {
            self.expr(&c.value);
            if let Some(native) = info
                .and_then(|i| i.consts().get(c.name.as_ref()))
                .and_then(|k| k.native_ty().as_ref())
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
            StmtKind::ClassDecl(e) if e.kind == ClassKind::Enum => {
                let fqn = ctx.declared_name(class_decl_name(e));
                self.class_like(&fqn, &e.methods, &[], &e.consts);
            }
            StmtKind::ClassDecl(c) => {
                let fqn = ctx.declared_name(class_decl_name(c));
                self.class_like(&fqn, &c.methods, &c.properties, &c.consts);
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
                walk_stmt(&mut Children { walker: self, ctx }, stmt);
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
        syntax::stack::guard(|| {
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
                ExprKind::Isset(_) => {
                    self.ban(BannedConstruct::Isset, e.span, "isset()".to_string())
                }
                ExprKind::Empty(_) => {
                    self.ban(BannedConstruct::Empty, e.span, "empty()".to_string())
                }
                ExprKind::Exit(kind, arg) => {
                    // A bare `exit;` spans just the keyword, and is reported as
                    // written.
                    let keyword = kind.as_str();
                    let bare = arg.is_none()
                        && e.span.start.line == e.span.end.line
                        && (e.span.end.col - e.span.start.col) as usize == keyword.len();
                    let shown = if bare {
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
                ExprKind::Cast(cast, _)
                    if cast.kind() == CastKind::Float && self.float_banned() =>
                {
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
                    self.class_like(
                        class_decl_name(decl),
                        &decl.methods,
                        &decl.properties,
                        &decl.consts,
                    );
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
            any_child_expr(e, |child| {
                self.expr(child);
                false
            });
        })
    }
}

/// Hands a statement's own expressions and the statements directly in it
/// back to the walker.
struct Children<'w, 'a, 'c> {
    walker: &'w mut Walker<'a>,
    ctx: &'c mut NameContext,
}

impl<'ast> Visitor<'ast> for Children<'_, '_, '_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        self.walker.stmt(stmt, self.ctx);
    }
    fn visit_expr(&mut self, e: &'ast Expr) {
        self.walker.expr(e);
    }
    fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}
    fn visit_class_decl(&mut self, _: &'ast ClassDecl) {}
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
