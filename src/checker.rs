use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::registry::{self, FunctionSig, NameContext, ParamSig, PropertySig, Registry};
use crate::token::Span;
use crate::types::{self, Type};

/// Loosely mirrors real PHPStan's own rule-gating levels (0 lowest/laxest,
/// verified against a fresh `phpstan/phpstan-src` clone's `conf/config.
/// levelN.neon` files rather than recalled from memory): 0 for the
/// "definitely broken" undefined-symbol/arity family (matches real
/// PHPStan's own level-0 bucket - "unknown classes, unknown functions,
/// unknown methods..., wrong number of arguments..., always undefined
/// variables" - though this project doesn't implement PHPStan's own
/// $this-only-vs-any-expression distinction for method/property checks,
/// a real, documented simplification, not an oversight), 2 for undefined-
/// property/array-shape-key (PHPDoc-derived structural checks, real
/// PHPStan's `checkThisOnly: false`/`checkPhpDocMissingReturn` territory),
/// 3 for return-type/property-type mismatches (real PHPStan's own
/// `checkPhpDocMethodSignatures` level exactly), 5 for argument-type
/// mismatches (real PHPStan's own `checkFunctionArgumentTypes` level
/// exactly), and 6 for this project's own generic bound/variance checks
/// (no direct real-PHPStan equivalent - generics are baked into its core
/// type engine, not level-gated - placed here as this project's own
/// "advanced, opt-in" tier, alongside real PHPStan's missing-typehints
/// level).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub location: String,
    pub message: String,
    pub span: Span,
    pub level: u8,
}

/// Walks function/method bodies, inferring a best-effort type for each
/// expression and flagging argument/return type mismatches against the
/// registry. Deliberately conservative: `Type::Mixed` is overloaded here to
/// mean both PHP's real `mixed` type AND "we don't know" for anything this
/// prototype can't infer (property access, closures, dynamic calls, ...) -
/// so every check below treats an inferred `Mixed` as inconclusive and
/// skips reporting, rather than risk false positives.
///
/// `if`/`while` fork `locals` into per-branch copies (narrowed via
/// `apply_narrowing` on the condition) and merge them back at the join
/// point (`merge_scopes`), accounting for a branch that unconditionally
/// terminates (`always_terminates`) so a guard clause's narrowing survives
/// past the `if`. Everything else (`for`/`foreach`/`switch`/`try`) still
/// shares one mutable scope with no forking or narrowing - a real but
/// bounded gap, not a soundness issue, since no narrowing ever happens
/// there rather than an incorrect one.
///
/// Resolved limitation (Phase 9.5): undefined-method detection was
/// initially built and then reverted after real-scale testing found it
/// trusted *which* `ClassInfo` a bare name resolved to, and this
/// project's registry didn't model namespaces at all - 1595 distinct
/// class names were found to collide across one real project plus its
/// `vendor/` (e.g. 77 unrelated declarations of `InvalidArgumentException`
/// alone), silently overwriting a class's real method table with an
/// unrelated one's. Fixed by making `Registry`/`Checker` resolve every
/// class/function reference to a fully-qualified name first
/// (`registry::NameContext`, tracking each file's `namespace`/`use`
/// statements) before any registry key is read or written - two classes
/// with the same short name in different namespaces now get distinct
/// keys, the same way real PHP tells them apart. Undefined-method
/// detection on `ExprKind::MethodCall` is safe again as of this fix.
///
/// Phase 9 completed since: `ExprKind::PropertyAccess` now flags undefined
/// properties on read (`__get`-exempt; the existing write-side check
/// stays lenient, since a write can legitimately create a real, if
/// deprecated, dynamic property - a read never does), and
/// `ExprKind::StaticAccess`/`StaticCall` now check constants, static
/// properties, and static methods (`__callStatic`-exempt) the same way.
pub struct Checker<'a> {
    registry: &'a Registry,
    diagnostics: Vec<Diagnostic>,
    // Namespace + `use`-import map, built once per file. Needed because
    // almost all real code refers to an imported class/function by its
    // short name, not a fully-qualified path - without resolving it here
    // first, undefined-class/function detection would flag the
    // overwhelming majority of legitimately-imported symbols. Also must
    // resolve to exactly the same fully-qualified names
    // `Registry::index` keys its entries under (Phase 9.5's fix for the
    // namespace-collision bug documented above), or the two would
    // silently disagree with each other.
    name_ctx: NameContext,
    // The class whose method body is currently being checked, if any -
    // needed to resolve self/parent/static to a concrete class name.
    current_class: Option<String>,
    // The name of the method whose body is currently being checked, if
    // any - needed to tell whether a `readonly` property write is
    // happening inside its own declaring class's constructor (Finding
    // 47's pragmatic first cut: any write outside that exact spot is
    // flagged).
    current_method: Option<String>,
    // Set for the duration of one function/method (or the top-level script)
    // body when it calls extract()/eval() - both can define variables this
    // checker has no way to see, so undefined-variable detection would be
    // pure noise for the rest of that body. Rather than track precisely
    // which diagnostics came from that check, the whole body's diagnostics
    // are discarded if this ends up true (see check_function).
    saw_dynamic_var_construct: bool,
}

impl<'a> Checker<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Checker {
            registry,
            diagnostics: Vec::new(),
            name_ctx: NameContext::default(),
            current_class: None,
            current_method: None,
            saw_dynamic_var_construct: false,
        }
    }

    pub fn into_diagnostics(mut self) -> Vec<Diagnostic> {
        // Internal checks run in traversal order (properties/bounds
        // before method bodies, methods in AST declaration order, an
        // assignment's right-hand side before its left, ...) - not
        // necessarily line/column order once different diagnostic
        // categories or evaluation orders interleave. Sorted once here,
        // the single real source every caller (CLI output, LSP
        // publishing, this module's own tests) goes through.
        self.diagnostics
            .sort_by_key(|d| (d.span.start.line, d.span.start.col));
        self.diagnostics
    }

    pub fn check_program(&mut self, program: &[Stmt]) {
        // A simple whole-file pass (last `namespace` seen wins), unlike
        // `Registry::index`'s positional tracking - brace-delimited
        // multi-namespace-per-file is rare enough in real application
        // code that the extra precision isn't worth it here; a mismatch
        // in such a rare file only risks a missed diagnostic, not the
        // silent method-table corruption the registry itself had to
        // guard against.
        self.name_ctx = NameContext::build(program);

        let mut top_locals: HashMap<String, Type> = HashMap::new();
        let diagnostics_before = self.diagnostics.len();
        for stmt in program {
            match &stmt.kind {
                StmtKind::FunctionDecl(f) => self.check_function(f, None, false),
                StmtKind::ClassDecl(c) => {
                    // `current_class` must be the fully-qualified name -
                    // it's looked up directly against
                    // `self.registry.classes` (via `resolve_pseudo` for
                    // self/parent/static, and the undefined-class check
                    // for `new self()`/`new static()`), which is now
                    // FQN-keyed.
                    let fqn = self.name_ctx.declared_name(&c.name);
                    self.check_class_generic_bounds(&fqn, c);
                    self.check_class_completeness(&fqn, c.span);
                    self.check_final_class_extension(&fqn, c.span);
                    self.check_method_overrides(&fqn, c);
                    self.check_trait_conflicts(&fqn, c.span);
                    for m in &c.methods {
                        self.check_function(&m.function, Some(&fqn), m.is_static);
                    }
                }
                StmtKind::EnumDecl(e) => {
                    let fqn = self.name_ctx.declared_name(&e.name);
                    self.check_class_completeness(&fqn, e.span);
                    self.check_enum_case_values(&fqn, e);
                    self.check_trait_conflicts(&fqn, e.span);
                    for m in &e.methods {
                        self.check_function(&m.function, Some(&fqn), m.is_static);
                    }
                }
                // Top-level script statements share one scope across the
                // whole file, so the extract()/eval() guard spans the
                // entire loop rather than resetting per statement.
                _ => self.check_stmt(stmt, &mut top_locals, None, "<main>"),
            }
        }
        if self.saw_dynamic_var_construct {
            self.diagnostics.truncate(diagnostics_before);
            self.saw_dynamic_var_construct = false;
        }
    }

    /// Looks up this exact declaration's own resolved signature in the
    /// registry (a free function by name, or a method by class+name).
    /// Returns a reference tied to the registry's lifetime, not `&self`'s,
    /// so holding it doesn't conflict with the `&mut self` calls that
    /// follow once its params/return type have been copied into `locals`.
    /// A real bug this found: for a top-level function, `Registry::index`
    /// keys `self.registry.functions` by the function's fully-qualified
    /// name (Phase 9.5), but a bare `self.registry.functions.get(&f.name)`
    /// here would look it up by its raw, unqualified AST name - a
    /// namespaced function's own signature (with correctly-resolved
    /// param/return types) was silently never found, falling through to
    /// `check_function`'s `None` branch, which re-derives types straight
    /// from native hints with *no* namespace resolution applied at all.
    /// Methods don't share this bug: `class_name` is already the FQN by
    /// the time it gets here (`check_program`'s own `StmtKind::ClassDecl`
    /// handling resolves it), and a method's own name is never namespace-
    /// qualified in the first place - only the class it's declared in is.
    fn lookup_own_signature(
        &self,
        f: &FunctionDecl,
        class_name: Option<&str>,
    ) -> Option<&'a FunctionSig> {
        match class_name {
            Some(c) => self.registry.classes.get(c)?.methods.get(&f.name),
            None => self
                .registry
                .functions
                .get(&self.name_ctx.declared_name(&f.name)),
        }
    }

    fn check_function(&mut self, f: &FunctionDecl, class_name: Option<&str>, is_static: bool) {
        let sig = self.lookup_own_signature(f, class_name);

        let mut locals: HashMap<String, Type> = HashMap::new();
        match sig {
            // The registry's FunctionSig already has PHPDoc types merged in
            // (Phase 2) - using it here, instead of re-deriving from the
            // raw AST's native hints, is what makes an untyped-but-
            // doc-commented parameter usable inside its own function body,
            // not just checkable from the outside at call sites.
            Some(sig) => {
                for param in &sig.params {
                    // `self`/`static`/`parent` need resolving against the
                    // enclosing class the same way `$this` and a return
                    // type already are - without this, a `self`-typed
                    // parameter (`function equals(self $other): bool`)
                    // stays an unresolved pseudo-type for the whole
                    // function body, and infer_expr's PropertyAccess/
                    // MethodCall arms only match a concrete `Object`, so
                    // every property/method access through it silently
                    // fell through to `Mixed` - no undefined-property/
                    // method checking at all, unlike the identical access
                    // through `$this`.
                    let ty = match class_name {
                        Some(c) => self.resolve_pseudo(&param.ty, c),
                        None => param.ty.clone(),
                    };
                    // `ParamSig::ty` holds a variadic parameter's own
                    // *element* type (the right shape for checking each
                    // individual argument at a call site) - inside the
                    // function's own body, real PHP always collects a
                    // variadic parameter into an `array` from the first
                    // statement onward, never the bare element type.
                    let ty = if param.variadic {
                        Type::Array(Box::new(ty))
                    } else {
                        ty
                    };
                    locals.insert(param.name.clone(), ty);
                }
            }
            // Shouldn't happen (this function had to be indexed to be
            // reachable here at all), but degrade rather than panic.
            None => {
                for p in &f.params {
                    let ty = p
                        .type_hint
                        .as_deref()
                        .map(types::parse_type_string)
                        .unwrap_or(Type::Mixed);
                    let ty = if p.variadic {
                        Type::Array(Box::new(ty))
                    } else {
                        ty
                    };
                    locals.insert(p.name.clone(), ty);
                }
            }
        }
        // A static method has no $this at all in real PHP (calling one
        // outside object context is a fatal error) - only bind it for a
        // genuinely non-static method, so a static method's own body
        // referencing $this correctly reads as an undefined variable
        // rather than silently resolving to the enclosing class.
        if let Some(c) = class_name
            && !is_static
        {
            locals.insert("this".to_string(), Type::Object(c.to_string(), Vec::new()));
        }

        // A signature's return type already defaults to Mixed when nothing
        // was declared, and checking a return statement against Mixed
        // never flags anything - so this doesn't need to distinguish "no
        // return type" from "declared mixed" the way the fallback below
        // does with an Option.
        let declared_return = sig
            .map(|s| s.return_type.clone())
            .or_else(|| f.return_type.as_deref().map(types::parse_type_string));
        let label = match class_name {
            Some(c) => format!("{c}::{}", f.name),
            None => f.name.clone(),
        };

        // Phase 10.5's bound-enforcement: checked once here, against the
        // declaration itself, not at every call site - and deliberately
        // outside the dynamic-var-construct truncation below, since a
        // bound violation is about the signature as written, not
        // something a `extract()` in the *body* should be able to hide.
        if let Some(sig) = sig {
            for (param, ast_param) in sig.params.iter().zip(&f.params) {
                self.check_generic_bounds(&param.ty, &label, ast_param.span);
            }
        }
        self.check_generic_bounds(
            declared_return.as_ref().unwrap_or(&Type::Mixed),
            &label,
            f.span,
        );

        // Phase 10.5's variance-enforcement half - same "check the
        // declaration once" placement as bound-enforcement above, and
        // only meaningful for a method (a plain top-level function has no
        // enclosing class to have declared variance for at all).
        // `__construct` is exempt (real PHPStan's own rule exempts it too):
        // a constructor is only ever called to build one specific,
        // concrete instance (`new Success<Cat>($cat)`), never invoked
        // polymorphically through a variable typed as the covariant
        // supertype the way an ordinary method can be - so a covariant
        // `T` constructor parameter doesn't actually violate covariance
        // safety at all. Real-world confirmation: every one of the 27
        // diagnostics this check produced on the first real-corpus pass,
        // with zero exceptions, was on a constructor - the extremely
        // common `readonly` value-holder pattern (`public function
        // __construct(private readonly T $value) {}`), not a genuine bug.
        if let (Some(sig), Some(c)) = (sig, class_name)
            && f.name != "__construct"
        {
            self.check_variance(sig, c, &label, f.span);
        }

        let previous_class = self.current_class.take();
        self.current_class = class_name.map(|s| s.to_string());
        let previous_method = self.current_method.take();
        self.current_method = Some(f.name.clone());

        let diagnostics_before = self.diagnostics.len();
        let previous_dynamic_flag = self.saw_dynamic_var_construct;
        self.saw_dynamic_var_construct = false;

        for stmt in &f.body {
            self.check_stmt(stmt, &mut locals, declared_return.as_ref(), &label);
        }

        // A function that falls off the end of its body with no `return`
        // at all implicitly returns `null` too - equally a real mismatch
        // against a non-nullable, non-void, non-mixed declared return
        // type, and equally easy to miss without this: `always_terminates`
        // (already used for guard-clause narrowing) answers "does this
        // statement definitely end control flow" - a `false` here means
        // this function's own body doesn't guarantee returning anything.
        if let Some(expected) = &declared_return
            && let Some(last) = f.body.last()
            && !self.always_terminates(last, &locals)
        {
            let expected = match &self.current_class {
                Some(c) => self.resolve_pseudo(expected, c),
                None => expected.clone(),
            };
            if expected != Type::Void
                && expected != Type::Mixed
                && !self.registry.is_assignable(&Type::Null, &expected)
            {
                self.diagnostics.push(Diagnostic {
                    location: label.clone(),
                    message: format!("return type mismatch: expected {expected}, got null"),
                    span: f.span,
                    level: 3,
                });
            }
        }

        if self.saw_dynamic_var_construct {
            self.diagnostics.truncate(diagnostics_before);
        }
        self.saw_dynamic_var_construct = previous_dynamic_flag;

        self.current_class = previous_class;
        self.current_method = previous_method;
    }

    /// Resolves `self`/`static`/`parent` to a concrete `Object` relative to
    /// `defining_class` - the class whose signature this type came from.
    /// `self`/`static` both collapse to the same class here since we don't
    /// track late static binding through actual runtime dispatch; `parent`
    /// resolves via the registry's extends chain, falling back to leaving
    /// it unresolved if the class has no declared parent (so it still hits
    /// the permissive Object fallback in `is_subtype`/`is_assignable`
    /// rather than being wrongly flagged).
    /// Resolves a bare name exactly as it appears at an expression site
    /// (`new Foo()`, `$x instanceof Foo`, ...) to the concrete `Type` it
    /// refers to - `self`/`parent`/`static` need `resolve_pseudo` against
    /// the class currently being checked, everything else goes through
    /// `name_ctx.resolve` (namespace/`use`-import resolution) as an
    /// ordinary class reference. A real bug this fixed: `apply_narrowing`'s
    /// `instanceof` handling used to treat `self`/`parent`/`static` as
    /// literal class names to look up via `name_ctx.resolve` directly
    /// (`$future instanceof self` in real code became a nonsensical
    /// `Amp\self` "class" once namespace-qualification existed to make
    /// the mistake visible) - `infer_expr`'s `ExprKind::New` arm already got
    /// this right; `apply_narrowing` never shared that logic.
    fn resolve_name_expr(&self, name: &str) -> Type {
        match name {
            "self" | "parent" | "static" => {
                let raw = match name {
                    "self" => Type::SelfRef,
                    "parent" => Type::ParentRef,
                    _ => Type::StaticRef,
                };
                match &self.current_class {
                    Some(c) => self.resolve_pseudo(&raw, c),
                    None => raw,
                }
            }
            _ => Type::Object(self.name_ctx.resolve(name), Vec::new()),
        }
    }

    /// Parses a raw native type-hint string (a closure/arrow-fn param or
    /// return type - the one place in this checker that builds a `Type`
    /// straight from source without going through the registry's own
    /// `FunctionSig::from_decl`, which already resolves names as part of
    /// its pipeline) and resolves any class name in it against this
    /// file's `NameContext`, exactly like a real declaration would. Found
    /// necessary the hard way: an unresolved bare name here compared as a
    /// real mismatch against the correctly-resolved FQN the closure body
    /// itself infers, on real code (a `static fn(array $row): ActionCount
    /// => new ActionCount(...)` where `ActionCount` is `use`-imported).
    fn resolve_type_hint(&self, hint: Option<&str>) -> Type {
        match hint {
            Some(h) => registry::resolve_names_in_type(types::parse_type_string(h), &self.name_ctx),
            None => Type::Mixed,
        }
    }

    /// Resolves a `StaticAccess`/`StaticCall` target (`self`/`parent`/
    /// `static`, or a plain/namespaced class name) to a concrete FQN.
    /// `"?"` is the parser's own placeholder for a genuinely dynamic
    /// class target (`$obj::method()`, `(new Foo())::CONST`) - there's no
    /// name to resolve at all there, so it's `None`, same as any other
    /// case `resolve_name_expr` can't turn into a concrete `Object`
    /// (`self`/`parent`/`static` used outside any class context).
    fn resolve_static_class_name(&self, class_name: &str) -> Option<String> {
        if class_name == "?" {
            return None;
        }
        match self.resolve_name_expr(class_name) {
            Type::Object(class, _) => Some(class),
            _ => None,
        }
    }

    /// Resolves `self`/`static`/`parent` to a concrete class - recurses
    /// into `Union`/`Intersection`/`Array`/`ArrayShape` the same way
    /// `types::substitute_template` does, not just the bare top-level
    /// case. A real, previously-live gap this found: the extremely
    /// common nullable-self return type (`?self`/`self|null`, e.g.
    /// `tryFromArray`-style factory methods) never resolved at all
    /// before this - only a *bare* `self` return type ever reached the
    /// `SelfRef` arm directly, so `self` wrapped in *any* union stayed
    /// unresolved, and `is_assignable`/`is_subtype` have no arm at all
    /// for a concrete class instance satisfying a still-unresolved
    /// `SelfRef` (only the reverse direction, `self` satisfying a
    /// concrete class, is handled) - a real return-type mismatch was
    /// reported on entirely correct code returning `new self(...)` or
    /// `null` from a `?self`-typed method.
    fn resolve_pseudo(&self, ty: &Type, defining_class: &str) -> Type {
        match ty {
            Type::SelfRef | Type::StaticRef => Type::Object(defining_class.to_string(), Vec::new()),
            Type::ParentRef => self
                .registry
                .classes
                .get(defining_class)
                .and_then(|info| info.extends.first())
                .map(|p| Type::Object(p.clone(), Vec::new()))
                .unwrap_or_else(|| ty.clone()),
            Type::Array(inner) => Type::Array(Box::new(self.resolve_pseudo(inner, defining_class))),
            Type::Union(members) => Type::Union(
                members
                    .iter()
                    .map(|m| self.resolve_pseudo(m, defining_class))
                    .collect(),
            ),
            Type::Intersection(members) => Type::Intersection(
                members
                    .iter()
                    .map(|m| self.resolve_pseudo(m, defining_class))
                    .collect(),
            ),
            Type::ArrayShape(fields) => Type::ArrayShape(
                fields
                    .iter()
                    .map(|(k, t, opt)| (k.clone(), self.resolve_pseudo(t, defining_class), *opt))
                    .collect(),
            ),
            _ => ty.clone(),
        }
    }

    /// The classic guard clause (`if ($x === null) { return; }`, or the
    /// less obvious `if (! $x instanceof Foo) { $this->fatalError(...); }`)
    /// depends on recognizing a branch that never falls through, so its
    /// (narrowed) scope doesn't incorrectly leak past the `if`. A bare
    /// call expression counts too, not just `return`/`throw`/`break`/
    /// `continue`, when its own return type is `never` - a real, common
    /// pattern (`private function fatalError(string $msg): never`) that a
    /// purely syntactic check would miss entirely.
    fn always_terminates(&self, stmt: &Stmt, locals: &HashMap<String, Type>) -> bool {
        match &stmt.kind {
            StmtKind::Return(_)
            | StmtKind::Throw(_)
            | StmtKind::Break(_)
            | StmtKind::Continue(_) => true,
            StmtKind::Expression(e) => self.infer_expr(e, locals) == Type::Never,
            StmtKind::Block(stmts) => stmts
                .last()
                .is_some_and(|s| self.always_terminates(s, locals)),
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.always_terminates(then_branch, locals)
                    && else_branch
                        .as_deref()
                        .is_some_and(|e| self.always_terminates(e, locals))
            }
            // A `switch` guarantees ending control flow only when there's
            // a `default` (otherwise the subject might match nothing and
            // fall straight through, doing nothing) and every non-empty
            // case body genuinely terminates - not just `break`s, which
            // only exits the switch itself and falls through to whatever
            // follows it, not out of the enclosing function. An empty
            // body (`case a: case b: ...`, a pure fallthrough label) is
            // skipped rather than required to independently terminate -
            // only the eventual non-empty body it falls into needs to.
            StmtKind::Switch { cases, .. } => {
                cases.iter().any(|c| c.test.is_none())
                    && cases.iter().all(|c| {
                        c.body.is_empty()
                            || c.body.last().is_some_and(|s| {
                                !matches!(s.kind, StmtKind::Break(_))
                                    && self.always_terminates(s, locals)
                            })
                    })
            }
            // A `try` guarantees ending control flow when its own body
            // does (whether by returning normally or by throwing - an
            // uncaught throw propagates out of the function just as
            // surely as a caught one returns from a catch block) *and*
            // every one of its `catch` clauses also does. An empty
            // `catches` list (a bare `try { ... } finally { ... }`)
            // vacuously satisfies the second half - nothing can catch and
            // fail to terminate if nothing catches at all. `finally`
            // itself doesn't need to independently terminate for this
            // question - it always runs before whatever the try/catch
            // already decided.
            StmtKind::Try { body, catches, .. } => {
                body.last()
                    .is_some_and(|s| self.always_terminates(s, locals))
                    && catches.iter().all(|c| {
                        c.body
                            .last()
                            .is_some_and(|s| self.always_terminates(s, locals))
                    })
            }
            _ => false,
        }
    }

    /// Whether `stmt` exits the whole enclosing function or loop
    /// (`return`/`throw`/`continue`, or a nested construct that
    /// guarantees one of those) rather than merely exiting a `switch`
    /// itself (`break`, or not transferring control at all). Distinct
    /// from `always_terminates`'s own notion of "doesn't fall through to
    /// the next case" (which correctly treats `break` as terminating
    /// *that* question) - a switch's own post-switch scope merge needs
    /// this different distinction instead: a case ending in `break`
    /// still reaches the code after the switch, one ending in
    /// `return`/`throw`/`continue` never does.
    fn exits_function_or_loop(&self, stmt: &Stmt, locals: &HashMap<String, Type>) -> bool {
        match &stmt.kind {
            StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Continue(_) => true,
            StmtKind::Break(_) => false,
            StmtKind::Expression(e) => self.infer_expr(e, locals) == Type::Never,
            StmtKind::Block(stmts) => stmts
                .last()
                .is_some_and(|s| self.exits_function_or_loop(s, locals)),
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.exits_function_or_loop(then_branch, locals)
                    && else_branch
                        .as_deref()
                        .is_some_and(|e| self.exits_function_or_loop(e, locals))
            }
            _ => false,
        }
    }

    /// Walks `ty` for every `Object(class, args)` naming a class with its
    /// own `@template T of Bound` (Phase 10.5's bound-enforcement half),
    /// flagging any argument that doesn't satisfy its bound - e.g.
    /// `Collection<Foo>` where `Collection`'s own `@template T of Shape`
    /// and `Foo` doesn't implement `Shape`. Recurses into every argument
    /// too (so `Collection<Box<Foo>>` still checks `Box`'s own bound), and
    /// into `Union`/`Intersection`/`Array`/`ArrayShape` members (a bound
    /// violation doesn't stop being real just because it's wrapped in a
    /// union or is one array's element type).
    ///
    /// Bindings are computed once, positionally, before any bound is
    /// checked - not just so multiple arguments against multiple bounds
    /// each see the right value, but because a bound can itself reference
    /// another of the *same* class's own templates (`@template A`
    /// `@template B of A`), which only resolves correctly once `A`'s own
    /// concrete argument is known.
    ///
    /// Called once per declared type (a function's params/return type, a
    /// class's own properties, its `@extends`/`@implements` tags) rather
    /// than at every call/access site - a generic type only needs
    /// flagging once, where it's *written*, the same way real PHPStan's
    /// own `GenericObjectTypeCheck` documents itself.
    fn check_generic_bounds(&mut self, ty: &Type, label: &str, span: Span) {
        match ty {
            Type::Object(class, args) => {
                if !args.is_empty()
                    && let Some(info) = self.registry.classes.get(class)
                    && !info.template_bounds.is_empty()
                {
                    let bindings: HashMap<String, Type> = info
                        .templates
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect();
                    for (name, arg) in info.templates.iter().zip(args.iter()) {
                        let Some(bound) = info.template_bounds.get(name) else {
                            continue;
                        };
                        let bound = types::substitute_template(bound, &bindings);
                        // Same "don't know, don't guess" leniency every
                        // other undefined-symbol-adjacent check in this
                        // project already applies to a class the registry
                        // has no real data for (an uninstalled dependency,
                        // or - the real case found verifying this against
                        // Piwigo's corpus - a PHAR-bundled one like
                        // `phpstan/phpstan` itself, which ships with no
                        // loose source files to parse at all): without it,
                        // every reference to one of its own real classes,
                        // e.g. `PHPStan\Type\ArrayType implements
                        // PHPStan\Type\Type`, reads as violating a bound
                        // this project simply has no way to verify either
                        // way, not one it's actually confirmed broken.
                        if *arg != Type::Mixed
                            && self.is_type_fully_known(arg)
                            && self.is_type_fully_known(&bound)
                            && !self.registry.is_assignable(arg, &bound)
                        {
                            self.diagnostics.push(Diagnostic {
                                location: label.to_string(),
                                message: format!(
                                    "{class}'s @template {name} requires {bound}, got {arg}"
                                ),
                                span,
                                level: 6,
                            });
                        }
                    }
                }
                for arg in args {
                    self.check_generic_bounds(arg, label, span);
                }
            }
            Type::Array(inner) => self.check_generic_bounds(inner, label, span),
            Type::Union(members) | Type::Intersection(members) => {
                for m in members {
                    self.check_generic_bounds(m, label, span);
                }
            }
            Type::ArrayShape(fields) => {
                for (_, t, _) in fields {
                    self.check_generic_bounds(t, label, span);
                }
            }
            _ => {}
        }
    }

    /// `false` if `ty` mentions any class this project has no real data
    /// for at all - an uninstalled dependency, or a PHAR-bundled one
    /// (real `phpstan/phpstan` ships as a compiled PHAR with no loose
    /// source files, the same gap Phase 9's own undefined-symbol checks
    /// already document). `check_generic_bounds` uses this to skip a
    /// bound it has no way to actually verify, rather than guess wrong.
    fn is_type_fully_known(&self, ty: &Type) -> bool {
        match ty {
            Type::Object(name, args) => {
                (name == "object" || self.registry.classes.contains_key(name))
                    && args.iter().all(|a| self.is_type_fully_known(a))
            }
            Type::Array(inner) => self.is_type_fully_known(inner),
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().all(|m| self.is_type_fully_known(m))
            }
            Type::ArrayShape(fields) => fields.iter().all(|(_, t, _)| self.is_type_fully_known(t)),
            _ => true,
        }
    }

    /// `check_generic_bounds` for everything a class declares outside its
    /// own methods (which `check_function` already covers per-method):
    /// every property's type, plus its own `@extends`/`@implements`
    /// arguments - both are real places a generic bound violation can be
    /// written (`class Bad extends Collection<NotAShape> {}`) without
    /// ever showing up in a method signature at all.
    fn check_class_generic_bounds(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        // Each property gets its own real span (declaration order, not a
        // sorted HashMap walk - the AST's `Vec<PropertyDecl>` is already
        // deterministic, unlike the registry's `HashMap`-backed lookup).
        for p in &c.properties {
            if let Some(prop_sig) = info.properties.get(&p.name) {
                self.check_generic_bounds(&prop_sig.ty, fqn, p.span);
            }
        }
        // `extends_args`/`implements_args` store just the argument list,
        // not paired with the parent/interface's own name - re-pair each
        // one into a real `Object(class, args)` here, or
        // `check_generic_bounds` has no class to look bounds up against
        // at all (it would just see the *arguments* as if they were the
        // generic reference themselves). Neither the parent name nor an
        // interface name carries its own span in the AST, so both use the
        // class declaration's own span - imprecise. but still real.
        let mut types_to_check: Vec<Type> = Vec::new();
        if let Some(parent) = info.extends.first() {
            types_to_check.push(Type::Object(parent.clone(), info.extends_args.clone()));
        }
        // `implements_args` is a `HashMap` with no guaranteed iteration
        // order - sorted by key so two diagnostics from it always come out
        // in the same relative order, not one that silently varies by
        // process (Rust's default hasher is randomly seeded per run).
        let mut iface_names: Vec<&String> = info.implements_args.keys().collect();
        iface_names.sort();
        for iface in iface_names {
            types_to_check.push(Type::Object(
                iface.clone(),
                info.implements_args[iface].clone(),
            ));
        }
        for ty in types_to_check {
            self.check_generic_bounds(&ty, fqn, c.span);
        }
    }

    /// Finding 46's class-level half: extending a class declared `final`
    /// is a real, unconditional fatal error in PHP at class-declaration
    /// time, regardless of whether the subclass overrides anything at
    /// all. The method-level half (overriding a `final` *method*) lives
    /// in `check_method_overrides`, sharing that function's own
    /// per-method ancestor walk instead of a second one here.
    fn check_final_class_extension(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let Some(parent) = info.extends.first() else {
            return;
        };
        let Some(parent_info) = self.registry.classes.get(parent) else {
            return;
        };
        if parent_info.is_final {
            self.diagnostics.push(Diagnostic {
                location: fqn.to_string(),
                message: format!("class {fqn} cannot extend final class {parent}"),
                span,
                level: 0,
            });
        }
    }

    /// Finding 58: two traits used by the same class declaring the same
    /// property (with a genuinely different type) or method (with a
    /// genuinely different signature) is a real, unconditional PHP fatal
    /// error at class-declaration time unless the class explicitly
    /// resolves the conflict (`insteadof`/`as` - Finding 30's own
    /// `resolved_trait_conflicts`, tracked regardless of whether this
    /// project models exactly which trait `insteadof` picked). Compares
    /// only pairs of *different* traits - a name declared by just one
    /// used trait is never a conflict, no matter what it looks like.
    fn check_trait_conflicts(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.uses.len() < 2 {
            return;
        }

        let mut seen_props: HashMap<&str, (&str, &PropertySig)> = HashMap::new();
        let mut seen_methods: HashMap<&str, (&str, &FunctionSig)> = HashMap::new();
        for trait_name in &info.uses {
            let Some(trait_info) = self.registry.classes.get(trait_name) else {
                continue;
            };
            for (prop_name, sig) in &trait_info.properties {
                if info.resolved_trait_conflicts.contains(prop_name) {
                    continue;
                }
                match seen_props.get(prop_name.as_str()) {
                    Some((first_trait, first_sig)) if first_sig.ty != sig.ty => {
                        self.diagnostics.push(Diagnostic {
                            location: fqn.to_string(),
                            message: format!(
                                "{first_trait} and {trait_name} define the same property (${prop_name}) with incompatible types in the composition of {fqn}"
                            ),
                            span,
                            level: 0,
                        });
                    }
                    Some(_) => {}
                    None => {
                        seen_props.insert(prop_name, (trait_name, sig));
                    }
                }
            }
            for (method_name, sig) in &trait_info.methods {
                if info.resolved_trait_conflicts.contains(method_name) {
                    continue;
                }
                match seen_methods.get(method_name.as_str()) {
                    Some((first_trait, first_sig)) if !signatures_compatible(first_sig, sig) => {
                        self.diagnostics.push(Diagnostic {
                            location: fqn.to_string(),
                            message: format!(
                                "{first_trait} and {trait_name} define the same method ({method_name}) with an incompatible signature in the composition of {fqn}"
                            ),
                            span,
                            level: 0,
                        });
                    }
                    Some(_) => {}
                    None => {
                        seen_methods.insert(method_name, (trait_name, sig));
                    }
                }
            }
        }
    }

    /// Finding 59: every case of a backed enum must have a unique backing
    /// value - a real, unconditional PHP fatal error at declaration time
    /// otherwise ("Duplicate value in enum ... for case ..."), not merely
    /// a design smell. Only compares cases whose value is a literal
    /// int/string (the overwhelming majority in practice) - a computed or
    /// otherwise non-literal case value isn't evaluated, matching this
    /// project's usual "don't guess" leniency.
    fn check_enum_case_values(&mut self, fqn: &str, e: &EnumDecl) {
        let mut seen: HashMap<(bool, String), &str> = HashMap::new();
        for case in &e.cases {
            let Some(value) = &case.value else {
                continue;
            };
            let key = match &value.kind {
                ExprKind::Literal(Literal::Int(n)) => (true, n.to_string()),
                ExprKind::Literal(Literal::String(s)) => (false, s.clone()),
                _ => continue,
            };
            if let Some(first_case) = seen.get(&key) {
                self.diagnostics.push(Diagnostic {
                    location: fqn.to_string(),
                    message: format!(
                        "duplicate value in enum {fqn} for case {} (already used by case {first_case})",
                        case.name
                    ),
                    span: case.span,
                    level: 0,
                });
            } else {
                seen.insert(key, &case.name);
            }
        }
    }

    /// Finding 45: a concrete class must provide a real implementation of
    /// every abstract method it inherits (from a parent class, an
    /// implemented interface, or a directly-`use`d trait) - real PHP
    /// raises a fatal error at declaration time otherwise, independent of
    /// whether anything ever calls the missing method. Interfaces,
    /// traits, and abstract classes are themselves exempt (a class that
    /// legitimately leaves a method unimplemented is required to declare
    /// itself `abstract`, and one that does is not itself required to be
    /// complete).
    fn check_class_completeness(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.kind != ClassKind::Class && info.kind != ClassKind::Enum {
            return;
        }
        if info.is_abstract {
            return;
        }
        let required = self.registry.required_abstract_methods(fqn);
        let missing: Vec<&String> = required
            .iter()
            .filter(|m| !self.registry.has_concrete_implementation(fqn, m))
            .collect();
        if missing.is_empty() {
            return;
        }
        let shown: Vec<&str> = missing.iter().take(3).map(|s| s.as_str()).collect();
        let mut message = format!(
            "class {fqn} does not implement abstract method(s): {}",
            shown.join(", ")
        );
        if missing.len() > shown.len() {
            message.push_str(&format!(", and {} more", missing.len() - shown.len()));
        }
        self.diagnostics.push(Diagnostic {
            location: fqn.to_string(),
            message,
            span,
            level: 0,
        });
    }

    /// Finding 50: an overriding method's signature must stay
    /// Liskov-compatible with the method it overrides - a parameter type
    /// may only be widened or kept the same (accepting more is safe),
    /// never narrowed; a return type may only be narrowed or kept the
    /// same (promising more is safe), never widened. Real PHP enforces
    /// this unconditionally at class-declaration time. Deliberately
    /// scoped to concrete parameter/return type mismatches only, not
    /// generic type-argument variance within an overridden signature -
    /// this project's own `is_assignable` is already lenient there by
    /// design (guessing wrong in either direction is worse than not
    /// checking), and that leniency is the right call here too.
    ///
    /// Also covers Finding 46's method-level half: overriding a method
    /// declared `final` anywhere in the ancestor chain, sharing this same
    /// per-method loop since both checks need the same "does this
    /// override collide with an ancestor's declaration" walk.
    fn check_method_overrides(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for m in &c.methods {
            // Finding 46: overriding a `final` method is a real,
            // unconditional fatal error in PHP - including for
            // __construct, unlike the signature-compatibility checks
            // below, so this runs before that exemption.
            if let Some(ancestor) = self
                .registry
                .find_final_ancestor_method(fqn, &m.function.name)
            {
                self.diagnostics.push(Diagnostic {
                    location: fqn.to_string(),
                    message: format!(
                        "{fqn}::{}() cannot override final method {ancestor}::{}()",
                        m.function.name, m.function.name
                    ),
                    span: m.span,
                    level: 0,
                });
            }
            // Real PHP deliberately exempts constructors from override-
            // compatibility checking - a subclass's own __construct is
            // allowed an entirely different, otherwise-incompatible
            // signature from its parent's, unlike every other method.
            if m.function.name == "__construct" {
                continue;
            }
            let Some(own_sig) = info.methods.get(&m.function.name) else {
                continue;
            };
            let Some((_, ancestor_sig)) = self
                .registry
                .lookup_ancestor_method_with_owner(fqn, &m.function.name)
            else {
                continue;
            };
            for (i, ancestor_param) in ancestor_sig.params.iter().enumerate() {
                let Some(own_param) = own_sig.params.get(i) else {
                    continue;
                };
                // `Type::Mixed` is overloaded throughout this project to
                // mean both real PHP `mixed` *and* "no type declared at
                // all, native or docblock" (see this file's own opening
                // comment) - and real PHP explicitly does NOT enforce
                // override compatibility for a parameter/return position
                // that isn't typed at all (only an explicitly *narrower*
                // declared type is a real fatal error). Since an
                // intentional, explicit `mixed` on an override is rare
                // next to "just wasn't declared", skipping either side
                // being Mixed avoids flagging the overwhelmingly common
                // case (confirmed via vendor/phpstorm-stubs: dozens of
                // real stub methods simply don't redeclare an inherited
                // native return type) at the cost of missing the rare
                // genuine `mixed`-narrows-a-real-type case - the same
                // "don't guess wrong on Mixed" tradeoff `is_assignable`
                // already makes elsewhere in this project.
                if own_param.ty == Type::Mixed || ancestor_param.ty == Type::Mixed {
                    continue;
                }
                if !self
                    .registry
                    .is_assignable(&ancestor_param.ty, &own_param.ty)
                {
                    self.diagnostics.push(Diagnostic {
                        location: fqn.to_string(),
                        message: format!(
                            "{fqn}::{}() parameter #{} (${}) narrows the inherited parameter type: expects {}, ancestor expects {}",
                            m.function.name,
                            i + 1,
                            own_param.name,
                            own_param.ty,
                            ancestor_param.ty
                        ),
                        span: m.span,
                        level: 0,
                    });
                }
            }
            // An ancestor's own `static`-typed return (`self.registry`
            // has no way to resolve what `static` meant relative to the
            // *ancestor's* declaring class, only the checker's own
            // current-class-aware `resolve_pseudo`, which isn't
            // available here) is real PHP's own explicit "late static
            // binding" covariance escape hatch - a subclass narrowing it
            // to its own concrete class name (the overwhelmingly common
            // real shape, confirmed via vendor/phpstorm-stubs'
            // `Ds\Collection::copy(): static` and its own subclasses'
            // `copy(): Vector`/`copy(): Stack`/...) is always exactly
            // what real PHP allows, never a real incompatibility.
            if own_sig.return_type != Type::Mixed
                && ancestor_sig.return_type != Type::Mixed
                && ancestor_sig.return_type != Type::StaticRef
                && !self
                    .registry
                    .is_assignable(&own_sig.return_type, &ancestor_sig.return_type)
            {
                self.diagnostics.push(Diagnostic {
                    location: fqn.to_string(),
                    message: format!(
                        "{fqn}::{}() return type {} is not compatible with the inherited return type {}",
                        m.function.name, own_sig.return_type, ancestor_sig.return_type
                    ),
                    span: m.span,
                    level: 0,
                });
            }
        }
    }

    /// Phase 10.5's variance-enforcement half: a class's own
    /// `@template-covariant T` may only appear in one of its methods'
    /// *return* types (an output position - covariance is only sound if
    /// nothing can ever hand a `T` back *into* the generic container,
    /// the way `Collection<Cat>` being usable where `Collection<Animal>`
    /// is expected relies on only ever reading a value out, never
    /// writing one in); `@template-contravariant T` is the mirror image,
    /// restricted to *parameter* positions only. A plain, un-annotated
    /// `@template T` (invariant, the only kind Phase 10 modeled at all)
    /// is unrestricted, same as always.
    ///
    /// Deliberately conservative in two ways, both matching this
    /// project's general preference for fewer false positives over
    /// catching everything: only checks method params/return types, not
    /// properties (this project doesn't track `readonly` at all yet, and
    /// a *readonly* property typed `T` is actually a legitimate output-
    /// only position - without that information, checking properties
    /// here would misflag every real one); and doesn't attempt real
    /// variance's positional *flipping* through a nested generic
    /// argument (a `T` found inside another class's own `Collection<T>`
    /// type argument is treated the same as a direct occurrence, not
    /// re-derived from that other class's own variance) - a full,
    /// correct algorithm needs real per-class variance composition this
    /// project doesn't attempt.
    fn check_variance(&mut self, sig: &FunctionSig, class_name: &str, label: &str, span: Span) {
        let Some(info) = self.registry.classes.get(class_name) else {
            return;
        };
        if info.covariant_templates.is_empty() && info.contravariant_templates.is_empty() {
            return;
        }
        let mut input_names = HashSet::new();
        for param in &sig.params {
            collect_template_params(&param.ty, &mut input_names);
        }
        let mut output_names = HashSet::new();
        collect_template_params(&sig.return_type, &mut output_names);

        // Sorted before iterating - both are `HashSet`s, and this project
        // has already been bitten once this phase by a `HashMap`/`HashSet`
        // iteration order silently varying a diagnostic's position
        // between runs (see `check_class_generic_bounds`'s own fix).
        let mut violations: Vec<String> = Vec::new();
        for name in &input_names {
            if info.covariant_templates.contains(name) {
                violations.push(format!(
                    "{class_name}'s @template-covariant {name} cannot be used in an input (parameter) position"
                ));
            }
        }
        for name in &output_names {
            if info.contravariant_templates.contains(name) {
                violations.push(format!(
                    "{class_name}'s @template-contravariant {name} cannot be used in an output (return) position"
                ));
            }
        }
        violations.sort();
        for message in violations {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message,
                span,
                level: 6,
            });
        }
    }

    /// Binds a generic-typed receiver's own carried type arguments
    /// (`Collection<User>` -> `{"T": Object("User", [])}`) against
    /// `target`'s own declared `@template` parameter names, walking up
    /// `class`'s `extends`/`implements` chain to reach it if `target`
    /// isn't `class` itself (Phase 10.5) - so a concrete subclass that
    /// fixes its parent's template param (`class IntCollection extends
    /// Collection<int> {}`) resolves `Collection`'s own `T` to `int` even
    /// though `IntCollection` isn't generic itself, and a generic
    /// subclass that passes its own template through (`class Box<T>
    /// extends Collection<T> {}`) resolves it to whatever `Box`'s own `T`
    /// is bound to at each use, not a fixed value.
    ///
    /// `target` matters because template names are scoped to the class
    /// that declares them, not globally unique - two unrelated classes in
    /// the same chain commonly both use `T` (confirmed the hard way on a
    /// real corpus: `symfony/type-info`'s `NullableType<T of Type>
    /// extends UnionType<T|BuiltinType<...>>`, where `UnionType` has its
    /// *own*, unrelated `T`). Returning bindings scoped to exactly
    /// `target`'s own level - not a single flat map merged across the
    /// whole chain - is what keeps those two `T`s from colliding into
    /// whichever one happened to be computed last.
    ///
    /// A non-generic class (empty `templates`), a bare unspecified-generic
    /// reference (empty `type_args`), or a chain with no matching
    /// `@extends`/`@implements` tag along the way to `target` all just
    /// leave that level's names unbound - `substitute_template` already
    /// treats an unbound `TemplateParam` as `Mixed`, the right
    /// conservative fallback for an "erased" generic.
    fn class_template_bindings(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
    ) -> HashMap<String, Type> {
        let mut seen = HashSet::new();
        self.class_template_bindings_rec(class, type_args, target, &mut seen)
            .unwrap_or_default()
    }

    /// `None` means `target` isn't reachable along this path at all (so
    /// the caller should treat every one of its templates as unbound,
    /// same as `Some` with an empty map) - the distinction only matters
    /// internally, to know when to keep searching sibling branches below.
    /// `seen` guards against a cyclic `extends`/`implements` graph
    /// (invalid PHP, but a malformed/partially-loaded real corpus could
    /// still produce one) - cloned per branch, not threaded through by
    /// mutable reference, so two independent ancestors (e.g. two
    /// interfaces that both eventually reach a common shared parent, a
    /// real "diamond" shape) can each still be searched.
    fn class_template_bindings_rec(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
        seen: &mut HashSet<String>,
    ) -> Option<HashMap<String, Type>> {
        if !seen.insert(class.to_string()) {
            return None;
        }
        let info = self.registry.classes.get(class)?;
        let own_bindings: HashMap<String, Type> = info
            .templates
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect();

        if class == target {
            return Some(own_bindings);
        }

        if let Some(parent) = info.extends.first() {
            let parent_args: Vec<Type> = info
                .extends_args
                .iter()
                .map(|t| types::substitute_template(t, &own_bindings))
                .collect();
            if let Some(found) =
                self.class_template_bindings_rec(parent, &parent_args, target, &mut seen.clone())
            {
                return Some(found);
            }
        }
        for iface in &info.implements {
            let iface_args: Vec<Type> = info
                .implements_args
                .get(iface)
                .map(|args| {
                    args.iter()
                        .map(|t| types::substitute_template(t, &own_bindings))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(found) =
                self.class_template_bindings_rec(iface, &iface_args, target, &mut seen.clone())
            {
                return Some(found);
            }
        }
        None
    }

    /// Applies `class_template_bindings`' substitution to every param and
    /// the return type of a method signature - the class-level counterpart
    /// to `infer_call_return_type`'s existing per-call-argument template
    /// binding (which only ever sees a method's *own* `@template`, not one
    /// declared on the enclosing class). `owner` is whichever ancestor of
    /// `class` actually declares `sig` (from `lookup_method_with_owner`/
    /// `lookup_property_with_owner`) - `class` itself when it's not
    /// inherited.
    fn substitute_class_templates(
        &self,
        sig: &FunctionSig,
        class: &str,
        type_args: &[Type],
        owner: &str,
    ) -> FunctionSig {
        let bindings = self.class_template_bindings(class, type_args, owner);
        FunctionSig {
            params: sig
                .params
                .iter()
                .map(|p| ParamSig {
                    ty: types::substitute_template(&p.ty, &bindings),
                    ..p.clone()
                })
                .collect(),
            return_type: types::substitute_template(&sig.return_type, &bindings),
        }
    }

    fn check_block(
        &mut self,
        stmts: &[Stmt],
        locals: &mut HashMap<String, Type>,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        for stmt in stmts {
            self.check_stmt(stmt, locals, declared_return, label);
        }
    }

    fn check_stmt(
        &mut self,
        stmt: &Stmt,
        locals: &mut HashMap<String, Type>,
        declared_return: Option<&Type>,
        label: &str,
    ) {
        match &stmt.kind {
            StmtKind::Expression(e) => {
                self.check_expr(e, locals, label);
            }
            StmtKind::Return(Some(e)) => {
                self.check_expr(e, locals, label);
                if let Some(expected) = declared_return {
                    let actual = self.infer_expr(e, locals);
                    let expected = match &self.current_class {
                        Some(c) => self.resolve_pseudo(expected, c),
                        None => expected.clone(),
                    };
                    if actual != Type::Mixed && !self.registry.is_assignable(&actual, &expected) {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!(
                                "return type mismatch: expected {expected}, got {actual}"
                            ),
                            span: e.span,
                            level: 3,
                        });
                    }
                }
            }
            // A bare `return;` always returns `null` in real PHP,
            // regardless of the function's declared return type - a
            // genuine mismatch inside a function declared to return a
            // concrete, non-nullable, non-`void` type (the same easy
            // slip as forgetting to add a return value after an
            // early-exit guard clause).
            StmtKind::Return(None) => {
                if let Some(expected) = declared_return {
                    let expected = match &self.current_class {
                        Some(c) => self.resolve_pseudo(expected, c),
                        None => expected.clone(),
                    };
                    if expected != Type::Void
                        && expected != Type::Mixed
                        && !self.registry.is_assignable(&Type::Null, &expected)
                    {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!("return type mismatch: expected {expected}, got null"),
                            span: stmt.span,
                            level: 3,
                        });
                    }
                }
            }
            StmtKind::Echo(exprs) => {
                for e in exprs {
                    self.check_expr(e, locals, label);
                }
            }
            StmtKind::Block(stmts) => self.check_block(stmts, locals, declared_return, label),
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr(cond, locals, label);

                let mut then_locals = locals.clone();
                self.apply_narrowing(cond, &mut then_locals, true);
                self.check_stmt(then_branch, &mut then_locals, declared_return, label);

                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                if let Some(e) = else_branch {
                    self.check_stmt(e, &mut else_locals, declared_return, label);
                }

                // The classic guard clause - `if ($x === null) { return; }`
                // - depends on this: a branch that unconditionally
                // returns/throws/breaks/continues never reaches the merge
                // point, so only the surviving branch's (narrowed) scope
                // should propagate past the if. Without this, the
                // terminated branch's narrowing (here, "$x is null")
                // would incorrectly leak into the code after the guard.
                let then_terminates = self.always_terminates(then_branch, &then_locals);
                let else_terminates = else_branch
                    .as_deref()
                    .is_some_and(|e| self.always_terminates(e, &else_locals));

                *locals = match (then_terminates, else_terminates) {
                    (true, false) => else_locals,
                    (false, true) => then_locals,
                    _ => merge_scopes(&then_locals, &else_locals),
                };
            }
            StmtKind::While { cond, body } => {
                self.check_expr(cond, locals, label);
                let mut body_locals = locals.clone();
                self.apply_narrowing(cond, &mut body_locals, true);
                self.check_stmt(body, &mut body_locals, declared_return, label);
                // The loop may run zero times, so what's true after it is
                // "either the pre-loop state, or whatever one narrowed
                // iteration produces" - not a real fixed point, but sound
                // enough to avoid claiming more precision than we have.
                *locals = merge_scopes(locals, &body_locals);
                // Reaching code after the loop always means `cond` just
                // evaluated false, regardless of how many iterations ran -
                // real, additional narrowing on top of the merge above,
                // independent of it (e.g. a "loop until no longer null"
                // idiom is fully narrowed non-null afterward).
                self.apply_narrowing(cond, locals, false);
            }
            StmtKind::DoWhile { body, cond } => {
                // The condition is only evaluated after the body has
                // already run once, so there's nothing to narrow going in.
                self.check_stmt(body, locals, declared_return, label);
                self.check_expr(cond, locals, label);
                // Same as `While` - reaching code after a do-while always
                // means `cond` just evaluated false.
                self.apply_narrowing(cond, locals, false);
            }
            StmtKind::For {
                init,
                cond,
                step,
                body,
            } => {
                for e in init.iter().chain(cond.iter()) {
                    self.check_expr(e, locals, label);
                }
                // A real `for`'s condition slot can hold several
                // comma-separated expressions - only the *last* one's
                // truthiness actually gates the loop, matching PHP's own
                // semantics (the others are only there for a side effect).
                match cond.last() {
                    Some(c) => {
                        let mut body_locals = locals.clone();
                        self.apply_narrowing(c, &mut body_locals, true);
                        self.check_stmt(body, &mut body_locals, declared_return, label);
                        // `step` runs after each body iteration, so it
                        // should see whatever the body just did, not the
                        // pre-loop state.
                        for e in step {
                            self.check_expr(e, &mut body_locals, label);
                        }
                        // Same "may run zero times" merge `While` uses,
                        // for the same reason - unlike before this fix,
                        // this is no longer treated as if the body always
                        // executes exactly once.
                        *locals = merge_scopes(locals, &body_locals);
                        self.apply_narrowing(c, locals, false);
                    }
                    None => {
                        // `for (;;)` - no condition means an unconditional
                        // infinite loop (barring an internal break/return/
                        // etc.), so the body genuinely always executes at
                        // least once - no "may run zero times" merge is
                        // needed or correct here.
                        self.check_stmt(body, locals, declared_return, label);
                        for e in step {
                            self.check_expr(e, locals, label);
                        }
                    }
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                body,
                ..
            } => {
                self.check_expr(subject, locals, label);
                // The loop variable(s) are a definition site, not a read -
                // bind them (to the subject's known element type, when
                // it's a homogeneous array) before checking anything that
                // could reference them.
                let element_ty = match self.infer_expr(subject, locals) {
                    Type::Array(inner) => *inner,
                    _ => Type::Mixed,
                };
                if let Some(k) = key {
                    self.bind_simple_target(k, Type::Mixed, locals);
                    self.check_expr(k, locals, label);
                }
                // A foreach value can itself be a destructuring target
                // (`foreach ($pairs as [$num, $letter])`, a real, common
                // way to iterate a list of tuples) - route it through the
                // same fully-recursive helpers a plain destructuring
                // assignment uses, not the bare-variable-only
                // bind_simple_target (which silently bound nothing at all
                // for anything else). check_assignment_target already
                // covers "check as a read where appropriate" for whatever
                // isn't a pure definition site (an Index's key, a
                // PropertyAccess's base, ...), so there's no separate
                // check_expr call needed on top of it.
                self.check_assignment_target(value, locals, label);
                self.bind_assignment_target(value, element_ty, locals);
                self.check_stmt(body, locals, declared_return, label);
            }
            StmtKind::Switch { subject, cases } => {
                self.check_expr(subject, locals, label);
                // Two independent facts are tracked while walking cases
                // in order:
                //
                // `narrowed_locals` accumulates "subject != case_i.test"
                // for every case with a test, unconditionally - this is
                // purely a statement about the dispatch comparison chain
                // (direct dispatch past case_i requires its value not to
                // have matched) and holds regardless of whether case_i's
                // own body terminates or falls through.
                //
                // `running`/`*locals` tracks what a case reached via real
                // fallthrough would see - only meaningful when the
                // *immediately preceding* case is known not to terminate,
                // tracked by `previous_terminates` (starts `true`: the
                // first case has no predecessor to fall through from, so
                // it's always reached via direct dispatch only).
                //
                // A case reached only by direct dispatch (previous case
                // terminates, or this is the first case) is checked
                // against `narrowed_locals` plus its own positive
                // "subject === test" narrowing. A case that might also be
                // reached via fallthrough is checked against the plain
                // fallthrough-inherited `running` state instead, without
                // extra narrowing - conservative (it doesn't claim the
                // precision of the direct-dispatch-only path when
                // fallthrough is also possible), matching this project's
                // general preference for missing precision over a false
                // positive.
                // A third thing tracked alongside the two above:
                // `exit_states` collects every case's own ending state
                // that legitimately reaches the code *after* the whole
                // switch - either it ends in `break` (exits the switch,
                // not the enclosing function/loop), or it's the very last
                // case and simply falls off its own end with no explicit
                // exit at all. A case ending in `return`/`throw`/
                // `continue` never contributes here, since control never
                // reaches past the switch on that path. If there's no
                // `default:`, the subject might also match nothing at
                // all, so the original pre-switch state is its own valid
                // exit path too.
                let mut running = locals.clone();
                let mut narrowed_locals = locals.clone();
                let mut previous_terminates = true;
                let mut exit_states: Vec<HashMap<String, Type>> = Vec::new();
                if !cases.iter().any(|c| c.test.is_none()) {
                    exit_states.push(locals.clone());
                }
                let case_count = cases.len();
                for (i, case) in cases.iter().enumerate() {
                    if let Some(t) = &case.test {
                        self.check_expr(t, locals, label);
                    }
                    let mut case_locals = if previous_terminates {
                        let mut nl = narrowed_locals.clone();
                        if let Some(t) = &case.test {
                            self.apply_equality_narrowing(subject, t, &mut nl, true);
                        }
                        nl
                    } else {
                        running.clone()
                    };
                    self.check_block(&case.body, &mut case_locals, declared_return, label);
                    let last_stmt = case.body.last();
                    let case_terminates =
                        last_stmt.is_some_and(|s| self.always_terminates(s, &case_locals));
                    if let Some(t) = &case.test {
                        self.apply_equality_narrowing(subject, t, &mut narrowed_locals, false);
                    }
                    let is_last = i + 1 == case_count;
                    let reaches_after_switch = match last_stmt {
                        Some(s) if matches!(s.kind, StmtKind::Break(_)) => true,
                        Some(s) => is_last && !self.exits_function_or_loop(s, &case_locals),
                        None => is_last,
                    };
                    if reaches_after_switch {
                        exit_states.push(case_locals.clone());
                    }
                    if !case_terminates {
                        running = case_locals;
                    }
                    previous_terminates = case_terminates;
                }
                // If no path reaches past the switch (every case
                // return/throw/continues, and a `default:` exists), the
                // code after it is dead - leave `locals` alone rather
                // than guess, the same shape `always_terminates`'s own
                // `Switch` arm independently recognizes.
                if let Some(first) = exit_states.first().cloned() {
                    *locals = exit_states
                        .into_iter()
                        .skip(1)
                        .fold(first, |acc, s| merge_scopes(&acc, &s));
                }
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.check_block(body, locals, declared_return, label);
                for c in catches {
                    // Scoped to this catch block only, in its own clone
                    // rather than the shared `locals` - matches real PHP
                    // semantics (the caught variable is unset at the end of
                    // its catch block) and keeps one catch's binding from
                    // leaking into a sibling catch or the code after the
                    // whole try.
                    // Finding 57: every type in a multi-type catch must
                    // implement Throwable, independently of whatever
                    // Finding 27's own union-binding does with them - a
                    // real, unconditional PHP fatal error at
                    // declaration time otherwise ("Cannot catch X
                    // because it does not implement Throwable").
                    for t in &c.types {
                        if let Type::Object(class, _) = self.resolve_name_expr(t)
                            && !self.registry.is_subclass_of(&class, "Throwable")
                        {
                            self.diagnostics.push(Diagnostic {
                                location: label.to_string(),
                                message: format!(
                                    "cannot catch {class}: it does not implement Throwable"
                                ),
                                span: c.span,
                                level: 0,
                            });
                        }
                    }
                    let mut catch_locals = locals.clone();
                    if let Some(var) = &c.var {
                        // A multi-type catch (`catch (TypeA|TypeB $e)`,
                        // real, valid PHP since 7.1) binds the caught
                        // variable to the *union* of every listed type -
                        // binding only the first discarded the others
                        // entirely, making the checker's verdict on
                        // member access through the variable depend on
                        // catch-type order, which real PHP itself doesn't
                        // care about at all.
                        let ty = match c.types.as_slice() {
                            [] => Type::Mixed,
                            [single] => self.resolve_name_expr(single),
                            multiple => Type::Union(
                                multiple.iter().map(|t| self.resolve_name_expr(t)).collect(),
                            ),
                        };
                        catch_locals.insert(var.clone(), ty);
                    }
                    self.check_block(&c.body, &mut catch_locals, declared_return, label);
                }
                if let Some(f) = finally {
                    self.check_block(f, locals, declared_return, label);
                }
            }
            StmtKind::Throw(e) => {
                self.check_expr(e, locals, label);
                // Finding 57: `throw` only accepts a real Throwable - a
                // real, unconditional PHP fatal error otherwise,
                // independent of anything about the thrown value at
                // runtime. Only checked when the thrown expression's own
                // class is actually known (a concrete `Type::Object`) -
                // anything else (Mixed, a dynamic expression) isn't
                // guessed at, matching this project's usual leniency for
                // "we don't know" cases.
                if let Type::Object(class, _) = self.infer_expr(e, locals)
                    && !self.registry.is_subclass_of(&class, "Throwable")
                {
                    self.diagnostics.push(Diagnostic {
                        location: label.to_string(),
                        message: format!("cannot throw {class}: it does not implement Throwable"),
                        span: e.span,
                        level: 0,
                    });
                }
            }
            // `global $x;` binds $x to whatever the global scope holds,
            // which this checker doesn't track - Mixed is the honest
            // answer, but the important part is that it's bound at all, so
            // using it afterward isn't flagged as undefined.
            StmtKind::Global(names) => {
                for name in names {
                    locals.entry(name.clone()).or_insert(Type::Mixed);
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, default) in vars {
                    if let Some(e) = default {
                        self.check_expr(e, locals, label);
                    }
                    let ty = default
                        .as_ref()
                        .map(|e| self.infer_expr(e, locals))
                        .unwrap_or(Type::Mixed);
                    locals.insert(name.clone(), ty);
                }
            }
            // `check_program`'s own top-level loop already handles a
            // top-level FunctionDecl/ClassDecl/EnumDecl by checking its
            // body - but any of the same three declared *nested* inside a
            // block (the extremely common `if (!function_exists('foo')) {
            // function foo() {} }` polyfill idiom, or a class conditionally
            // declared inside an if) used to fall into the wildcard arm
            // below and never got its own body checked at all - it was
            // indexed into the registry (so calls to it resolve), but
            // silently exempt from every diagnostic, undefined-variable
            // detection included. Mirrors check_program's handling.
            StmtKind::FunctionDecl(f) => self.check_function(f, None, false),
            StmtKind::ClassDecl(c) => {
                let fqn = self.name_ctx.declared_name(&c.name);
                for m in &c.methods {
                    self.check_function(&m.function, Some(&fqn), m.is_static);
                }
            }
            StmtKind::EnumDecl(e) => {
                let fqn = self.name_ctx.declared_name(&e.name);
                for m in &e.methods {
                    self.check_function(&m.function, Some(&fqn), m.is_static);
                }
            }
            _ => {}
        }
    }

    /// The actual variable-binding side effect of an assignment - lives in
    /// `check_expr` (called from *every* place an assignment expression can
    /// appear: a bare expression statement, a `for` init/step, a `while`
    /// condition, ...) rather than a separate pass triggered only for
    /// top-level expression statements, which is exactly what let `for ($j
    /// = 0; ...)`'s `$j` go unbound (and then misfire as "undefined") until
    /// this was caught.
    /// Checks an assignment's own left-hand side for whatever isn't a
    /// pure "definition site" - a plain variable (or nested variable,
    /// recursing through any level of `[$a, $b] = ...`-style
    /// destructuring) is never a read, but an `Index`'s own key
    /// expression and a `PropertyAccess`'s own base still are. Shared
    /// between the top-level assignment target and every nested
    /// destructuring element, so `[[$a, $b], $c] = $nested;` doesn't
    /// wrongly check `$a`/`$b` as reads of not-yet-defined variables the
    /// way only handling one level of nesting used to.
    fn check_assignment_target(
        &mut self,
        target: &Expr,
        locals: &mut HashMap<String, Type>,
        label: &str,
    ) {
        match &target.kind {
            ExprKind::Variable(_) => {}
            ExprKind::ArrayLiteral(items) => {
                for item in items {
                    self.check_assignment_target(&item.value, locals, label);
                }
            }
            // A property being *assigned* isn't a read of that property -
            // only its base is (`$obj->prop = ...` reads $obj). Routing
            // the full PropertyAccess through check_expr here would
            // wrongly flag legitimate dynamic-property creation as
            // "undefined property".
            ExprKind::PropertyAccess(base, _, _) => {
                self.check_expr(base, locals, label);
            }
            // Same reasoning as PropertyAccess just above, for the exact
            // same reason: `$x['newKey'] = ...` defines that key, it
            // doesn't read it - routing the full Index expression through
            // check_expr's own Index arm would call check_shape_key_access
            // and wrongly flag the ordinary "start from an array literal,
            // build it up one key at a time" idiom as accessing a
            // nonexistent key of an already-fully-known shape (Finding
            // 48 made this a live, common-case bug: an array literal's
            // own inferred shape is only what's known so far, not a
            // sealed contract the array can never grow past).
            ExprKind::Index(base, idx) => {
                self.check_expr(base, locals, label);
                if let Some(i) = idx {
                    self.check_expr(i, locals, label);
                }
            }
            _ => {
                self.check_expr(target, locals, label);
            }
        }
    }

    /// Binds an assignment's (or a `foreach` value's) left-hand side to
    /// `ty`, the type of whatever was just assigned/iterated - recursing
    /// into a nested `ArrayLiteral` target (`[$a, $b] = $arr;` and
    /// `list($a, $b) = $arr;` both parse this way, including a nested
    /// `[[$a, $b], $c] = $nested;`) rather than only handling one level
    /// deep. Per-key precision (`['name' => $n] = $shapedArray`) isn't
    /// resolved for a plain `Type::Array` source - every target gets the
    /// source's homogeneous element type, or Mixed for anything else.
    fn bind_assignment_target(&self, lhs: &Expr, ty: Type, locals: &mut HashMap<String, Type>) {
        match &lhs.kind {
            ExprKind::Variable(name) => {
                locals.insert(name.clone(), ty);
            }
            ExprKind::ArrayLiteral(items) => match &ty {
                // A known array shape (from a `@return array{0: int, 1:
                // string}`-typed source, most commonly) carries real
                // per-position/per-key types - a plain homogeneous
                // `Type::Array` fallback would silently discard that
                // precision. A keyed destructuring element (`['key' =>
                // $x] = $shaped`) looks up its own literal key directly;
                // a positional element (`[$a, $b] = $shaped`, the common
                // tuple-destructuring case) looks up the field whose key
                // is that position's own index ("0", "1", ...).
                Type::ArrayShape(fields) => {
                    for (position, item) in items.iter().enumerate() {
                        let field_key = match item.key.as_ref().map(|k| &k.kind) {
                            Some(ExprKind::Literal(Literal::String(s))) => Some(s.clone()),
                            Some(ExprKind::Literal(Literal::Int(n))) => Some(n.to_string()),
                            Some(_) => None,
                            None => Some(position.to_string()),
                        };
                        let element_ty = field_key
                            .and_then(|k| fields.iter().find(|(fk, _, _)| *fk == k))
                            .map(|(_, t, _)| t.clone())
                            .unwrap_or(Type::Mixed);
                        self.bind_assignment_target(&item.value, element_ty, locals);
                    }
                }
                Type::Array(inner) => {
                    let element_ty = (**inner).clone();
                    for item in items {
                        self.bind_assignment_target(&item.value, element_ty.clone(), locals);
                    }
                }
                _ => {
                    for item in items {
                        self.bind_assignment_target(&item.value, Type::Mixed, locals);
                    }
                }
            },
            // Finding 48's own follow-on: `$x['newKey'] = $value;` adds a
            // real key to `$x`'s own tracked shape when `$x` already
            // holds a known one (most commonly, from an earlier `$x = []`
            // or `$x = ['a' => 1]` literal) - without this, an array
            // literal's own new precision made the extremely common
            // "start from a literal, build it up one key at a time"
            // idiom look like every later read was accessing a key the
            // shape could never have, since the shape itself never grew
            // past its own starting literal. Only updates a *known*
            // shape already sitting in `locals` under a bare variable
            // name; anything else (a property chain, a computed base, a
            // variable with a non-shape or not-yet-known type) is left
            // alone, matching this project's usual "don't guess" scope.
            ExprKind::Index(base, Some(idx)) => {
                if let ExprKind::Variable(name) = &base.kind
                    && let Some(key) = literal_string_key(idx)
                    && let Some(Type::ArrayShape(fields)) = locals.get(name)
                {
                    let mut fields = fields.clone();
                    match fields.iter_mut().find(|(k, _, _)| *k == key) {
                        Some(field) => field.1 = ty,
                        None => fields.push((key, ty, false)),
                    }
                    locals.insert(name.clone(), Type::ArrayShape(fields));
                }
            }
            _ => {}
        }
    }

    fn bind_simple_target(&self, target: &Expr, ty: Type, locals: &mut HashMap<String, Type>) {
        if let ExprKind::Variable(name) = &target.kind {
            locals.insert(name.clone(), ty);
        }
    }

    /// Narrows `locals` in place based on `cond` being true (`branch_true`)
    /// or false, along one branch of an `if`/`while`. Only handles the
    /// common, safe patterns - equality against a known-type value
    /// (`$x === null`, `null !== $x`), `is_*()` guards, and `instanceof` -
    /// anything else leaves `locals` untouched rather than guess.
    fn apply_narrowing(&self, cond: &Expr, locals: &mut HashMap<String, Type>, branch_true: bool) {
        match &cond.kind {
            // `&&` narrows the "both true" branch by composing each side
            // in order (the right operand's own narrowing can rely on the
            // left's, same as check_expr's short-circuit-aware handling)
            // - the false branch would need "at least one is false, but
            // not sure which", which isn't a narrowing this can express,
            // so it's left alone, same conservative choice `||` mirrors
            // for its own unrepresentable branch below.
            ExprKind::Binary(BinOp::And, l, r) if branch_true => {
                self.apply_narrowing(l, locals, true);
                self.apply_narrowing(r, locals, true);
            }
            ExprKind::Binary(BinOp::Or, l, r) if !branch_true => {
                self.apply_narrowing(l, locals, false);
                self.apply_narrowing(r, locals, false);
            }
            // Deliberately `===`/`!==` only, not `==`/`!=` - a loose
            // comparison matches PHP's own type-juggling table (`0 ==
            // false`, `0 == null`, `"0" == 0`, ...), a much wider set of
            // values than the literal compared-to one, so narrowing a
            // variable down to exactly that value on a loose match would
            // discard every other value `==` could also have matched.
            ExprKind::Binary(op, l, r) => {
                let is_eq = matches!(op, BinOp::Identical);
                let is_neq = matches!(op, BinOp::NotIdentical);
                if is_eq || is_neq {
                    let matches_when_true = if is_eq { branch_true } else { !branch_true };
                    self.apply_equality_narrowing(l, r, locals, matches_when_true);
                }
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.apply_narrowing(inner, locals, !branch_true);
            }
            ExprKind::Grouping(inner) => self.apply_narrowing(inner, locals, branch_true),
            ExprKind::Call(callee, args) => {
                if let (ExprKind::Name(name), [single_arg]) = (&callee.kind, args.as_slice())
                    && let ExprKind::Variable(var_name) = &single_arg.value.kind
                {
                    if name == "empty" {
                        // `empty($x)` is PHP's own falsy check - only the
                        // branch where it evaluated `false` (`$x` proven
                        // truthy) is safely narrowable, the same asymmetry
                        // as a bare truthiness check.
                        if !branch_true {
                            let current = locals.get(var_name).cloned().unwrap_or(Type::Mixed);
                            locals.insert(var_name.clone(), narrow_type_excluding_falsy(&current));
                        }
                    } else if let Some(target_ty) = narrowing_type_for_predicate(name) {
                        let current = locals.get(var_name).cloned().unwrap_or(Type::Mixed);
                        let narrowed = if branch_true {
                            narrow_type_to(&current, &target_ty)
                        } else {
                            narrow_type_excluding(&current, &target_ty)
                        };
                        locals.insert(var_name.clone(), narrowed);
                    }
                }
            }
            ExprKind::Instanceof(inner, target) => {
                // The false branch would need a "not this class" type we
                // don't have a representation for, so it's left alone. A
                // dynamic target (`$x instanceof $className`) doesn't name
                // a concrete class to narrow to either.
                if branch_true
                    && let Some(key) = narrowing_key(inner)
                    && let InstanceofTarget::Name(class_name) = target
                {
                    locals.insert(key, self.resolve_name_expr(class_name));
                }
            }
            // A bare truthiness check (`if ($x)` / `if (!$x)`, the latter
            // reaching here via the `Not` arm's recursion above) - only
            // the truthy branch is narrowable, by excluding the falsy
            // members this project can represent (`Null`/`False`); the
            // falsy branch isn't safely narrowable to anything more
            // specific than the current type.
            _ if branch_true && narrowing_key(cond).is_some() => {
                let key = narrowing_key(cond).expect("checked by guard above");
                let current = locals.get(&key).cloned().unwrap_or(Type::Mixed);
                locals.insert(key, narrow_type_excluding_falsy(&current));
            }
            _ => {}
        }
    }

    fn apply_equality_narrowing(
        &self,
        l: &Expr,
        r: &Expr,
        locals: &mut HashMap<String, Type>,
        matches_when_true: bool,
    ) {
        let pair = match (&l.kind, &r.kind) {
            (ExprKind::Variable(name), _) => Some((name.clone(), self.infer_expr(r, locals))),
            (_, ExprKind::Variable(name)) => Some((name.clone(), self.infer_expr(l, locals))),
            _ => None,
        };
        let Some((var_name, target_ty)) = pair else {
            return;
        };
        if target_ty == Type::Mixed {
            return;
        }
        let current = locals.get(&var_name).cloned().unwrap_or(Type::Mixed);
        let narrowed = if matches_when_true {
            narrow_type_to(&current, &target_ty)
        } else {
            narrow_type_excluding(&current, &target_ty)
        };
        locals.insert(var_name, narrowed);
    }

    /// Whether a method/property is confirmed missing from every value
    /// `ty` could hold - `has_member(class)` decides for one concrete
    /// class (already covering whatever fallback, like `__call`/`__get`,
    /// makes a class "have" the member without declaring it directly). A
    /// bare `Object` is the base case; a `Union` value could genuinely be
    /// any member, so missing is confirmed as soon as *any* resolvable
    /// member lacks it; a value satisfying an `Intersection` has every
    /// member's capabilities at once, so missing is only confirmed when
    /// *every* member lacks it. Anything else (a type this checker can't
    /// resolve into class members at all, or a non-`Object` member found
    /// while walking a `Union`/`Intersection`) never confirms "missing" -
    /// this project's consistent preference for fewer false positives
    /// over catching everything.
    fn member_confirmed_missing(
        &self,
        ty: &Type,
        has_member: impl Fn(&str) -> bool + Copy,
    ) -> bool {
        match ty {
            Type::Object(class, _) => {
                self.registry.classes.contains_key(class) && !has_member(class)
            }
            Type::Union(members) => members
                .iter()
                .any(|m| self.member_confirmed_missing(m, has_member)),
            Type::Intersection(members) => members
                .iter()
                .all(|m| self.member_confirmed_missing(m, has_member)),
            _ => false,
        }
    }

    /// Checks an `isset()`/`??`-style "quiet" left-hand-side chain: walks
    /// down through `Index`/`PropertyAccess`/`DynamicPropertyAccess`,
    /// checking each level's own sub-expressions (an index's key, a
    /// dynamic property's name) normally, but suppressing the root
    /// variable's own "undefined variable" check and `PropertyAccess`'s
    /// own "access to undefined property" check at every level along the
    /// way - exactly what `isset()`/`??` themselves suppress in real PHP,
    /// and nothing more. `StaticAccess` is never part of this walk (an
    /// undeclared *static* property is a real, non-suppressible PHP
    /// fatal error), and anything past a function/method call is checked
    /// normally, since `isset()`/`??` don't evaluate quietly past one.
    fn check_expr_quiet_root(
        &mut self,
        e: &Expr,
        locals: &mut HashMap<String, Type>,
        label: &str,
    ) -> Type {
        // Every arm here is consumed only for its side effects - see this
        // function's own doc comment and Phase 3's plan note: neither `??`
        // nor `??=` (its only two call sites) depend on the quiet-checked
        // left side's own type, so the exact value returned doesn't matter
        // beyond satisfying the type system post-unification.
        match &e.kind {
            ExprKind::Variable(_) => {
                // Deliberately not `check_expr` - a bare variable here is
                // exactly the "might not be defined" case `??` exists to
                // handle safely.
                Type::Mixed
            }
            ExprKind::Index(base, idx) => {
                self.check_expr_quiet_root(base, locals, label);
                if let Some(i) = idx {
                    self.check_expr(i, locals, label);
                }
                Type::Mixed
            }
            ExprKind::PropertyAccess(base, _, _) => {
                self.check_expr_quiet_root(base, locals, label);
                Type::Mixed
            }
            ExprKind::DynamicPropertyAccess(base, name_expr) => {
                self.check_expr_quiet_root(base, locals, label);
                self.check_expr(name_expr, locals, label);
                Type::Mixed
            }
            _ => self.check_expr(e, locals, label),
        }
    }

    fn check_expr(&mut self, e: &Expr, locals: &mut HashMap<String, Type>, label: &str) -> Type {
        match &e.kind {
            ExprKind::Call(callee, args) => {
                // `unset($x)` genuinely removes `$x` from scope in real
                // PHP - a real bug if it's used afterward. Checked before
                // anything else below: `predefine_byref_or_unknown_args`
                // treats *any* bare-variable argument to an unknown-
                // signature call as a definition site (a mechanism built
                // for a different reason - conservatively assuming an
                // unresolvable call might take the argument by
                // reference - that happens to also make `isset`/`empty`
                // safe to use on a maybe-undefined variable), which is
                // backwards for `unset` specifically: it would make
                // `unset($x)` look like it *defines* `$x` instead of
                // removing it.
                if let ExprKind::Name(name) = &callee.kind
                    && name == "unset"
                {
                    for a in args {
                        if let ExprKind::Variable(var_name) = &a.value.kind {
                            locals.remove(var_name);
                        } else {
                            self.check_expr(&a.value, locals, label);
                        }
                    }
                    return Type::Mixed;
                }
                // `PHPStan\Testing\assertType('expected type string', $expr)`
                // is real PHPStan's own testing-DSL function, used
                // throughout its vendored `tests/PHPStan/Analyser/nsrt/`
                // fixture corpus (see Item 2 of the architecture-audit
                // plan) to pin down an exact expected inferred type at a
                // point of interest. Recognizing it here - comparing this
                // project's own `infer_expr` result (rendered via `Type`'s
                // existing PHPStan-style `Display` impl) against the
                // literal expected string - turns that whole corpus into
                // directly-runnable ground truth against this project's
                // own checker, with no separate test harness needed.
                // Real PHPStan itself treats this the same way (a no-op
                // stub function whose call sites its *own* internal
                // tooling specially recognizes), so this is additive,
                // not a real diagnostic anyone would trip on outside of
                // a fixture file that imports it.
                if let ExprKind::Name(name) = &callee.kind
                    && (name == "assertType" || name.ends_with("\\assertType"))
                    && let [expected_arg, actual_arg] = args.as_slice()
                    && let ExprKind::Literal(Literal::String(expected)) = &expected_arg.value.kind
                {
                    let actual = self.infer_expr(&actual_arg.value, locals).to_string();
                    if &actual != expected {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!(
                                "assertType() mismatch: expected type \"{expected}\", actual type \"{actual}\""
                            ),
                            span: actual_arg.value.span,
                            level: 0,
                        });
                    }
                    self.check_expr(&actual_arg.value, locals, label);
                    return Type::Mixed;
                }
                if let ExprKind::Name(name) = &callee.kind
                    && (name == "extract" || name == "eval")
                {
                    self.saw_dynamic_var_construct = true;
                }
                // Resolved once up front (namespace/`use`-import rules
                // applied, with the same global-namespace fallback real
                // PHP gives an unqualified function call) so both the
                // existing arg-checking path and the undefined-call
                // diagnostic below see the same name(s) a real call would
                // actually resolve to - previously `lookup_function` used
                // the raw, unnormalized name, so e.g. `\class_exists(...)`
                // silently skipped arg-checking even when the plain name
                // was a known signature. `is_language_construct` is
                // checked against the *original* name, before any
                // namespace-qualification - `isset`/`empty`/etc. are
                // never real namespaced symbols, unlike an actual function
                // call, even though the parser models both the same way.
                let resolved_name = match &callee.kind {
                    ExprKind::Name(name) if is_language_construct(name) => None,
                    ExprKind::Name(name) => {
                        let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                        if self.registry.lookup_function(&primary).is_some() {
                            Some(primary)
                        } else if let Some(global) = &fallback
                            && self.registry.lookup_function(global).is_some()
                        {
                            Some(global.clone())
                        } else {
                            Some(primary)
                        }
                    }
                    _ => None,
                };
                let sig = resolved_name
                    .as_deref()
                    .and_then(|name| self.registry.lookup_function(name));
                // Item 1: resolves the call's own arguments against the
                // real signature (named/reordered-named arguments no
                // longer get checked as if they were positional in
                // written order) before any consumer below sees them.
                let resolved = sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(sig, args, &resolved, locals);
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                if let Some(name) = &resolved_name {
                    if let Some(sig) = sig {
                        // Finding 29: a spread argument (`f(...$args)`)
                        // means the real argument count/types aren't
                        // known statically at all - checking count/type
                        // here would treat the spread array itself as if
                        // it were meant to fill the very next positional
                        // slot, producing a real, severe false positive
                        // for the standard "forward a dynamically-sized
                        // argument list" idiom.
                        let has_spread = args.iter().any(|a| a.spread);
                        if !has_spread {
                            self.check_args(
                                name, sig, args, &resolved, locals, label, None, e.span,
                            );
                            if matches!(name.as_str(), "sprintf" | "printf") {
                                self.check_format_string_call(name, args, locals, label);
                            }
                        }
                    } else {
                        // Since resolve_function_candidates always
                        // produces a definite name (there's no "can't
                        // resolve" case left for functions the way
                        // `ExprKind::New`'s class resolution still has for a
                        // multi-segment name with no matching import),
                        // every miss here is a real undefined-function
                        // finding.
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!("call to undefined function {name}()"),
                            span: e.span,
                            level: 0,
                        });
                    }
                }
                // Reuses the `resolved_name`/`sig` already resolved above
                // - the same lookup `infer_expr`'s own `Call` arm would
                // otherwise redundantly redo from scratch.
                sig.map(|s| {
                    self.infer_builtin_call_return_type(
                        resolved_name.as_deref().unwrap_or_default(),
                        s,
                        args,
                        locals,
                    )
                })
                .unwrap_or(Type::Mixed)
            }
            ExprKind::MethodCall(recv, method, args, nullsafe) => {
                // Finding 49's other real shape: `(function () {...})
                // ->bindTo($newThis, $scope)` - same idea as
                // `Closure::bind`, just on the receiver instead of the
                // first argument.
                //
                // The `bindTo` branch can't reuse `check_expr`'s own
                // return value the way the ordinary branch below does -
                // `check_closure_literal_with_this` checks the closure's
                // *body* under a rebound `$this`, it doesn't compute
                // `recv`'s own type - so `recv` still needs its own
                // `infer_expr` call there specifically.
                let recv_ty = if method == "bindTo"
                    && matches!(
                        unwrap_grouping(recv).kind,
                        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                    ) {
                    let this_ty = args
                        .first()
                        .map(|a| self.infer_expr(&a.value, locals))
                        .unwrap_or(Type::Mixed);
                    self.check_closure_literal_with_this(
                        unwrap_grouping(recv),
                        locals,
                        label,
                        this_ty,
                    );
                    self.infer_expr(recv, locals)
                } else {
                    self.check_expr(recv, locals, label)
                };
                // Re-enabled (Phase 9.5): the registry is now
                // namespace-aware, so a method lookup trusts the specific
                // `ClassInfo` stored under a *fully-qualified* name, not a
                // bare one two unrelated packages could collide on - see
                // the module-level doc comment's "Known limitation" note,
                // now resolved, for the full history.
                //
                // A `Union`/`Intersection` receiver (e.g. a multi-type
                // `catch` variable, or an ordinarily union-typed
                // parameter) used to fall to `_ => None` here entirely,
                // silently skipping this whole check - `member_missing`
                // below picks up the "should this be flagged" question
                // for those cases instead, using a representative member
                // (the first `Object` found) for argument-checking.
                let recv_class = match &recv_ty {
                    Type::Object(class, type_args) => Some((class.clone(), type_args.clone())),
                    Type::Union(members) | Type::Intersection(members) => {
                        members.iter().find_map(|m| match m {
                            Type::Object(class, type_args) => {
                                Some((class.clone(), type_args.clone()))
                            }
                            _ => None,
                        })
                    }
                    _ => None,
                };
                // Substituted through the receiver's own carried type
                // arguments (Phase 10), walking up to whichever ancestor
                // actually declares the method if it's inherited (Phase
                // 10.5) - a method declared `add(T $item)` on
                // `Collection<User>`, or inherited from `Collection` by a
                // concrete `IntCollection extends Collection<int>`, gets
                // checked against `User`/`int`, not an unbound `T`/`Mixed`.
                let sig = recv_class.as_ref().and_then(|(class, type_args)| {
                    self.registry
                        .lookup_method_with_owner(class, method)
                        .map(|(owner, sig)| {
                            self.substitute_class_templates(sig, class, type_args, &owner)
                        })
                });
                let resolved = sig
                    .as_ref()
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(sig.as_ref(), args, &resolved, locals);
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                if let Some((class, _)) = &recv_class {
                    // Checked *before* falling back to whether the
                    // representative member's own `sig` was found: for a
                    // `Union`, the representative is just the first
                    // `Object` member encountered, which could easily be
                    // one that *does* have the method even though another
                    // member doesn't - the real question is never "does
                    // the first member have it," it's "is it missing from
                    // any member (Union) / every member (Intersection)."
                    // A class not known to the registry at all (an
                    // unresolvable/uninstalled dependency, the same gap
                    // `ExprKind::New`'s check documents) is left unchecked
                    // rather than risking a false positive - only fires
                    // once the class itself is confirmed known. `__call`
                    // anywhere in the chain means any method name is
                    // legitimately callable at runtime, so its presence
                    // suppresses this entirely, the same way real
                    // PHPStan treats it.
                    if self.registry.classes.contains_key(class)
                        && self.member_confirmed_missing(&recv_ty, |c| {
                            self.registry.lookup_method(c, method).is_some()
                                || self.registry.lookup_method(c, "__call").is_some()
                        })
                    {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!("call to undefined method {recv_ty}::{method}()"),
                            span: e.span,
                            level: 0,
                        });
                    } else if let Some(sig) = &sig
                        && !args.iter().any(|a| a.spread)
                    {
                        // Finding 29: see the identical check on the
                        // plain-function Call arm above.
                        let qualified = format!("{class}::{method}");
                        self.check_args(
                            &qualified,
                            sig,
                            args,
                            &resolved,
                            locals,
                            label,
                            Some(class),
                            e.span,
                        );
                    }
                }
                let result = match &recv_class {
                    Some((class, _)) => sig
                        .as_ref()
                        .map(|s| {
                            let resolved_ty = self.infer_call_return_type(s, &resolved, locals);
                            self.resolve_pseudo(&resolved_ty, class)
                        })
                        .unwrap_or(Type::Mixed),
                    None => Type::Mixed,
                };
                // `$x?->method()` short-circuits to `null` when `$x` is
                // null, on top of whatever the method's own return type
                // is - real, implicit nullability a plain `->` doesn't
                // carry.
                if *nullsafe && result != Type::Mixed {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            // `&&`/`||` short-circuit, so the right operand only ever runs
            // once the left one's already settled the outcome - it needs
            // to see whatever that implies (real code leans on this
            // constantly: `$x instanceof Foo && $x->bar()`,
            // `$x !== null && $x->prop`). Without this, checking the
            // right side against the un-narrowed left-hand type produced
            // real false positives once undefined-property detection
            // started exercising exactly this idiom on real code.
            ExprKind::Binary(BinOp::And, l, r) => {
                self.check_expr(l, locals, label);
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, true);
                self.check_expr(r, &mut narrowed, label);
                self.infer_binary(&BinOp::And, l, r, locals)
            }
            ExprKind::Binary(BinOp::Or, l, r) => {
                self.check_expr(l, locals, label);
                let mut narrowed = locals.clone();
                self.apply_narrowing(l, &mut narrowed, false);
                self.check_expr(r, &mut narrowed, label);
                self.infer_binary(&BinOp::Or, l, r, locals)
            }
            // `??` is built on isset()-style quiet semantics - its left
            // side is evaluated without ever raising an undefined-
            // variable/undefined-property notice, even when the root
            // variable was never assigned anywhere in scope. Real PHP
            // silences this for the whole dereference chain on the left
            // (`$config['key'] ?? 'default'` is just as warning-free as
            // a bare `$x ?? 'default'`), which is exactly what
            // `check_expr_quiet_root` walks. `??`'s own result type is
            // always just the right side's type (`infer_binary`'s own
            // Coalesce arm does nothing but `self.infer_expr(r, locals)`)
            // - reusing `check_expr(r, ...)`'s return value directly
            // avoids a real, confirmed double-walk of `r`.
            ExprKind::Binary(BinOp::Coalesce, l, r) => {
                self.check_expr_quiet_root(l, locals, label);
                self.check_expr(r, locals, label)
            }
            ExprKind::Binary(op, l, r) => {
                self.check_expr(l, locals, label);
                self.check_expr(r, locals, label);
                self.infer_binary(op, l, r, locals)
            }
            ExprKind::Variable(name) => {
                if !locals.contains_key(name) && !is_superglobal(name) {
                    self.diagnostics.push(Diagnostic {
                        location: label.to_string(),
                        message: format!("undefined variable ${name}"),
                        span: e.span,
                        level: 0,
                    });
                }
                locals.get(name).cloned().unwrap_or(Type::Mixed)
            }
            ExprKind::Unary(
                op @ (UnaryOp::PreInc | UnaryOp::PostInc | UnaryOp::PreDec | UnaryOp::PostDec),
                inner,
            ) => {
                self.check_expr(inner, locals, label);
                // Real PHP: incrementing `null` produces `int(1)`;
                // decrementing `null`, and either direction on a `bool`,
                // is a no-op (the value is unchanged) - only the
                // null-to-int case actually changes the tracked type.
                // A string operand's own exact value isn't modeled
                // precisely enough to distinguish numeric from
                // alphanumeric-increment behavior, so it conservatively
                // stays a string either way.
                if let ExprKind::Variable(name) = &inner.kind {
                    let current = locals.get(name).cloned().unwrap_or(Type::Mixed);
                    let is_increment = matches!(op, UnaryOp::PreInc | UnaryOp::PostInc);
                    let updated = match &current {
                        Type::Null if is_increment => Type::Int,
                        other => other.clone(),
                    };
                    locals.insert(name.clone(), updated);
                }
                self.infer_expr(inner, locals)
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.check_expr(inner, locals, label);
                Type::Bool
            }
            // Real PHP's unary minus/plus convert a numeric string
            // operand to a number (`-"5"` is `int(-5)`, not `"-5"`) - the
            // same implicit-numeric-coercion rule arithmetic operators
            // apply generally.
            ExprKind::Unary(UnaryOp::Neg | UnaryOp::Plus, inner) => {
                match self.check_expr(inner, locals, label) {
                    Type::String | Type::StringLiteral(_) => Type::Int,
                    other => other,
                }
            }
            ExprKind::Unary(_, inner) | ExprKind::Grouping(inner) => {
                self.check_expr(inner, locals, label)
            }
            ExprKind::Cast(ty, inner) => {
                self.check_expr(inner, locals, label);
                match ty.as_str() {
                    "int" => Type::Int,
                    "float" => Type::Float,
                    "string" => Type::String,
                    "bool" => Type::Bool,
                    "array" => Type::Array(Box::new(Type::Mixed)),
                    "object" => Type::Object("object".to_string(), Vec::new()),
                    "unset" => Type::Null,
                    _ => Type::Mixed,
                }
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used
            // for a function that always throws or exits.
            ExprKind::Throw(inner) => {
                self.check_expr(inner, locals, label);
                Type::Never
            }
            ExprKind::YieldFrom(inner) => {
                self.check_expr(inner, locals, label);
                Type::Mixed
            }
            // Never given a stable nominal type by infer_expr either,
            // since multiple clone-with-overrides expressions could
            // produce runtime objects that don't share a common shape
            // this checker tracks - the clone target's own type is the
            // honest answer.
            ExprKind::Clone(inner, overrides) => {
                let inner_ty = self.check_expr(inner, locals, label);
                if let Some(o) = overrides {
                    self.check_expr(o, locals, label);
                }
                inner_ty
            }
            ExprKind::Yield(key, value) => {
                if let Some(k) = key {
                    self.check_expr(k, locals, label);
                }
                if let Some(v) = value {
                    self.check_expr(v, locals, label);
                }
                Type::Mixed
            }
            ExprKind::Instanceof(inner, target) => {
                self.check_expr(inner, locals, label);
                if let InstanceofTarget::Expr(e) = target {
                    self.check_expr(e, locals, label);
                }
                Type::Mixed
            }
            ExprKind::Assign(lhs, rhs) => {
                let rhs_ty = self.check_expr(rhs, locals, label);
                // A bare variable (or a destructuring target inside one)
                // on the left is a definition site, not a read - checking
                // it here, before it's bound below, would make undefined-
                // variable detection flag every variable on the very
                // statement that defines it. PropertyAccess/Index targets
                // still need their *base* checked (`$obj->prop = ...`
                // reads $obj).
                self.check_assignment_target(lhs, locals, label);
                self.check_property_assignment(lhs, rhs, locals, label);
                self.bind_assignment_target(lhs, rhs_ty.clone(), locals);
                rhs_ty
            }
            ExprKind::CompoundAssign(op, lhs, rhs) => {
                // `$x ??= $default;` is defined as exactly
                // `$x = $x ?? $default;` - the same quiet-left-side
                // exemption as plain `??` applies to its own left side.
                if *op == BinOp::Coalesce {
                    self.check_expr_quiet_root(lhs, locals, label);
                } else {
                    self.check_expr(lhs, locals, label);
                }
                self.check_expr(rhs, locals, label);
                if let ExprKind::Variable(name) = &lhs.kind {
                    let ty = self.infer_binary(op, lhs, rhs, locals);
                    locals.insert(name.clone(), ty);
                }
                Type::Mixed
            }
            ExprKind::Ternary(cond, then_, else_) => {
                self.check_expr(cond, locals, label);
                // Same clone-and-narrow shape as `StmtKind::If` - a
                // ternary is just as branch-based, and without this,
                // `$x === null ? "default" : $x` never benefits from the
                // narrowing an equivalent `if` already gets.
                let mut then_locals = locals.clone();
                self.apply_narrowing(cond, &mut then_locals, true);
                let then_ty = then_
                    .as_ref()
                    .map(|t| self.check_expr(t, &mut then_locals, label));
                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                let else_ty = self.check_expr(else_, &mut else_locals, label);
                *locals = merge_scopes(&then_locals, &else_locals);
                // Never let an unknown branch compose into a partial
                // Union that looks more precise than it is.
                match then_ty {
                    Some(then_ty) => {
                        if then_ty == Type::Mixed || else_ty == Type::Mixed {
                            Type::Mixed
                        } else if then_ty == else_ty {
                            then_ty
                        } else {
                            Type::Union(vec![then_ty, else_ty])
                        }
                    }
                    None => Type::Mixed,
                }
            }
            ExprKind::ArrayLiteral(items) => {
                for item in items {
                    if let Some(k) = &item.key {
                        self.check_expr(k, locals, label);
                    }
                    self.check_expr(&item.value, locals, label);
                }
                self.infer_array_literal_type(items, locals)
            }
            ExprKind::Index(base, idx) => {
                let base_ty = self.check_expr(base, locals, label);
                match idx {
                    Some(i) => {
                        self.check_expr(i, locals, label);
                        self.check_shape_key_access(base, i, locals, label);
                        match base_ty {
                            Type::Array(inner) => *inner,
                            Type::ArrayShape(fields) => literal_string_key(i)
                                .and_then(|key| {
                                    fields.into_iter().find(|(name, _, _)| *name == key)
                                })
                                .map(|(_, ty, _)| ty)
                                .unwrap_or(Type::Mixed),
                            _ => Type::Mixed,
                        }
                    }
                    None => Type::Mixed,
                }
            }
            ExprKind::New(_, args) => {
                // `new Foo()` never carries known type arguments itself -
                // real PHP's constructor syntax has no way to write
                // `new Foo<Bar>()` at all, and inferring them from the
                // constructor's own arguments (the way real PHPStan often
                // does) isn't attempted here, so this is always `Object`
                // with an empty arg list.
                let new_ty = self.infer_expr(e, locals);
                let resolved_class = match &new_ty {
                    Type::Object(class, _) => Some(class.clone()),
                    _ => None,
                };
                // A dynamic target (`new $className()`) or an unresolved
                // pseudo-type (`new parent()` with no parent class) has no
                // concrete name to check - only a plain `new Name()` (or
                // `self`/`static` resolving to the current class) reaches
                // here with `Some`.
                let ctor_sig = resolved_class
                    .as_deref()
                    .and_then(|class| self.registry.lookup_method(class, "__construct"));
                let resolved = ctor_sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(ctor_sig, args, &resolved, locals);
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                if let Some(class) = &resolved_class {
                    // `resolved_class` is already the fully-qualified name
                    // (`infer_expr`'s New arm resolves it via `name_ctx`),
                    // so every miss here is a real undefined-class finding
                    // - no "can't resolve a namespaced name" case left, the
                    // way there used to be before Phase 9.5's namespace
                    // support.
                    if let Some(info) = self.registry.classes.get(class) {
                        if info.kind == ClassKind::Interface {
                            self.diagnostics.push(Diagnostic {
                                location: label.to_string(),
                                message: format!("cannot instantiate interface {class}"),
                                span: e.span,
                                level: 0,
                            });
                        } else if info.is_abstract {
                            self.diagnostics.push(Diagnostic {
                                location: label.to_string(),
                                message: format!("cannot instantiate abstract class {class}"),
                                span: e.span,
                                level: 0,
                            });
                        } else if let Some(sig) = ctor_sig
                            && !args.iter().any(|a| a.spread)
                        {
                            self.check_args(
                                &format!("{class}::__construct"),
                                sig,
                                args,
                                &resolved,
                                locals,
                                label,
                                Some(class),
                                e.span,
                            );
                        }
                    } else {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!("instantiated class {class} not found"),
                            span: e.span,
                            level: 0,
                        });
                    }
                }
                new_ty
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                // The anonymous class's own methods are still worth
                // checking, even though the class itself isn't indexed
                // into the registry under any stable name - check_function
                // only needs a FunctionDecl and a label to check against.
                for m in &decl.methods {
                    self.check_function(&m.function, Some(&decl.name), m.is_static);
                }
                Type::Mixed
            }
            ExprKind::StaticCall(class_name, method, args) => {
                // Mirrors ExprKind::MethodCall's own structure exactly -
                // resolve first (self/parent/static-aware), predefine
                // byref/unknown args using the real signature before
                // checking them, then the existence/arg-type check.
                let resolved_class = self.resolve_static_class_name(class_name);
                let sig = resolved_class
                    .as_ref()
                    .and_then(|class| self.registry.lookup_method(class, method));
                let resolved = sig
                    .map(|s| resolve_call_args(args, &s.params))
                    .unwrap_or_default();
                self.predefine_byref_or_unknown_args(sig, args, &resolved, locals);
                // Finding 49: `Closure::bind($closure, $newThis, $scope)`
                // makes `$this` (and, for private/protected access, the
                // calling scope) available inside a closure that
                // otherwise wouldn't have it - real, idiomatic PHP for
                // reaching into an object's state from outside its class.
                // Only special-cased when the first argument is literally
                // a closure expression right here (not some arbitrary
                // `Type::Object("Closure", _)`-typed value) - the closure
                // body needs to still be reachable as an `Expr` to
                // re-check with the rebound `$this`.
                if resolved_class.as_deref() == Some("Closure")
                    && method == "bind"
                    && !args.is_empty()
                    && matches!(
                        unwrap_grouping(&args[0].value).kind,
                        ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }
                    )
                {
                    let this_ty = args
                        .get(1)
                        .map(|a| self.infer_expr(&a.value, locals))
                        .unwrap_or(Type::Mixed);
                    self.check_closure_literal_with_this(
                        unwrap_grouping(&args[0].value),
                        locals,
                        label,
                        this_ty,
                    );
                    for a in &args[1..] {
                        self.check_expr(&a.value, locals, label);
                    }
                } else {
                    for a in args {
                        self.check_expr(&a.value, locals, label);
                    }
                }
                if let Some(class) = &resolved_class {
                    if let Some(sig) = sig {
                        // Finding 29: see the identical check on the
                        // plain-function Call arm above.
                        if !args.iter().any(|a| a.spread) {
                            let qualified = format!("{class}::{method}");
                            self.check_args(
                                &qualified,
                                sig,
                                args,
                                &resolved,
                                locals,
                                label,
                                Some(class),
                                e.span,
                            );
                        }
                    } else if self.registry.classes.contains_key(class)
                        && self.registry.lookup_method(class, "__callStatic").is_none()
                    {
                        // Same leniency as MethodCall's own undefined-
                        // method check: only fires once the class itself
                        // is confirmed known, and __callStatic (the
                        // static equivalent of __call) suppresses it
                        // entirely, matching real PHPStan.
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!("call to undefined static method {class}::{method}()"),
                            span: e.span,
                            level: 0,
                        });
                    }
                }
                self.infer_expr(e, locals)
            }
            ExprKind::StaticAccess(class_name, member) => {
                // `Foo::class` (member == "class") always resolves to a
                // real string constant, not an actual member of the
                // class - and a fully dynamic member name (`Foo::{$e}`/
                // `Foo::$$e`) has no statically known name to check in
                // the first place. Both are the parser's own documented
                // encoding for these two cases, not something to check.
                if member == "class" {
                    return Type::String;
                }
                if member == "{dynamic}" {
                    return Type::Mixed;
                }
                let Some(class) = self.resolve_static_class_name(class_name) else {
                    return Type::Mixed;
                };
                if !self.registry.classes.contains_key(&class) {
                    return Type::Mixed;
                }
                if let Some(prop_name) = member.strip_prefix('$') {
                    // Unlike an instance property, PHP has no legal way
                    // to dynamically create a static property at all -
                    // no "could be legitimate" case to stay lenient for
                    // here, the way check_property_assignment's write
                    // side has to for instance properties. Finding 9: a
                    // property that exists but was declared as an
                    // ordinary *instance* property is equally invalid
                    // through `::` in real PHP ("Access to undeclared
                    // static property") - from the caller's own
                    // perspective there's no real static property by
                    // this name either way, so both share one message.
                    let is_valid_static = self
                        .registry
                        .lookup_property(&class, prop_name)
                        .is_some_and(|sig| sig.is_static);
                    if !is_valid_static {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!(
                                "access to undefined static property {class}::${prop_name}"
                            ),
                            span: e.span,
                            level: 0,
                        });
                    }
                } else if self.registry.lookup_const(&class, member).is_none() {
                    self.diagnostics.push(Diagnostic {
                        location: label.to_string(),
                        message: format!("access to undefined constant {class}::{member}"),
                        span: e.span,
                        level: 0,
                    });
                }
                self.infer_expr(e, locals)
            }
            ExprKind::DynamicVariable(name_expr) => {
                self.check_expr(name_expr, locals, label);
                Type::Mixed
            }
            ExprKind::PropertyAccess(base, prop, _) => {
                self.check_expr(base, locals, label);
                // Unlike a *write* (check_property_assignment, and the
                // Assign/ArrayLiteral-destructuring arms below that
                // deliberately route a PropertyAccess target through only
                // its base, not here) - a genuinely undeclared property
                // creates nothing on read, it just returns null with a
                // warning in real PHP (unless __get exists). So there's
                // no "could be a legitimate dynamic property" case to
                // stay lenient for here, the way there is on write.
                // Same `Union`/`Intersection` handling as `MethodCall`
                // just above, and the identical reasoning for why: a
                // multi-type `catch` variable, or an ordinarily
                // union-typed parameter, used to skip this check
                // entirely rather than fall to `_ => {}`.
                let base_ty = self.infer_expr(base, locals);
                if self.member_confirmed_missing(&base_ty, |c| {
                    self.registry.lookup_property(c, prop).is_some()
                        || self.registry.lookup_method(c, "__get").is_some()
                }) {
                    self.diagnostics.push(Diagnostic {
                        location: label.to_string(),
                        message: format!("access to undefined property {base_ty}::${prop}"),
                        span: e.span,
                        level: 2,
                    });
                }
                self.infer_expr(e, locals)
            }
            ExprKind::DynamicPropertyAccess(base, name_expr) => {
                self.check_expr(base, locals, label);
                self.check_expr(name_expr, locals, label);
                Type::Mixed
            }
            ExprKind::DynamicMethodCall(recv, name_expr, args) => {
                self.check_expr(recv, locals, label);
                self.check_expr(name_expr, locals, label);
                for a in args {
                    self.check_expr(&a.value, locals, label);
                }
                Type::Mixed
            }
            ExprKind::Interpolated(parts) => {
                for p in parts {
                    self.check_expr(p, locals, label);
                }
                self.infer_expr(e, locals)
            }
            ExprKind::Match {
                subject,
                arms,
                default,
            } => {
                self.check_expr(subject, locals, label);
                // Real `match` has no fallthrough - each arm is mutually
                // exclusive, so (unlike `switch`) a later arm/default can
                // always safely assume every earlier arm's own condition
                // was false. `match(true) { $cond => ... }` is a common,
                // deliberate `switch`-replacement idiom specifically
                // because each arm's own condition is checked with `===`
                // - treat the arm's condition itself as an `if`-style
                // condition in that case; otherwise narrow via an
                // implicit `subject === cond` equality, the same as an
                // ordinary `switch` case. Only attempted for a
                // single-condition arm (`1, 2 => ...` multi-value arms
                // fall back to unnarrowed, rather than guessing at a
                // combined "or" narrowing).
                let match_true = matches!(&subject.kind, ExprKind::Literal(Literal::Bool(true)));
                let mut accumulated = locals.clone();
                let mut merged: Option<HashMap<String, Type>> = None;
                for (conds, result) in arms {
                    for c in conds {
                        self.check_expr(c, locals, label);
                    }
                    let mut arm_locals = accumulated.clone();
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut arm_locals, true);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut arm_locals,
                                true,
                            );
                        }
                    }
                    self.check_expr(result, &mut arm_locals, label);
                    merged = Some(match merged {
                        Some(m) => merge_scopes(&m, &arm_locals),
                        None => arm_locals.clone(),
                    });
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut accumulated, false);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut accumulated,
                                false,
                            );
                        }
                    }
                }
                if let Some(d) = default {
                    let mut default_locals = accumulated.clone();
                    self.check_expr(d, &mut default_locals, label);
                    merged = Some(match merged {
                        Some(m) => merge_scopes(&m, &default_locals),
                        None => default_locals,
                    });
                }
                if let Some(m) = merged {
                    *locals = m;
                }
                self.infer_expr(e, locals)
            }
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                ..
            } => {
                // Unlike an arrow function, a plain closure does NOT
                // automatically see the enclosing scope - only variables
                // explicitly named in `use (...)` are captured (by value
                // or by reference; either way, the type it starts with is
                // whatever the outer scope currently has it as). `$this`
                // is the one implicit exception: a non-static closure
                // declared inside a method still auto-binds it, the same
                // as real PHP.
                let mut closure_locals: HashMap<String, Type> = HashMap::new();
                for u in uses {
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    closure_locals.insert(u.name.clone(), ty);
                }
                if !is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
                }
                for p in params {
                    let ty = self.resolve_type_hint(p.type_hint.as_deref());
                    let ty = if p.variadic {
                        Type::Array(Box::new(ty))
                    } else {
                        ty
                    };
                    closure_locals.insert(p.name.clone(), ty);
                }
                let declared_return = Some(self.resolve_type_hint(return_type.as_deref()));
                let closure_label = format!("{label} (closure)");
                self.check_block(
                    body,
                    &mut closure_locals,
                    declared_return.as_ref(),
                    &closure_label,
                );
                self.infer_expr(e, locals)
            }
            ExprKind::ArrowFn {
                params,
                body,
                return_type,
                ..
            } => {
                // An arrow function DOES automatically capture the whole
                // enclosing scope by value (implicit, no `use (...)`
                // needed) - a plain clone of `locals` is exactly that.
                let mut arrow_locals = locals.clone();
                for p in params {
                    let ty = self.resolve_type_hint(p.type_hint.as_deref());
                    let ty = if p.variadic {
                        Type::Array(Box::new(ty))
                    } else {
                        ty
                    };
                    arrow_locals.insert(p.name.clone(), ty);
                }
                self.check_expr(body, &mut arrow_locals, label);
                // Its body is a single implicit-return expression, not a
                // StmtKind::Return - the equivalent of check_stmt's own
                // Return-vs-declared_return check, kept here since
                // there's no StmtKind::Return for it to share.
                if return_type.is_some() {
                    let expected = self.resolve_type_hint(return_type.as_deref());
                    let actual = self.infer_expr(body, &arrow_locals);
                    if actual != Type::Mixed && !self.registry.is_assignable(&actual, &expected) {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!(
                                "return type mismatch: expected {expected}, got {actual}"
                            ),
                            span: body.span,
                            level: 3,
                        });
                    }
                }
                self.infer_expr(e, locals)
            }
            // Finding 28: `foo(...)`/`$obj->method(...)`/`Class::method(...)`
            // (PHP 8.1 first-class-callable syntax) always wraps a
            // Call/MethodCall/StaticCall/DynamicMethodCall with an empty
            // argument list (this project's own parser convention) - not
            // dispatching to that inner node's own ordinary arm directly:
            // its own `check_args` would misread "no arguments were
            // written" as "zero arguments were passed to this call,"
            // producing a false "expects at least N argument(s)"
            // diagnostic for any real function/method with required
            // parameters. So this checks only existence - the one thing
            // that doesn't depend on argument count at all - mirroring
            // each inner arm's own existence-check logic without its
            // argument handling.
            ExprKind::FirstClassCallable(inner) => {
                match &inner.kind {
                    ExprKind::Call(callee, _) => {
                        if let ExprKind::Name(name) = &callee.kind {
                            if !is_language_construct(name) {
                                let (primary, fallback) =
                                    self.name_ctx.resolve_function_candidates(name);
                                let found = self.registry.lookup_function(&primary).is_some()
                                    || fallback.as_deref().is_some_and(|g| {
                                        self.registry.lookup_function(g).is_some()
                                    });
                                if !found {
                                    self.diagnostics.push(Diagnostic {
                                        location: label.to_string(),
                                        message: format!("call to undefined function {primary}()"),
                                        span: e.span,
                                        level: 0,
                                    });
                                }
                            }
                        } else {
                            self.check_expr(callee, locals, label);
                        }
                    }
                    ExprKind::MethodCall(recv, method, _, _) => {
                        self.check_expr(recv, locals, label);
                        let recv_ty = self.infer_expr(recv, locals);
                        let recv_class = match &recv_ty {
                            Type::Object(class, _) => Some(class.clone()),
                            Type::Union(members) | Type::Intersection(members) => {
                                members.iter().find_map(|m| match m {
                                    Type::Object(class, _) => Some(class.clone()),
                                    _ => None,
                                })
                            }
                            _ => None,
                        };
                        if let Some(class) = &recv_class
                            && self.registry.classes.contains_key(class)
                            && self.member_confirmed_missing(&recv_ty, |c| {
                                self.registry.lookup_method(c, method).is_some()
                                    || self.registry.lookup_method(c, "__call").is_some()
                            })
                        {
                            self.diagnostics.push(Diagnostic {
                                location: label.to_string(),
                                message: format!("call to undefined method {recv_ty}::{method}()"),
                                span: e.span,
                                level: 0,
                            });
                        }
                    }
                    ExprKind::StaticCall(class_name, method, _) => {
                        if let Some(class) = self.resolve_static_class_name(class_name)
                            && self.registry.classes.contains_key(&class)
                            && self.registry.lookup_method(&class, method).is_none()
                            && self
                                .registry
                                .lookup_method(&class, "__callStatic")
                                .is_none()
                        {
                            self.diagnostics.push(Diagnostic {
                                location: label.to_string(),
                                message: format!(
                                    "call to undefined static method {class}::{method}()"
                                ),
                                span: e.span,
                                level: 0,
                            });
                        }
                    }
                    ExprKind::DynamicMethodCall(recv, name_expr, _) => {
                        self.check_expr(recv, locals, label);
                        self.check_expr(name_expr, locals, label);
                    }
                    _ => {
                        self.check_expr(inner, locals, label);
                    }
                }
                self.infer_expr(e, locals)
            }
            _ => self.infer_expr(e, locals),
        }
    }

    /// A bare-variable argument that isn't defined yet is either a real bug
    /// or PHP's by-reference "output parameter" idiom
    /// (`preg_match($pattern, $subject, $matches)` defines `$matches` as a
    /// side effect). Without tracking every stdlib function's exact
    /// by-ref positions, the safe default for a call to an *unknown*
    /// function is to assume it might be exactly that and define the
    /// variable (as Mixed) rather than flag it; a *known* function only
    /// gets this leniency for the specific parameter position actually
    /// marked by-ref.
    /// `sig: None` (an unresolvable call) conservatively treats *any*
    /// bare-variable argument, in its own written position, as a
    /// definition site - the real reason this function exists at all
    /// (see its own callers' comments on `isset`/`empty`). Once a real
    /// signature is known, `resolved` (Item 1's own per-parameter
    /// resolution, already computed by the caller) is used instead of the
    /// raw argument list - a named or reordered-named by-ref argument
    /// needs to be matched against its *real* declared parameter, not
    /// whichever parameter happens to sit at the same index it was
    /// written at.
    fn predefine_byref_or_unknown_args(
        &self,
        sig: Option<&FunctionSig>,
        args: &[Arg],
        resolved: &[Option<&Expr>],
        locals: &mut HashMap<String, Type>,
    ) {
        let Some(sig) = sig else {
            for arg in args {
                let ExprKind::Variable(name) = &arg.value.kind else {
                    continue;
                };
                locals.entry(name.clone()).or_insert(Type::Mixed);
            }
            return;
        };
        for (i, slot) in resolved.iter().enumerate() {
            let Some(arg_expr) = slot else {
                continue;
            };
            let ExprKind::Variable(name) = &arg_expr.kind else {
                continue;
            };
            if locals.contains_key(name) {
                continue;
            }
            let by_ref = sig
                .params
                .get(i)
                .map(|p| p.by_ref)
                .or_else(|| sig.params.last().filter(|p| p.variadic).map(|p| p.by_ref))
                .unwrap_or(false);
            if by_ref {
                locals.insert(name.clone(), Type::Mixed);
            }
        }
    }

    /// Checks a closure/arrow-fn literal's body with `$this` seeded to
    /// `this_ty` instead of whatever (if anything) the enclosing scope's
    /// own `locals` would otherwise provide - used by `Closure::bind`/
    /// `->bindTo()` (Finding 49), both of which explicitly rebind `$this`
    /// independently of where the closure was lexically declared. A
    /// clone of `locals`, not a mutation of it: the seeded `$this` (and
    /// anything the closure's own body binds) must not leak back into the
    /// caller's real scope, matching how `ExprKind::Closure`'s own
    /// ordinary handling already keeps `closure_locals` separate.
    fn check_closure_literal_with_this(
        &mut self,
        closure_expr: &Expr,
        locals: &HashMap<String, Type>,
        label: &str,
        this_ty: Type,
    ) {
        let mut seeded = locals.clone();
        seeded.insert("this".to_string(), this_ty);
        self.check_expr(closure_expr, &mut seeded, label);
    }

    fn check_property_assignment(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        locals: &HashMap<String, Type>,
        label: &str,
    ) {
        let ExprKind::PropertyAccess(base, prop, _) = &lhs.kind else {
            return;
        };
        let Type::Object(class, type_args) = self.infer_expr(base, locals) else {
            return;
        };
        let Some((owner, prop_sig)) = self.registry.lookup_property_with_owner(&class, prop) else {
            // Not a declared property: could be a dynamic property, which
            // is legal (if deprecated) PHP - not flagged here, only wrong
            // types on properties we do know about.
            return;
        };
        // Finding 47: a `readonly` property may only be initialized once,
        // from within the scope of the class that declares it - in
        // practice, almost always its own constructor. Pragmatic first
        // cut (per the plan's own text): flag every write outside the
        // literal syntactic body of the declaring class's own
        // constructor, rather than attempting real "has this exact
        // property already been initialized" dataflow - a promoted
        // constructor parameter never reaches this check at all (no
        // `ExprKind::Assign` node exists for it in source), so it counts
        // as the allowed first write automatically.
        if prop_sig.is_readonly
            && (self.current_class.as_deref() != Some(owner.as_str())
                || self.current_method.as_deref() != Some("__construct"))
        {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!("{owner}::${prop} is readonly and cannot be modified here"),
                span: lhs.span,
                level: 0,
            });
        }
        let bindings = self.class_template_bindings(&class, &type_args, &owner);
        let expected = types::substitute_template(&prop_sig.ty, &bindings);
        let expected = self.resolve_pseudo(&expected, &class);
        let actual = self.infer_expr(rhs, locals);
        if actual == Type::Mixed || self.registry.is_assignable(&actual, &expected) {
            return;
        }
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!("{class}::${prop} expects {expected}, {actual} given"),
            span: rhs.span,
            level: 3,
        });
    }

    /// Flags `$x['key']` when `$x`'s type is a known `ArrayShape` and 'key'
    /// isn't one of its declared fields - a real typo-catcher, not a guess:
    /// only fires when the shape is fully known and the key is a literal,
    /// so a dynamic key or an unknown array type is never flagged. Reading
    /// a genuinely *optional* field is not flagged either - that's a
    /// possibly-undefined-index warning, a different (and noisier) check
    /// this prototype doesn't attempt.
    fn check_shape_key_access(
        &mut self,
        base: &Expr,
        idx: &Expr,
        locals: &HashMap<String, Type>,
        label: &str,
    ) {
        let Type::ArrayShape(fields) = self.infer_expr(base, locals) else {
            return;
        };
        let Some(key) = literal_string_key(idx) else {
            return;
        };
        if fields.iter().any(|(name, _, _)| *name == key) {
            return;
        }
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!("array{{...}} has no key '{key}'"),
            span: idx.span,
            level: 2,
        });
    }

    #[allow(clippy::too_many_arguments)]
    /// `args` is the raw, as-written argument list (needed for the two
    /// count-based checks below, both of which are about how many
    /// arguments were actually written, not which parameter each one
    /// resolves to); `resolved` is Item 1's own per-parameter resolution
    /// of that same list (`resolve_call_args`), used for the "too few"
    /// check's own missing-required-parameter test (correct for a named
    /// call that fills required parameters out of order, where a raw
    /// argument *count* alone can't tell) and for the per-argument
    /// type-checking loop.
    fn check_args(
        &mut self,
        callee_label: &str,
        sig: &FunctionSig,
        args: &[Arg],
        resolved: &[Option<&Expr>],
        locals: &HashMap<String, Type>,
        label: &str,
        defining_class: Option<&str>,
        span: Span,
    ) {
        let required = sig
            .params
            .iter()
            .filter(|p| !p.variadic && !p.has_default)
            .count();
        let missing_required = resolved
            .iter()
            .take(required)
            .filter(|s| s.is_none())
            .count();
        if missing_required > 0 {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!(
                    "{callee_label}() expects at least {required} argument(s), {} given",
                    args.len()
                ),
                span,
                level: 0,
            });
        }

        // Finding 40: calling a non-variadic function/method with more
        // positional arguments than it can possibly accept is a real PHP
        // fatal error ("Too many arguments... expected exactly N"), just
        // as basic a check as the "too few" one right above it - but
        // nothing here caught it before, since the per-argument loop
        // below simply stops (via its own fallback once `sig.params.get(i)`
        // runs out and the last param isn't variadic) rather than
        // reporting anything. Uses the raw `args.len()` deliberately, not
        // `resolved`'s own length - a non-variadic signature's resolved
        // slice is always exactly `params.len()` long by
        // `resolve_call_args`'s own design, which would make this check
        // always read `false` if it read from that shape instead.
        let last_is_variadic = sig.params.last().is_some_and(|p| p.variadic);
        if !last_is_variadic && args.len() > sig.params.len() {
            let quantifier = if required == sig.params.len() {
                "exactly"
            } else {
                "at most"
            };
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!(
                    "{callee_label}() expects {quantifier} {} argument(s), {} given",
                    sig.params.len(),
                    args.len()
                ),
                span,
                level: 0,
            });
        }

        for (i, slot) in resolved.iter().enumerate() {
            let Some(arg) = slot else {
                continue;
            };
            let param = match sig.params.get(i) {
                Some(p) => p,
                None => match sig.params.last() {
                    Some(p) if p.variadic => p,
                    _ => continue,
                },
            };
            let arg_ty = self.infer_expr(arg, locals);
            if arg_ty == Type::Mixed {
                continue;
            }
            let param_ty = match defining_class {
                Some(c) => self.resolve_pseudo(&param.ty, c),
                None => param.ty.clone(),
            };
            if !self.registry.is_assignable(&arg_ty, &param_ty) {
                self.diagnostics.push(Diagnostic {
                    location: label.to_string(),
                    message: format!(
                        "{callee_label}() argument #{} (${}) expects {}, {} given",
                        i + 1,
                        param.name,
                        param_ty,
                        arg_ty
                    ),
                    span: arg.span,
                    level: 5,
                });
            }
        }
    }

    /// Phase 12 item 3: validates a `sprintf`/`printf` call's format
    /// string against the arguments actually passed - only when the
    /// format string is a literal (mirrors `check_shape_key_access`'s
    /// own "only a literal, never a dynamic value" precedent) and
    /// `parse_format_specifiers` confidently understood every specifier
    /// in it (bails silently on anything it doesn't, e.g. a positional
    /// `%1$s` specifier). Two checks, both deliberately narrow to avoid
    /// false positives: too few arguments for the placeholders present
    /// (real PHP throws `ValueError` for this - unambiguously a bug,
    /// never intentional) - checked instead of "too many," since passing
    /// extra arguments is valid, unremarkable PHP that real `sprintf`
    /// just ignores; and an argument whose type couldn't possibly satisfy
    /// its specifier (an array, or an object for a numeric specifier).
    fn check_format_string_call(
        &mut self,
        name: &str,
        args: &[Arg],
        locals: &HashMap<String, Type>,
        label: &str,
    ) {
        let Some(fmt_expr) = args.first() else {
            return;
        };
        let Some(fmt) = literal_string_value(&fmt_expr.value) else {
            return;
        };
        let Some(specs) = parse_format_specifiers(fmt) else {
            return;
        };
        let provided = &args[1..];
        if specs.len() > provided.len() {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!(
                    "{name}() format string expects {} argument(s), {} given",
                    specs.len(),
                    provided.len()
                ),
                span: fmt_expr.value.span,
                level: 0,
            });
            return;
        }
        for (spec, arg) in specs.iter().zip(provided) {
            let arg_ty = self.infer_expr(&arg.value, locals);
            if !format_specifier_accepts(*spec, &arg_ty) {
                self.diagnostics.push(Diagnostic {
                    location: label.to_string(),
                    message: format!("{name}() format specifier %{spec} does not accept {arg_ty}"),
                    span: arg.value.span,
                    level: 5,
                });
            }
        }
    }

    fn infer_expr(&self, e: &Expr, locals: &HashMap<String, Type>) -> Type {
        match &e.kind {
            ExprKind::Literal(lit) => match lit {
                Literal::Int(n) => Type::IntLiteral(*n),
                Literal::Float(_) => Type::Float,
                Literal::String(s) => Type::StringLiteral(s.clone()),
                Literal::Bool(true) => Type::True,
                Literal::Bool(false) => Type::False,
                Literal::Null => Type::Null,
            },
            ExprKind::Variable(name) => locals.get(name).cloned().unwrap_or(Type::Mixed),
            ExprKind::Grouping(inner) | ExprKind::Clone(inner, _) => self.infer_expr(inner, locals),
            // A closure/arrow-fn literal, and first-class-callable syntax
            // (`strlen(...)`, `$obj->method(...)`), all produce a real
            // `Closure` instance at runtime in PHP - infer the same
            // `Object("Closure", ...)` the native `Closure` type hint
            // itself now resolves to (rather than the broader
            // `Type::Callable`, which `callable`-hinted parameters
            // accept via `is_assignable`'s own separate leniency), so a
            // strictly `Closure`-typed parameter correctly accepts them.
            ExprKind::FirstClassCallable(_)
            | ExprKind::Closure { .. }
            | ExprKind::ArrowFn { .. } => Type::Object("Closure".to_string(), Vec::new()),
            ExprKind::Match {
                subject,
                arms,
                default,
            } => {
                // Same "don't let an unknown branch compose into a
                // partial Union that looks more precise than it is" rule
                // Ternary already applies, generalized to N arms plus an
                // optional default - and the same per-arm narrowing
                // `check_expr`'s own Match handling applies, so a
                // `default` arm returning the match's own subject infers
                // its real, narrowed type rather than the subject's
                // unnarrowed original type.
                let match_true = matches!(&subject.kind, ExprKind::Literal(Literal::Bool(true)));
                let mut accumulated = locals.clone();
                let mut result_types: Vec<Type> = Vec::new();
                for (conds, result) in arms {
                    let mut arm_locals = accumulated.clone();
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut arm_locals, true);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut arm_locals,
                                true,
                            );
                        }
                    }
                    result_types.push(self.infer_expr(result, &arm_locals));
                    if let [only_cond] = conds.as_slice() {
                        if match_true {
                            self.apply_narrowing(only_cond, &mut accumulated, false);
                        } else {
                            self.apply_equality_narrowing(
                                subject,
                                only_cond,
                                &mut accumulated,
                                false,
                            );
                        }
                    }
                }
                if let Some(d) = default {
                    result_types.push(self.infer_expr(d, &accumulated));
                }
                if result_types.contains(&Type::Mixed) {
                    Type::Mixed
                } else {
                    let mut deduped: Vec<Type> = Vec::new();
                    for t in result_types {
                        if !deduped.contains(&t) {
                            deduped.push(t);
                        }
                    }
                    match deduped.len() {
                        0 => Type::Never,
                        1 => deduped.into_iter().next().unwrap(),
                        _ => Type::Union(deduped),
                    }
                }
            }
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used for
            // a function that always throws or exits.
            ExprKind::Throw(_) => Type::Never,
            // Never given a stable nominal type, since multiple such
            // expressions in the same file could share the placeholder
            // name - Mixed is the honest answer, not an oversight.
            ExprKind::NewAnonymousClass(..) => Type::Mixed,
            ExprKind::Cast(ty, _) => match ty.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "string" => Type::String,
                "bool" => Type::Bool,
                "array" => Type::Array(Box::new(Type::Mixed)),
                "object" => Type::Object("object".to_string(), Vec::new()),
                "unset" => Type::Null,
                _ => Type::Mixed,
            },
            ExprKind::Unary(UnaryOp::Not, _) => Type::Bool,
            // Real PHP's unary minus/plus convert a numeric string
            // operand to a number (`-"5"` is `int(-5)`, not `"-5"`) -
            // the same implicit-numeric-coercion rule arithmetic
            // operators apply generally.
            ExprKind::Unary(UnaryOp::Neg | UnaryOp::Plus, inner) => {
                match self.infer_expr(inner, locals) {
                    Type::String | Type::StringLiteral(_) => Type::Int,
                    other => other,
                }
            }
            ExprKind::Unary(_, inner) => self.infer_expr(inner, locals),
            ExprKind::Binary(op, l, r) => self.infer_binary(op, l, r, locals),
            ExprKind::Assign(_, rhs) => self.infer_expr(rhs, locals),
            // `self`/`parent`/`static` aren't lexed as keywords (see the
            // parser: they're indistinguishable from any other identifier),
            // so a literal `ExprKind::Name("self")` has to be special-cased
            // here rather than showing up as an Object of that name.
            ExprKind::New(class_expr, _) => match &class_expr.kind {
                ExprKind::Name(name) => match name.as_str() {
                    "self" | "parent" | "static" => {
                        let raw = match name.as_str() {
                            "self" => Type::SelfRef,
                            "parent" => Type::ParentRef,
                            _ => Type::StaticRef,
                        };
                        match &self.current_class {
                            Some(c) => self.resolve_pseudo(&raw, c),
                            None => raw,
                        }
                    }
                    _ => Type::Object(self.name_ctx.resolve(name), Vec::new()),
                },
                _ => Type::Mixed,
            },
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) if !is_language_construct(name) => {
                    let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                    self.registry
                        .lookup_function(&primary)
                        .map(|sig| (primary.as_str(), sig))
                        .or_else(|| {
                            fallback
                                .as_deref()
                                .and_then(|g| self.registry.lookup_function(g).map(|sig| (g, sig)))
                        })
                        .map(|(resolved_name, sig)| {
                            self.infer_builtin_call_return_type(resolved_name, sig, args, locals)
                        })
                        .unwrap_or(Type::Mixed)
                }
                _ => Type::Mixed,
            },
            ExprKind::MethodCall(recv, method, args, nullsafe) => {
                // A nullsafe receiver is virtually always nullable
                // (that's the entire reason to write `?->`), so its own
                // inferred type is virtually always `Union([Object(...),
                // Null])`, not a bare `Object` - strip `Null` out before
                // matching, the same real class the method is actually
                // being looked up on either way.
                let recv_ty = narrow_type_excluding(&self.infer_expr(recv, locals), &Type::Null);
                let result = match recv_ty {
                    Type::Object(class, type_args) => self
                        .registry
                        .lookup_method_with_owner(&class, method)
                        .map(|(owner, sig)| {
                            // Class-level substitution (Collection<User>'s
                            // own `T`, or an inherited method's
                            // owner-level `T` via Phase 10.5) runs first,
                            // then infer_call_return_type's existing
                            // per-call-argument binding handles any
                            // template the *method itself* separately
                            // declares - the two are independent
                            // parameter scopes that can both appear on
                            // the same signature.
                            let sig =
                                self.substitute_class_templates(sig, &class, &type_args, &owner);
                            let resolved_args = resolve_call_args(args, &sig.params);
                            let resolved =
                                self.infer_call_return_type(&sig, &resolved_args, locals);
                            self.resolve_pseudo(&resolved, &class)
                        })
                        .unwrap_or(Type::Mixed),
                    _ => Type::Mixed,
                };
                // `$x?->method()` short-circuits to `null` when `$x` is
                // null, on top of whatever the method's own return type
                // is - real, implicit nullability a plain `->` doesn't
                // carry.
                if *nullsafe && result != Type::Mixed {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            ExprKind::StaticCall(class_name, method, args) => self
                .resolve_static_class_name(class_name)
                .and_then(|class| {
                    self.registry.lookup_method(&class, method).map(|sig| {
                        let resolved_args = resolve_call_args(args, &sig.params);
                        let resolved = self.infer_call_return_type(sig, &resolved_args, locals);
                        self.resolve_pseudo(&resolved, &class)
                    })
                })
                .unwrap_or(Type::Mixed),
            ExprKind::StaticAccess(class_name, member) => {
                if member == "class" {
                    return Type::String;
                }
                self.resolve_static_class_name(class_name)
                    .and_then(|class| match member.strip_prefix('$') {
                        Some(prop_name) => self
                            .registry
                            .lookup_property(&class, prop_name)
                            .map(|p| self.resolve_pseudo(&p.ty, &class)),
                        None => self
                            .registry
                            .lookup_const(&class, member)
                            .map(|ty| self.resolve_pseudo(ty, &class)),
                    })
                    .unwrap_or(Type::Mixed)
            }
            ExprKind::Ternary(cond, then_, else_) => {
                let mut else_locals = locals.clone();
                self.apply_narrowing(cond, &mut else_locals, false);
                let else_ty = self.infer_expr(else_, &else_locals);
                match then_ {
                    Some(t) => {
                        let mut then_locals = locals.clone();
                        self.apply_narrowing(cond, &mut then_locals, true);
                        let then_ty = self.infer_expr(t, &then_locals);
                        // Never let an unknown branch compose into a partial
                        // Union that looks more precise than it is.
                        if then_ty == Type::Mixed || else_ty == Type::Mixed {
                            Type::Mixed
                        } else if then_ty == else_ty {
                            then_ty
                        } else {
                            Type::Union(vec![then_ty, else_ty])
                        }
                    }
                    None => Type::Mixed,
                }
            }
            ExprKind::ArrayLiteral(items) => self.infer_array_literal_type(items, locals),
            ExprKind::Interpolated(_) => Type::String,
            ExprKind::PropertyAccess(base, prop, nullsafe) => {
                if let Some(base_key) = narrowing_key(base)
                    && let Some(narrowed) = locals.get(&format!("{base_key}->{prop}"))
                {
                    return narrowed.clone();
                }
                let base_ty = narrow_type_excluding(&self.infer_expr(base, locals), &Type::Null);
                let result = match base_ty {
                    Type::Object(class, type_args) => self
                        .registry
                        .lookup_property_with_owner(&class, prop)
                        .map(|(owner, p)| {
                            let bindings = self.class_template_bindings(&class, &type_args, &owner);
                            let resolved = types::substitute_template(&p.ty, &bindings);
                            self.resolve_pseudo(&resolved, &class)
                        })
                        .unwrap_or(Type::Mixed),
                    _ => Type::Mixed,
                };
                // Same implicit nullability as `?->method()` just above,
                // for `$x?->prop`.
                if *nullsafe && result != Type::Mixed {
                    union_types(result, Type::Null)
                } else {
                    result
                }
            }
            // A homogeneous array's element type applies regardless of
            // which key is accessed; optimistically assumes the access is
            // in bounds/present rather than widening to `V|null` for a
            // possibly-missing element - consistent with this checker's
            // general preference for fewer false positives over catching
            // every possible runtime-undefined access.
            ExprKind::Index(base, Some(idx)) => match self.infer_expr(base, locals) {
                Type::Array(inner) => *inner,
                Type::ArrayShape(fields) => literal_string_key(idx)
                    .and_then(|key| fields.into_iter().find(|(name, _, _)| *name == key))
                    .map(|(_, ty, _)| ty)
                    .unwrap_or(Type::Mixed),
                _ => Type::Mixed,
            },
            _ => Type::Mixed,
        }
    }

    /// Finding 48: an array literal's own element types were never
    /// inferred precisely - every literal read as `Array(Mixed)`
    /// regardless of its actual contents, which silently defeated
    /// `strict_mixed_arrays` (Phase 11 Stage 5) for its own headline use
    /// case (checking a hand-written literal against a declared shape)
    /// and made any destructuring from an inline literal lose all
    /// per-position precision (Finding 42's own gap, from the same root
    /// cause). Builds a real `Type::ArrayShape`, keyed by each literal
    /// key (or, for a positional element, PHP's own real auto-index
    /// rule: one past the highest explicit integer key seen so far, not
    /// just a running count - `[5 => 'a', 'b']`'s `'b'` really is key
    /// `6` in real PHP). A spread element (`...$other`) or a computed,
    /// non-literal key both defeat precise inference for the *whole*
    /// literal - real PHP inserts a variable number of the spread
    /// value's own elements at that point, shifting every subsequent
    /// positional key by an amount this checker has no way to know
    /// statically, and a non-literal key has no fixed name to key a
    /// field by at all - degrading to the honest `Array(Mixed)` for
    /// those specific cases, matching the status quo everywhere else.
    fn infer_array_literal_type(
        &self,
        items: &[ArrayItem],
        locals: &HashMap<String, Type>,
    ) -> Type {
        let mut fields: Vec<(String, Type, bool)> = Vec::new();
        let mut next_index: i64 = 0;
        for item in items {
            if item.spread {
                return Type::Array(Box::new(Type::Mixed));
            }
            let key = match &item.key {
                None => {
                    let k = next_index.to_string();
                    next_index += 1;
                    k
                }
                Some(k) => match &k.kind {
                    ExprKind::Literal(Literal::String(s)) => s.clone(),
                    ExprKind::Literal(Literal::Int(n)) => {
                        next_index = next_index.max(n + 1);
                        n.to_string()
                    }
                    _ => return Type::Array(Box::new(Type::Mixed)),
                },
            };
            let value_ty = self.infer_expr(&item.value, locals);
            // A later duplicate key overwrites an earlier one, matching
            // real PHP's own array-literal semantics.
            match fields.iter_mut().find(|(k, _, _)| *k == key) {
                Some(field) => field.1 = value_ty,
                None => fields.push((key, value_ty, false)),
            }
        }
        Type::ArrayShape(fields)
    }

    /// A generic function's return type may mention its own `@template`
    /// parameters (`TemplateParam`) - this binds each one against the
    /// actual argument at this call site (`identity(5)` binds `T` to
    /// `int`) and substitutes the result into the return type. A signature
    /// with no templates at all is untouched (the common case, cheap).
    fn infer_call_return_type(
        &self,
        sig: &FunctionSig,
        resolved: &[Option<&Expr>],
        locals: &HashMap<String, Type>,
    ) -> Type {
        let mut bindings: HashMap<String, Type> = HashMap::new();
        for (param, slot) in sig.params.iter().zip(resolved.iter()) {
            let Some(arg) = slot else { continue };
            let arg_ty = self.infer_expr(arg, locals);
            types::bind_template_param(&param.ty, &arg_ty, &mut bindings);
        }
        types::substitute_template(&sig.return_type, &bindings)
    }

    /// Finding 48 made a literal array's own inferred type a real
    /// `Type::ArrayShape` instead of always `Array(Mixed)` - every one of
    /// Phase 12's own special cases below that used to pattern-match
    /// `Type::Array(elem_ty)` directly needs this instead, so
    /// `array_map(fn($x) => ..., [1, 2, 3])` (a literal, now a shape) keeps
    /// working the same way it already did for a plain `array<int>`-typed
    /// variable. A shape's own "effective element type" is the union of
    /// every field's type (`Mixed` for an empty literal - nothing to infer
    /// a narrower type from).
    fn array_element_type(ty: &Type) -> Option<Type> {
        match ty {
            Type::Array(elem) => Some((**elem).clone()),
            Type::ArrayShape(fields) => Some(
                fields
                    .iter()
                    .map(|(_, t, _)| t.clone())
                    .reduce(union_types)
                    .unwrap_or(Type::Mixed),
            ),
            _ => None,
        }
    }

    /// Phase 12's bespoke per-function return-type extensions (real
    /// PHPStan's own `src/Type/Php/` is 190 files of exactly this kind of
    /// special-casing, not generic machinery) - `array_map`/`array_filter`
    /// always return a bare, structureless `array` today (element type
    /// `Mixed`), discarding real information already available: the
    /// input array's own element type, and (for `array_map`) the
    /// callback's own return type. Falls through to the ordinary
    /// generic-template path (`infer_call_return_type`) for every other
    /// function, and for any shape of these two this doesn't specially
    /// handle.
    /// Deliberately stays on the raw, positional `args` (not Item 1's own
    /// per-parameter resolution) for every special case below - these are
    /// a small, fixed set of well-known standard-library functions
    /// nobody realistically calls with named arguments, so resolving
    /// them by name would add risk (a resolved slice isn't sized the same
    /// way a raw argument count is - see `check_args`'s own note on this
    /// exact distinction) for no real benefit. Only the generic fallback
    /// at the end needs the real resolution, computed here from the same
    /// raw `args` this function already has.
    fn infer_builtin_call_return_type(
        &self,
        name: &str,
        sig: &FunctionSig,
        args: &[Arg],
        locals: &HashMap<String, Type>,
    ) -> Type {
        match name {
            // Scoped to the common 2-arg single-array form - the real
            // function is variadic (multiple arrays); a 3+-arg call falls
            // through to the generic path below, unchanged.
            "array_map" if args.len() == 2 => {
                if let Some(elem_ty) =
                    Self::array_element_type(&self.infer_expr(&args[1].value, locals))
                    && let Some(mapped) = self.infer_closure_return_with_param_overrides(
                        &args[0].value,
                        &[elem_ty],
                        locals,
                    )
                {
                    return Type::Array(Box::new(mapped));
                }
            }
            // Filtering never transforms elements, so the result is
            // simply whatever the input array's own type already is - no
            // closure introspection needed at all. Real PHP requires at
            // least 1 argument, but a syntactically-valid call passing
            // zero (invalid at runtime, real code in the vendor/ corpus
            // this project verifies against - a genuine, previously
            // unguarded panic here, found via --check-project against
            // the *full* corpus, not just first-party src/) still parses
            // fine and must not be indexed into blindly.
            "array_filter" if !args.is_empty() => {
                let arr_ty = self.infer_expr(&args[0].value, locals);
                if matches!(arr_ty, Type::Array(_) | Type::ArrayShape(_)) {
                    return arr_ty;
                }
            }
            // array_reduce(array $array, callable $callback, mixed
            // $initial = null): mixed - a fold, so the result is
            // ultimately whatever the callback's own return type is
            // (each call's return becomes the next call's $carry). Binds
            // the callback's two params: $carry to $initial's own type
            // (Null when omitted, matching real PHP's own default), $item
            // to the array's element type.
            "array_reduce" if args.len() == 2 || args.len() == 3 => {
                if let Some(item_ty) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                {
                    let carry_ty = args
                        .get(2)
                        .map(|initial| self.infer_expr(&initial.value, locals))
                        .unwrap_or(Type::Null);
                    if let Some(reduced) = self.infer_closure_return_with_param_overrides(
                        &args[1].value,
                        &[carry_ty, item_ty],
                        locals,
                    ) {
                        return reduced;
                    }
                }
            }
            // Combining never changes the values' own type - $keys
            // becomes what our Type::Array can't represent at all (key
            // types aren't tracked), so the result is simply $values'
            // own type verbatim, the same "no transformation" shape as
            // array_filter above.
            "array_combine" if args.len() == 2 => {
                let values_ty = self.infer_expr(&args[1].value, locals);
                if matches!(values_ty, Type::Array(_) | Type::ArrayShape(_)) {
                    return values_ty;
                }
            }
            // Both fill every slot with the same, already-known $value -
            // no closure or element-type binding needed at all.
            "array_fill" if args.len() == 3 => {
                return Type::Array(Box::new(self.infer_expr(&args[2].value, locals)));
            }
            "array_fill_keys" if args.len() == 2 => {
                return Type::Array(Box::new(self.infer_expr(&args[1].value, locals)));
            }
            // Splits one array into several same-shaped ones - the
            // result is an array *of* arrays, each holding the original
            // element type.
            "array_chunk" if args.len() == 2 || args.len() == 3 => {
                if let Some(elem_ty) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                {
                    return Type::Array(Box::new(Type::Array(Box::new(elem_ty))));
                }
            }
            // Extracts one named field from an array of known shapes
            // (e.g. `array<array{id: int, name: string}>`) - only fires
            // when the element type is a real, known ArrayShape and the
            // column key is a compile-time-known literal, mirroring
            // check_shape_key_access's own "only a literal, never a
            // dynamic value" precedent.
            "array_column" if args.len() == 2 || args.len() == 3 => {
                if let Some(Type::ArrayShape(fields)) =
                    Self::array_element_type(&self.infer_expr(&args[0].value, locals))
                    && let Some(key) = literal_string_key(&args[1].value)
                    && let Some((_, field_ty, _)) = fields.into_iter().find(|(n, _, _)| *n == key)
                {
                    return Type::Array(Box::new(field_ty));
                }
            }
            _ => {}
        }
        let resolved = resolve_call_args(args, &sig.params);
        self.infer_call_return_type(sig, &resolved, locals)
    }

    /// Infers a closure or arrow-fn's own return type *as if* its
    /// parameters were bound to `param_overrides` (positionally),
    /// instead of the `Mixed` they'd otherwise get standalone (a
    /// closure/arrow-fn's declared param, with no further context, has
    /// no type unless natively hinted) - this is what lets
    /// `array_map(fn($x) => $x * 2, $ints)` infer a real `array<int>`
    /// return instead of `array<mixed>`, and `array_reduce` bind both its
    /// callback's `$carry`/`$item` params at once. Only the common
    /// exact-arity, non-variadic, non-by-ref shape is handled - a
    /// mismatched param count, or any variadic/by-ref param, safely
    /// returns `None`, falling back to the caller's own generic
    /// (imprecise but correct) behavior. Mirrors the closure-capture-
    /// scope construction `check_expr`'s own `ExprKind::Closure`/
    /// `ExprKind::ArrowFn` arms already do, but on this separate,
    /// read-only inference pass rather than the diagnostic-producing one.
    fn infer_closure_return_with_param_overrides(
        &self,
        callback: &Expr,
        param_overrides: &[Type],
        locals: &HashMap<String, Type>,
    ) -> Option<Type> {
        match &callback.kind {
            ExprKind::ArrowFn {
                params,
                body,
                return_type,
                ..
            } => {
                if return_type.is_some() {
                    return Some(self.resolve_type_hint(return_type.as_deref()));
                }
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut arrow_locals = locals.clone();
                for (p, ty) in params.iter().zip(param_overrides) {
                    arrow_locals.insert(p.name.clone(), ty.clone());
                }
                Some(self.infer_expr(body, &arrow_locals))
            }
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                return_type,
                ..
            } => {
                if return_type.is_some() {
                    return Some(self.resolve_type_hint(return_type.as_deref()));
                }
                if params.len() != param_overrides.len()
                    || params.iter().any(|p| p.variadic || p.by_ref)
                {
                    return None;
                }
                let mut closure_locals: HashMap<String, Type> = HashMap::new();
                for u in uses {
                    let ty = locals.get(&u.name).cloned().unwrap_or(Type::Mixed);
                    closure_locals.insert(u.name.clone(), ty);
                }
                if !*is_static && let Some(this_ty) = locals.get("this") {
                    closure_locals.insert("this".to_string(), this_ty.clone());
                }
                for (p, ty) in params.iter().zip(param_overrides) {
                    closure_locals.insert(p.name.clone(), ty.clone());
                }
                // Only the common single-implicit-return-statement body
                // shape is handled (mirrors check_shape_key_access's own
                // "only handle the shape that actually comes up, don't
                // guess at the rest" precedent) - a closure with a more
                // complex body safely falls back to None too.
                match body.as_slice() {
                    [
                        Stmt {
                            kind: StmtKind::Return(Some(e)),
                            ..
                        },
                    ] => Some(self.infer_expr(e, &closure_locals)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn infer_binary(&self, op: &BinOp, l: &Expr, r: &Expr, locals: &HashMap<String, Type>) -> Type {
        match op {
            BinOp::Concat => Type::String,
            BinOp::Eq
            | BinOp::Identical
            | BinOp::NotEq
            | BinOp::NotIdentical
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::LtEq
            | BinOp::GtEq
            | BinOp::And
            | BinOp::Or
            | BinOp::LogicalXor => Type::Bool,
            // The spaceship operator returns -1/0/1 (an int), not a bool -
            // its single most common real use is returning a value
            // directly from a comparator (`usort`/`uasort`/...).
            BinOp::Spaceship => Type::Int,
            BinOp::Add => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                // `+` on two arrays is PHP's array-union operator, not
                // arithmetic - it keeps every key from the left operand,
                // filling in any key only present on the right. The real
                // per-key result type isn't something `Type::Array`/
                // `Type::ArrayShape` can express precisely today, so
                // `Array(Mixed)` is the honest answer.
                if matches!(lt, Type::Array(_) | Type::ArrayShape(_))
                    && matches!(rt, Type::Array(_) | Type::ArrayShape(_))
                {
                    return Type::Array(Box::new(Type::Mixed));
                }
                if lt == Type::Float || rt == Type::Float {
                    Type::Float
                } else {
                    Type::Int
                }
            }
            BinOp::Sub | BinOp::Mul | BinOp::Pow => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if lt == Type::Float || rt == Type::Float {
                    Type::Float
                } else {
                    Type::Int
                }
            }
            // `%` and the bitwise/shift operators always cast both
            // operands to int before operating, regardless of whether
            // either side was a float - unlike Add/Sub/Mul/Pow, there is
            // no float-promotion rule for these.
            BinOp::Mod | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                Type::Int
            }
            // Real PHP division returns int or float depending on the
            // runtime values, not just their static types - not modeled.
            BinOp::Div => Type::Mixed,
            BinOp::Coalesce => self.infer_expr(r, locals),
            // `$value |> callable` calls callable($value) - its return
            // type depends on resolving whatever expression `callable` is
            // (a string, a closure, a first-class callable reference...)
            // to a real signature, which this checker doesn't attempt.
            BinOp::Pipe => Type::Mixed,
        }
    }
}

/// PHP superglobals are implicitly defined in every scope without any
/// assignment ever appearing in source - undefined-variable detection must
/// never flag these.
/// Real PHP language constructs that are syntactically indistinguishable
/// from an ordinary function call in this parser (none of `isset`,
/// `empty`, `unset`, `eval`, `exit`, `die` are lexed as keywords - see
/// `token.rs` - so `isset($x)` parses as `ExprKind::Call(Name("isset"), ...)`
/// exactly like a real call). None of them are declared in any stub
/// source (they're compiler intrinsics, not functions - a real PHP
/// function can be called through a variable, `$f = 'isset'; $f();` is a
/// fatal error), so without this list every real codebase's constant use
/// of `isset`/`empty`/`unset` would be flagged as calling an undefined
/// function. Matched case-insensitively since PHP itself treats function
/// (and construct) names case-insensitively.
fn is_language_construct(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "isset"
            | "empty"
            | "unset"
            | "eval"
            | "exit"
            | "die"
            // The parser deliberately models these four as a call to a
            // synthetic name (see parser.rs's own comment on that
            // decision) rather than a dedicated AST node - real language
            // constructs, not functions, so they belong on this list for
            // exactly the same reason as isset/empty/unset above.
            | "include"
            | "include_once"
            | "require"
            | "require_once"
            // Same synthetic-call modeling, for the same reason.
            | "print"
    )
}

fn is_superglobal(name: &str) -> bool {
    matches!(
        name,
        "GLOBALS"
            | "_GET"
            | "_POST"
            | "_REQUEST"
            | "_SERVER"
            | "_SESSION"
            | "_COOKIE"
            | "_FILES"
            | "_ENV"
            | "argv"
            | "argc"
    )
}

/// Extracts a shape-lookup key from a literal index expression - `'name'`
/// or a bare int (PHP array keys are always stringified/int-normalized
/// internally, so both forms are compared the same way here). A
/// non-literal index (a variable, a call, ...) returns `None`, since it
/// can't be resolved against a shape's field names statically.
fn literal_string_key(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.clone()),
        ExprKind::Literal(Literal::Int(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// A literal string's own text, or `None` for anything else (a variable,
/// a concatenation, ...) - unlike `literal_string_key`, this doesn't also
/// accept/stringify an int literal, since a format string is never
/// legitimately written as a bare number.
fn literal_string_value(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// Parses a `sprintf`/`printf`-style format string into its ordered list
/// of conversion specifiers (`%d`, `%s`, ...), skipping `%%` (a literal
/// percent, not a placeholder). Returns `None` - "don't know, don't
/// guess" - for anything this doesn't confidently understand: a
/// positional specifier (`%1$s`, real and common but out of scope for
/// this pass, since a lone specifier count no longer maps 1:1 to argument
/// count once positions can repeat or skip), an unknown specifier
/// character, or a malformed/truncated `%` sequence. `%b`/`%c`/`%d`/`%e`/
/// `%E`/`%f`/`%F`/`%g`/`%G`/`%o`/`%s`/`%u`/`%x`/`%X` are real PHP
/// specifier characters (confirmed against the language's own
/// documented format).
fn parse_format_specifiers(fmt: &str) -> Option<Vec<char>> {
    let bytes = fmt.as_bytes();
    let mut specs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        let &next = bytes.get(i)?;
        if next == b'%' {
            i += 1;
            continue;
        }
        // A positional prefix (`%1$s`) is digits immediately followed by
        // `$` - checked before flags/width, since a bare width (`%5d`)
        // is also just digits and must NOT be mistaken for one.
        let digits_start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'$') {
            return None;
        }
        i = digits_start;
        while bytes
            .get(i)
            .is_some_and(|b| matches!(b, b'-' | b'+' | b' ' | b'0'))
        {
            i += 1;
        }
        if bytes.get(i) == Some(&b'\'') {
            i += 2; // custom padding char: the quote plus the pad character itself
        }
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        let &spec = bytes.get(i)?;
        if !b"bcdeEfFgGosuxX".contains(&spec) {
            return None;
        }
        specs.push(spec as char);
        i += 1;
    }
    Some(specs)
}

/// Whether a real PHP conversion specifier could plausibly accept
/// `arg_ty` - deliberately lenient (PHP casts numeric strings and floats
/// for `%d`, and almost anything with a `__toString` for `%s`), so this
/// only flags the unambiguous cases: an array (or a known array shape)
/// for any specifier, or an object for a numeric one - both produce
/// either a fatal error or the literal, useless string `"Array"` in real
/// PHP, never a legitimate value.
fn format_specifier_accepts(spec: char, arg_ty: &Type) -> bool {
    match spec {
        'b' | 'c' | 'd' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'o' | 'u' | 'x' | 'X' => !matches!(
            arg_ty,
            Type::Array(_) | Type::ArrayShape(_) | Type::Object(_, _)
        ),
        's' => !matches!(arg_ty, Type::Array(_) | Type::ArrayShape(_)),
        _ => true,
    }
}

/// Every `TemplateParam` name appearing anywhere in `ty`, recursing into
/// `Array`/`Union`/`Intersection`/`ArrayShape` (structural wrappers that
/// don't change what "position" a template occupies) but *not* into
/// another `Object`'s own type arguments - see `Checker::check_variance`'s
/// doc comment for why that second part is a deliberate scope cut, not an
/// oversight.
fn collect_template_params(ty: &Type, found: &mut HashSet<String>) {
    match ty {
        Type::TemplateParam(name) => {
            found.insert(name.clone());
        }
        Type::Array(inner) => collect_template_params(inner, found),
        Type::Union(members) | Type::Intersection(members) => {
            for m in members {
                collect_template_params(m, found);
            }
        }
        Type::ArrayShape(fields) => {
            for (_, t, _) in fields {
                collect_template_params(t, found);
            }
        }
        _ => {}
    }
}

/// A stable string key for the small set of expressions this checker can
/// narrow: a bare local variable, or a property-access chain rooted in
/// one (`$a->b->c`). Lets `locals` (otherwise keyed by plain variable
/// name) also carry narrowed property-chain types, reusing the exact
/// same map rather than a second, parallel one - safe because a
/// property-chain key always contains `->`, which can never appear in a
/// real PHP variable name, so the two key spaces can't collide.
///
/// Without this, `apply_narrowing`'s `instanceof` handling could only
/// ever narrow a bare variable - `$arg->value instanceof Variable &&
/// $arg->value->name` (an extremely common AST-visitor idiom - narrow a
/// property, then read a property only the narrowed-to subtype has) had
/// no way to remember that narrowing at all, so the second half was
/// checked against `$arg->value`'s un-narrowed declared type. Found via
/// real false positives once undefined-property-on-read detection
/// started actually exercising this: nikic/php-parser AST-visitor code
/// in this project's own `tools/` does exactly this pattern.
fn narrowing_key(e: &Expr) -> Option<String> {
    match &e.kind {
        // No prefix - `locals` is keyed by bare variable name everywhere
        // else in this checker (`ExprKind::Variable(name) => locals.get(name)`),
        // and this has to produce the exact same key for a bare variable
        // to stay a drop-in replacement at the Instanceof-narrowing call
        // site. Collision-free anyway: only the PropertyAccess case below
        // ever produces a key containing `->`, which can't appear in a
        // real PHP variable name.
        ExprKind::Variable(name) => Some(name.clone()),
        ExprKind::PropertyAccess(base, prop, _) => {
            narrowing_key(base).map(|base_key| format!("{base_key}->{prop}"))
        }
        _ => None,
    }
}

fn narrowing_type_for_predicate(name: &str) -> Option<Type> {
    match name {
        "is_string" => Some(Type::String),
        "is_int" | "is_integer" | "is_long" => Some(Type::Int),
        "is_float" | "is_double" => Some(Type::Float),
        "is_bool" | "is_boolean" => Some(Type::Bool),
        "is_array" => Some(Type::Array(Box::new(Type::Mixed))),
        "is_null" => Some(Type::Null),
        "is_object" => Some(Type::Object("object".to_string(), Vec::new())),
        "is_callable" => Some(Type::Callable),
        _ => None,
    }
}

/// Narrows `original` to `target` along the branch where a check for
/// `target` succeeded: if `original` is a union, keep only the member(s)
/// that match; otherwise just trust the more specific `target`.
fn narrow_type_to(original: &Type, target: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members.iter().filter(|m| *m == target).cloned().collect();
            match kept.len() {
                0 => target.clone(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        _ => target.clone(),
    }
}

/// Narrows `original` by removing `excluded` along the branch where a check
/// for it failed - the "not null" half of a `=== null` guard clause, the
/// most common narrowing pattern in real code.
fn narrow_type_excluding(original: &Type, excluded: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members.iter().filter(|m| *m != excluded).cloned().collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        other if other == excluded => Type::Never,
        other => other.clone(),
    }
}

/// Narrows `original` by excluding every falsy member this project can
/// represent (`Null`, `False`) - used for a bare truthiness check
/// (`if ($x)`) and `empty($x)`'s own truthy branch. Real PHP's full falsy
/// set also includes `0`/`0.0`/`""`/`"0"`/an empty array, but those aren't
/// independently representable as excludable members of a `Union` today.
fn narrow_type_excluding_falsy(original: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| !matches!(m, Type::Null | Type::False))
                .cloned()
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::Null | Type::False => Type::Never,
        other => other.clone(),
    }
}

/// Union of two types for a scope-merge join point - flattens nested
/// unions and keeps `Mixed` "unknown" rather than let it compose into a
/// falsely-precise-looking Union (same principle as the ternary inference).
fn union_types(a: Type, b: Type) -> Type {
    if a == b {
        return a;
    }
    if a == Type::Mixed || b == Type::Mixed {
        return Type::Mixed;
    }
    let mut members = Vec::new();
    match a {
        Type::Union(m) => members.extend(m),
        other => members.push(other),
    }
    match b {
        Type::Union(m) => {
            for x in m {
                if !members.contains(&x) {
                    members.push(x);
                }
            }
        }
        other => {
            if !members.contains(&other) {
                members.push(other);
            }
        }
    }
    if members.len() == 1 {
        members.into_iter().next().unwrap()
    } else {
        Type::Union(members)
    }
}

/// Peels through any `(...)`-grouping wrapper(s) to the real expression
/// underneath - `ExprKind::Grouping` is a genuine, distinct AST node
/// (not something parens collapse away during parsing), so a
/// closure-literal-shaped check like Finding 49's own `Closure::bind`/
/// `->bindTo()` special-casing needs to see through
/// `(function () {...})` the same as a bare `function () {...}`.
fn unwrap_grouping(e: &Expr) -> &Expr {
    match &e.kind {
        ExprKind::Grouping(inner) => unwrap_grouping(inner),
        _ => e,
    }
}

/// Item 1: resolves a call's own argument list against the declared
/// parameter list it's being checked against, producing one slot per
/// declared parameter - `Some(&Expr)` when some argument (positional or
/// named) actually fills that parameter, `None` when it doesn't (an
/// omitted optional parameter, or anything past the first spread
/// argument, whose real contents can't be resolved statically at all).
/// Every consumer that used to assume `args[i]` lines up with
/// `params[i]` - true only for the overwhelmingly common all-positional
/// case - now goes through this first, so a named or reordered-named
/// call resolves against the *real* parameter, not whichever one happens
/// to sit at the same index in the written argument list.
///
/// The returned `Vec` can be *longer* than `params.len()` when the last
/// parameter is variadic: real PHP has no way to fill a variadic
/// parameter by name (there's no single name that means "the rest of the
/// arguments"), so once a variadic parameter is in play, any further
/// positional argument is necessarily extra, appended past the initial
/// `params.len()`-sized block - the same shape `check_args`'s own
/// existing variadic fallback (`sig.params.last()` when `i` runs past
/// `params.len()`) already expects.
///
/// A positional argument fills the next open slot by declaration order,
/// tracked via a running count of positional arguments seen so far (not
/// "the next unfilled slot overall" - the two only diverge for a
/// positional argument written *after* a named one, which real PHP
/// itself forbids as a parse-time error; this project's own parser
/// doesn't enforce that ordering, so such input could reach here anyway,
/// but only needs to avoid panicking on it, not produce a meaningful
/// result for syntactically-invalid PHP).
fn resolve_call_args<'a>(args: &'a [Arg], params: &[ParamSig]) -> Vec<Option<&'a Expr>> {
    let mut resolved: Vec<Option<&Expr>> = vec![None; params.len()];
    let mut positional_index = 0usize;
    for arg in args {
        if arg.spread {
            break;
        }
        if let Some(name) = &arg.name {
            if let Some(pos) = params.iter().position(|p| &p.name == name) {
                resolved[pos] = Some(&arg.value);
            }
            continue;
        }
        if positional_index < resolved.len() {
            resolved[positional_index] = Some(&arg.value);
        } else if params.last().is_some_and(|p| p.variadic) {
            resolved.push(Some(&arg.value));
        }
        positional_index += 1;
    }
    resolved
}

/// Finding 58's own notion of "two trait methods with the same name are
/// genuinely incompatible, not just written slightly differently" -
/// deliberately narrow (exact param count and exact per-position/return
/// type equality) rather than a real Liskov-style compatibility check:
/// this is comparing two *unrelated* trait declarations with no
/// inheritance relationship between them at all, so there's no
/// contravariance/covariance direction to apply - only "are these the
/// same signature" makes sense here.
fn signatures_compatible(a: &FunctionSig, b: &FunctionSig) -> bool {
    if a.params.len() != b.params.len() {
        return false;
    }
    if a.return_type != b.return_type {
        return false;
    }
    a.params
        .iter()
        .zip(&b.params)
        .all(|(pa, pb)| pa.ty == pb.ty)
}

/// Joins two branch-local scopes back into one after an `if`/`while`. A
/// variable present in both branches gets the union of its two types; one
/// present in only one branch was introduced there and can't be assumed to
/// exist afterward, so it's dropped rather than carried forward unsoundly.
fn merge_scopes(
    then_locals: &HashMap<String, Type>,
    else_locals: &HashMap<String, Type>,
) -> HashMap<String, Type> {
    let mut merged = HashMap::new();
    for (name, then_ty) in then_locals {
        if let Some(else_ty) = else_locals.get(name) {
            merged.insert(name.clone(), union_types(then_ty.clone(), else_ty.clone()));
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::registry::Registry;
    use crate::token::Position;

    fn diagnostics_for(src: &str) -> Vec<Diagnostic> {
        diagnostics_for_with(src, |_| {})
    }

    fn diagnostics_for_with(src: &str, configure: impl FnOnce(&mut Registry)) -> Vec<Diagnostic> {
        let tokens = Lexer::new(src).tokenize_with_spans();
        let program = Parser::new(tokens).parse_program();
        let mut registry = Registry::build(&program);
        registry.merge_stubs(crate::php_stubs::generated_stubs());
        registry.merge_stub_classes(crate::php_stubs::generated_stub_classes());
        configure(&mut registry);
        let mut checker = Checker::new(&registry);
        checker.check_program(&program);
        checker.into_diagnostics()
    }

    #[test]
    fn flags_wrong_argument_type() {
        let d = diagnostics_for(r#"<?php strlen(42);"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("expects string"));
        // Points at the offending argument `42` itself, not the whole
        // call or the statement.
        assert_eq!(d[0].span.start, Position { line: 1, col: 14 });
        assert_eq!(d[0].span.end, Position { line: 1, col: 16 });
        assert_eq!(d[0].level, 5);
    }

    #[test]
    fn flags_too_few_arguments() {
        let d = diagnostics_for(
            r#"<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            needsTwo(1);
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("expects at least 2"));
        // No single argument represents "the whole call" for an arity
        // mismatch - points at the whole call expression instead.
        assert_eq!(d[0].span.start, Position { line: 3, col: 13 });
        assert_eq!(d[0].span.end, Position { line: 3, col: 24 });
        assert_eq!(d[0].level, 0);
    }

    #[test]
    fn flags_wrong_return_type() {
        let d = diagnostics_for(
            r#"<?php
            function addOne(int $x): int { return $x . "1"; }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("return type mismatch"));
        // Points at the returned expression, not the `return` keyword or
        // the enclosing function declaration.
        assert_eq!(d[0].span.start, Position { line: 2, col: 51 });
        assert_eq!(d[0].span.end, Position { line: 2, col: 59 });
        assert_eq!(d[0].level, 3);
    }

    #[test]
    fn optional_and_variadic_params_do_not_trigger_arity_errors() {
        let d = diagnostics_for(
            r#"<?php
            function f(int $a, int $b = 1, string ...$rest): int { return $a + $b; }
            f(1);
            f(1, 2, "x", "y");
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_wrong_typed_array_literal_element_is_flagged_regardless_of_strict_mixed_arrays() {
        // Finding 48: an array literal's own element types are now
        // inferred precisely (a real ArrayShape, not Array(Mixed)), so
        // real information flows through unconditionally - unlike before
        // Finding 48, `strict_mixed_arrays` isn't what makes this catch a
        // genuinely wrong literal element; ordinary, non-strict checking
        // already does.
        let src = r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            needsInts([1, "two", 3]);
        "#;
        let d = diagnostics_for(src);
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("expects array<int>"), "got: {d:?}");
        // strict_mixed_arrays doesn't change this verdict either way.
        let d = diagnostics_for_with(src, |r| r.strict_mixed_arrays = true);
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn a_correctly_typed_array_literal_is_not_flagged_with_or_without_strict_mixed_arrays() {
        // The other half of the same fix: before Finding 48, a *correct*
        // literal was indistinguishable from an incorrect one once
        // strict_mixed_arrays removed the Array(Mixed) leniency, since
        // neither carried any real element-type information. Now that a
        // literal carries a real shape, the genuinely correct case must
        // stay silent.
        let src = r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            needsInts([1, 2, 3]);
        "#;
        assert!(diagnostics_for(src).is_empty());
        assert!(diagnostics_for_with(src, |r| r.strict_mixed_arrays = true).is_empty());
    }

    #[test]
    fn strict_mixed_arrays_still_flags_a_genuinely_unknown_mixed_typed_array() {
        // strict_mixed_arrays' own remaining, real purpose: a value whose
        // element type is genuinely Mixed for a reason other than being
        // an array literal (here, read back out of an untyped array
        // parameter) - not affected by Finding 48's own literal-specific
        // fix at all.
        let src = r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function test(array $untyped): void { needsInts($untyped); }
        "#;
        assert!(diagnostics_for(src).is_empty());
        let d = diagnostics_for_with(src, |r| r.strict_mixed_arrays = true);
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("expects array<int>"), "got: {d:?}");
    }

    #[test]
    fn a_literal_array_shape_mismatch_is_flagged_against_a_docblock_shape() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{0: int, 1: string} $pair */
            function needsPair(array $pair): void {}
            function test(): void { needsPair(["backwards", 5]); }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("needsPair()")),
            "expected a shape-mismatch diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_correctly_shaped_literal_is_not_flagged_against_a_docblock_shape() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{0: int, 1: string} $pair */
            function needsPair(array $pair): void {}
            function test(): void { needsPair([1, "a"]); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn destructuring_a_swap_from_an_inline_array_literal_gets_real_per_position_types() {
        // Finding 48's shared root cause with Finding 42: before this
        // fix, [$a, $b] = [$b, $a] silently retyped both sides to Mixed,
        // since the literal's own element types were never inferred -
        // masking a real type mismatch the swap introduces.
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            function test(): void {
                $a = 1;
                $b = "x";
                [$a, $b] = [$b, $a];
                needsString($b);
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("needsString() argument #1 ($s) expects string, 1 given")),
            "expected the post-swap type mismatch to be caught, got: {d:?}"
        );
    }

    #[test]
    fn building_an_array_progressively_via_index_assignment_is_not_flagged() {
        // A real regression found while verifying Finding 48 against the
        // full vendor/phpstorm-stubs corpus: $x = []; $x['a'] = ...; is
        // one of the most common ways to build an array in real PHP -
        // check_shape_key_access must not treat the literal's own
        // starting shape as a sealed contract the array can never grow
        // past, either at the write site itself or at a later read.
        let d = diagnostics_for(
            r#"<?php
            function test(): string {
                $x = [];
                $x['a'] = 'hello';
                $x['b'] = 42;
                return $x['a'];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn reading_a_key_never_assigned_after_building_an_array_progressively_is_still_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function test(): void {
                $x = [];
                $x['a'] = 'hello';
                echo $x['b'];
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("array{...} has no key 'b'")),
            "expected a missing-key diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn overwriting_an_existing_key_via_index_assignment_updates_its_tracked_type() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function test(): int {
                $x = ['a' => 1];
                $x['a'] = 5;
                return needsInt($x['a']);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn an_array_callable_shaped_as_a_literal_still_satisfies_callable() {
        let d = diagnostics_for(
            r#"<?php
            class Handler { public function handle(int $x): int { return $x; } }
            function apply(callable $fn, int $x): int { return $fn($x); }
            function test(Handler $h): int { return apply([$h, 'handle'], 5); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_spread_element_inside_a_literal_defeats_precise_shape_inference() {
        // A spread element's own contents aren't known statically (real
        // PHP inserts a variable number of the spread value's own
        // elements, shifting every later positional key by an amount
        // this checker can't know) - must degrade to the honest
        // Array(Mixed) for the whole literal rather than mis-keying the
        // spread expression itself as if it occupied one fixed slot.
        let d = diagnostics_for(
            r#"<?php
            /** @param int[] $nums */
            function needsInts(array $nums): void {}
            function test(array $other): void {
                needsInts([1, ...$other, "not an int"]);
            }
        "#,
        );
        assert!(
            d.is_empty(),
            "a spread element should defeat shape inference, not produce a wrong verdict: {d:?}"
        );
    }

    // Phase 12: array_map/array_filter return-type precision. Each test
    // proves the inferred type is real, not just "still Mixed and
    // therefore compatible with everything" - is_assignable's own
    // Array(Mixed) leniency means an imprecise result would silently
    // satisfy *any* declared element type, so the discriminating check
    // here is a call the real, precise type must actually fail.

    #[test]
    fn array_map_infers_the_real_element_type_from_an_untyped_arrow_fn_param() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            $mapped = array_map(fn($x) => $x * 2, [1, 2, 3]);
            needsInts($mapped);
            needsStrings($mapped);
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("needsStrings"), "got: {d:?}");
        assert!(
            d[0].message
                .contains("expects array<string>, array<int> given"),
            "got: {d:?}"
        );
    }

    #[test]
    fn array_map_infers_the_real_element_type_from_an_untyped_closure_param() {
        // Same as the arrow-fn test, but the common
        // `function($x) { return ...; }` single-return-statement body
        // shape instead.
        let d = diagnostics_for(
            r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            $mapped = array_map(function ($x) { return $x * 2; }, [1, 2, 3]);
            needsStrings($mapped);
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(
            d[0].message
                .contains("expects array<string>, array<int> given"),
            "got: {d:?}"
        );
    }

    #[test]
    fn array_map_trusts_an_explicit_closure_return_type_hint_over_the_array_element_type() {
        // The closure's own declared return type wins, independent of
        // what the array's element type would otherwise suggest.
        let d = diagnostics_for(
            r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            $mapped = array_map(fn(int $x): string => (string) $x, [1, 2, 3]);
            needsStrings($mapped);
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_filter_preserves_the_input_array_s_element_type() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param int[] $xs */
            function f(array $xs) {
                $filtered = array_filter($xs, fn($x) => $x > 0);
                needsInts($filtered);
                needsStrings($filtered);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(
            d[0].message
                .contains("expects array<string>, array<int> given"),
            "got: {d:?}"
        );
    }

    #[test]
    fn array_reduce_infers_the_callback_s_own_return_type() {
        // The result of a fold is ultimately whatever the callback's own
        // return type is (each call's return becomes the next call's
        // $carry) - here that's Int, so passing it where a string is
        // expected must be flagged, proving this isn't just Mixed.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function needsString(string $s): void {}
            $sum = array_reduce([1, 2, 3], fn($carry, $item) => $carry + $item, 0);
            needsInt($sum);
            needsString($sum);
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("needsString"), "got: {d:?}");
    }

    #[test]
    fn array_reduce_binds_item_to_the_array_s_own_element_type() {
        // Returning $item directly (ignoring $carry) isolates the other
        // half of the binding - the array's own element type, not just
        // $initial's.
        let d = diagnostics_for(
            r#"<?php
            /** @param int[] $xs */
            function f(array $xs) {
                $last = array_reduce($xs, fn($carry, $item) => $item, 0);
                needsString($last);
            }
            function needsString(string $s): void {}
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("needsString"), "got: {d:?}");
    }

    #[test]
    fn array_reduce_with_no_initial_value_binds_carry_to_null() {
        // Real PHP defaults $initial to null when omitted - returning
        // $carry directly (ignoring $item) isolates exactly what it was
        // bound to; Null isn't assignable to string, so this must be
        // flagged, proving it wasn't left as an unbound Mixed (which
        // would silently pass any check).
        let d = diagnostics_for(
            r#"<?php
            /** @param int[] $xs */
            function f(array $xs) {
                $result = array_reduce($xs, fn($carry, $item) => $carry);
                needsString($result);
            }
            function needsString(string $s): void {}
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("needsString"), "got: {d:?}");
    }

    #[test]
    fn array_combine_infers_the_values_array_s_own_element_type() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param string[] $keys
             *  @param int[] $vals */
            function f(array $keys, array $vals) {
                $combined = array_combine($keys, $vals);
                needsInts($combined);
                needsStrings($combined);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("needsStrings"), "got: {d:?}");
    }

    #[test]
    fn array_fill_and_array_fill_keys_infer_the_fill_value_s_type() {
        let d = diagnostics_for(
            r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            function f(array $keys) {
                $a = array_fill(0, 5, 42);
                $b = array_fill_keys($keys, 42);
                needsStrings($a);
                needsStrings($b);
            }
        "#,
        );
        assert_eq!(d.len(), 2, "got: {d:?}");
        assert!(d.iter().all(|x| x.message.contains("needsStrings")));
    }

    #[test]
    fn array_chunk_wraps_the_original_element_type_in_an_extra_array_layer() {
        let d = diagnostics_for(
            r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param int[] $xs */
            function f(array $xs) {
                $chunks = array_chunk($xs, 2);
                needsStrings($chunks);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(
            d[0].message
                .contains("expects string[], array<array<int>> given")
                || d[0]
                    .message
                    .contains("expects array<string>, array<array<int>> given"),
            "got: {d:?}"
        );
    }

    #[test]
    fn array_column_extracts_a_known_shape_field_s_type() {
        let d = diagnostics_for(
            r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param array<array{id: int, name: string}> $rows */
            function f(array $rows) {
                $ids = array_column($rows, 'id');
                needsStrings($ids);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("needsStrings"), "got: {d:?}");
    }

    #[test]
    fn array_column_falls_back_safely_without_a_known_shape_or_literal_key() {
        // Neither a plain, unshaped array<int> element type nor a
        // dynamic (non-literal) column key can be resolved to a
        // specific field type - both must fall through to the existing
        // generic behavior without panicking or guessing wrong.
        let d = diagnostics_for(
            r#"<?php
            /** @param int[] $xs */
            function f(array $xs, string $dynamicKey) {
                $a = array_column($xs, 'whatever');
                /** @param array<array{id: int}> $rows */
                function g(array $rows, string $key) {
                    $b = array_column($rows, $key);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_map_array_filter_and_array_reduce_fall_back_safely_for_shapes_they_do_not_special_case()
     {
        // A non-closure callback (a variable, not introspectable), a
        // 3+-arg multi-array call, a callback with more than one param,
        // array_filter on an argument this checker has no specific type
        // for (Mixed), and array_reduce with a callback whose arity
        // doesn't match (carry, item) all safely fall through to the
        // existing, imprecise (but correct) generic behavior - no panic,
        // no false positive. Untyped params (Mixed) throughout,
        // deliberately, so this only exercises the fallback paths
        // themselves, not check_args's own separate, unrelated
        // argument-type checking.
        let d = diagnostics_for(
            r#"<?php
            function f(callable $cb, array $a, array $b) {
                $x = array_map($cb, $a);
                $y = array_map(fn($p, $q) => $p + $q, $a, $b);
                $z = array_reduce($a, fn($only_one_param) => $only_one_param);
            }
            function g($m) {
                $w = array_filter($m);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn sprintf_flags_too_few_arguments_for_its_own_placeholders() {
        let d = diagnostics_for(r#"<?php sprintf("%s and %s", "only one");"#);
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(
            d[0].message.contains("expects 2 argument(s), 1 given"),
            "got: {d:?}"
        );
    }

    #[test]
    fn sprintf_does_not_flag_extra_arguments_real_php_just_ignores() {
        let d = diagnostics_for(r#"<?php sprintf("%s", "a", "b", "c");"#);
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn sprintf_flags_an_array_passed_for_any_specifier() {
        let d = diagnostics_for(
            r#"<?php
            function f(array $arr) {
                sprintf("%d", $arr);
                sprintf("%s", $arr);
            }
        "#,
        );
        assert_eq!(d.len(), 2, "got: {d:?}");
        assert!(d.iter().all(|x| x.message.contains("does not accept")));
    }

    #[test]
    fn sprintf_flags_an_object_for_a_numeric_specifier_but_not_a_string_one() {
        // Real PHP's %s accepts anything with __toString (extremely
        // common), so objects are only flagged against a numeric
        // specifier, which no object can legitimately satisfy.
        let d = diagnostics_for(
            r#"<?php
            class Money {}
            function f(Money $m) {
                sprintf("%d", $m);
                sprintf("%s", $m);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("%d"), "got: {d:?}");
    }

    #[test]
    fn sprintf_and_printf_accept_ordinary_scalar_arguments() {
        let d = diagnostics_for(
            r#"<?php
            sprintf("%s is %d years old, %.2f%% sure", "Bob", 30, 99.5);
            printf("%x", 255);
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn sprintf_skips_the_check_entirely_for_shapes_it_does_not_confidently_understand() {
        // A positional specifier (%1$s), a dynamic (non-literal) format
        // string, and an unrecognized specifier character all safely
        // fall back to no check at all - "don't know, don't guess" - even
        // though the positional-args call below is actually missing an
        // argument in spirit (it repeats the same one), and the
        // unknown-specifier call is arguably malformed PHP.
        let d = diagnostics_for(
            r#"<?php
            function f(string $fmt) {
                sprintf('%1$s %2$s', "only one");
                sprintf($fmt, "whatever");
                sprintf("%v", "x");
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_filter_called_with_zero_arguments_does_not_panic() {
        // Real, previously-unguarded panic (index out of bounds) found
        // via --check-project against the *full* real corpus (including
        // vendor/, not just first-party src/) - a syntactically valid
        // but semantically invalid 0-arg array_filter() call parses fine
        // and must not be blindly indexed into.
        let d = diagnostics_for(r#"<?php array_filter();"#);
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("expects at least 1"), "got: {d:?}");
    }

    #[test]
    fn concrete_class_name_satisfies_self_return_type() {
        // Regression test: `new UserRepository()` and `new self()` are
        // equally valid ways to satisfy a `: self` return type declared on
        // UserRepository itself - this used to be a false positive.
        let d = diagnostics_for(
            r#"<?php
            class UserRepository {
                public static function make(): self {
                    return new UserRepository();
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn nullable_self_return_type_is_resolved_the_same_way_a_bare_self_is() {
        // Real, previously-live bug found via real code in this
        // project's own verification corpus: resolve_pseudo only ever
        // handled a *bare* self/static/parent, never one wrapped in a
        // Union - the extremely common `?self`/`self|null` factory-
        // method return type (real example: a `tryFromArray`-style
        // static constructor) stayed unresolved as a bare `SelfRef`,
        // which is_assignable/is_subtype have no arm to accept a
        // concrete class instance against.
        let d = diagnostics_for(
            r#"<?php
            class Widget {
                public static function tryFromArray(array $data): ?self {
                    if (!isset($data['id'])) {
                        return null;
                    }
                    return new self();
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn wrong_class_returned_from_self_typed_method_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                public static function make(): self { return new Bar(); }
            }
            class Bar {}
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("expected Foo"));
        assert!(d[0].message.contains("got Bar"));
    }

    #[test]
    fn fluent_method_chain_through_self_return_type_infers_concretely() {
        let d = diagnostics_for(
            r#"<?php
            class QueryBuilder {
                public function where(): self { return $this; }
            }
            function needsQueryBuilder(QueryBuilder $q): void {}
            $qb = new QueryBuilder();
            needsQueryBuilder($qb->where());
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn flags_wrong_type_assigned_to_typed_property() {
        let d = diagnostics_for(
            r#"<?php
            class Box { public int $value = 0; }
            $box = new Box();
            $box->value = "not a number";
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("Box::$value"));
    }

    #[test]
    fn correct_property_assignment_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Box { public int $value = 0; }
            $box = new Box();
            $box->value = 5;
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn promoted_property_assignment_is_type_checked() {
        let d = diagnostics_for(
            r#"<?php
            class Point {
                public function __construct(public int $x) {}
            }
            $p = new Point(1);
            $p->x = "nope";
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("Point::$x"));
    }

    #[test]
    fn dynamic_property_not_declared_on_class_is_not_flagged() {
        // Undeclared/dynamic properties are legal (if deprecated) PHP -
        // this prototype only checks types on properties it knows about.
        let d = diagnostics_for(
            r#"<?php
            class Box {}
            $box = new Box();
            $box->anything = "fine, not tracked";
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn static_access_flags_undefined_constants_properties_and_methods() {
        // Phase 9's last remaining rule: existence checks for
        // StaticAccess (constants, static properties) and StaticCall
        // (static methods) - the same shape of check as MethodCall's,
        // just for `Foo::` instead of `$foo->`.
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                const BAR = 1;
                public static int $counter = 0;
                public static function baz(): int { return 1; }
            }
            function f() {
                echo Foo::BAR;
                echo Foo::$counter;
                echo Foo::baz();
                echo Foo::class;
                echo Foo::TOTALLY_MADE_UP;
                echo Foo::$madeUpProp;
                echo Foo::madeUpMethod();
            }
        "#,
        );
        assert_eq!(d.len(), 3, "got: {d:?}");
        assert!(d.iter().any(|x| {
            x.message
                .contains("undefined constant Foo::TOTALLY_MADE_UP")
        }));
        assert!(d.iter().any(|x| {
            x.message
                .contains("undefined static property Foo::$madeUpProp")
        }));
        assert!(d.iter().any(|x| {
            x.message
                .contains("undefined static method Foo::madeUpMethod()")
        }));
    }

    #[test]
    fn a_class_with_call_static_suppresses_undefined_static_method_calls() {
        let d = diagnostics_for(
            r#"<?php
            class Proxy {
                public static function __callStatic($name, $args) { return null; }
            }
            function f() {
                Proxy::anything();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn enum_gets_its_built_in_cases_and_backed_enum_gets_from_and_try_from() {
        // Real, built-in static methods every enum has from the language
        // itself (UnitEnum::cases(); BackedEnum::from()/tryFrom() only on
        // a backed one) - none declared in source. The very first
        // real-corpus check after shipping undefined-static-method
        // detection flagged every single real Enum::cases()/::tryFrom()
        // call in the wild before this was added.
        let d = diagnostics_for(
            r#"<?php
            enum Status: string {
                case Active = 'active';
            }
            enum Pure {
                case A;
            }
            function f() {
                foreach (Status::cases() as $s) {
                    echo $s->value;
                }
                Status::from('active');
                Status::tryFrom('nope');
                Pure::cases();
                Pure::from('x');
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(
            d[0].message
                .contains("undefined static method Pure::from()"),
            "a non-backed enum should not have from()/tryFrom(): {d:?}"
        );
    }

    #[test]
    fn enum_cases_are_recognized_by_static_access_and_a_bogus_case_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            enum Status {
                case Active;
                case Inactive;
            }
            function f() {
                $s = Status::Active;
                echo Status::MadeUpCase;
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("undefined constant Status::MadeUpCase")),
            "got: {d:?}"
        );
        assert_eq!(d.len(), 1, "Status::Active should not be flagged: {d:?}");
    }

    #[test]
    fn self_parent_and_static_resolve_to_the_real_class_for_static_access() {
        let d = diagnostics_for(
            r#"<?php
            class Base {
                const SHARED = 1;
                public static function helper(): int { return 1; }
            }
            class Child extends Base {
                public function useParent(): int {
                    return parent::helper() + self::SHARED + static::helper();
                }
                public function useMadeUp(): int {
                    return parent::madeUpParentMethod();
                }
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        // Proves parent:: resolved to the real class Base, not a literal
        // "parent" - the same silent-vs-wrong distinction this whole
        // audit has repeatedly had to design tests around.
        assert!(
            d[0].message
                .contains("undefined static method Base::madeUpParentMethod()"),
            "got: {d:?}"
        );
    }

    #[test]
    fn a_dynamic_static_target_is_never_checked() {
        // `$className::method()` has no statically known class - nothing
        // to check, and definitely not something to guess at.
        let d = diagnostics_for(
            r#"<?php
            function f($className) {
                return $className::method();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn reading_an_undefined_property_is_flagged_but_writing_one_is_not() {
        // Phase 9's last remaining piece: undefined-property-on-*read*
        // detection. Deliberately asymmetric with the write side just
        // above - a write can legitimately create a real (if deprecated)
        // dynamic property, but a read of one that was never declared
        // and never dynamically assigned just returns null with a
        // warning in real PHP, so there's no legitimate case to stay
        // lenient for.
        let d = diagnostics_for(
            r#"<?php
            class Box {
                public int $known = 1;
            }
            function f(Box $box) {
                echo $box->known;
                echo $box->totallyMadeUp;
                $box->dynamicWrite = 5;
                [$box->a, $box->b] = [1, 2];
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("Box::$totallyMadeUp"), "got: {d:?}");
    }

    #[test]
    fn a_class_with_a_magic_get_method_suppresses_undefined_property_reads() {
        let d = diagnostics_for(
            r#"<?php
            class Box {
                public function __get($name) { return null; }
            }
            function f(Box $box) {
                echo $box->anything;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn compound_assignment_to_an_undefined_property_is_flagged_as_a_read() {
        // `+=` reads the current value before writing the new one, so
        // (unlike a plain `=`) the property really does need to already
        // exist - this should NOT get the write side's leniency.
        let d = diagnostics_for(
            r#"<?php
            class Box {}
            function f(Box $box) {
                $box->counter += 1;
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("Box::$counter")),
            "got: {d:?}"
        );
    }

    #[test]
    fn property_read_flows_into_argument_type_check() {
        let d = diagnostics_for(
            r#"<?php
            class Box { public string $label = ""; }
            function needsInt(int $x): int { return $x; }
            $box = new Box();
            needsInt($box->label);
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn guard_clause_narrows_nullable_param_after_early_return() {
        // The single most common narrowing idiom in real PHP: without
        // recognizing that the `if` branch terminates, the narrowed-to-null
        // state from that branch would incorrectly leak past the guard.
        let d = diagnostics_for(
            r#"<?php
            function greet(?string $name): string {
                if ($name === null) {
                    return "stranger";
                }
                return $name;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn guard_clause_narrows_in_else_branch_too() {
        let d = diagnostics_for(
            r#"<?php
            function greet(?string $name): string {
                if ($name === null) {
                    $name = "stranger";
                } else {
                    // $name narrowed to string here already, this is a no-op
                }
                return $name;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn is_string_narrowing_catches_misuse_inside_the_guarded_branch() {
        // Before narrowing, an untyped param stays Mixed forever and this
        // bug would have been silently skipped rather than caught.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function process($value) {
                if (is_string($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn is_callable_narrowing_catches_misuse_inside_the_guarded_branch() {
        // Phase 12 item 6: widened narrowing_type_for_predicate's table
        // to is_callable, the one predicate from the roadmap's own list
        // that actually fits this mechanism (a single-arg call narrowing
        // a bare variable to one concrete Type). The others on that list
        // (class_exists/function_exists/in_array/array_key_exists/
        // method_exists/property_exists) are all real PHP predicates but
        // don't fit this table at all - they're either 2-arg calls (this
        // project's narrowing only matches a single-arg predicate call)
        // or don't narrow a *value's type* the way is_string/is_array/etc.
        // do, so they were deliberately not force-fit here.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function process($value) {
                if (is_callable($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("needsInt"), "got: {d:?}");
    }

    #[test]
    fn instanceof_narrows_to_the_checked_class() {
        let d = diagnostics_for(
            r#"<?php
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if ($animal instanceof Cat) {
                    return useCat($animal);
                }
                return "";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn instanceof_narrows_to_the_checked_class_in_a_namespaced_file() {
        // The bare-name twin above passes trivially even if namespace
        // resolution were completely broken, since an empty namespace
        // makes FQN == bare name. This is the version that actually
        // exercises it: `useCat(Cat $c)`'s param type is resolved to an
        // FQN at *registry-build* time (resolve_names_in_type), while the
        // `instanceof Cat` narrowing resolves it separately at *check*
        // time (name_ctx.resolve) - both need to land on the exact same
        // string for is_assignable's `a == b` class-name comparison to
        // ever succeed.
        let d = diagnostics_for(
            r#"<?php
            namespace App;
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if ($animal instanceof Cat) {
                    return useCat($animal);
                }
                return "";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn scope_fork_does_not_leak_then_branch_assignment_into_else() {
        // Before real forking, then/else shared one mutable scope
        // sequentially, so an assignment in the then-branch could leak into
        // what the else-branch (wrongly) saw.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function f(bool $cond) {
                if ($cond) {
                    $x = "a string";
                } else {
                    $x = 5;
                    needsInt($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn merged_type_after_non_terminating_if_else_is_a_union() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function f(bool $cond) {
                if ($cond) {
                    $x = "a string";
                } else {
                    $x = 5;
                }
                needsInt($x);
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn phpdoc_only_param_type_catches_a_bug_previously_invisible_as_mixed() {
        // Before PHPDoc types, an untyped native param stayed Mixed forever
        // and this call would have been silently skipped.
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double($x) { return $x * 2; }
            double("not an int");
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("double"));
    }

    #[test]
    fn phpdoc_only_param_type_does_not_flag_correct_usage() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double($x) { return $x * 2; }
            double(5);
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn template_binds_from_argument_and_flags_downstream_misuse() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T
             * @param T $value
             * @return T
             */
            function identity($value) { return $value; }
            function needsInt(int $x): int { return $x; }
            needsInt(identity("wrong"));
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn template_binds_from_argument_in_a_namespaced_file() {
        // The non-namespaced twin above already covers the substitution
        // logic itself - what's specifically under test here is that
        // resolving `identity` as a call target (through
        // resolve_function_candidates, since Phase 9.5) doesn't disturb
        // the separate template-binding step that runs afterward on
        // whatever signature that resolution finds.
        let d = diagnostics_for(
            r#"<?php
            namespace App;
            /**
             * @template T
             * @param T $value
             * @return T
             */
            function identity($value) { return $value; }
            function needsInt(int $x): int { return $x; }
            needsInt(identity("wrong"));
        "#,
        );
        assert_eq!(d.len(), 1, "unexpected diagnostics: {d:?}");
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn template_binding_accepts_correctly_typed_usage() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T
             * @param T $value
             * @return T
             */
            function identity($value) { return $value; }
            function needsInt(int $x): int { return $x; }
            needsInt(identity(5));
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_template_binds_element_type_from_a_precisely_typed_argument() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T
             * @param T[] $items
             * @return T
             */
            function firstOf(array $items) { return $items[0]; }
            function needsInt(int $x): int { return $x; }
            /** @param string[] $words */
            function process(array $words) {
                return needsInt(firstOf($words));
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn phpdoc_param_type_is_usable_inside_the_function_own_body() {
        // Regression test for a real bug: check_function used to seed
        // parameter locals from the raw AST's native hint only, so a
        // PHPDoc-only type worked for callers but not for the function's
        // own body analyzing its own parameter.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string $s */
            function f($s) {
                return needsInt($s);
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn homogeneous_array_index_infers_the_element_type() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string[] $words */
            function f(array $words) {
                return needsInt($words[0]);
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn homogeneous_array_index_of_matching_type_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param int[] $nums */
            function f(array $nums) {
                return needsInt($nums[0]);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn shape_key_access_infers_the_field_type() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param array{name: string, age: int} $p */
            function f(array $p) {
                return needsInt($p['age']);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn shape_key_typo_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{name: string, age: int} $p */
            function f(array $p) {
                return $p['agee'];
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("'agee'"));
        assert_eq!(d[0].level, 2);
    }

    #[test]
    fn shape_key_access_with_dynamic_index_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{name: string, age: int} $p */
            function f(array $p, string $key) {
                return $p[$key];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn shape_optional_field_access_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{name: string, age?: int} $p */
            function f(array $p) {
                return $p['age'];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn foreach_binds_the_value_variable_to_the_element_type() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string[] $words */
            function f(array $words) {
                foreach ($words as $w) {
                    needsInt($w);
                }
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn foreach_over_correctly_typed_array_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param int[] $nums */
            function f(array $nums) {
                foreach ($nums as $n) {
                    needsInt($n);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn catch_binds_the_exception_variable_to_its_class() {
        let d = diagnostics_for(
            r#"<?php
            class MyError extends \Exception {
                public function detail(): string { return "detail"; }
            }
            function f() {
                try {
                    throw new MyError();
                } catch (MyError $e) {
                    return $e->detail();
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn catch_binds_the_exception_variable_to_its_class_in_a_namespaced_file() {
        // The bare-name twin above passes trivially even with zero
        // namespace resolution, since an empty namespace makes FQN ==
        // bare name - it can't tell "resolved correctly" apart from
        // "resolution never ran at all". This is the version that
        // actually distinguishes them: asserting a diagnostic *does*
        // appear on an undefined method call is what proves $e's type
        // resolved to the real "App\MyError", not an unresolved bare
        // "MyError" (which the undefined-method check's own leniency
        // guard would have silently suppressed, since a bogus class
        // isn't in the registry - the exact same "looks like silence"
        // failure mode the instanceof/self bug had).
        let d = diagnostics_for(
            r#"<?php
            namespace App;
            class MyError extends \Exception {}
            function f() {
                try {
                    throw new MyError();
                } catch (MyError $e) {
                    return $e->totallyMadeUp();
                }
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("App\\MyError::totallyMadeUp")),
            "expected the resolved fully-qualified class in the diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn static_call_arguments_are_still_checked_for_undefined_variables() {
        // ExprKind::StaticCall had no arm in check_expr's match at all at the
        // time this was written - it fell into the wildcard `_ => {}`,
        // so its argument expressions were never visited by any
        // diagnostic. StaticCall gained its own existence checking later
        // (Phase 9); this test predates that and still just proves the
        // narrower claim - its args are real code checked like any other
        // call's - independent of whichever existence check runs later.
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                public static function bar($x) {}
            }
            function f() {
                Foo::bar($undefinedVar);
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("undefined variable")),
            "expected an undefined-variable diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn nested_function_declared_inside_an_if_still_gets_its_body_checked() {
        // The extremely common `if (!function_exists('foo')) { function
        // foo() {} }` polyfill idiom (every symfony/polyfill-* package)
        // nests a real FunctionDecl inside a block. check_program's own
        // top-level loop only handles FunctionDecl/ClassDecl/EnumDecl at
        // strictly top-level - Registry::index already recurses into
        // if-bodies to find and index these (so calls to them resolve),
        // but check_stmt itself had no arm for the three declaration
        // kinds at all, so a nested one's own body was invisible to every
        // diagnostic, undefined-variable detection included.
        let d = diagnostics_for(
            r#"<?php
            if (!function_exists('foo')) {
                function foo() {
                    return $undefinedVar;
                }
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("undefined variable")),
            "expected an undefined-variable diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn nested_class_and_enum_declared_inside_an_if_get_their_bodies_checked_too() {
        // Same fix as the FunctionDecl test above, but for the other two
        // arms check_stmt gained at the same time - ClassDecl and
        // EnumDecl nested inside a block (`if (!class_exists('Foo')) {
        // class Foo {...} }`, a real BC-compat idiom). Coverage
        // measurement found these two arms specifically still had zero
        // executions even after that fix landed, since the earlier test
        // only exercised the FunctionDecl arm.
        let class_diagnostics = diagnostics_for(
            r#"<?php
            if (!class_exists('Foo')) {
                class Foo {
                    public function bar() {
                        return $undefinedInNestedClass;
                    }
                }
            }
        "#,
        );
        assert!(
            class_diagnostics
                .iter()
                .any(|x| x.message.contains("undefined variable")),
            "expected an undefined-variable diagnostic, got: {class_diagnostics:?}"
        );

        let enum_diagnostics = diagnostics_for(
            r#"<?php
            if (!enum_exists('Status')) {
                enum Status {
                    case Active;
                    public function label(): string {
                        return $undefinedInNestedEnum;
                    }
                }
            }
        "#,
        );
        assert!(
            enum_diagnostics
                .iter()
                .any(|x| x.message.contains("undefined variable")),
            "expected an undefined-variable diagnostic, got: {enum_diagnostics:?}"
        );
    }

    #[test]
    fn do_while_switch_and_static_var_bodies_are_all_checked() {
        // Coverage measurement found StmtKind::DoWhile, StmtKind::Switch, and
        // StmtKind::StaticVar all had zero executions anywhere in checker.rs's
        // own test suite - manually verified correct via the CLI earlier
        // in this audit; this locks all three in as permanent tests.
        let do_while = diagnostics_for(
            r#"<?php
            function f() {
                do {
                    echo $undefinedInDoWhile;
                } while (true);
            }
        "#,
        );
        assert!(
            do_while
                .iter()
                .any(|x| x.message.contains("undefined variable")),
            "expected an undefined-variable diagnostic, got: {do_while:?}"
        );

        let switch = diagnostics_for(
            r#"<?php
            function f($x) {
                switch ($x) {
                    case 1:
                        echo $undefinedInSwitchCase;
                        break;
                }
            }
        "#,
        );
        assert!(
            switch
                .iter()
                .any(|x| x.message.contains("undefined variable")),
            "expected an undefined-variable diagnostic, got: {switch:?}"
        );

        let static_var = diagnostics_for(
            r#"<?php
            function f() {
                static $count = 0;
                return $undefinedInStaticVarBody;
            }
        "#,
        );
        assert!(
            static_var
                .iter()
                .any(|x| x.message.contains("undefined variable")),
            "expected an undefined-variable diagnostic, got: {static_var:?}"
        );
    }

    #[test]
    fn check_expr_recurses_into_every_simple_wrapper_expression() {
        // Coverage measurement found none of these check_expr arms had
        // ever run in the test suite - Grouping, Clone, (expression-
        // position) Throw, Yield, DynamicVariable, string interpolation,
        // and Cast. Given that the exact same "the wildcard arm never
        // recurses into this" failure mode was a real bug for StaticCall
        // earlier in this audit, each is independently verified here
        // rather than assumed correct from reading the source.
        let d = diagnostics_for(
            r#"<?php
            function f() {
                ($undefinedInGrouping);
                clone $undefinedInClone;
                $x = fn() => throw $undefinedInThrowExpr;
                $y = (int)$undefinedInCast;
                $$undefinedDynVarName;
                "interpolated $undefinedInInterp";
            }
        "#,
        );
        for name in [
            "undefinedInGrouping",
            "undefinedInClone",
            "undefinedInThrowExpr",
            "undefinedInCast",
            "undefinedDynVarName",
            "undefinedInInterp",
        ] {
            assert!(
                d.iter()
                    .any(|x| x.message.contains(&format!("undefined variable ${name}"))),
                "expected ${name} to be flagged as undefined, got: {d:?}"
            );
        }
    }

    #[test]
    fn check_expr_recurses_into_dynamic_property_and_method_name_expressions() {
        // $obj->$prop (a single plain variable) is deliberately treated
        // as a literal property name, not the variable's value (see the
        // parser's own "best-effort" comment on that arm) - it does NOT
        // produce a DynamicPropertyAccess/DynamicMethodCall, so it can't
        // exercise this. The brace and double-dollar forms are the ones
        // that actually do, and those are what this checks.
        let d = diagnostics_for(
            r#"<?php
            function f($obj) {
                $obj->{$undefinedInDynPropBrace};
                $obj->$$undefinedInDynMethodDollarDollar();
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("undefined variable $undefinedInDynPropBrace")),
            "got: {d:?}"
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("undefined variable $undefinedInDynMethodDollarDollar")),
            "got: {d:?}"
        );
    }

    #[test]
    fn parent_pseudo_type_resolves_to_the_real_parent_class() {
        // Coverage measurement (cargo llvm-cov) found resolve_pseudo's
        // Type::ParentRef branch had zero test coverage - the exact same
        // pseudo-type-resolution machinery that turned out buggy for
        // self/static elsewhere in this audit. Manually verified this
        // particular path is currently correct; this test locks that in
        // rather than leaving it to keep passing by luck.
        let d = diagnostics_for(
            r#"<?php
            class Animal {}
            class Unrelated {}
            class Dog extends Animal {
                /** @return parent */
                public function asAnimal() {
                    return new Unrelated();
                }
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("expected Animal, got Unrelated")),
            "expected 'parent' to resolve to the real parent class Animal, got: {d:?}"
        );
    }

    #[test]
    fn while_loop_narrows_its_body_and_merges_types_correctly_after() {
        // Same coverage-driven finding: StmtKind::While had zero test coverage
        // for both its instanceof-narrowing inside the body and its
        // post-loop merge_scopes (the loop may run zero times, so a type
        // reassigned only inside the body must come out as a union of the
        // pre-loop and post-body types afterward, not just the body's).
        let inside_narrowing = diagnostics_for(
            r#"<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            function f(?Foo $x) {
                while ($x instanceof Foo) {
                    $x->bar();
                    $x->totallyMadeUp();
                }
            }
        "#,
        );
        assert!(
            inside_narrowing
                .iter()
                .any(|x| x.message.contains("Foo::totallyMadeUp")),
            "expected instanceof narrowing to apply inside a while body, got: {inside_narrowing:?}"
        );

        let post_loop_merge = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function f() {
                $x = 5;
                while (rand(0, 1)) {
                    $x = "hello";
                }
                needsInt($x);
            }
        "#,
        );
        assert!(
            post_loop_merge
                .iter()
                .any(|x| x.message.contains("expects int")),
            "expected the post-loop type to be a union of both possible types, got: {post_loop_merge:?}"
        );
    }

    #[test]
    fn destructuring_assignment_binds_element_type() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string[] $pair */
            function f(array $pair) {
                [$a, $b] = $pair;
                return needsInt($a);
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn list_style_destructuring_also_binds_element_type() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            /** @param string[] $pair */
            function f(array $pair) {
                list($a, $b) = $pair;
                return needsInt($a);
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsInt"));
    }

    #[test]
    fn flags_a_genuinely_undefined_variable() {
        let d = diagnostics_for(
            r#"<?php
            function f() {
                return $neverDefined;
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("$neverDefined"));
        // A real, non-line-1 position - the whole point of this phase.
        assert_eq!(d[0].span.start, Position { line: 3, col: 24 });
        assert_eq!(d[0].span.end, Position { line: 3, col: 37 });
        assert_eq!(d[0].level, 0);
    }

    #[test]
    fn diagnostics_are_reported_in_source_order_not_internal_check_order() {
        // ExprKind::Assign checks its right-hand side before its left
        // (so a bare-variable assignment target isn't flagged as its own
        // undefined read) - a real, internal traversal order that
        // doesn't match source order here: `$rhs` (the later column) is
        // pushed before `$obj` (the earlier one). Without sorting,
        // `$rhs`'s diagnostic would come out first despite reading
        // second in the file.
        let d = diagnostics_for(r#"<?php function f() { $obj->prop = $rhs; }"#);
        assert_eq!(d.len(), 2, "got: {d:?}");
        assert!(d[0].message.contains("$obj"), "got: {d:?}");
        assert!(d[1].message.contains("$rhs"), "got: {d:?}");
        assert!(d[0].span.start.col < d[1].span.start.col);
    }

    #[test]
    fn superglobals_are_never_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f() {
                return $_GET['id'] . $_SERVER['REQUEST_METHOD'] . $GLOBALS['x'];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn for_loop_init_binds_its_variable() {
        // Regression test: the actual assignment-binding side effect used
        // to live only in a separate pass triggered from bare expression
        // statements, so `for ($j = 0; ...)`'s own $j was never bound and
        // would have misfired as undefined the moment this check existed.
        let d = diagnostics_for(
            r#"<?php
            function f() {
                for ($j = 0; $j < 3; $j++) {
                    echo $j;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn global_statement_binds_its_names() {
        let d = diagnostics_for(
            r#"<?php
            function f() {
                global $config;
                return $config;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn chained_assignment_binds_both_targets() {
        let d = diagnostics_for(
            r#"<?php
            function f() {
                $a = $b = 5;
                return $a + $b;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn known_by_ref_output_param_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f(string $text) {
                if (preg_match('/(\d+)/', $text, $matches)) {
                    return $matches[1];
                }
                return null;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn unknown_function_argument_is_leniently_defined_not_flagged() {
        // Calling an undeclared function is itself now flagged (Phase 9),
        // but the original point of this test still holds: $result isn't
        // known to be by-ref (the callee's signature is itself unknown),
        // so it's leniently treated as defined by the call rather than
        // also producing a spurious "undefined variable" diagnostic.
        let d = diagnostics_for(
            r#"<?php
            function f() {
                some_undeclared_function($result);
                return $result;
            }
        "#,
        );
        assert!(
            !d.iter().any(|x| x.message.contains("undefined variable")),
            "unexpected undefined-variable diagnostic: {d:?}"
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("undefined function some_undeclared_function")),
            "expected an undefined-function diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn known_function_non_by_ref_argument_is_still_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function f() {
                return needsInt($neverDefined);
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("$neverDefined"));
    }

    #[test]
    fn extract_suppresses_undefined_variable_checking_for_the_whole_function() {
        let d = diagnostics_for(
            r#"<?php
            function f(array $data) {
                extract($data);
                return $definitelyNotDefinedAnywhere;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn closure_bodies_are_not_checked_so_captures_are_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f(int $base) {
                $fn = function (int $x) use ($base) {
                    return $base + $x;
                };
                return $fn(5);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn keyed_destructuring_binds_its_target() {
        let d = diagnostics_for(
            r#"<?php
            function f(array $data) {
                ['name' => $n] = $data;
                return $n;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn variable_defined_in_only_one_if_branch_is_flagged_after_the_if() {
        let d = diagnostics_for(
            r#"<?php
            function f(bool $cond) {
                if ($cond) {
                    $x = 5;
                }
                return $x;
            }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("$x"));
    }

    #[test]
    fn variable_defined_in_both_if_and_else_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f(bool $cond) {
                if ($cond) {
                    $x = 5;
                } else {
                    $x = 10;
                }
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn generated_stub_return_type_catches_a_real_bug() {
        // json_encode() can return false on failure - a well-known real PHP
        // gotcha - so passing it straight to a plain-string param is a
        // genuine bug this real (not hand-approximated) signature catches.
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): string { return $s; }
            needsString(json_encode(["a" => 1]));
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("needsString"));
    }

    #[test]
    fn narrowing_a_generated_stub_union_return_type_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): string { return $s; }
            function f() {
                $encoded = json_encode(["a" => 1]);
                if ($encoded !== false) {
                    return needsString($encoded);
                }
                return "";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn enum_method_body_is_type_checked() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            enum Suit: string {
                case Hearts = 'H';
                public function bad(): string {
                    return needsInt("not an int");
                }
            }
        "#,
        );
        assert!(d.iter().any(|d| d.message.contains("needsInt")));
    }

    #[test]
    fn enum_method_with_correct_usage_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            enum Suit: string {
                case Hearts = 'H';
                public function good(): int {
                    return needsInt(5);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn anonymous_class_method_body_is_type_checked() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            interface Greeter { public function greet(): string; }
            function make() {
                return new class implements Greeter {
                    public function greet(): string {
                        return needsInt("wrong");
                    }
                };
            }
        "#,
        );
        assert!(d.iter().any(|d| d.message.contains("needsInt")));
    }

    #[test]
    fn anonymous_class_with_correct_usage_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function make() {
                return new class {
                    public function value(): int {
                        return needsInt(5);
                    }
                };
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn instantiating_an_undefined_class_is_flagged() {
        let d = diagnostics_for("<?php function f() { return new TotallyMadeUpClass(); }");
        assert!(
            d.iter()
                .any(|x| x.message.contains("TotallyMadeUpClass not found")),
            "expected an undefined-class diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn instantiating_a_known_class_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Foo {}
            function f() { return new Foo(); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn instantiating_an_interface_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            interface Doer { public function doIt(): void; }
            function test(): Doer { return new Doer(); }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("cannot instantiate interface Doer")),
            "expected a cannot-instantiate-interface diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn instantiating_an_abstract_class_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            abstract class AbstractThing { abstract public function doIt(): void; }
            function test(): AbstractThing { return new AbstractThing(); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("cannot instantiate abstract class AbstractThing")),
            "expected a cannot-instantiate-abstract-class diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn instantiating_a_concrete_subclass_of_an_abstract_class_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            abstract class AbstractThing { abstract public function doIt(): void; }
            class Concrete extends AbstractThing { public function doIt(): void {} }
            function test(): Concrete { return new Concrete(); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_class_missing_an_inherited_abstract_method_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            abstract class Shape2 { abstract public function area(): float; }
            class Circle2 extends Shape2 {}
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("does not implement abstract method(s): area")),
            "expected a missing-abstract-method diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_class_partially_implementing_an_interface_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            interface Multi7 { public function a(): void; public function b(): void; }
            class Impl7 implements Multi7 { public function a(): void {} }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("does not implement abstract method(s): b")),
            "expected a missing-abstract-method diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_class_using_a_trait_with_an_unimplemented_abstract_method_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            trait Greetable9 {
                abstract public function getName(): string;
                public function greet(): string { return "Hello " . $this->getName(); }
            }
            class Person9 { use Greetable9; }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("does not implement abstract method(s): getName")),
            "expected a missing-abstract-method diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_class_that_supplies_a_traits_abstract_method_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            trait Greetable9 {
                abstract public function getName(): string;
                public function greet(): string { return "Hello " . $this->getName(); }
            }
            class Person9 { use Greetable9; public function getName(): string { return "x"; } }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_concrete_implementation_two_hops_up_an_extends_chain_satisfies_a_directly_implemented_interface()
     {
        // Regression for a real BFS-distance bug found while implementing
        // this: a class can `implements Throwable` directly while its
        // real concrete `getCode()` only lives two `extends` hops further
        // up (e.g. `BaseException -> Exception`) - the *closest* ancestor
        // mentioning the name at all must not win over a real, if
        // farther, concrete implementation.
        let d = diagnostics_for(
            r#"<?php
            interface Throwable2 {
                public function getMessage(): string;
                public function getCode();
            }
            class Exception2 implements Throwable2 {
                public function getMessage(): string {}
                public function getCode() {}
            }
            class BaseException2 extends Exception2 implements Throwable2 {}
            class Leaf2 extends BaseException2 implements Throwable2 {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_method_narrowing_an_inherited_parameter_type_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Animal2 {}
            class Dog2 extends Animal2 {}
            class Handler2 { public function handle(Animal2 $a): void {} }
            class DogOnlyHandler2 extends Handler2 { public function handle(Dog2 $a): void {} }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains(
                "DogOnlyHandler2::handle() parameter #1 ($a) narrows the inherited parameter type"
            )),
            "expected a narrowed-parameter diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_method_widening_an_inherited_parameter_type_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Animal2 {}
            class Handler2 { public function handle(Animal2 $a): void {} }
            class WideningHandler2 extends Handler2 { public function handle(object $a): void {} }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_constructor_with_an_incompatible_signature_is_not_flagged() {
        // Real PHP deliberately exempts __construct from override-
        // compatibility checking - a subclass constructor may have an
        // entirely different signature from its parent's.
        let d = diagnostics_for(
            r#"<?php
            class Base10 { public function __construct(string $a) {} }
            class Sub10 extends Base10 { public function __construct(int $a, int $b) {} }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn an_override_with_no_declared_type_is_not_flagged_as_narrowing() {
        // Type::Mixed here means "nothing declared, native or docblock",
        // not a genuinely explicit `mixed` - real PHP doesn't enforce
        // override compatibility for an omitted type at all.
        let d = diagnostics_for(
            r#"<?php
            class Base11 { public function make(): int { return 1; } }
            class Sub11 extends Base11 { public function make() { return 1; } }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn an_override_narrowing_an_inherited_static_return_type_is_not_flagged() {
        // Real PHP's late-static-binding covariance rule: an ancestor's
        // own `static`-typed return is always safely satisfiable by an
        // override returning its own concrete class name.
        let d = diagnostics_for(
            r#"<?php
            interface Collection12 { /** @return static */ public function copy(); }
            class Vector12 implements Collection12 { public function copy(): Vector12 {} }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn overriding_a_final_method_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Base4 { final public function locked(): int { return 1; } }
            class Sub4 extends Base4 { public function locked(): int { return 2; } }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("cannot override final method Base4::locked")),
            "expected a final-override diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn overriding_a_final_method_two_hops_up_the_extends_chain_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Base6 { final public function locked(): void {} }
            class Mid6 extends Base6 {}
            class Leaf6 extends Mid6 { public function locked(): void {} }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("cannot override final method Base6::locked")),
            "expected a final-override diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn overriding_a_non_final_method_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Ok1 { public function open(): int { return 1; } }
            class Ok2 extends Ok1 { public function open(): int { return 2; } }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn redeclaring_a_final_private_method_is_not_flagged() {
        // A `private` method isn't part of a class's inherited/
        // overridable surface at all in real PHP - a subclass declaring
        // its own same-named private method is an independent,
        // unrelated declaration, not an override.
        let d = diagnostics_for(
            r#"<?php
            class Base13 { final private function __clone() {} }
            class Sub13 extends Base13 { final private function __clone() {} }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn extending_a_final_class_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            final class Sealed5 {}
            class Sub5 extends Sealed5 {}
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("class Sub5 cannot extend final class Sealed5")),
            "expected a final-class-extension diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn extending_a_non_final_class_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Open5 {}
            class Sub5b extends Open5 {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn writing_a_readonly_property_from_outside_its_declaring_class_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Point1 {
                public function __construct(public readonly int $x, public readonly int $y) {}
            }
            function mutateReadonly(Point1 $p): void { $p->x = 5; }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("Point1::$x is readonly")),
            "expected a readonly-write diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn writing_a_readonly_property_from_a_non_constructor_method_of_its_own_class_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Immutable {
                public function __construct(public readonly int $v) {}
                public function bump(): void { $this->v = $this->v + 1; }
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("Immutable::$v is readonly")),
            "expected a readonly-write diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_promoted_readonly_property_is_not_flagged_by_its_own_promotion() {
        let d = diagnostics_for(
            r#"<?php
            class Point1 {
                public function __construct(public readonly int $x, public readonly int $y) {}
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn writing_a_readonly_property_inside_its_own_constructor_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Widget14 {
                public readonly int $a;
                public function __construct() { $this->a = 1; }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn writing_an_ordinary_mutable_property_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Ok3 {
                public int $mutable = 0;
                public function change(): void { $this->mutable = 5; }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn throwing_a_non_throwable_class_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class NotAnException2 {}
            function test(): void { throw new NotAnException2(); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("cannot throw NotAnException2: it does not implement Throwable")),
            "expected a cannot-throw diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn throwing_a_real_exception_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function test(): void { throw new \RuntimeException("x"); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn catching_a_non_throwable_class_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class NotAnException1 {}
            function doStuff1(): void {}
            function test(): void {
                try { doStuff1(); } catch (NotAnException1 $e) { echo "caught"; }
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("cannot catch NotAnException1: it does not implement Throwable")),
            "expected a cannot-catch diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn catching_a_real_exception_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function doStuff1(): void {}
            function test(): void {
                try { doStuff1(); } catch (\RuntimeException $e) { echo "caught"; }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_backed_enum_with_a_duplicate_int_value_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            enum Status3: int {
                case Active = 1;
                case Inactive = 1;
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("duplicate value in enum Status3 for case Inactive")),
            "expected a duplicate-enum-value diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_backed_enum_with_a_duplicate_string_value_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            enum Suit: string {
                case Hearts = 'H';
                case Spades = 'H';
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("duplicate value in enum Suit for case Spades")),
            "expected a duplicate-enum-value diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_backed_enum_with_all_unique_values_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            enum StatusOk: int {
                case Active = 1;
                case Inactive = 2;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn calling_a_method_from_a_transitively_used_trait_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            trait Greets { public function greet(): string { return "hi"; } }
            trait Combined { use Greets; public function extra(): int { return 1; } }
            class Foo { use Combined; }
            function test(Foo $f): string { return $f->greet(); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_trait_alias_introduced_via_as_is_callable_under_its_new_name() {
        let d = diagnostics_for(
            r#"<?php
            trait A { public function hello(): string { return "A"; } }
            trait B { public function hello(): string { return "B"; } }
            class C {
                use A, B {
                    A::hello insteadof B;
                    B::hello as helloB;
                }
            }
            function test(C $c): void { echo $c->hello(); echo $c->helloB(); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn an_unqualified_trait_alias_resolves_against_the_single_used_trait() {
        let d = diagnostics_for(
            r#"<?php
            trait X { public function greet(): string { return "x"; } }
            class Y {
                use X {
                    greet as greetAlias;
                }
            }
            function test(Y $y): void { echo $y->greet(); echo $y->greetAlias(); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn calling_an_undeclared_trait_alias_is_still_flagged() {
        let d = diagnostics_for(
            r#"<?php
            trait X { public function greet(): string { return "x"; } }
            class Y {
                use X {
                    greet as greetAlias;
                }
            }
            function test(Y $y): void { echo $y->totallyMadeUp(); }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("undefined method")),
            "expected an undefined-method diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn two_traits_declaring_the_same_property_with_incompatible_types_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            trait T1a { public int $x = 1; }
            trait T1b { public string $x = "a"; }
            class C1 { use T1a, T1b; }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("T1a and T1b define the same property ($x) with incompatible types")),
            "expected a trait-property-conflict diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn two_traits_declaring_the_same_method_with_incompatible_signatures_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            trait T2a { public function foo(): int { return 1; } }
            trait T2b { public function foo(): string { return "a"; } }
            class C2 { use T2a, T2b; }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains(
                "T2a and T2b define the same method (foo) with an incompatible signature"
            )),
            "expected a trait-method-conflict diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn two_traits_declaring_the_same_property_with_identical_types_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            trait T3a { public int $y = 1; }
            trait T3b { public int $y = 2; }
            class C3 { use T3a, T3b; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_trait_method_conflict_resolved_via_insteadof_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            trait T4a { public function bar(): int { return 1; } }
            trait T4b { public function bar(): int { return 2; } }
            class C4 {
                use T4a, T4b {
                    T4a::bar insteadof T4b;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn accessing_a_non_static_property_through_double_colon_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Config {
                public int $instanceOnly = 0;
            }
            function test(): int { return Config::$instanceOnly; }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("access to undefined static property Config::$instanceOnly")),
            "expected an undefined-static-property diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn accessing_a_real_static_property_through_double_colon_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Config {
                public static int $staticOne = 1;
            }
            function test(): int { return Config::$staticOne; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn this_is_available_inside_a_closure_literal_passed_to_closure_bind() {
        let d = diagnostics_for(
            r#"<?php
            class Secret3 { private int $value = 42; }
            function test(Secret3 $s): int {
                $getter = Closure::bind(function () { return $this->value; }, $s, Secret3::class);
                return $getter();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn closure_bind_still_flags_a_real_bug_inside_the_bound_closure() {
        let d = diagnostics_for(
            r#"<?php
            function test(): void {
                Closure::bind(function () { return $this->totallyUndefinedVar; }, new \stdClass());
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("undefined property")),
            "expected the real bug inside the bound closure to still be caught, got: {d:?}"
        );
    }

    #[test]
    fn this_is_available_inside_an_inline_closure_bound_via_bind_to() {
        let d = diagnostics_for(
            r#"<?php
            class Secret4 { private int $value = 99; }
            function test(Secret4 $s): int {
                $bound = (function () { return $this->value; })->bindTo($s, Secret4::class);
                return $bound();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn first_class_callable_of_an_undefined_function_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function test(): void {
                $f = totallyUndefinedFunction(...);
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("call to undefined function totallyUndefinedFunction")),
            "expected an undefined-function diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn first_class_callable_of_a_real_function_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function real(int $a, int $b): int { return $a + $b; }
            function test(): void {
                $f = real(...);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn first_class_callable_of_an_undefined_method_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Foo { public function bar(): void {} }
            function test(Foo $foo): void {
                $g = $foo->totallyUndefinedMethod(...);
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("call to undefined method Foo::totallyUndefinedMethod")),
            "expected an undefined-method diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn first_class_callable_of_a_real_method_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Foo { public function bar(): void {} }
            function test(Foo $foo): void {
                $g = $foo->bar(...);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn first_class_callable_of_an_undefined_static_method_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Foo {}
            function test(): void {
                $h = Foo::totallyUndefinedStatic(...);
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("call to undefined static method Foo::totallyUndefinedStatic")),
            "expected an undefined-static-method diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn first_class_callable_does_not_flag_too_few_arguments() {
        // A real, previously-plausible regression: naively dispatching
        // the wrapped call through its own ordinary arm (which also runs
        // check_args) would misread "no arguments were written" as
        // "zero arguments were passed."
        let d = diagnostics_for(
            r#"<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            function test(): void {
                $f = needsTwo(...);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn spread_argument_is_not_checked_as_the_first_positional_argument() {
        let d = diagnostics_for(
            r#"<?php
            function needsThree(int $a, int $b, int $c): int { return $a + $b + $c; }
            function test(array $args): int { return needsThree(...$args); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn spread_argument_in_a_constructor_call_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Point {
                public function __construct(int $x, int $y) {}
            }
            function test(array $coords): Point { return new Point(...$coords); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn spread_argument_in_sprintf_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function test(array $args): string { return sprintf("%s %s", ...$args); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn spread_argument_in_a_method_call_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Calc { public function add(int $a, int $b): int { return $a + $b; } }
            function test(Calc $c, array $args): int { return $c->add(...$args); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn spread_argument_in_a_static_call_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Calc { public static function multiply(int $a, int $b): int { return $a * $b; } }
            function test(array $args): int { return Calc::multiply(...$args); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_non_spread_call_with_too_few_arguments_is_still_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            function test(): int { return needsTwo(1); }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("expects at least 2 argument(s)")),
            "expected a too-few-arguments diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_call_with_too_many_arguments_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            function test(): int { return needsTwo(1, 2, 3, 4, 5); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("needsTwo() expects exactly 2 argument(s), 5 given")),
            "expected a too-many-arguments diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_constructor_call_with_too_many_arguments_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Point40 { public function __construct(int $x, int $y) {} }
            function test(): Point40 { return new Point40(1, 2, 3, 4, 5); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("Point40::__construct() expects exactly 2 argument(s), 5 given")),
            "expected a too-many-arguments diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_method_call_with_too_many_arguments_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Calc40 { public function add(int $a, int $b): int { return $a + $b; } }
            function test(Calc40 $c): int { return $c->add(1, 2, 3, 4); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("Calc40::add() expects exactly 2 argument(s), 4 given")),
            "expected a too-many-arguments diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_static_call_with_too_many_arguments_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Calc40b { public static function multiply(int $a, int $b): int { return $a * $b; } }
            function test(): int { return Calc40b::multiply(1, 2, 3, 4); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("Calc40b::multiply() expects exactly 2 argument(s), 4 given")),
            "expected a too-many-arguments diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn too_many_arguments_says_at_most_when_a_parameter_has_a_default() {
        let d = diagnostics_for(
            r#"<?php
            function withDefault40(int $a, int $b = 5): int { return $a + $b; }
            function test(): int { return withDefault40(1, 2, 3); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("withDefault40() expects at most 2 argument(s), 3 given")),
            "expected an at-most-N-arguments diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_variadic_function_accepting_extra_positional_arguments_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function withVariadic40(int $a, int ...$rest): int { return $a; }
            function test(): int { return withVariadic40(1, 2, 3, 4, 5); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_docblock_only_variadic_param_type_is_still_checked_against_call_sites() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int ...$nums */
            function sumDocVariadic(...$nums): void {}
            function test(): void { sumDocVariadic(1, 2, "not an int"); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("sumDocVariadic() argument #3 ($nums) expects int, 'not an int' given")),
            "expected a variadic-argument-type diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_named_argument_call_in_declaration_order_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f1(int $a, string $b): void {}
            function test(): void { f1(a: 1, b: "x"); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_named_argument_call_in_a_different_order_than_declared_is_not_misflagged() {
        // The real bug Item 1 fixes: naive positional reasoning would
        // compare "x" against $a's own int type and "1" against $b's
        // own string type, both wrongly.
        let d = diagnostics_for(
            r#"<?php
            function f1(int $a, string $b): void {}
            function test(): void { f1(b: "x", a: 1); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_named_argument_with_the_wrong_type_is_still_flagged_against_its_real_parameter() {
        let d = diagnostics_for(
            r#"<?php
            function f1(int $a, string $b): void {}
            function test(): void { f1(b: "x", a: "not an int"); }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("f1() argument #1 ($a) expects int")),
            "expected a wrong-type diagnostic attributed to $a, got: {d:?}"
        );
    }

    #[test]
    fn a_mix_of_positional_then_named_arguments_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f3(int $a, int $b, int $c): int { return $a + $b + $c; }
            function test(): int { return f3(1, c: 3, b: 2); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn an_omitted_defaulted_parameter_filled_by_neither_position_nor_name_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f4(int $a, int $b = 5): int { return $a + $b; }
            function test(): int { return f4(a: 1); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_spread_call_leaves_every_parameter_position_unresolved_rather_than_guessing() {
        let d = diagnostics_for(
            r#"<?php
            function f6(int $a, int $b): void {}
            function test(array $args): void { f6(...$args); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_variadic_call_with_more_positional_arguments_than_declared_params_resolves_correctly() {
        let d = diagnostics_for(
            r#"<?php
            function f7(int $a, string ...$rest): void {}
            function test(): void { f7(1, "x", "y", "z"); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_by_ref_parameter_filled_via_a_reordered_named_argument_binds_the_right_variable() {
        // The specific failure mode that originally motivated Item 1: a
        // by-ref output parameter, called with reordered named arguments,
        // must bind the *correct* variable - not whichever one lands at
        // the naive positional index.
        let d = diagnostics_for(
            r#"<?php
            function f9(string $pattern, string $subject, &$matches): int { $matches = []; return 1; }
            function test(string $s): void {
                f9(subject: $s, matches: $m, pattern: '/x/');
                echo $m;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn use_import_resolves_to_its_fully_qualified_name_before_lookup() {
        let d = diagnostics_for(
            r#"<?php
            use App\Domain\Widget;
            function f() { return new Widget(); }
        "#,
        );
        // App\Domain\Widget isn't actually declared anywhere in this
        // snippet, so this documents the known, accepted limitation (no
        // `use` target is ever really loaded here) rather than a false
        // negative in the resolution logic itself - what's under test is
        // that `use`-import resolution runs (Widget -> App\Domain\Widget)
        // before the registry lookup, without panicking and without also
        // flagging an unrelated spurious diagnostic; a real project
        // supplying the actual declaration file is what makes this
        // resolve cleanly, verified separately against Piwigo/vendor.
        assert!(
            d.iter()
                .any(|x| x.message.contains("App\\Domain\\Widget not found")),
            "expected the fully-qualified name in the diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn instantiating_a_class_via_a_leading_backslash_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Foo {}
            function f() { return new \Foo(); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn short_circuit_and_narrows_its_right_operand_and_the_enclosing_if_body() {
        // A real, common AST-visitor idiom (found via a false positive on
        // this project's own tools/ directory once undefined-property
        // detection actually exercised it): narrow a property chain via
        // `instanceof`, then read a property only the narrowed-to
        // subtype has - both within the same `&&` (needs check_expr's
        // short-circuit-aware traversal) and in the enclosing `if`'s body
        // (needs apply_narrowing to compose `&&`'s operands, not just
        // handle a single top-level condition shape).
        let and_operand = diagnostics_for(
            r#"<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                return $arg->value instanceof Variable && $arg->value->name;
            }
        "#,
        );
        assert!(and_operand.is_empty(), "unexpected: {and_operand:?}");

        let if_body = diagnostics_for(
            r#"<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                if ($arg->value instanceof Variable) {
                    echo $arg->value->name;
                    echo $arg->value->totallyMadeUp;
                }
            }
        "#,
        );
        assert!(
            if_body
                .iter()
                .any(|x| x.message.contains("Variable::$totallyMadeUp")),
            "expected the genuinely-undefined property to still be caught: {if_body:?}"
        );
    }

    #[test]
    fn or_short_circuit_narrows_its_right_operand_on_the_false_branch() {
        // `||`'s right side only runs when the left is false - the mirror
        // image of `&&`'s "both true" composition.
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x) {
                return $x === null || strlen($x) > 0;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn instanceof_self_narrows_to_the_real_enclosing_class_not_a_literal_self() {
        // Real, common pattern (amphp/amp's Future::iterate does exactly
        // this: `if ($future instanceof self)`) - apply_narrowing's
        // instanceof handling used to treat "self"/"parent"/"static" as
        // ordinary bare class names to resolve via name_ctx.resolve
        // directly, instead of through resolve_pseudo against the
        // enclosing class the way every other self/parent/static site
        // already does. In a namespaced file this narrowed to a
        // nonsensical class name ("App\self") instead of the real one
        // ("App\Box") - which would have looked like *no* diagnostics at
        // all below (a bogus, not-in-the-registry class silently
        // suppresses the undefined-method check), not a wrong one, so
        // this asserts a diagnostic *does* appear, to actually
        // distinguish correct narrowing from the old silent failure.
        let d = diagnostics_for(
            r#"<?php
            namespace App;
            class Box {
                public function useSame($other): string {
                    if ($other instanceof self) {
                        return $other->totallyMadeUp();
                    }
                    return "";
                }
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("App\\Box::totallyMadeUp")),
            "expected narrowing to resolve to the real class App\\Box, got: {d:?}"
        );
    }

    #[test]
    fn negated_instanceof_guard_clause_narrows_past_the_if() {
        // `if (! $x instanceof Y) { throw ...; }` must narrow $x to Y for
        // the code after the guard - depends on `!` correctly binding
        // looser than `instanceof` at the parser level (a real bug this
        // session found and fixed: it used to parse as `(!$x) instanceof
        // Y`, which silently narrowed nothing).
        let d = diagnostics_for(
            r#"<?php
            interface Base {}
            interface Specific extends Base {
                public function specificMethod(): int;
            }
            function make(): Base { return new class implements Specific {
                public function specificMethod(): int { return 1; }
            }; }
            function f() {
                $x = make();
                if (! $x instanceof Specific) {
                    throw new \Exception('nope');
                }
                return $x->specificMethod();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn new_self_and_static_are_never_flagged_as_undefined() {
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                public static function make(): self { return new self(); }
                public static function makeStatic(): static { return new static(); }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn constructor_argument_count_is_checked() {
        let d = diagnostics_for(
            r#"<?php
            class Point {
                public function __construct(int $x, int $y) {}
            }
            function f() { return new Point(1); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("Point::__construct() expects at least 2")),
            "expected a constructor argument-count diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn constructor_argument_type_is_checked() {
        let d = diagnostics_for(
            r#"<?php
            class Point {
                public function __construct(int $x) {}
            }
            function f() { return new Point("not an int"); }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("Point::__construct()")
                    && x.message.contains("expects int")),
            "expected a constructor argument-type diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn calling_an_undefined_function_is_flagged() {
        let d = diagnostics_for("<?php totallyMadeUpFunction();");
        assert!(
            d.iter().any(|x| x
                .message
                .contains("undefined function totallyMadeUpFunction")),
            "expected an undefined-function diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn calling_a_known_function_is_not_flagged() {
        let d = diagnostics_for("<?php function greet() {} greet();");
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn isset_empty_unset_eval_exit_die_are_never_flagged_as_undefined() {
        let d = diagnostics_for(
            r#"<?php
            $x = 1;
            isset($x);
            empty($x);
            unset($x);
            if (false) { exit(); }
            if (false) { die(); }
        "#,
        );
        assert!(
            !d.iter().any(|x| x.message.contains("undefined function")),
            "unexpected undefined-function diagnostic: {d:?}"
        );
    }

    #[test]
    fn require_include_and_print_are_never_flagged_as_undefined() {
        // These parse as a synthetic ExprKind::Call (see parser.rs's own
        // comment on that choice) - real language constructs, not
        // functions, so they must be exempted the same way isset/empty
        // are, or every real codebase would light up on every include.
        let d = diagnostics_for(
            r#"<?php
            require 'a.php';
            require_once 'b.php';
            include 'c.php';
            include_once 'd.php';
            print 'hello';
        "#,
        );
        assert!(
            !d.iter().any(|x| x.message.contains("undefined function")),
            "unexpected undefined-function diagnostic: {d:?}"
        );
    }

    #[test]
    fn calling_a_function_via_its_use_imported_short_name_resolves_before_lookup() {
        // Mirrors the analogous `new` test: what's under test is that
        // `use function` resolution runs (format_date ->
        // App\Helpers\format_date) before the registry lookup, not that
        // the target actually exists (it isn't declared in this snippet,
        // so a real project supplying the real declaration is what makes
        // this resolve cleanly - verified separately against Piwigo/vendor).
        let d = diagnostics_for(
            r#"<?php
            use function App\Helpers\format_date;
            function f() { return format_date(); }
        "#,
        );
        assert!(
            d.iter().any(|x| x
                .message
                .contains("undefined function App\\Helpers\\format_date")),
            "expected the fully-qualified name in the diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn calling_a_function_via_a_leading_backslash_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function greet() {}
            function f() { return \greet(); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn conditionally_declared_function_is_visible_to_undefined_function_checks() {
        // The real, common `if (!function_exists('foo')) { function foo()
        // {} }` polyfill idiom (symfony/deprecation-contracts and every
        // symfony/polyfill-* package use exactly this) - Registry::index
        // recurses into if-bodies specifically so this isn't flagged.
        let d = diagnostics_for(
            r#"<?php
            if (!function_exists('polyfilled')) {
                function polyfilled() {}
            }
            function f() { return polyfilled(); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn constructor_call_with_correct_arguments_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Point {
                public function __construct(int $x, int $y) {}
            }
            function f() { return new Point(1, 2); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn class_level_template_substitutes_a_method_return_type() {
        // Collection<User>::first(): T resolves to User, not an unbound T
        // (which would otherwise read as Mixed and never mismatch
        // anything) - proven here by returning it from a function
        // declared to return `int`, which only fails if the inferred type
        // really is User. $c's own generic type comes from a `@param`
        // annotation - this checker has no notion of an inline `@var`
        // annotation on an arbitrary local-variable assignment, only on
        // an actual property/param/return declaration.
        let d = diagnostics_for(
            r#"<?php
            class User {}
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            /**
             * @param Collection<User> $c
             * @return int
             */
            function f($c): int {
                return $c->first();
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("return type mismatch") && x.message.contains("User")),
            "expected a return-type mismatch naming User, got: {d:?}"
        );
    }

    #[test]
    fn class_level_template_substitutes_a_property_type() {
        let d = diagnostics_for(
            r#"<?php
            class User {}
            /**
             * @template T
             */
            class Box {
                /** @var T */
                public $value;
            }
            /**
             * @param Box<User> $b
             * @return int
             */
            function f($b): int {
                return $b->value;
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("return type mismatch") && x.message.contains("User")),
            "expected a return-type mismatch naming User, got: {d:?}"
        );
    }

    #[test]
    fn class_level_template_substitutes_a_method_argument_type() {
        let d = diagnostics_for(
            r#"<?php
            class User {}
            class Post {}
            /**
             * @template T
             */
            class Collection {
                /** @param T $item */
                public function add($item): void {}
            }
            /** @param Collection<User> $c */
            function f($c) {
                $c->add(new Post());
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("expects User")),
            "expected an argument-type mismatch naming User, got: {d:?}"
        );
    }

    #[test]
    fn class_level_template_argument_matching_the_bound_type_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class User {}
            /**
             * @template T
             */
            class Collection {
                /** @param T $item */
                public function add($item): void {}
            }
            /** @param Collection<User> $c */
            function f($c) {
                $c->add(new User());
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn bare_reference_to_a_generic_class_treats_template_as_mixed() {
        // No `<...>` at all - an "erased"/unspecified generic - so T stays
        // unbound and reads as Mixed, matching this checker's general
        // "don't know, don't flag" policy rather than guessing.
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            /** @return int */
            function f(): int {
                $c = new Collection();
                return $c->first();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn extends_tag_fixes_a_non_generic_subclass_s_inherited_template_param() {
        // IntCollection isn't generic itself, but its own `@extends
        // Collection<int>` permanently binds Collection's own T to int
        // (Phase 10.5) - proven the same way the class-level tests above
        // do, via a return-type mismatch that only fires if T really did
        // resolve to int rather than staying unbound (Mixed).
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            /**
             * @param IntCollection $c
             * @return string
             */
            function f($c): string {
                return $c->first();
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("return type mismatch") && x.message.contains("int")),
            "expected a return-type mismatch naming int, got: {d:?}"
        );
    }

    #[test]
    fn extends_tag_argument_matching_the_fixed_type_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @param T $item */
                public function add($item): void {}
            }
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            function f(IntCollection $c) {
                $c->add(1);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn extends_tag_argument_of_the_wrong_type_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @param T $item */
                public function add($item): void {}
            }
            /** @extends Collection<int> */
            class IntCollection extends Collection {}
            function f(IntCollection $c) {
                $c->add("not an int");
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("expects int")),
            "expected an argument-type mismatch naming int, got: {d:?}"
        );
    }

    #[test]
    fn extends_tag_passes_a_generic_subclass_s_own_template_through() {
        // Box<T> extends Collection<T> - Box doesn't fix T to a concrete
        // type, it just forwards its own T - so Box<User>'s inherited
        // first() should resolve to User, not stay unbound.
        let d = diagnostics_for(
            r#"<?php
            class User {}
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            /**
             * @template T
             * @extends Collection<T>
             */
            class Box extends Collection {}
            /**
             * @param Box<User> $b
             * @return int
             */
            function f($b): int {
                return $b->first();
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("return type mismatch") && x.message.contains("User")),
            "expected a return-type mismatch naming User, got: {d:?}"
        );
    }

    #[test]
    fn implements_tag_fixes_an_interface_s_own_template_param() {
        let d = diagnostics_for(
            r#"<?php
            class User {}
            class Post {}
            /**
             * @template T
             */
            interface Comparator {
                /** @param T $other */
                public function compareTo($other): int;
            }
            // Deliberately doesn't redeclare compareTo - this checker
            // doesn't enforce interface completeness (a separate,
            // untracked gap), and the point here is exercising
            // lookup_method's fall-through to the interface's own
            // declaration, not a concrete override's.
            /** @implements Comparator<User> */
            class UserComparator implements Comparator {}
            function f(UserComparator $c) {
                $c->compareTo(new Post());
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("expects User")),
            "expected an argument-type mismatch naming User, got: {d:?}"
        );
    }

    #[test]
    fn a_subclass_with_no_extends_tag_leaves_the_inherited_template_unbound() {
        // Plain native `extends` with no `@extends` generic-fixing tag at
        // all - T should stay unbound (Mixed), the same conservative
        // fallback as a bare `new Collection()` reference, not an
        // accidental false positive from treating "extends something
        // generic" alone as enough to bind anything.
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T
             */
            class Collection {
                /** @return T */
                public function first() { return null; }
            }
            class PlainCollection extends Collection {}
            /** @return string */
            function f(PlainCollection $c): string {
                return $c->first();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_subclass_s_own_template_name_is_not_shadowed_by_an_unrelated_ancestor_s_same_name() {
        // Real bug this reproduces (found verifying Phase 10.5 against
        // symfony/type-info's real NullableType<T of Type> extends
        // UnionType<T|BuiltinType<...>>, where UnionType has its own,
        // unrelated T): binding a whole flat map across the entire
        // extends chain lets an ancestor's own template name silently
        // overwrite an unrelated same-named one further down the chain,
        // since `T` is by far the most common single-letter generic name
        // in real PHPStan code and nothing stops two unrelated classes
        // from both using it. Child's own `ownValue()` must resolve
        // Child's own `T` (bound to Money at the call site) - not
        // ParentGeneric's unrelated `T` (fixed to Whatever via Child's
        // own `@extends` tag).
        let d = diagnostics_for(
            r#"<?php
            class Money {}
            class Whatever {}
            /**
             * @template T
             */
            class ParentGeneric {
                /** @return T */
                public function parentValue() { return null; }
            }
            /**
             * @template T
             * @extends ParentGeneric<Whatever>
             */
            class Child extends ParentGeneric {
                /** @param T $value */
                public function __construct(private $value) {}
                /** @return T */
                public function ownValue() { return $this->value; }
            }
            /**
             * @param Child<Money> $c
             * @return string
             */
            function f($c): string {
                return $c->ownValue();
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("return type mismatch")
                && x.message.contains("Money")
                && !x.message.contains("Whatever")),
            "expected a return-type mismatch naming Money (not Whatever), got: {d:?}"
        );
    }

    #[test]
    fn a_bounded_template_flags_a_param_type_that_violates_the_bound() {
        let d = diagnostics_for(
            r#"<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @param Collection<NotAShape> $c */
            function f($c) {}
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("@template T requires Shape")
                    && x.message.contains("NotAShape")),
            "expected a bound-violation diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_bounded_template_argument_satisfying_the_bound_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            interface Shape {}
            class Circle implements Shape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @param Collection<Circle> $c */
            function f($c) {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_bare_unspecified_generic_never_triggers_bound_checking() {
        // No `<...>` at all - nothing to check, same "don't know, don't
        // guess" fallback as everywhere else an erased generic shows up.
        let d = diagnostics_for(
            r#"<?php
            interface Shape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            function f(Collection $c) {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_bound_violation_in_a_return_type_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @return Collection<NotAShape> */
            function f() {}
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("@template T requires Shape")),
            "expected a bound-violation diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_bound_violation_in_a_property_type_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            class Holder {
                /** @var Collection<NotAShape> */
                public $items;
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("@template T requires Shape")),
            "expected a bound-violation diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_bound_violation_via_an_extends_tag_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @extends Collection<NotAShape> */
            class Bad extends Collection {}
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("@template T requires Shape")),
            "expected a bound-violation diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_bound_referencing_the_same_class_s_own_earlier_template_resolves_positionally() {
        // @template B of A - checking B's argument against "A" only makes
        // sense once A's own concrete argument (from the same <...> list)
        // is known, not treated as a literal unrelated class named "A".
        let d = diagnostics_for(
            r#"<?php
            interface Shape {}
            class Circle implements Shape {}
            class NotAShape {}
            /**
             * @template A of Shape
             * @template B of A
             */
            class Pair {}
            /** @param Pair<Circle, NotAShape> $p */
            function f($p) {}
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("@template B requires Circle")
                    && x.message.contains("NotAShape")),
            "expected B's bound to resolve to Circle (A's own argument), got: {d:?}"
        );
    }

    #[test]
    fn a_bound_of_object_is_satisfied_by_any_class_the_common_doctrine_repository_shape() {
        // Real bug found verifying bound-enforcement against the real
        // Piwigo corpus: `is_assignable` had never treated the native
        // `object` type hint as a universal supertype of any class
        // instance before this (nothing had compared against it this way
        // until bound-checking started) - `Doctrine\ORM\EntityRepository
        // <T of object>` is an extremely common real-world shape every
        // one of Piwigo's own repository classes uses.
        let d = diagnostics_for(
            r#"<?php
            class ActivityEntity {}
            /**
             * @template T of object
             */
            class EntityRepository {}
            /** @extends EntityRepository<ActivityEntity> */
            class ActivityRepository extends EntityRepository {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_native_object_return_type_accepts_any_class_instance() {
        // The narrower, non-generics-related form of the same bug -
        // caught investigating the Doctrine case above, not hypothetical.
        let d = diagnostics_for(
            r#"<?php
            class Foo {}
            function f(): object { return new Foo(); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_bound_argument_that_s_an_unregistered_class_is_never_flagged() {
        // Real bug found on the real Piwigo corpus: real phpstan/phpstan
        // ships as a compiled PHAR (no loose source files at all), so
        // `PHPStan\Type\ArrayType` (a real class implementing the real
        // `PHPStan\Type\Type` interface) is never actually registered -
        // this project has no way to verify that relationship, so it
        // shouldn't guess a violation. Uses `Bound` itself as the
        // stand-in for a class this project simply never saw declared.
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T of Bound
             */
            class Collection {}
            /** @param Collection<UnregisteredArg> $c */
            function f($c) {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_covariant_template_used_only_in_return_position_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template-covariant T
             */
            class Producer {
                /** @return T */
                public function produce() { return null; }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_covariant_template_used_in_a_parameter_position_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param T $x */
                public function consume($x): void {}
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("@template-covariant T")
                && x.message.contains("input (parameter) position")),
            "expected a covariance-violation diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_contravariant_template_used_only_in_parameter_position_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template-contravariant T
             */
            class Consumer {
                /** @param T $x */
                public function consume($x): void {}
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_contravariant_template_used_in_return_position_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template-contravariant T
             */
            class Bad {
                /** @return T */
                public function produce() { return null; }
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("@template-contravariant T")
                    && x.message.contains("output (return) position")),
            "expected a contravariance-violation diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_plain_invariant_template_is_unrestricted_in_either_position() {
        // No `-covariant`/`-contravariant` at all - Phase 10's original,
        // unrestricted behavior must still hold for a plain @template.
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template T
             */
            class Box {
                /** @param T $x */
                public function set($x): void {}
                /** @return T */
                public function get() { return null; }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_covariant_template_nested_in_an_array_parameter_type_is_still_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template-covariant T
             */
            class Bad {
                /** @param T[] $items */
                public function consumeAll($items): void {}
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("@template-covariant T")
                && x.message.contains("input (parameter) position")),
            "expected a covariance-violation diagnostic for the nested T, got: {d:?}"
        );
    }

    #[test]
    fn a_top_level_function_has_no_enclosing_class_variance_to_violate() {
        // Sanity check that check_variance's class_name lookup doesn't
        // panic or misfire for a plain function (no class at all).
        let d = diagnostics_for(
            r#"<?php
            /** @param mixed $x */
            function f($x): void {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_covariant_template_in_a_constructor_parameter_is_exempt() {
        // Real bug found on the real Piwigo corpus: every single one of
        // 27 real-corpus diagnostics this check produced before this
        // exemption was on a constructor - the extremely common readonly
        // value-holder pattern real PHPStan's own variance rule
        // explicitly exempts, since a constructor is only ever called to
        // build one specific concrete instance, never invoked
        // polymorphically through a supertype-typed variable.
        let d = diagnostics_for(
            r#"<?php
            /**
             * @template-covariant T
             */
            final class Success {
                /** @param T $value */
                public function __construct(private $value) {}
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Findings 12/16/18/19 from the architecture audit - infer_binary
    // grouped several operators by superficial similarity rather than by
    // each one's real PHP return-type rule; the Unary handling never
    // rebound `++`/`--`'s own variable, and never modeled numeric-string
    // coercion for unary `-`/`+`.

    #[test]
    fn spaceship_operator_infers_as_int_not_bool() {
        let d = diagnostics_for(
            r#"<?php
            function compare(int $a, int $b): int {
                return $a <=> $b;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let wrong = diagnostics_for(
            r#"<?php
            function compareWrong(int $a, int $b): string {
                return $a <=> $b;
            }
        "#,
        );
        assert_eq!(wrong.len(), 1, "got: {wrong:?}");
    }

    #[test]
    fn modulo_and_bitwise_and_shift_operators_always_infer_as_int() {
        for src in [
            r#"<?php function f(float $a, int $b): int { return $a % $b; }"#,
            r#"<?php function f(float $a, int $b): int { return $a << $b; }"#,
            r#"<?php function f(float $a, int $b): int { return $a >> $b; }"#,
            r#"<?php function f(float $a, int $b): int { return $a & $b; }"#,
            r#"<?php function f(float $a, int $b): int { return $a | $b; }"#,
            r#"<?php function f(float $a, int $b): int { return $a ^ $b; }"#,
        ] {
            let d = diagnostics_for(src);
            assert!(d.is_empty(), "unexpected diagnostics for {src}: {d:?}");
        }
    }

    #[test]
    fn array_plus_array_infers_as_array_not_arithmetic() {
        let d = diagnostics_for(
            r#"<?php
            function merge(array $config, array $defaults): int {
                $merged = $config + $defaults;
                return count($merged);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn increment_on_a_null_variable_rebinds_it_to_int() {
        let d = diagnostics_for(
            r#"<?php
            function counter(): int {
                $x = null;
                $x++;
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn decrement_on_a_null_variable_leaves_it_null() {
        let d = diagnostics_for(
            r#"<?php
            function f(): int {
                $x = null;
                $x--;
                return $x;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("got null"), "got: {d:?}");
    }

    #[test]
    fn increment_on_a_bool_variable_leaves_it_a_bool() {
        let d = diagnostics_for(
            r#"<?php
            function f(): bool {
                $x = true;
                $x++;
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn unary_minus_on_a_string_infers_as_int() {
        let d = diagnostics_for(
            r#"<?php
            function negate(string $s): int { return -$s; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Findings 54/56 from the architecture audit - a bare truthiness
    // check (`if ($x)` / `if (!$x)`) and `empty($x)` both applied no
    // narrowing at all.

    #[test]
    fn negated_truthy_guard_narrows_past_the_if() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                if (!$x) {
                    return "empty";
                }
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn positive_truthy_guard_narrows_inside_the_if() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                if ($x) {
                    return $x;
                }
                return "default";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn positive_truthy_guard_does_not_narrow_the_false_branch() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                if ($x) {
                    return "yes";
                }
                return $x;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn empty_guard_narrows_past_the_if() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                if (empty($x)) {
                    return "empty";
                }
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 55 from the architecture audit - ternary/switch/match never
    // applied condition-based narrowing at all, for any condition shape.

    #[test]
    fn ternary_narrows_both_branches() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                return $x === null ? "default" : $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                return $x !== null ? $x : "default";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let wrong = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                return $x === null ? $x : "default";
            }
        "#,
        );
        assert_eq!(wrong.len(), 1, "got: {wrong:?}");
    }

    #[test]
    fn switch_default_narrows_past_a_terminating_case() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                        return "was-null";
                    default:
                        return $x;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let wrong = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                        return $x;
                    default:
                        return "not null";
                }
            }
        "#,
        );
        assert_eq!(wrong.len(), 1, "got: {wrong:?}");
    }

    #[test]
    fn switch_fallthrough_case_does_not_leak_its_own_narrowing_past_a_later_terminating_case() {
        // Regression test for a real bug caught while implementing this
        // fix: an empty `case null: case "": ...` fallthrough group's own
        // optimistic narrowing (subject === null) must not leak past the
        // *second* case (which itself terminates) into `default`.
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                    case "":
                        return "empty-ish";
                    default:
                        return $x;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn match_true_pattern_narrows_each_arm_and_default() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                return match (true) {
                    $x === null => "default",
                    default => $x,
                };
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let wrong = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                return match (true) {
                    $x === null => $x,
                    default => "not null",
                };
            }
        "#,
        );
        assert_eq!(wrong.len(), 1, "got: {wrong:?}");
    }

    #[test]
    fn match_on_a_real_subject_narrows_via_implicit_equality() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): string {
                return match ($x) {
                    null => "default",
                    default => $x,
                };
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 26 from the architecture audit - `??`/`??=`'s left side was
    // checked for "undefined variable"/"undefined property" like an
    // ordinary read, even though both are specifically designed to be
    // warning-free for a variable/property that isn't set at all.

    #[test]
    fn coalesce_does_not_flag_a_conditionally_assigned_variable_as_undefined() {
        let d = diagnostics_for(
            r#"<?php
            function f(bool $c): string {
                if ($c) { $result = "computed"; }
                return $result ?? "default";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn coalesce_does_not_flag_an_undefined_array_key_root_variable() {
        let d = diagnostics_for(
            r#"<?php
            function f(): string {
                return $config['key'] ?? "default";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn coalesce_does_not_flag_an_undeclared_instance_property() {
        let d = diagnostics_for(
            r#"<?php
            class Foo {}
            function f(Foo $x): string { return $x->missing ?? "default"; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn coalesce_still_flags_an_undeclared_static_property() {
        // isset()/?? silence a suppressible *notice* - an undeclared
        // *static* property is a real, non-suppressible PHP fatal error,
        // and must still be flagged under `??`.
        let d = diagnostics_for(
            r#"<?php
            class Bar {}
            function f(): string { return Bar::$missing ?? "default"; }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn coalesce_assign_does_not_flag_its_own_left_side_as_undefined() {
        let d = diagnostics_for(
            r#"<?php
            function f(): string {
                $x ??= "default";
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 17 from the architecture audit - loose equality (`==`) was
    // narrowed exactly like strict equality (`===`), discarding every
    // other value PHP's own type-juggling rules also match.

    #[test]
    fn loose_equality_does_not_narrow_to_the_literal_compared_to_value() {
        // `$x == 0` also matches the real bool `false` (PHP's own
        // `false == 0` rule) - narrowing exactly to the literal `0`
        // would incorrectly discard that possibility. This project's own
        // separate, deliberate choice to analyze every file as if
        // strict_types were always active means passing the
        // *unnarrowed* `bool|int` union to a `bool`-only parameter can
        // still legitimately be flagged on its own terms - what this
        // test actually guards against is over-narrowing to the wrong,
        // overly-specific literal value.
        let d = diagnostics_for(
            r#"<?php
            function g(bool $flag): void {}
            function f(bool|int $x): void {
                if ($x == 0) {
                    g($x);
                }
            }
        "#,
        );
        assert!(
            !d.iter()
                .any(|x| x.message.contains("'0' given") || x.message.contains(", 0 given")),
            "still over-narrowing to the literal compared-to value: {d:?}"
        );
    }

    #[test]
    fn strict_equality_still_narrows() {
        let d = diagnostics_for(
            r#"<?php
            function h(int $y): void {}
            function f(?int $x): void {
                if ($x === null) {
                    return;
                }
                h($x);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Findings 52/53 from the architecture audit - a `while`/`do-while`
    // loop never applied its negated condition as narrowing after exit,
    // and a `for` loop's body was checked as if it always executes
    // exactly once, with no "may run zero times" merge at all.

    #[test]
    fn while_loop_narrows_the_negated_condition_after_exit() {
        let d = diagnostics_for(
            r#"<?php
            function f(): int {
                $x = null;
                while ($x === null) {
                    $x = 5;
                }
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn do_while_loop_narrows_the_negated_condition_after_exit() {
        let d = diagnostics_for(
            r#"<?php
            function f(): int {
                $x = null;
                do {
                    $x = 5;
                } while ($x === null);
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn for_loop_that_can_run_zero_times_does_not_define_a_body_only_variable() {
        let d = diagnostics_for(
            r#"<?php
            function f(): int {
                for ($i = 0; $i < 0; $i++) {
                    $x = 5;
                }
                return $x;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("undefined variable $x"), "got: {d:?}");
    }

    #[test]
    fn for_loop_narrows_the_negated_condition_after_exit() {
        let d = diagnostics_for(
            r#"<?php
            function f(): int {
                for ($x = null; $x === null;) {
                    $x = 5;
                }
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 31 from the architecture audit - a bare `return;`, and
    // falling off the end of the function body with no `return` at all,
    // were never checked against a non-nullable, non-void declared
    // return type.

    #[test]
    fn bare_return_is_flagged_against_a_non_nullable_return_type() {
        let d = diagnostics_for(
            r#"<?php
            function f(bool $c): int {
                if ($c) {
                    return;
                }
                return 5;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("got null"), "got: {d:?}");
    }

    #[test]
    fn bare_return_is_not_flagged_against_void_or_nullable() {
        let d = diagnostics_for(
            r#"<?php
            function f(bool $c): void {
                if ($c) {
                    return;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

        let d = diagnostics_for(
            r#"<?php
            function f(bool $c): ?int {
                if ($c) {
                    return;
                }
                return 5;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn falling_off_the_end_is_flagged_against_a_non_nullable_return_type() {
        let d = diagnostics_for(
            r#"<?php
            function f(): int { echo "hello"; }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("got null"), "got: {d:?}");
    }

    #[test]
    fn a_function_whose_every_path_returns_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f(bool $c): int {
                if ($c) {
                    return 1;
                } else {
                    return 2;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_function_ending_in_an_exhaustive_switch_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f(int $n): string {
                switch ($n) {
                    case 1:
                        return "one";
                    default:
                        return "other";
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_function_ending_in_a_switch_with_no_default_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f(int $n): string {
                switch ($n) {
                    case 1:
                        return "one";
                }
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn switch_cases_ending_in_break_still_propagate_their_definitions_past_the_switch() {
        // Regression test for a real bug caught while implementing this
        // fix: the Finding 55 rewrite of Switch's own narrowing stopped
        // tracking what a `break`-terminated case (the most common real
        // switch style) leaves defined for the code after the switch.
        let d = diagnostics_for(
            r#"<?php
            function f(int $x): int {
                switch ($x) {
                    case 1:
                        $y = 1;
                        break;
                    default:
                        $y = 2;
                        break;
                }
                return $y;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_function_ending_in_a_try_catch_that_both_return_is_not_flagged() {
        // Regression test for a real bug caught while implementing this
        // fix: `always_terminates` had no case for `Try` at all, so a
        // function ending in `try { return ...; } catch (...) { return
        // ...; }` was incorrectly flagged as possibly falling off the
        // end.
        let d = diagnostics_for(
            r#"<?php
            function f(bool $c): string {
                try {
                    if ($c) {
                        throw new \Exception("bad");
                    }
                    return "ok";
                } catch (\Throwable $e) {
                    return "error";
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 37 from the architecture audit - array destructuring bound
    // nothing in a `foreach` value position, and a nested destructuring
    // pattern bound nothing even in a plain assignment; checking the
    // left-hand side itself had the identical one-level-only gap.

    #[test]
    fn foreach_value_destructuring_binds_its_variables() {
        let d = diagnostics_for(
            r#"<?php
            function f(): void {
                $pairs = [[1, 'a'], [2, 'b']];
                foreach ($pairs as [$num, $letter]) {
                    echo $num . $letter;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn foreach_value_list_syntax_destructuring_binds_its_variables() {
        let d = diagnostics_for(
            r#"<?php
            function f(): void {
                $pairs = [[1, 'a'], [2, 'b']];
                foreach ($pairs as list($num, $letter)) {
                    echo $num . $letter;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn nested_destructuring_assignment_binds_every_level() {
        let d = diagnostics_for(
            r#"<?php
            function f(): void {
                $nested = [[1, 2], 3];
                [[$a, $b], $c] = $nested;
                echo $a . $b . $c;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn nested_destructuring_does_not_check_its_own_targets_as_reads() {
        // Regression test for a second, related bug found while fixing
        // this one: checking the assignment's own left-hand side (not
        // just binding it) only recursed one level into a destructuring
        // pattern, so `$a`/`$b` were flagged as undefined on the
        // destructuring statement itself, before binding ever ran.
        let d = diagnostics_for(
            r#"<?php
            function f(): void {
                $nested = [[1, 2], 3];
                [[$a, $b], $c] = $nested;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn swap_idiom_still_leaves_a_genuinely_undefined_variable_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f(): void {
                [$a, $b] = [1, 2];
                echo $c;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("undefined variable $c"), "got: {d:?}");
    }
    // Findings 21/22 from the architecture audit - a plain int wasn't
    // accepted wherever a float was declared, and a native type hint
    // defaulted to null wasn't treated as implicitly nullable.

    #[test]
    fn int_widens_to_float_in_arguments_returns_and_property_assignment() {
        let d = diagnostics_for(
            r#"<?php
            function distance(float $x, float $y): float { return $x + $y; }
            function test(): float { return distance(3, 4); }
            class Config { public float $rate = 0; }
            function test2(Config $c): void { $c->rate = 5; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_genuinely_wrong_float_argument_is_still_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function needsFloat(float $x): void {}
            function test(): void { needsFloat("not a number"); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn a_native_type_hint_defaulted_to_null_is_implicitly_nullable() {
        let d = diagnostics_for(
            r#"<?php
            function f(int $x = null): void {}
            function test(): void { f(null); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_promoted_constructor_property_defaulted_to_null_is_implicitly_nullable() {
        let d = diagnostics_for(
            r#"<?php
            class Widget {
                public function __construct(public int $size = null) {}
            }
            function test(): void { new Widget(null); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 33 from the architecture audit - `iterable` was parsed as
    // plain `array`, silently dropping its `|Traversable` half.

    #[test]
    fn iterable_accepts_a_real_traversable_not_just_an_array() {
        let d = diagnostics_for(
            r#"<?php
            function f(iterable $items): void {
                foreach ($items as $item) { echo $item; }
            }
            function test(): void {
                f(new ArrayIterator([1, 2, 3]));
                f([1, 2, 3]);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn iterable_still_rejects_a_genuinely_non_iterable_value() {
        let d = diagnostics_for(
            r#"<?php
            function f(iterable $items): void {}
            function test(): void { f("not iterable"); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn iterable_traversable_half_is_not_namespace_mangled() {
        // Regression test for a real bug caught while implementing this
        // fix: the synthesized `Traversable` half of `iterable` is this
        // project's own internal desugaring, not something the
        // programmer wrote as a class reference - it must stay global
        // regardless of the current namespace, the same way the native
        // `object` hint already does.
        let d = diagnostics_for(
            r#"<?php
            namespace App;
            function f(iterable $items): void {
                foreach ($items as $item) { echo $item; }
            }
            function test(): void { f(new \ArrayIterator([1, 2, 3])); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 34 from the architecture audit - array-key/scalar/numeric/
    // resource pseudo-types weren't recognized, silently falling through
    // to a bogus literal class reference.

    #[test]
    fn array_key_accepts_both_string_and_int() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array-key $key */
            function f($key): void {}
            function test(): void { f("hello"); f(42); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_key_still_rejects_a_genuinely_wrong_type() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array-key $key */
            function f($key): void {}
            function test(): void { f([1, 2]); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn resource_pseudo_type_matches_across_namespaces() {
        let d = diagnostics_for(
            r#"<?php
            namespace App;
            /** @param resource $handle */
            function useHandle($handle): void {}

            namespace {
                /** @param resource $h */
                function passthrough($h): void { \App\useHandle($h); }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 35 from the architecture audit - none of PHP's real
    // "callable" shapes other than a literal Closure satisfied the
    // `callable` type hint.

    #[test]
    fn callable_accepts_an_invokable_object() {
        let d = diagnostics_for(
            r#"<?php
            class Multiplier { public function __invoke(int $x): int { return $x * 2; } }
            function apply(callable $fn, int $x): int { return $fn($x); }
            function test(): int { return apply(new Multiplier(), 5); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn callable_accepts_a_string_function_name() {
        let d = diagnostics_for(
            r#"<?php
            function apply(callable $fn, string $s): int { return $fn($s); }
            function test(): int { return apply('strlen', "hello"); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn callable_accepts_a_two_element_method_array() {
        let d = diagnostics_for(
            r#"<?php
            class Handler { public function handle(int $x): int { return $x; } }
            function apply(callable $fn, int $x): int { return $fn($x); }
            function test(Handler $h): int { return apply([$h, 'handle'], 5); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn callable_still_rejects_a_non_invokable_object() {
        let d = diagnostics_for(
            r#"<?php
            class NotInvokable {}
            function apply(callable $fn): void {}
            function test(NotInvokable $n): void { apply($n); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn closure_type_hint_is_distinct_from_callable_and_still_rejects_a_string() {
        // Regression-guard for the fix's own caveat: applying callable's
        // new leniency to an undifferentiated Type::Callable would make
        // a strictly Closure-typed parameter wrongly start accepting a
        // string/array/invokable-object too.
        let d = diagnostics_for(
            r#"<?php
            function needsClosure(Closure $fn): void {}
            function test(): void { needsClosure('strlen'); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn a_real_closure_arrow_fn_and_first_class_callable_all_satisfy_the_closure_type_hint() {
        // Regression test for a real bug caught while implementing this
        // fix: splitting `closure`/`callable` into distinct types left
        // infer_expr still inferring a closure/arrow-fn literal (and
        // first-class-callable syntax) as the old, undifferentiated
        // Type::Callable, so a genuine closure literal stopped
        // satisfying the now-stricter native `Closure` type hint.
        let d = diagnostics_for(
            r#"<?php
            function needsClosure(Closure $fn): void {}
            function test(): void {
                needsClosure(fn($x) => $x + 1);
                needsClosure(function ($x) { return $x + 1; });
                needsClosure(strlen(...));
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 36 from the architecture audit - PHP's implicit Stringable
    // conformance for any class declaring __toString() wasn't modeled.

    #[test]
    fn a_class_declaring_to_string_implicitly_implements_stringable() {
        let d = diagnostics_for(
            r#"<?php
            class Money { public function __toString(): string { return "money"; } }
            function display(string|Stringable $x): void {}
            function test(Money $m): void { display($m); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_class_without_to_string_still_does_not_satisfy_stringable() {
        let d = diagnostics_for(
            r#"<?php
            class NoToString {}
            function display(string|Stringable $x): void {}
            function test(NoToString $n): void { display($n); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }
    // Finding 41 from the architecture audit (integer-literal half only -
    // the float-literal half needs a new Type::FloatLiteral variant,
    // deferred as lower priority per the plan) - PHPStan-style
    // literal-integer union types (`1|2|3`) rejected every value,
    // including genuinely valid ones, because a bare integer in a type
    // string fell through to a bogus literal-class-reference fallback.

    #[test]
    fn literal_integer_union_type_accepts_a_genuinely_valid_value() {
        let d = diagnostics_for(
            r#"<?php
            /** @param 1|2|3 $level */
            function f($level): void {}
            function test(): void { f(2); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn literal_integer_union_type_still_rejects_a_genuinely_invalid_value() {
        let d = diagnostics_for(
            r#"<?php
            /** @param 1|2|3 $level */
            function f($level): void {}
            function test(): void { f(5); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }
    // Finding 27 from the architecture audit - a multi-type catch clause
    // bound only its first listed type, and MethodCall/PropertyAccess
    // never matched a Union/Intersection receiver at all, silently
    // skipping undefined-method/property checking for either.

    #[test]
    fn multi_catch_binds_the_union_of_every_listed_type_regardless_of_order() {
        let first_order = diagnostics_for(
            r#"<?php
            class LogicEx extends \LogicException {}
            class SpecialEx extends \Exception {
                public function getSpecialData(): string { return "data"; }
            }
            function f(): void {
                try { throw new SpecialEx(); }
                catch (LogicEx|SpecialEx $e) { echo $e->getSpecialData(); }
            }
        "#,
        );
        let second_order = diagnostics_for(
            r#"<?php
            class LogicEx extends \LogicException {}
            class SpecialEx extends \Exception {
                public function getSpecialData(): string { return "data"; }
            }
            function f(): void {
                try { throw new SpecialEx(); }
                catch (SpecialEx|LogicEx $e) { echo $e->getSpecialData(); }
            }
        "#,
        );
        // Both orderings describe the same real ambiguity ($e could be
        // LogicEx, which lacks getSpecialData()) - the verdict must not
        // depend on which type happens to be written first.
        assert_eq!(first_order.len(), 1, "got: {first_order:?}");
        assert_eq!(second_order.len(), 1, "got: {second_order:?}");
    }

    #[test]
    fn union_typed_receiver_requires_every_member_to_have_the_method() {
        let d = diagnostics_for(
            r#"<?php
            class A { public function foo(): void {} }
            class B {}
            function f(A|B $x): void { $x->foo(); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("undefined method"), "got: {d:?}");
    }

    #[test]
    fn union_typed_receiver_requires_every_member_to_have_the_property() {
        let d = diagnostics_for(
            r#"<?php
            class A { public int $x = 0; }
            class B {}
            function f(A|B $obj): int { return $obj->x; }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("undefined property"), "got: {d:?}");
    }

    #[test]
    fn intersection_typed_receiver_accepts_a_method_found_on_any_member() {
        let d = diagnostics_for(
            r#"<?php
            interface Countable2 { public function count2(): int; }
            interface Nameable { public function name(): string; }
            function f(Countable2&Nameable $x): void {
                echo $x->count2();
                echo $x->name();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn intersection_typed_receiver_still_rejects_a_method_missing_from_every_member() {
        let d = diagnostics_for(
            r#"<?php
            interface Countable2 { public function count2(): int; }
            interface Nameable { public function name(): string; }
            function f(Countable2&Nameable $x): void { $x->totallyMissingMethod(); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }
    // Finding 14 from the architecture audit - a PHPDoc type completely
    // replaced the native hint, silently dropping nullability the native
    // signature already stated.

    #[test]
    fn docblock_type_merges_the_native_hints_nullability_when_not_repeated() {
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                /** @param int $x */
                public function bar(?int $x = null): void {}
            }
            function test(Foo $f): void { $f->bar(null); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn docblock_type_still_rejects_a_genuinely_wrong_type() {
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                /** @param int $x */
                public function bar(?int $x = null): void {}
            }
            function test(Foo $f): void { $f->bar("not an int"); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }
    // Finding 23 from the architecture audit - a variadic parameter was
    // bound inside its own function/closure/arrow-fn body as the element
    // type, not an array of it, in three independent binding sites.

    #[test]
    fn variadic_parameter_is_an_array_inside_a_top_level_function_body() {
        let d = diagnostics_for(
            r#"<?php
            function f(int ...$nums): int { return count($nums); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn variadic_parameter_is_an_array_inside_a_closure_body() {
        let d = diagnostics_for(
            r#"<?php
            function test(): void {
                $f = function (int ...$nums): int { return count($nums); };
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn variadic_parameter_is_an_array_inside_an_arrow_fn_body() {
        let d = diagnostics_for(
            r#"<?php
            function test(): void {
                $g = fn(int ...$nums): int => count($nums);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 25 from the architecture audit - a self/static/parent-typed
    // parameter was never resolved, silently disabling undefined-
    // property/method checking through it (unlike the identical access
    // through $this).

    #[test]
    fn self_typed_parameter_gets_undefined_property_checking() {
        let d = diagnostics_for(
            r#"<?php
            class Point {
                public int $id = 0;
                public function equals(self $other): bool {
                    return $this->id === $other->missingProp;
                }
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("missingProp"), "got: {d:?}");
    }

    #[test]
    fn self_typed_parameter_accessing_a_real_property_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Point {
                public int $id = 0;
                public function equals(self $other): bool {
                    return $this->id === $other->id;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 15 from the architecture audit - the nullsafe operator
    // (?->) and plain -> produced identical AST nodes, so infer_expr
    // never knew a nullsafe access's result could be null.

    #[test]
    fn nullsafe_method_call_result_is_implicitly_nullable() {
        let d = diagnostics_for(
            r#"<?php
            class Owner { public function getFoo(): Foo { return new Foo(); } }
            class Foo {}
            function needsFoo(Foo $x): void {}
            function test(?Owner $obj): void { needsFoo($obj?->getFoo()); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("Foo|null"), "got: {d:?}");
    }

    #[test]
    fn nullsafe_property_access_result_is_implicitly_nullable() {
        let d = diagnostics_for(
            r#"<?php
            class Owner { public ?Foo $foo = null; }
            class Foo {}
            function needsFoo(Foo $x): void {}
            function test(?Owner $obj): void { needsFoo($obj?->foo); }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn nullsafe_result_accepted_by_a_nullable_parameter_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Owner { public function getFoo(): Foo { return new Foo(); } }
            class Foo {}
            function needsMaybeFoo(?Foo $x): void {}
            function test(?Owner $obj): void { needsMaybeFoo($obj?->getFoo()); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn plain_arrow_on_a_non_nullable_receiver_is_unaffected() {
        let d = diagnostics_for(
            r#"<?php
            class Owner { public function getFoo(): Foo { return new Foo(); } }
            class Foo {}
            function needsFoo(Foo $x): void {}
            function test(Owner $obj): void { needsFoo($obj->getFoo()); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 42 from the architecture audit - destructuring from an
    // array{...} shape didn't extract each position/key's own precise
    // type, falling back to Mixed.

    #[test]
    fn positional_destructuring_extracts_the_shapes_real_per_position_types() {
        let d = diagnostics_for(
            r#"<?php
            /** @return array{0: int, 1: string} */
            function pair(): array { return [1, "a"]; }
            function test(): string {
                [$num, $str] = pair();
                return $num;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(
            d[0].message.contains("expected string, got int"),
            "got: {d:?}"
        );
    }

    #[test]
    fn keyed_destructuring_extracts_the_shapes_real_per_key_types() {
        let d = diagnostics_for(
            r#"<?php
            /** @return array{name: string, age: int} */
            function person(): array { return ["name" => "Al", "age" => 30]; }
            function test(): int {
                ["name" => $name, "age" => $age] = person();
                return $name;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(
            d[0].message.contains("expected int, got string"),
            "got: {d:?}"
        );
    }

    #[test]
    fn correctly_typed_shape_destructuring_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            /** @return array{0: int, 1: string} */
            function pair(): array { return [1, "a"]; }
            function test(): int {
                [$num, $str] = pair();
                return $num;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }
    // Finding 38 from the architecture audit - unset($x) was treated as
    // *defining* $x (a side effect of the same mechanism that makes
    // isset()/empty() safe on a maybe-undefined variable) rather than
    // removing it.

    #[test]
    fn using_a_variable_after_unset_is_flagged_as_undefined() {
        let d = diagnostics_for(
            r#"<?php
            function f(): void {
                $x = 5;
                unset($x);
                echo $x;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("undefined variable $x"), "got: {d:?}");
    }

    #[test]
    fn unset_with_multiple_arguments_removes_every_one() {
        let d = diagnostics_for(
            r#"<?php
            function f(): void {
                $a = 1;
                $b = 2;
                unset($a, $b);
                echo $a;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn isset_and_empty_are_still_unaffected_by_the_unset_fix() {
        let d = diagnostics_for(
            r#"<?php
            function f(bool $c): string {
                if ($c) { $r = "x"; }
                return isset($r) ? $r : "no";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Item 2: a small initial port of real, ground-truth-labeled fixtures
    // from `phpstan/phpstan-src` (MIT-licensed - see `vendor/phpstan-src/LICENSE`),
    // adapted from the specific `tests/PHPStan/Rules/**/data/*.php` scenarios
    // that motivated Group E's own findings above. Each fixture is trimmed to
    // just the class/enum shapes this project's own (simpler) checker models -
    // PHPStan-specific assertions this project doesn't attempt (class-case-
    // sensitivity, named-constructor-argument checks, generic/template
    // variance, ...) are deliberately left out rather than chased for
    // byte-for-byte parity. These replace this session's own hand-written
    // repros with real, independently-authored ground truth for the same
    // gaps, per the audit plan's own Item 2.

    #[test]
    fn assert_type_is_silent_when_the_inferred_type_matches() {
        let d = diagnostics_for(
            r#"<?php
            use function PHPStan\Testing\assertType;
            function test(int $x): void { assertType('int', $x); }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn assert_type_flags_a_real_mismatch() {
        let d = diagnostics_for(
            r#"<?php
            use function PHPStan\Testing\assertType;
            function test(int $x): void { assertType('string', $x); }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(
            d[0].message
                .contains("assertType() mismatch: expected type \"string\", actual type \"int\"")
        );
    }

    #[test]
    fn assert_type_also_recognizes_the_fully_qualified_call_form() {
        let d = diagnostics_for(
            r#"<?php
            function test(int $x): void { \PHPStan\Testing\assertType('string', $x); }
        "#,
        );
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("assertType() mismatch"));
    }

    #[test]
    fn phpstan_instantiation_flags_abstract_class_and_interface() {
        // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Classes/data/instantiation.php
        // (lines ~29-30 of the vendored `2.2.11` tag): `new LoremInstantiation()` /
        // `new IpsumInstantiation()`, an abstract class and an interface.
        let d = diagnostics_for(
            r#"<?php
            abstract class LoremInstantiation {}
            interface IpsumInstantiation {}
            function test(): void {
                new LoremInstantiation();
                new IpsumInstantiation();
            }
        "#,
        );
        assert!(d.iter().any(|x| {
            x.message
                .contains("cannot instantiate abstract class LoremInstantiation")
        }));
        assert!(d.iter().any(|x| {
            x.message
                .contains("cannot instantiate interface IpsumInstantiation")
        }));
    }

    #[test]
    fn phpstan_missing_method_impl_flags_interface_and_abstract_gaps() {
        // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Methods/data/missing-method-impl.php:
        // a concrete class missing both an interface-required method
        // (`doFoo` from `Foo`) and an inherited abstract method (`doBaz`
        // from `Bar`) - upstream's own `Baz` message names both origins.
        let d = diagnostics_for(
            r#"<?php
            interface Foo { public function doFoo(); }
            abstract class Bar implements Foo {
                public function doBar() {}
                abstract public function doBaz();
            }
            class Baz extends Bar {
                public function doBar() {}
            }
        "#,
        );
        assert!(d.iter().any(
            |x| x.message.contains("does not implement abstract method(s):")
                && x.message.contains("doFoo")
                && x.message.contains("doBaz")
        ));
    }

    #[test]
    fn phpstan_overriding_method_flags_a_final_method_override() {
        // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Methods/data/overriding-method.php's
        // `Foo`/`Bar` pair: upstream's own expected message is "Method
        // OverridingFinalMethod\Bar::doFoo() overrides final method
        // OverridingFinalMethod\Foo::doFoo().".
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                final public function doFoo(): void {}
            }
            class Bar extends Foo {
                public function doFoo(): void {}
            }
        "#,
        );
        assert!(d.iter().any(|x| {
            x.message
                .contains("cannot override final method Foo::doFoo")
        }));
    }

    #[test]
    fn phpstan_parameter_contravariance_flags_a_narrowed_override() {
        // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Methods/data/parameter-contravariance.php's
        // `Foo`/`Lorem` pair: upstream flags `Lorem::doFoo(InvalidArgumentException $e)`
        // as "not contravariant with" `Foo::doFoo(Exception $e)` - the same
        // narrowed-parameter shape Finding 50 covers, using real,
        // already-registered Exception-hierarchy classes from the vendored
        // phpstorm-stubs corpus rather than made-up ones.
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                public function doFoo(\Exception $e): void {}
            }
            class Lorem extends Foo {
                public function doFoo(\InvalidArgumentException $e): void {}
            }
        "#,
        );
        assert!(d.iter().any(|x| x.message.contains("Lorem::doFoo")
            && x.message.contains("narrows the inherited parameter type")));
    }

    #[test]
    fn phpstan_readonly_property_assign_flags_writes_outside_the_constructor() {
        // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Properties/data/readonly-assign.php:
        // a setter re-assigning its own class's readonly property ("setter -
        // report"), and a subclass's constructor assigning a readonly
        // property it inherits rather than declares ("not in declaring
        // class - report").
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                public function __construct(public readonly int $x) {}
                public function setX(int $x): void { $this->x = $x; }
            }
            class Bar extends Foo {
                public function __construct(int $x) {
                    parent::__construct($x);
                    $this->x = $x;
                }
            }
        "#,
        );
        let readonly_hits = d
            .iter()
            .filter(|x| x.message.contains("Foo::$x is readonly"))
            .count();
        assert_eq!(readonly_hits, 2, "expected both writes flagged, got: {d:?}");
    }

    #[test]
    fn phpstan_caught_exception_existence_flags_a_non_throwable_catch() {
        // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Exceptions/data/catch.php:
        // upstream's own expected message is "Caught class
        // TestCatch\FooCatch is not an exception."
        let d = diagnostics_for(
            r#"<?php
            class FooCatch {}
            function test(): void {
                try {
                } catch (FooCatch $e) {
                }
            }
        "#,
        );
        assert!(d.iter().any(|x| {
            x.message
                .contains("cannot catch FooCatch: it does not implement Throwable")
        }));
    }

    #[test]
    fn phpstan_throw_expr_type_flags_throwing_a_non_throwable() {
        // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Exceptions/data/throw-values.php's
        // `throw new InvalidException();` case (a plain class with no
        // Throwable in its hierarchy).
        let d = diagnostics_for(
            r#"<?php
            class InvalidException {}
            function test(): void {
                throw new InvalidException();
            }
        "#,
        );
        assert!(d.iter().any(|x| {
            x.message
                .contains("cannot throw InvalidException: it does not implement Throwable")
        }));
    }

    #[test]
    fn phpstan_enum_sanity_flags_duplicate_backing_values() {
        // Derived from vendor/phpstan-src/tests/PHPStan/Rules/Classes/data/enum-sanity.php's
        // `EnumDuplicateValue` case - upstream's own expected messages are
        // "has duplicate value 1 for cases A, E." and "... value 2 for
        // cases B, C.".
        let d = diagnostics_for(
            r#"<?php
            enum EnumDuplicateValue: int {
                case A = 1;
                case B = 2;
                case C = 2;
                case D = 3;
                case E = 1;
            }
        "#,
        );
        let dupes: Vec<&Diagnostic> = d
            .iter()
            .filter(|x| {
                x.message
                    .contains("duplicate value in enum EnumDuplicateValue")
            })
            .collect();
        assert_eq!(
            dupes.len(),
            2,
            "expected 2 duplicate-value diagnostics, got: {d:?}"
        );
        assert!(dupes.iter().any(|x| x.message.contains("for case C")));
        assert!(dupes.iter().any(|x| x.message.contains("for case E")));
    }

    // The `assertType()` special case in `check_expr`'s `Call` handling
    // (see its own doc comment) turns any of phpstan-src's real
    // `tests/PHPStan/Analyser/nsrt/*.php` fixtures into a directly
    // runnable regression test: `diagnostics_for` returning empty means
    // this project's own `infer_expr` matches PHPStan's own labeled
    // ground truth exactly, at that exact call site. These two are
    // copied verbatim (not trimmed) from real, small fixtures whose
    // expected type happens to already line up with this project's own
    // (coarser) `Type` representation - most `assertType` calls in the
    // corpus assert a precision (integer ranges, literal array shapes,
    // ...) this project doesn't attempt, so exact matches are the
    // minority, not the norm; these were found by direct trial against
    // the vendored corpus, not assumed.
    #[test]
    fn phpstan_nsrt_bool_cast_matches_real_assert_type() {
        // Verbatim from vendor/phpstan-src/tests/PHPStan/Analyser/nsrt/bug-2997.php.
        let d = diagnostics_for(
            r#"<?php
            use function PHPStan\Testing\assertType;
            function test(\SimpleXMLElement $xml): void {
                assertType('bool', (bool) $xml->Exists);
            }
        "#,
        );
        assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
    }

    #[test]
    fn phpstan_nsrt_float_increment_decrement_matches_real_assert_type() {
        // Derived from vendor/phpstan-src/tests/PHPStan/Analyser/nsrt/bug-10122.php's
        // `float $f` block - real PHP's `++`/`--` on a float stays a
        // float, unlike Finding 16's own `null`/string special cases.
        let d = diagnostics_for(
            r#"<?php
            use function PHPStan\Testing\assertType;
            function test(float $f): void { assertType('float', ++$f); }
            function test2(float $f): void { assertType('float', --$f); }
            function test3(float $f): void { assertType('float', $f++); }
            function test4(float $f): void { assertType('float', $f--); }
        "#,
        );
        assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
    }
}
