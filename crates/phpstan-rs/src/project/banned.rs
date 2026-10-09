//! `Project::check_banned`: the configured `banned_constructs` and
//! `banned_types` against every analysed file. A banned type is found in
//! the PHPDoc-refined types of functions, methods and properties, in class
//! constants' native types, in closures' and arrow functions' native
//! signatures, and - for `float` - in float literals and `(float)` casts.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use crate::banned::BannedConstruct;
use crate::registry::Registry;
use syntax::ast::*;
use syntax::span::{LineIndex, Span};
use syntax::visit::arena::{self, Visitor};
use types::Type;

use super::Project;
use diagnostics::{Diagnostic, DiagnosticBuilder};

const SUPERGLOBALS: [&str; 9] = [
    "GLOBALS", "_SERVER", "_GET", "_POST", "_FILES", "_COOKIE", "_SESSION", "_REQUEST", "_ENV",
];

struct Walker<'a> {
    arenas: &'a NodeArenas,
    constructs: &'a FxHashSet<BannedConstruct>,
    types: &'a [String],
    registry: &'a Registry,
    /// The lines of the file being walked.
    line_index: &'a LineIndex,
    /// The classes whose declarations enclose the walk, innermost last;
    /// `None` for an anonymous class not named yet.
    classes: Vec<Option<String>>,
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
            self.out.push(builder.build(self.line_index));
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
                    .build(self.line_index),
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
        arenas: &NodeArenas,
        sig: &crate::registry::FunctionReflection,
        decl: &FunctionDecl,
        what: &str,
    ) {
        for (param, p) in sig.params().iter().zip(arenas.params(decl.params.clone())) {
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
                .unwrap_or(decl.name.span),
            format!("The return type of {what}"),
            sig.return_type(),
        );
    }

    /// A closure's or arrow function's native signature, which has no
    /// reflection to refine it.
    fn hint_types(
        &mut self,
        arenas: &NodeArenas,
        params: ParamRange,
        return_type: Option<&TypeHint>,
        what: &str,
    ) {
        for p in arenas.params(params) {
            if let Some(hint) = &p.type_hint {
                self.ban_hint(
                    arenas,
                    p.span,
                    format!("Parameter ${} of {what}", p.name),
                    hint,
                );
            }
        }
        if let Some(hint) = return_type {
            self.ban_hint(
                arenas,
                hint.span,
                format!("The return type of {what}"),
                hint,
            );
        }
    }

    fn ban_hint(&mut self, arenas: &NodeArenas, span: Span, subject: String, hint: &TypeHint) {
        for name in self.types {
            if hint_mentions(arenas, hint, name) {
                self.out.push(
                    DiagnosticBuilder::new(
                        format!("{subject} uses banned type {name}."),
                        span,
                        0,
                        "banned.type",
                    )
                    .build(self.line_index),
                );
            }
        }
    }

    fn arguments(&mut self, arenas: &NodeArenas, args: ArgRange, span: Span) {
        let args = arenas.args(args);
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
        self.classes.last().and_then(Option::as_deref)
    }

    /// The constructs `e` itself is; its children are walked after.
    fn expr_itself(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else {
            return;
        };
        match &e.kind {
            ExprKind::Call(callee, args) => {
                if let Some(ExprNode {
                    kind: ExprKind::Name(name),
                    ..
                }) = arenas.expr(*callee)
                {
                    let name = arenas
                        .qualified_name(name)
                        .expect("call name range belongs to its retained file owner");
                    let lower = name.trim_start_matches('\\').to_ascii_lowercase();
                    let construct = match lower.as_str() {
                        "compact" => Some(BannedConstruct::Compact),
                        "extract" => Some(BannedConstruct::Extract),
                        _ => None,
                    };
                    if let Some(construct) = construct {
                        self.ban(
                            construct,
                            arenas
                                .expr_span(e.id)
                                .expect("expression has a parallel span"),
                            format!("{lower}()"),
                        );
                    }
                }
                self.arguments(
                    arenas,
                    args.clone(),
                    arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                );
            }
            ExprKind::Eval(_) => self.ban(
                BannedConstruct::Eval,
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                "eval()".to_string(),
            ),
            // Reported at the `@` itself.
            ExprKind::ErrorSuppress(_) => {
                let at = Span {
                    start: arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span")
                        .start,
                    end: arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span")
                        .start
                        + 1,
                };
                self.ban(
                    BannedConstruct::ErrorSuppression,
                    at,
                    "The @ error-suppression operator".to_string(),
                );
            }
            ExprKind::Isset(_) => self.ban(
                BannedConstruct::Isset,
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                "isset()".to_string(),
            ),
            ExprKind::Empty(_) => self.ban(
                BannedConstruct::Empty,
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                "empty()".to_string(),
            ),
            ExprKind::Exit(kind, args) => {
                // Reported as written: bare `exit` or `exit()`.
                let keyword = kind.as_str();
                let shown = if args.is_none() {
                    keyword.to_string()
                } else {
                    format!("{keyword}()")
                };
                self.ban(
                    BannedConstruct::Exit,
                    arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                    shown,
                );
            }
            ExprKind::New(_, args)
            | ExprKind::MethodCall(_, _, args, _)
            | ExprKind::StaticCall(_, _, args)
            | ExprKind::DynamicMethodCall(_, _, args, _)
            | ExprKind::DynamicStaticCall(_, _, args) => {
                self.arguments(
                    arenas,
                    args.clone(),
                    arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                );
            }
            ExprKind::AssignRef(target, _) => {
                self.ban(
                    BannedConstruct::AssignByReference,
                    arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                    "Assignment by reference".to_string(),
                );
                if matches!(
                    arenas.expr(*target).map(|e| &e.kind),
                    Some(ExprKind::Index(_, None))
                ) {
                    self.ban(
                        BannedConstruct::ArrayAppend,
                        arenas.expr_span(*target).unwrap_or(
                            arenas
                                .expr_span(e.id)
                                .expect("expression has a parallel span"),
                        ),
                        "Appending with []".to_string(),
                    );
                }
            }
            ExprKind::Assign(target, _) | ExprKind::CompoundAssign(_, target, _)
                if matches!(
                    arenas.expr(*target).map(|e| &e.kind),
                    Some(ExprKind::Index(_, None))
                ) =>
            {
                self.ban(
                    BannedConstruct::ArrayAppend,
                    arenas.expr_span(*target).unwrap_or(
                        arenas
                            .expr_span(e.id)
                            .expect("expression has a parallel span"),
                    ),
                    "Appending with []".to_string(),
                );
            }
            ExprKind::Ternary(_, None, _) => self.ban(
                BannedConstruct::ShortTernary,
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                "The short ternary ?:".to_string(),
            ),
            ExprKind::Binary(op @ (BinOp::Eq | BinOp::NotEq), _, _) => {
                let symbol = if matches!(op, BinOp::Eq) { "==" } else { "!=" };
                self.ban(
                    BannedConstruct::LooseComparison,
                    arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                    format!("Loose comparison {symbol}"),
                );
            }
            ExprKind::ShellExec(_) => self.ban(
                BannedConstruct::Backticks,
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                "A backtick shell command".to_string(),
            ),
            ExprKind::Variable(name) if SUPERGLOBALS.contains(&name.as_ref()) => {
                self.ban(
                    BannedConstruct::Superglobals,
                    arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                    format!("${name}"),
                );
            }
            ExprKind::DynamicVariable(_) => self.ban(
                BannedConstruct::VariableVariables,
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                "A variable variable".to_string(),
            ),
            ExprKind::DynamicPropertyAccess(..)
            | ExprKind::DynamicClassConstFetch(..)
            | ExprKind::DynamicStaticPropertyFetch(..) => self.ban(
                BannedConstruct::DynamicMemberAccess,
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                "A dynamic member name".to_string(),
            ),
            ExprKind::Literal(Literal::Float(f)) if self.float_banned() => {
                self.out.push(
                    DiagnosticBuilder::new(
                        format!("Float literal {f} uses banned type float."),
                        arenas
                            .expr_span(e.id)
                            .expect("expression has a parallel span"),
                        0,
                        "banned.type",
                    )
                    .build(self.line_index),
                );
            }
            ExprKind::Cast(cast, _) if cast.kind() == CastKind::Float && self.float_banned() => {
                self.out.push(
                    DiagnosticBuilder::new(
                        "A (float) cast uses banned type float.".to_string(),
                        arenas
                            .expr_span(e.id)
                            .expect("expression has a parallel span"),
                        0,
                        "banned.type",
                    )
                    .build(self.line_index),
                );
            }
            ExprKind::Closure(closure_id) => {
                let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure_id) else {
                    return;
                };
                if closure.by_ref {
                    self.by_ref_return(
                        arenas
                            .expr_span(e.id)
                            .expect("expression has a parallel span"),
                        "a closure",
                    );
                }
                self.hint_types(
                    arenas,
                    closure.params.clone(),
                    closure.return_type.as_deref(),
                    "a closure",
                );
            }
            ExprKind::ArrowFn(arrow_id) => {
                let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*arrow_id) else {
                    return;
                };
                if arrow.by_ref {
                    self.by_ref_return(
                        arenas
                            .expr_span(e.id)
                            .expect("expression has a parallel span"),
                        "an arrow function",
                    );
                }
                self.hint_types(
                    arenas,
                    arrow.params.clone(),
                    arrow.return_type.as_deref(),
                    "an arrow function",
                );
            }
            ExprKind::NewAnonymousClass(class_id) => {
                if let Some(class) = arenas.anonymous_class(*class_id) {
                    self.arguments(
                        arenas,
                        class.args.clone(),
                        arenas
                            .expr_span(e.id)
                            .expect("expression has a parallel span"),
                    );
                }
            }
            _ => {}
        }
        if let ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..) = &e.kind {
            self.ban(
                BannedConstruct::DynamicMemberAccess,
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                "A dynamic member name".to_string(),
            );
        }
    }
}

/// Whether a native type hint names the banned type `name` (lowercase,
/// without a leading backslash).
fn hint_mentions(arenas: &NodeArenas, hint: &TypeHint, name: &str) -> bool {
    match &hint.kind {
        TypeHintKind::Keyword(keyword, _) => keyword.as_str() == name,
        TypeHintKind::Name(range) => arenas.qualified_name(range).is_some_and(|written| {
            written
                .written
                .trim_start_matches('\\')
                .eq_ignore_ascii_case(name)
        }),
        TypeHintKind::Nullable(inner) => hint_mentions(arenas, inner, name),
        TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => arenas
            .type_hints(members)
            .iter()
            .any(|m| hint_mentions(arenas, m, name)),
    }
}

impl Visitor for Walker<'_> {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else {
            return;
        };
        let stmt_span = arenas
            .stmt_span(id)
            .expect("banned-construct statement has an arena span");
        match &stmt.kind {
            StmtKind::Switch { .. } => {
                self.ban(BannedConstruct::Switch, stmt_span, "switch".to_string())
            }
            StmtKind::Goto(label) => {
                self.ban(BannedConstruct::Goto, stmt_span, format!("goto {label}"))
            }
            StmtKind::Global(names) => self.ban(
                BannedConstruct::Global,
                stmt_span,
                format!(
                    "global {}",
                    self.arenas
                        .global_vars(names.clone())
                        .iter()
                        .map(|v| match v {
                            GlobalVar::Name(n) => format!("${n}"),
                            GlobalVar::Dynamic(e) => {
                                format!("${{{}}}", syntax::printer::print_expr(self.arenas, *e))
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
            StmtKind::FunctionDecl(function_id) => {
                let Some(f) = arenas.function(*function_id) else {
                    return;
                };
                let fqn = f.fq_name().to_string();
                let what = format!("{fqn}()");
                if f.by_ref {
                    self.by_ref_return(f.name.span, &what);
                }
                if let Some(sig) = self.registry.lookup_function(&fqn) {
                    self.signature_types(arenas, sig, f, &what);
                }
            }
            StmtKind::ClassDecl(c) => {
                let c = arenas.class_decl(c.clone());
                let fqn = c.fq_name().to_string();
                self.classes.push(Some(fqn));
                arena::walk_stmt(self, arenas, id);
                self.classes.pop();
                return;
            }
            _ => {}
        }
        arena::walk_stmt(self, arenas, id);
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        self.expr_itself(arenas, id);
        let Some(e) = arenas.expr(id) else {
            return;
        };
        if let ExprKind::NewAnonymousClass(class) = &e.kind {
            self.classes
                .push(arenas.anonymous_class_name(*class).map(ToString::to_string));
            arena::walk_expr(self, arenas, id);
            self.classes.pop();
        } else {
            arena::walk_expr(self, arenas, id);
        }
    }

    fn visit_method_decl(&mut self, arenas: &NodeArenas, m: &MethodDecl) {
        if let Some(fqn) = self.class().map(str::to_string) {
            let f = &m.function;
            let what = format!("{fqn}::{}()", f.name);
            if f.by_ref {
                self.by_ref_return(f.name.span, &what);
            }
            if let Some(sig) = self.registry.lookup_method(&fqn, &f.name) {
                self.signature_types(arenas, sig, f, &what);
            }
        }
        arena::walk_method_decl(self, arenas, m);
    }

    fn visit_property_decl(&mut self, arenas: &NodeArenas, p: &PropertyDecl) {
        if let Some(fqn) = self.class().map(str::to_string) {
            if let Some(prop) = self.registry.lookup_property(&fqn, &p.name) {
                self.ban_type(p.span, format!("Property {fqn}::${}", p.name), prop.ty());
            }
            if p.hooks
                .as_ref()
                .and_then(|h| h.get.as_ref())
                .is_some_and(|get| get.by_ref)
            {
                self.by_ref_return(p.span, &format!("{fqn}::${}'s get hook", p.name));
            }
        }
        arena::walk_property_decl(self, arenas, p);
    }

    fn visit_class_const(&mut self, arenas: &NodeArenas, c: &ClassConst) {
        if let Some(fqn) = self.class().map(str::to_string)
            && let Some(native) = self
                .registry
                .class_by_name(fqn.as_str())
                .and_then(|i| i.consts().get(&c.name.name))
                .and_then(|k| k.native_ty().as_ref())
        {
            let native = native.clone();
            self.ban_type(c.span, format!("Constant {fqn}::{}", c.name), &native);
        }
        arena::walk_class_const(self, arenas, c);
    }

    fn visit_param(&mut self, arenas: &NodeArenas, p: &Param) {
        if p.by_ref {
            self.ban(
                BannedConstruct::ByReferenceParameters,
                p.span,
                format!("By-reference parameter ${}", p.name),
            );
        }
        arena::walk_param(self, arenas, p);
    }

    fn visit_attribute(&mut self, arenas: &NodeArenas, a: &Attribute) {
        self.arguments(arenas, a.args.clone(), a.span);
        arena::walk_attribute(self, arenas, a);
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
            let tree = file
                .tree()
                .expect("analyzed banned-check file must retain its syntax tree");
            let arenas = tree.arenas();
            let mut walker = Walker {
                arenas,
                constructs: &constructs,
                types: &types,
                registry: &self.registry,
                line_index: &file.line_index,
                classes: Vec::new(),
                out: Vec::new(),
            };
            arena::walk_stmts(&mut walker, arenas, file.ast());
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
                let at = file.line_index.line_start(line).unwrap_or_default();
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
