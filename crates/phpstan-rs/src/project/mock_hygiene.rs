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

use rustc_hash::FxHashMap;
use std::path::PathBuf;

use syntax::ast::*;
use syntax::span::{LineIndex, Span};
use syntax::visit::arena::{self, Visitor};

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
fn is_test_helper_call(arenas: &NodeArenas, id: ExprId, name: &str) -> Option<ArgRange> {
    match &arenas.expr(id)?.kind {
        ExprKind::MethodCall(recv, method, args, _)
            if method.eq_ignore_ascii_case(name)
                && matches!(arenas.expr(*recv).map(|e| &e.kind), Some(ExprKind::Variable(v)) if v.as_ref() == "this") =>
        {
            Some(args.clone())
        }
        ExprKind::StaticCall(StaticTarget::Name(target), method, args)
            if method.eq_ignore_ascii_case(name)
                && ["self", "static", "parent"]
                    .iter()
                    .any(|t| target.eq_ignore_ascii_case(t)) =>
        {
            Some(args.clone())
        }
        _ => None,
    }
}

/// The method names of the call chain ending at `e`, innermost first.
fn chain(arenas: &NodeArenas, id: ExprId) -> Vec<(&str, &[Arg], Span)> {
    let mut out = Vec::new();
    let mut current = id;
    while let Some(node) = arenas.expr(current)
        && let ExprKind::MethodCall(recv, method, args, _) = &node.kind
    {
        out.push((
            method.as_ref(),
            arenas.args(args.clone()),
            arenas
                .expr_span(node.id)
                .expect("call expression has a parallel span"),
        ));
        current = *recv;
    }
    out.reverse();
    out
}

struct Walker<'a> {
    banned: &'a [String],
    ctx: NameContext,
    line_index: &'a LineIndex,
    out: Vec<Diagnostic>,
    // `$name` / `this->prop` -> whether it holds a mock (`true`) or a
    // stub (`false`), from what it was assigned.
    origins: FxHashMap<String, bool>,
}

/// Whether `e` creates a mock (`Some(true)`) or a stub (`Some(false)`).
fn mock_origin(arenas: &NodeArenas, id: ExprId) -> Option<bool> {
    if is_test_helper_call(arenas, id, "createStub").is_some() {
        return Some(false);
    }
    for factory in ["createMock", "createPartialMock", "createConfiguredMock"] {
        if is_test_helper_call(arenas, id, factory).is_some() {
            return Some(true);
        }
    }
    let e = arenas.expr(id)?;
    if let ExprKind::MethodCall(_, method, _, _) = &e.kind
        && method.eq_ignore_ascii_case("getMock")
    {
        let mut current = id;
        while let Some(node) = arenas.expr(current)
            && let ExprKind::MethodCall(recv, _, _, _) = &node.kind
        {
            if is_test_helper_call(arenas, *recv, "getMockBuilder").is_some() {
                return Some(true);
            }
            current = *recv;
        }
    }
    None
}

/// The origin key of an assignment target or chain base.
fn origin_key(arenas: &NodeArenas, id: ExprId) -> Option<String> {
    match &arenas.expr(id)?.kind {
        ExprKind::Variable(name) => Some(format!("${name}")),
        ExprKind::PropertyAccess(object, prop, _) if matches!(arenas.expr(*object).map(|e| &e.kind), Some(ExprKind::Variable(v)) if v.as_ref() == "this") => {
            Some(format!("this->{prop}"))
        }
        _ => None,
    }
}

fn collect_origins(arenas: &NodeArenas, stmts: &[StmtId], out: &mut FxHashMap<String, bool>) {
    struct Origins<'o> {
        out: &'o mut FxHashMap<String, bool>,
    }
    impl Visitor for Origins<'_> {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            if let Some(e) = arenas.expr(id)
                && let ExprKind::Assign(target, value) = &e.kind
                && let (Some(key), Some(is_mock)) =
                    (origin_key(arenas, *target), mock_origin(arenas, *value))
            {
                self.out.insert(key, is_mock);
            }
            arena::walk_expr_in_scope(self, arenas, id);
        }
        fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}
        fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}
    }
    arena::walk_stmts(&mut Origins { out }, arenas, stmts);
}

impl<'a> Walker<'a> {
    /// A class or keyword type named in PHPDoc text, as the class it
    /// resolves to here, or the keyword in lowercase.
    fn resolve_doc_type_name(&self, name: &str) -> String {
        let name = name.trim().trim_matches(|c| c == '(' || c == ')');
        if NativeType::from_name(name).is_some() {
            return name.to_ascii_lowercase();
        }
        self.ctx.resolve(name).trim_start_matches('\\').to_string()
    }

    fn report(&mut self, span: Span, message: String, id: &'static str) {
        self.out
            .push(DiagnosticBuilder::new(message, span, 2, id).build(self.line_index));
    }

    fn class(&mut self, arenas: &NodeArenas, c: &ClassDecl, def: &ClassDef) {
        let mut mock_properties = 0;
        for p in def.properties(arenas) {
            let (hint, members) = match &p.type_hint {
                Some(native) => (
                    native.to_string_with(arenas),
                    native_members(native, arenas),
                ),
                None => {
                    let Some(node) = p
                        .doc_comment
                        .as_ref()
                        .and_then(|ranges| arenas.doc_comment_text(ranges))
                        .as_deref()
                        .and_then(|doc| phpdoc::docblock::parse_shared(doc).var_type.clone())
                    else {
                        continue;
                    };
                    let mut written = Vec::new();
                    union_members(&node, &mut written);
                    let members = written
                        .iter()
                        .map(|member| {
                            member
                                .split('&')
                                .map(|part| self.resolve_doc_type_name(part))
                                .collect()
                        })
                        .collect();
                    (node.to_source_string(), members)
                }
            };
            let is_mock = |m: &String| MOCK_TYPES.iter().any(|t| t.eq_ignore_ascii_case(m));
            let is_plain_mock = |m: &Vec<String>| matches!(m.as_slice(), [only] if is_mock(only));
            if members.iter().any(is_plain_mock) {
                mock_properties += 1;
                let is_null = |m: &Vec<String>| matches!(m.as_slice(), [only] if only == "null");
                if members.len() > 1 && members.iter().any(|m| !is_plain_mock(m) && !is_null(m)) {
                    self.report(
                        p.span,
                        format!(
                            "Property ${} is typed {hint}; a mock of a class is its intersection with the mock type, not a union.",
                            p.name
                        ),
                        "mock.unionPropertyType",
                    );
                }
            } else if members.iter().any(|m| m.len() > 1 && m.iter().any(is_mock)) {
                mock_properties += 1;
            }
        }
        if mock_properties > 0
            && mock_properties == def.properties(arenas).len()
            && def
                .extends(arenas)
                .first()
                .and_then(QualifiedName::resolved_class)
                .is_some_and(|e| e.ends_with("TestCase"))
        {
            self.report(
                c.name.span,
                format!(
                    "Test class {} has only mock properties; nothing real is under test in its fixtures.",
                    c.name
                ),
                "mock.onlyMockProperties",
            );
        }
        let mut class_origins = FxHashMap::default();
        for m in arenas.methods(def.methods.clone()) {
            collect_origins(arenas, m.function.body_stmts(arenas), &mut class_origins);
        }
        class_origins.retain(|k, _| k.starts_with("this->"));
        for m in arenas.methods(def.methods.clone()) {
            let mut origins = class_origins.clone();
            collect_origins(arenas, m.function.body_stmts(arenas), &mut origins);
            let previous = std::mem::replace(&mut self.origins, origins);
            arena::walk_stmts(self, arenas, m.function.body_stmts(arenas));
            self.origins = previous;
        }
    }

    /// Rules over one statement's mock-configuration chain.
    fn configuration_chain(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(original) = arenas.expr(id) else {
            return;
        };
        let base_id = match &original.kind {
            ExprKind::Assign(_, value) => *value,
            _ => id,
        };
        let Some(e) = arenas.expr(base_id) else {
            return;
        };
        let calls = chain(arenas, base_id);
        if calls.is_empty() {
            return;
        }
        let has = |name: &str| calls.iter().any(|(m, _, _)| m.eq_ignore_ascii_case(name));
        if has("method") && has("with") && !has("expects") {
            let span = calls
                .iter()
                .find(|(m, _, _)| m.eq_ignore_ascii_case("with"))
                .map_or(
                    arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                    |(_, _, s)| *s,
                );
            self.report(
                span,
                "->with() on a method configured without ->expects() is never verified; add an expectation or drop the argument constraint.".to_string(),
                "mock.withWithoutExpects",
            );
        }
        let mut base = base_id;
        while let Some(node) = arenas.expr(base)
            && let ExprKind::MethodCall(recv, _, _, _) = &node.kind
        {
            base = *recv;
        }
        if has("method")
            && !has("expects")
            && origin_key(arenas, base).and_then(|k| self.origins.get(&k).copied()) == Some(true)
        {
            self.report(
                arenas.expr_span(e.id).expect("expression has a parallel span"),
                "A mock configured with ->method() but no ->expects() is a stub in disguise; create it with createStub(), or state the expected calls.".to_string(),
                "mock.methodWithoutExpects",
            );
        }
        if has("willReturnOnConsecutiveCalls") && has("willReturnCallback") {
            self.report(
                arenas.expr_span(e.id).expect("expression has a parallel span"),
                "willReturnOnConsecutiveCalls() and willReturnCallback() configure the same return; keep one.".to_string(),
                "mock.conflictingReturns",
            );
        }
    }
}

impl Visitor for Walker<'_> {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => self.ctx.add_uses(arenas.use_imports(imports)),
            StmtKind::Expression(e) => self.configuration_chain(arenas, *e),
            _ => {}
        }
        arena::walk_stmt(self, arenas, id);
    }

    fn visit_function_decl(&mut self, arenas: &NodeArenas, f: &FunctionDecl) {
        arena::walk_stmts(self, arenas, f.body_stmts(arenas));
    }

    fn visit_class_decl(&mut self, arenas: &NodeArenas, c: &ClassDecl) {
        let Some(def) = arenas.class(c.def) else {
            return;
        };
        if def.kind == ClassKind::Enum {
            for m in arenas.methods(def.methods.clone()) {
                arena::walk_stmts(self, arenas, m.function.body_stmts(arenas));
            }
        } else {
            self.class(arenas, c, def);
        }
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        if let ExprKind::MethodCall(_, method, args, _) = &e.kind
            && method.eq_ignore_ascii_case("expects")
            && let Some(arg) = arenas.args(args.clone()).first()
            && is_test_helper_call(arenas, arg.value, "any").is_some()
        {
            self.report(
                arenas
                    .expr_span(e.id)
                    .expect("expression has a parallel span"),
                "->expects($this->any()) asserts nothing; use a stub, or an explicit call count."
                    .to_string(),
                "mock.expectsAny",
            );
        }
        if let Some(args) = is_test_helper_call(arenas, id, "atLeast")
            && matches!(
                arenas
                    .args(args)
                    .first()
                    .and_then(|a| arenas.expr(a.value))
                    .map(|e| &e.kind),
                Some(ExprKind::Literal(Literal::Int(0)))
            )
        {
            self.report(
                arenas.expr_span(e.id).expect("expression has a parallel span"),
                "atLeast(0) matches any number of calls, including none; use at least 1, or a stub.".to_string(),
                "mock.atLeastZero",
            );
        }
        if !self.banned.is_empty() {
            for factory in MOCK_FACTORIES {
                if let Some(args) = is_test_helper_call(arenas, id, factory)
                    && let Some(ExprKind::ClassNameFetch(StaticTarget::Name(class))) = arenas
                        .args(args.clone())
                        .first()
                        .and_then(|a| arenas.expr(a.value))
                        .map(|e| &e.kind)
                {
                    let Some(resolved) = class.resolved_class() else {
                        continue;
                    };
                    if self
                        .banned
                        .iter()
                        .any(|b| b.trim_start_matches('\\').eq_ignore_ascii_case(resolved))
                    {
                        self.report(
                            arenas.expr_span(e.id).expect("expression has a parallel span"),
                            format!("{resolved} must not be mocked; use a real instance or a purpose-built fake."),
                            "mock.bannedClass",
                        );
                    }
                }
            }
        }
        arena::walk_expr(self, arenas, id);
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
                line_index: &file.line_index,
                out: Vec::new(),
                origins: FxHashMap::default(),
            };
            let tree = file
                .tree()
                .expect("analyzed mock-hygiene file must retain its syntax tree");
            arena::walk_stmts(&mut walker, tree.arenas(), tree.statements());
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
/// A native type's union members, each as its intersected parts: a class
/// by its resolved name, a keyword type in lowercase.
fn native_members(hint: &TypeHint, arenas: &NodeArenas) -> Vec<Vec<String>> {
    fn part(hint: &TypeHint, arenas: &NodeArenas) -> String {
        match &hint.kind {
            TypeHintKind::Name(range) => arenas
                .qualified_name(range)
                .map(|name| name.resolved_class().unwrap_or(&name.written).to_string())
                .unwrap_or_else(|| "<unknown>".to_string()),
            TypeHintKind::Keyword(_, written) => written.to_ascii_lowercase(),
            _ => hint.to_string_with(arenas),
        }
    }
    match &hint.kind {
        TypeHintKind::Union(members) => arenas
            .type_hints(members)
            .iter()
            .flat_map(|member| native_members(member, arenas))
            .collect(),
        TypeHintKind::Nullable(inner) => {
            let mut out = native_members(inner, arenas);
            out.push(vec!["null".to_string()]);
            out
        }
        TypeHintKind::Intersection(parts) => vec![
            arenas
                .type_hints(parts)
                .iter()
                .map(|part_hint| part(part_hint, arenas))
                .collect(),
        ],
        _ => vec![vec![part(hint, arenas)]],
    }
}

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
