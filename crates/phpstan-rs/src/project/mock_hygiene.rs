//! `Project::check_mock_hygiene`: PHPUnit mock-builder misuse
//! (`rector/mockstan`'s family), opt-in via `check_mock_hygiene`:
//!
//! - `mock.expectsAny`: `->expects($this->any())` - neither a stub nor a
//!   real expectation;
//! - `mock.atLeastZero`: `$this->atLeast(0)` matches every call count;
//! - `mock.withWithoutExpects`: `->method(...)->with(...)` with no
//!   `->expects(...)` in the chain - PHPUnit never verifies arguments on
//!   a stubbed method;
//! - `mock.conflictingReturns`: `willReturnOnConsecutiveCalls()` and
//!   `willReturnCallback()` on one configuration;
//! - `mock.unionPropertyType`: a property typed as a union of a mock and
//!   a real class (the intersection `Foo&MockObject` is what's meant);
//! - `mock.onlyMockProperties`: a test class whose properties are all
//!   mocks;
//! - `mock.methodWithoutExpects`: a mock (not a stub) configured with
//!   `->method(...)` but no `->expects(...)` - make it a stub, or state
//!   the expected calls;
//! - `mock.bannedClass`: mocking a class `mock_banned_classes` lists.

use crate::registry::class_decl_name;
use rustc_hash::FxHashMap;
use std::path::PathBuf;

use syntax::ast::*;
use syntax::token::Span;
use syntax::visit::{Visitor, walk_expr, walk_expr_in_scope, walk_stmt, walk_stmts};

use super::Project;
use diagnostics::{Diagnostic, DiagnosticBuilder};
use syntax::name_resolution::NameContext;

const MOCK_TYPES: [&str; 3] = [
    "PHPUnit\\Framework\\MockObject\\MockObject",
    "PHPUnit\\Framework\\MockObject\\Stub",
    "Mockery\\MockInterface",
];

const MOCK_FACTORIES: [&str; 5] = [
    "createMock",
    "createStub",
    "createPartialMock",
    "createConfiguredMock",
    "getMockBuilder",
];

/// Whether `e` is `$this->name(...)`, `self::name(...)` or
/// `static::name(...)`.
fn is_test_helper_call<'e>(e: &'e Expr, name: &str) -> Option<&'e [Arg]> {
    match &e.kind {
        ExprKind::MethodCall(recv, method, args, _)
            if method.eq_ignore_ascii_case(name)
                && matches!(&recv.kind, ExprKind::Variable(v) if v.as_ref() == "this") =>
        {
            Some(args)
        }
        ExprKind::StaticCall(StaticTarget::Name(target, _), method, args)
            if method.eq_ignore_ascii_case(name)
                && ["self", "static", "parent"]
                    .iter()
                    .any(|t| target.eq_ignore_ascii_case(t)) =>
        {
            Some(args)
        }
        _ => None,
    }
}

/// The method names of the call chain ending at `e`, innermost first.
fn chain(e: &Expr) -> Vec<(&str, &[Arg], Span)> {
    let mut out = Vec::new();
    let mut current = e;
    while let ExprKind::MethodCall(recv, method, args, _) = &current.kind {
        out.push((method.as_ref(), args.as_slice(), current.span));
        current = recv;
    }
    out.reverse();
    out
}

struct Walker<'a> {
    banned: &'a [String],
    ctx: NameContext,
    out: Vec<Diagnostic>,
    // `$name` / `this->prop` -> whether it holds a mock (`true`) or a
    // stub (`false`), from what it was assigned.
    origins: FxHashMap<String, bool>,
}

/// Whether `e` creates a mock (`Some(true)`) or a stub (`Some(false)`).
fn mock_origin(e: &Expr) -> Option<bool> {
    if is_test_helper_call(e, "createStub").is_some() {
        return Some(false);
    }
    for factory in ["createMock", "createPartialMock", "createConfiguredMock"] {
        if is_test_helper_call(e, factory).is_some() {
            return Some(true);
        }
    }
    if let ExprKind::MethodCall(_, method, _, _) = &e.kind
        && method.eq_ignore_ascii_case("getMock")
    {
        let mut current = e;
        while let ExprKind::MethodCall(recv, _, _, _) = &current.kind {
            if is_test_helper_call(recv, "getMockBuilder").is_some() {
                return Some(true);
            }
            current = recv;
        }
    }
    None
}

/// The origin key of an assignment target or chain base.
fn origin_key(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Variable(name) => Some(format!("${name}")),
        ExprKind::PropertyAccess(object, prop, _) if matches!(&object.kind, ExprKind::Variable(v) if v.as_ref() == "this") => {
            Some(format!("this->{prop}"))
        }
        _ => None,
    }
}

fn collect_origins(stmts: &[Stmt], out: &mut FxHashMap<String, bool>) {
    struct Origins<'o> {
        out: &'o mut FxHashMap<String, bool>,
    }
    impl<'ast> Visitor<'ast> for Origins<'_> {
        fn visit_expr(&mut self, e: &'ast Expr) {
            if let ExprKind::Assign(target, value) = &e.kind
                && let (Some(key), Some(is_mock)) = (origin_key(target), mock_origin(value))
            {
                self.out.insert(key, is_mock);
            }
            walk_expr_in_scope(self, e);
        }
        fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}
        fn visit_class_decl(&mut self, _: &'ast ClassDecl) {}
    }
    walk_stmts(&mut Origins { out }, stmts);
}

impl<'a> Walker<'a> {
    fn report(&mut self, span: Span, message: String, id: &'static str) {
        self.out
            .push(DiagnosticBuilder::new(message, span, 2, id).build());
    }

    fn class(&mut self, c: &ClassDecl) {
        let mut mock_properties = 0;
        for p in &c.properties {
            let (hint, written_members) = match &p.type_hint {
                Some(native) => {
                    let text = native.to_string();
                    let members = text
                        .trim_start_matches('?')
                        .split('|')
                        .map(|m| m.trim().to_string())
                        .collect();
                    (text, members)
                }
                None => {
                    let Some(node) = p
                        .doc_comment
                        .as_deref()
                        .and_then(|doc| phpdoc::docblock::parse(doc).var_type)
                    else {
                        continue;
                    };
                    let mut members = Vec::new();
                    union_members(&node, &mut members);
                    (node.to_source_string(), members)
                }
            };
            let hint = &hint;
            let members: Vec<String> = written_members
                .iter()
                .map(|m| self.ctx.resolve(m).trim_start_matches('\\').to_string())
                .collect();
            let is_mock = |m: &String| MOCK_TYPES.iter().any(|t| t.eq_ignore_ascii_case(m));
            if members.iter().any(is_mock) {
                mock_properties += 1;
                if members.len() > 1
                    && members
                        .iter()
                        .any(|m| !is_mock(m) && !m.eq_ignore_ascii_case("null"))
                {
                    self.report(
                        p.span,
                        format!(
                            "Property ${} is typed {hint}; a mock of a class is its intersection with the mock type, not a union.",
                            p.name
                        ),
                        "mock.unionPropertyType",
                    );
                }
            } else if hint.contains('&')
                && hint.split('&').any(|m| {
                    let resolved = self.ctx.resolve(m.trim());
                    MOCK_TYPES
                        .iter()
                        .any(|t| t.eq_ignore_ascii_case(resolved.trim_start_matches('\\')))
                })
            {
                mock_properties += 1;
            }
        }
        if mock_properties > 0
            && mock_properties == c.properties.len()
            && c.extends
                .first()
                .is_some_and(|e| e.trim_start_matches('\\').ends_with("TestCase"))
        {
            self.report(
                c.name_span,
                format!(
                    "Test class {} has only mock properties; nothing real is under test in its fixtures.",
                    class_decl_name(c)
                ),
                "mock.onlyMockProperties",
            );
        }
        let mut class_origins = FxHashMap::default();
        for m in &c.methods {
            collect_origins(&m.function.body, &mut class_origins);
        }
        class_origins.retain(|k, _| k.starts_with("this->"));
        for m in &c.methods {
            let mut origins = class_origins.clone();
            collect_origins(&m.function.body, &mut origins);
            let previous = std::mem::replace(&mut self.origins, origins);
            walk_stmts(self, &m.function.body);
            self.origins = previous;
        }
    }

    /// Rules over one statement's mock-configuration chain.
    fn configuration_chain(&mut self, e: &Expr) {
        let e = match &e.kind {
            ExprKind::Assign(_, value) => value.as_ref(),
            _ => e,
        };
        let calls = chain(e);
        if calls.is_empty() {
            return;
        }
        let has = |name: &str| calls.iter().any(|(m, _, _)| m.eq_ignore_ascii_case(name));
        if has("method") && has("with") && !has("expects") {
            let span = calls
                .iter()
                .find(|(m, _, _)| m.eq_ignore_ascii_case("with"))
                .map_or(e.span, |(_, _, s)| *s);
            self.report(
                span,
                "->with() on a method configured without ->expects() is never verified; add an expectation or drop the argument constraint.".to_string(),
                "mock.withWithoutExpects",
            );
        }
        let mut base = e;
        while let ExprKind::MethodCall(recv, _, _, _) = &base.kind {
            base = recv;
        }
        if has("method")
            && !has("expects")
            && origin_key(base).and_then(|k| self.origins.get(&k).copied()) == Some(true)
        {
            self.report(
                e.span,
                "A mock configured with ->method() but no ->expects() is a stub in disguise; create it with createStub(), or state the expected calls.".to_string(),
                "mock.methodWithoutExpects",
            );
        }
        if has("willReturnOnConsecutiveCalls") && has("willReturnCallback") {
            self.report(
                e.span,
                "willReturnOnConsecutiveCalls() and willReturnCallback() configure the same return; keep one.".to_string(),
                "mock.conflictingReturns",
            );
        }
    }
}

impl<'ast> Visitor<'ast> for Walker<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => self.ctx.add_uses(imports),
            StmtKind::Expression(e) => self.configuration_chain(e),
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_function_decl(&mut self, f: &'ast FunctionDecl) {
        walk_stmts(self, &f.body);
    }

    fn visit_class_decl(&mut self, c: &'ast ClassDecl) {
        if c.kind == ClassKind::Enum {
            for m in &c.methods {
                walk_stmts(self, &m.function.body);
            }
        } else {
            self.class(c);
        }
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        if let ExprKind::MethodCall(_, method, args, _) = &e.kind
            && method.eq_ignore_ascii_case("expects")
            && let Some(arg) = args.first()
            && is_test_helper_call(&arg.value, "any").is_some()
        {
            self.report(
                e.span,
                "->expects($this->any()) asserts nothing; use a stub, or an explicit call count."
                    .to_string(),
                "mock.expectsAny",
            );
        }
        if let Some(args) = is_test_helper_call(e, "atLeast")
            && matches!(
                args.first().map(|a| &a.value.kind),
                Some(ExprKind::Literal(Literal::Int(0)))
            )
        {
            self.report(
                e.span,
                "atLeast(0) matches any number of calls, including none; use at least 1, or a stub.".to_string(),
                "mock.atLeastZero",
            );
        }
        if !self.banned.is_empty() {
            for factory in MOCK_FACTORIES {
                if let Some(args) = is_test_helper_call(e, factory)
                    && let Some(ExprKind::ClassNameFetch(StaticTarget::Name(class, _))) =
                        args.first().map(|a| &a.value.kind)
                {
                    let resolved = self.ctx.resolve(class);
                    let resolved = resolved.trim_start_matches('\\');
                    if self
                        .banned
                        .iter()
                        .any(|b| b.trim_start_matches('\\').eq_ignore_ascii_case(resolved))
                    {
                        self.report(
                            e.span,
                            format!("{resolved} must not be mocked; use a real instance or a purpose-built fake."),
                            "mock.bannedClass",
                        );
                    }
                }
            }
        }
        walk_expr(self, e);
    }
}

impl Project {
    /// PHPUnit mock-builder misuse in the analysed files; `banned` lists
    /// classes that must not be mocked.
    pub fn check_mock_hygiene(&self, banned: &[String]) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut out = FxHashMap::default();
        for (path, file) in &self.files {
            if !file.analyzed {
                continue;
            }
            let mut walker = Walker {
                banned,
                ctx: NameContext::default(),
                out: Vec::new(),
                origins: FxHashMap::default(),
            };
            walk_stmts(&mut walker, &file.ast);
            if !walker.out.is_empty() {
                let mut found = walker.out;
                found.sort_by_key(|d| (d.span.start.line, d.span.start.col));
                out.insert(path.clone(), found);
            }
        }
        out
    }
}

/// The members of a PHPDoc union as written, `?T` as `T` and `null`.
fn union_members(node: &phpdoc::grammar::TypeNode, out: &mut Vec<String>) {
    use phpdoc::grammar::TypeNode;
    match node.unwrap_paren() {
        TypeNode::Union(members) => members.iter().for_each(|m| union_members(m, out)),
        TypeNode::Nullable(inner) => {
            union_members(inner, out);
            out.push("null".to_string());
        }
        other => out.push(other.to_source_string()),
    }
}
