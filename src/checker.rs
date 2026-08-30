use std::collections::HashMap;

use crate::ast::*;
use crate::registry::{FunctionSig, NameContext, ParamSig, Registry};
use crate::types::{self, Type};

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub location: String,
    pub message: String,
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
/// detection on `Expr::MethodCall` is safe again as of this fix.
/// `Expr::PropertyAccess` still doesn't flag undefined properties on
/// read (the existing write-side check already made that call, for
/// dynamic-property reasons - unrelated to namespaces), and
/// `Expr::StaticAccess`/`StaticCall` still have no existence checks at
/// all (unstarted, not blocked).
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
            saw_dynamic_var_construct: false,
        }
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
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
            match stmt {
                Stmt::FunctionDecl(f) => self.check_function(f, None),
                Stmt::ClassDecl(c) => {
                    // `current_class` must be the fully-qualified name -
                    // it's looked up directly against
                    // `self.registry.classes` (via `resolve_pseudo` for
                    // self/parent/static, and the undefined-class check
                    // for `new self()`/`new static()`), which is now
                    // FQN-keyed.
                    let fqn = self.name_ctx.declared_name(&c.name);
                    for m in &c.methods {
                        self.check_function(&m.function, Some(&fqn));
                    }
                }
                Stmt::EnumDecl(e) => {
                    let fqn = self.name_ctx.declared_name(&e.name);
                    for m in &e.methods {
                        self.check_function(&m.function, Some(&fqn));
                    }
                }
                // Top-level script statements share one scope across the
                // whole file, so the extract()/eval() guard spans the
                // entire loop rather than resetting per statement.
                other => self.check_stmt(other, &mut top_locals, None, "<main>"),
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
    fn lookup_own_signature(
        &self,
        f: &FunctionDecl,
        class_name: Option<&str>,
    ) -> Option<&'a FunctionSig> {
        match class_name {
            Some(c) => self.registry.classes.get(c)?.methods.get(&f.name),
            None => self.registry.functions.get(&f.name),
        }
    }

    fn check_function(&mut self, f: &FunctionDecl, class_name: Option<&str>) {
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
                    locals.insert(param.name.clone(), param.ty.clone());
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
                    locals.insert(p.name.clone(), ty);
                }
            }
        }
        if let Some(c) = class_name {
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

        let previous_class = self.current_class.take();
        self.current_class = class_name.map(|s| s.to_string());

        let diagnostics_before = self.diagnostics.len();
        let previous_dynamic_flag = self.saw_dynamic_var_construct;
        self.saw_dynamic_var_construct = false;

        for stmt in &f.body {
            self.check_stmt(stmt, &mut locals, declared_return.as_ref(), &label);
        }

        if self.saw_dynamic_var_construct {
            self.diagnostics.truncate(diagnostics_before);
        }
        self.saw_dynamic_var_construct = previous_dynamic_flag;

        self.current_class = previous_class;
    }

    /// Resolves `self`/`static`/`parent` to a concrete `Object` relative to
    /// `defining_class` - the class whose signature this type came from.
    /// `self`/`static` both collapse to the same class here since we don't
    /// track late static binding through actual runtime dispatch; `parent`
    /// resolves via the registry's extends chain, falling back to leaving
    /// it unresolved if the class has no declared parent (so it still hits
    /// the permissive Object fallback in `is_subtype`/`is_assignable`
    /// rather than being wrongly flagged).
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
        match stmt {
            Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::Expression(e) => self.infer_expr(e, locals) == Type::Never,
            Stmt::Block(stmts) => stmts
                .last()
                .is_some_and(|s| self.always_terminates(s, locals)),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.always_terminates(then_branch, locals)
                    && else_branch
                        .as_deref()
                        .is_some_and(|e| self.always_terminates(e, locals))
            }
            _ => false,
        }
    }

    /// Binds a generic-typed receiver's own carried type arguments
    /// (`Collection<User>` -> `{"T": Object("User", [])}`) against the
    /// class's own declared `@template` parameter names, in order. A
    /// non-generic class (empty `templates`) or a bare, unspecified-generic
    /// reference (empty `type_args`) both just produce an empty binding
    /// map - `substitute_template` already treats an unbound `TemplateParam`
    /// as `Mixed`, which is exactly the right conservative fallback here
    /// too (an "erased" generic shouldn't read as a concrete, checkable
    /// type). Deliberately invariant, call-site-only - no inheritance-chain
    /// substitution (a class using its parent's own template param isn't
    /// resolved through this).
    fn class_template_bindings(&self, class: &str, type_args: &[Type]) -> HashMap<String, Type> {
        match self.registry.classes.get(class) {
            Some(info) => info
                .templates
                .iter()
                .cloned()
                .zip(type_args.iter().cloned())
                .collect(),
            None => HashMap::new(),
        }
    }

    /// Applies `class_template_bindings`' substitution to every param and
    /// the return type of a method signature - the class-level counterpart
    /// to `infer_call_return_type`'s existing per-call-argument template
    /// binding (which only ever sees a method's *own* `@template`, not one
    /// declared on the enclosing class).
    fn substitute_class_templates(
        &self,
        sig: &FunctionSig,
        class: &str,
        type_args: &[Type],
    ) -> FunctionSig {
        let bindings = self.class_template_bindings(class, type_args);
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
        match stmt {
            Stmt::Expression(e) => {
                self.check_expr(e, locals, label);
            }
            Stmt::Return(Some(e)) => {
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
                        });
                    }
                }
            }
            Stmt::Echo(exprs) => {
                for e in exprs {
                    self.check_expr(e, locals, label);
                }
            }
            Stmt::Block(stmts) => self.check_block(stmts, locals, declared_return, label),
            Stmt::If {
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
            Stmt::While { cond, body } => {
                self.check_expr(cond, locals, label);
                let mut body_locals = locals.clone();
                self.apply_narrowing(cond, &mut body_locals, true);
                self.check_stmt(body, &mut body_locals, declared_return, label);
                // The loop may run zero times, so what's true after it is
                // "either the pre-loop state, or whatever one narrowed
                // iteration produces" - not a real fixed point, but sound
                // enough to avoid claiming more precision than we have.
                *locals = merge_scopes(locals, &body_locals);
            }
            Stmt::DoWhile { body, cond } => {
                // The condition is only evaluated after the body has
                // already run once, so there's nothing to narrow going in.
                self.check_stmt(body, locals, declared_return, label);
                self.check_expr(cond, locals, label);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                for e in init.iter().chain(cond).chain(step) {
                    self.check_expr(e, locals, label);
                }
                self.check_stmt(body, locals, declared_return, label);
            }
            Stmt::Foreach {
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
                self.bind_simple_target(value, element_ty, locals);
                self.check_expr(value, locals, label);
                self.check_stmt(body, locals, declared_return, label);
            }
            Stmt::Switch { subject, cases } => {
                self.check_expr(subject, locals, label);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.check_expr(t, locals, label);
                    }
                    self.check_block(&case.body, locals, declared_return, label);
                }
            }
            Stmt::Try {
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
                    let mut catch_locals = locals.clone();
                    if let Some(var) = &c.var {
                        let ty = c
                            .types
                            .first()
                            .map(|t| Type::Object(t.clone(), Vec::new()))
                            .unwrap_or(Type::Mixed);
                        catch_locals.insert(var.clone(), ty);
                    }
                    self.check_block(&c.body, &mut catch_locals, declared_return, label);
                }
                if let Some(f) = finally {
                    self.check_block(f, locals, declared_return, label);
                }
            }
            Stmt::Throw(e) => self.check_expr(e, locals, label),
            // `global $x;` binds $x to whatever the global scope holds,
            // which this checker doesn't track - Mixed is the honest
            // answer, but the important part is that it's bound at all, so
            // using it afterward isn't flagged as undefined.
            Stmt::Global(names) => {
                for name in names {
                    locals.entry(name.clone()).or_insert(Type::Mixed);
                }
            }
            Stmt::StaticVar(vars) => {
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
    fn bind_assignment_target(&self, lhs: &Expr, rhs: &Expr, locals: &mut HashMap<String, Type>) {
        match lhs {
            Expr::Variable(name) => {
                let ty = self.infer_expr(rhs, locals);
                locals.insert(name.clone(), ty);
            }
            // Destructuring: `[$a, $b] = $arr;` and `list($a, $b) = $arr;`
            // both parse as an ArrayLiteral target. Per-key precision
            // (`['name' => $n] = $shapedArray`) isn't resolved - every
            // target gets the source's homogeneous element type, or Mixed
            // for anything else.
            Expr::ArrayLiteral(items) => {
                let element_ty = match self.infer_expr(rhs, locals) {
                    Type::Array(inner) => *inner,
                    _ => Type::Mixed,
                };
                for (_, item) in items {
                    self.bind_simple_target(item, element_ty.clone(), locals);
                }
            }
            _ => {}
        }
    }

    fn bind_simple_target(&self, target: &Expr, ty: Type, locals: &mut HashMap<String, Type>) {
        if let Expr::Variable(name) = target {
            locals.insert(name.clone(), ty);
        }
    }

    /// Narrows `locals` in place based on `cond` being true (`branch_true`)
    /// or false, along one branch of an `if`/`while`. Only handles the
    /// common, safe patterns - equality against a known-type value
    /// (`$x === null`, `null !== $x`), `is_*()` guards, and `instanceof` -
    /// anything else leaves `locals` untouched rather than guess.
    fn apply_narrowing(&self, cond: &Expr, locals: &mut HashMap<String, Type>, branch_true: bool) {
        match cond {
            Expr::Binary(op, l, r) => {
                let is_eq = matches!(op, BinOp::Identical | BinOp::Eq);
                let is_neq = matches!(op, BinOp::NotIdentical | BinOp::NotEq);
                if is_eq || is_neq {
                    let matches_when_true = if is_eq { branch_true } else { !branch_true };
                    self.apply_equality_narrowing(l, r, locals, matches_when_true);
                }
            }
            Expr::Unary(UnaryOp::Not, inner) => {
                self.apply_narrowing(inner, locals, !branch_true);
            }
            Expr::Grouping(inner) => self.apply_narrowing(inner, locals, branch_true),
            Expr::Call(callee, args) => {
                if let (Expr::Name(name), [Expr::Variable(var_name)]) =
                    (callee.as_ref(), args.as_slice())
                    && let Some(target_ty) = narrowing_type_for_predicate(name)
                {
                    let current = locals.get(var_name).cloned().unwrap_or(Type::Mixed);
                    let narrowed = if branch_true {
                        narrow_type_to(&current, &target_ty)
                    } else {
                        narrow_type_excluding(&current, &target_ty)
                    };
                    locals.insert(var_name.clone(), narrowed);
                }
            }
            Expr::Instanceof(inner, target) => {
                // The false branch would need a "not this class" type we
                // don't have a representation for, so it's left alone. A
                // dynamic target (`$x instanceof $className`) doesn't name
                // a concrete class to narrow to either.
                if branch_true
                    && let Expr::Variable(var_name) = inner.as_ref()
                    && let InstanceofTarget::Name(class_name) = target
                {
                    locals.insert(
                        var_name.clone(),
                        Type::Object(self.name_ctx.resolve(class_name), Vec::new()),
                    );
                }
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
        let pair = match (l, r) {
            (Expr::Variable(name), other) => Some((name.clone(), self.infer_expr(other, locals))),
            (other, Expr::Variable(name)) => Some((name.clone(), self.infer_expr(other, locals))),
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

    fn check_expr(&mut self, e: &Expr, locals: &mut HashMap<String, Type>, label: &str) {
        match e {
            Expr::Call(callee, args) => {
                if let Expr::Name(name) = callee.as_ref()
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
                let resolved_name = match callee.as_ref() {
                    Expr::Name(name) if is_language_construct(name) => None,
                    Expr::Name(name) => {
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
                self.predefine_byref_or_unknown_args(sig, args, locals);
                for a in args {
                    self.check_expr(a, locals, label);
                }
                if let Some(name) = &resolved_name {
                    if let Some(sig) = sig {
                        self.check_args(name, sig, args, locals, label, None);
                    } else {
                        // Since resolve_function_candidates always
                        // produces a definite name (there's no "can't
                        // resolve" case left for functions the way
                        // `Expr::New`'s class resolution still has for a
                        // multi-segment name with no matching import),
                        // every miss here is a real undefined-function
                        // finding.
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!("call to undefined function {name}()"),
                        });
                    }
                }
            }
            Expr::MethodCall(recv, method, args) => {
                self.check_expr(recv, locals, label);
                // Re-enabled (Phase 9.5): the registry is now
                // namespace-aware, so a method lookup trusts the specific
                // `ClassInfo` stored under a *fully-qualified* name, not a
                // bare one two unrelated packages could collide on - see
                // the module-level doc comment's "Known limitation" note,
                // now resolved, for the full history.
                let recv_class = match self.infer_expr(recv, locals) {
                    Type::Object(class, type_args) => Some((class, type_args)),
                    _ => None,
                };
                // Substituted through the receiver's own carried type
                // arguments (Phase 10) - a method declared `add(T $item)`
                // on `Collection<User>` gets checked against `User`, not
                // an unbound `T`/`Mixed`.
                let sig = recv_class.as_ref().and_then(|(class, type_args)| {
                    self.registry
                        .lookup_method(class, method)
                        .map(|sig| self.substitute_class_templates(sig, class, type_args))
                });
                self.predefine_byref_or_unknown_args(sig.as_ref(), args, locals);
                for a in args {
                    self.check_expr(a, locals, label);
                }
                if let Some((class, _)) = &recv_class {
                    if let Some(sig) = &sig {
                        let qualified = format!("{class}::{method}");
                        self.check_args(&qualified, sig, args, locals, label, Some(class));
                    } else if self.registry.classes.contains_key(class)
                        && self.registry.lookup_method(class, "__call").is_none()
                    {
                        // A class not known to the registry at all (an
                        // unresolvable/uninstalled dependency, the same
                        // gap `Expr::New`'s check documents) is left
                        // unchecked rather than risking a false positive -
                        // only fires once the class itself is confirmed
                        // known. `__call` anywhere in the chain means any
                        // method name is legitimately callable at
                        // runtime, so its presence suppresses this
                        // entirely, the same way real PHPStan treats it.
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!("call to undefined method {class}::{method}()"),
                        });
                    }
                }
            }
            Expr::Binary(_, l, r) => {
                self.check_expr(l, locals, label);
                self.check_expr(r, locals, label);
            }
            Expr::Variable(name) => {
                if !locals.contains_key(name) && !is_superglobal(name) {
                    self.diagnostics.push(Diagnostic {
                        location: label.to_string(),
                        message: format!("undefined variable ${name}"),
                    });
                }
            }
            Expr::Unary(_, inner)
            | Expr::Grouping(inner)
            | Expr::Cast(_, inner)
            | Expr::Throw(inner)
            | Expr::YieldFrom(inner) => {
                self.check_expr(inner, locals, label);
            }
            Expr::Clone(inner, overrides) => {
                self.check_expr(inner, locals, label);
                if let Some(o) = overrides {
                    self.check_expr(o, locals, label);
                }
            }
            Expr::Yield(key, value) => {
                if let Some(k) = key {
                    self.check_expr(k, locals, label);
                }
                if let Some(v) = value {
                    self.check_expr(v, locals, label);
                }
            }
            Expr::Instanceof(inner, target) => {
                self.check_expr(inner, locals, label);
                if let InstanceofTarget::Expr(e) = target {
                    self.check_expr(e, locals, label);
                }
            }
            Expr::Assign(lhs, rhs) => {
                self.check_expr(rhs, locals, label);
                // A bare variable (or a destructuring target inside one)
                // on the left is a definition site, not a read - checking
                // it here, before it's bound below, would make undefined-
                // variable detection flag every variable on the very
                // statement that defines it. PropertyAccess/Index targets
                // still need their *base* checked (`$obj->prop = ...`
                // reads $obj).
                match lhs.as_ref() {
                    Expr::Variable(_) => {}
                    Expr::ArrayLiteral(items) => {
                        for (_, item) in items {
                            if !matches!(item, Expr::Variable(_)) {
                                self.check_expr(item, locals, label);
                            }
                        }
                    }
                    other => self.check_expr(other, locals, label),
                }
                self.check_property_assignment(lhs, rhs, locals, label);
                self.bind_assignment_target(lhs, rhs, locals);
            }
            Expr::CompoundAssign(op, lhs, rhs) => {
                self.check_expr(lhs, locals, label);
                self.check_expr(rhs, locals, label);
                if let Expr::Variable(name) = lhs.as_ref() {
                    let ty = self.infer_binary(op, lhs, rhs, locals);
                    locals.insert(name.clone(), ty);
                }
            }
            Expr::Ternary(cond, then_, else_) => {
                self.check_expr(cond, locals, label);
                if let Some(t) = then_ {
                    self.check_expr(t, locals, label);
                }
                self.check_expr(else_, locals, label);
            }
            Expr::ArrayLiteral(items) => {
                for (k, v) in items {
                    if let Some(k) = k {
                        self.check_expr(k, locals, label);
                    }
                    self.check_expr(v, locals, label);
                }
            }
            Expr::Index(base, idx) => {
                self.check_expr(base, locals, label);
                if let Some(i) = idx {
                    self.check_expr(i, locals, label);
                    self.check_shape_key_access(base, i, locals, label);
                }
            }
            Expr::New(_, args) => {
                // `new Foo()` never carries known type arguments itself -
                // real PHP's constructor syntax has no way to write
                // `new Foo<Bar>()` at all, and inferring them from the
                // constructor's own arguments (the way real PHPStan often
                // does) isn't attempted here, so this is always `Object`
                // with an empty arg list.
                let resolved_class = match self.infer_expr(e, locals) {
                    Type::Object(class, _) => Some(class),
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
                self.predefine_byref_or_unknown_args(ctor_sig, args, locals);
                for a in args {
                    self.check_expr(a, locals, label);
                }
                if let Some(class) = &resolved_class {
                    // `resolved_class` is already the fully-qualified name
                    // (`infer_expr`'s New arm resolves it via `name_ctx`),
                    // so every miss here is a real undefined-class finding
                    // - no "can't resolve a namespaced name" case left, the
                    // way there used to be before Phase 9.5's namespace
                    // support.
                    if !self.registry.classes.contains_key(class) {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!("instantiated class {class} not found"),
                        });
                    } else if let Some(sig) = ctor_sig {
                        self.check_args(
                            &format!("{class}::__construct"),
                            sig,
                            args,
                            locals,
                            label,
                            Some(class),
                        );
                    }
                }
            }
            Expr::NewAnonymousClass(decl, args) => {
                for a in args {
                    self.check_expr(a, locals, label);
                }
                // The anonymous class's own methods are still worth
                // checking, even though the class itself isn't indexed
                // into the registry under any stable name - check_function
                // only needs a FunctionDecl and a label to check against.
                for m in &decl.methods {
                    self.check_function(&m.function, Some(&decl.name));
                }
            }
            Expr::DynamicVariable(name_expr) => self.check_expr(name_expr, locals, label),
            Expr::PropertyAccess(base, _) => self.check_expr(base, locals, label),
            Expr::DynamicPropertyAccess(base, name_expr) => {
                self.check_expr(base, locals, label);
                self.check_expr(name_expr, locals, label);
            }
            Expr::DynamicMethodCall(recv, name_expr, args) => {
                self.check_expr(recv, locals, label);
                self.check_expr(name_expr, locals, label);
                for a in args {
                    self.check_expr(a, locals, label);
                }
            }
            Expr::Interpolated(parts) => {
                for p in parts {
                    self.check_expr(p, locals, label);
                }
            }
            _ => {}
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
    fn predefine_byref_or_unknown_args(
        &self,
        sig: Option<&FunctionSig>,
        args: &[Expr],
        locals: &mut HashMap<String, Type>,
    ) {
        for (i, arg) in args.iter().enumerate() {
            let Expr::Variable(name) = arg else {
                continue;
            };
            if locals.contains_key(name) {
                continue;
            }
            let treat_as_defining = match sig {
                None => true,
                Some(sig) => sig
                    .params
                    .get(i)
                    .map(|p| p.by_ref)
                    .or_else(|| sig.params.last().filter(|p| p.variadic).map(|p| p.by_ref))
                    .unwrap_or(false),
            };
            if treat_as_defining {
                locals.insert(name.clone(), Type::Mixed);
            }
        }
    }

    fn check_property_assignment(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        locals: &HashMap<String, Type>,
        label: &str,
    ) {
        let Expr::PropertyAccess(base, prop) = lhs else {
            return;
        };
        let Type::Object(class, type_args) = self.infer_expr(base, locals) else {
            return;
        };
        let Some(prop_sig) = self.registry.lookup_property(&class, prop) else {
            // Not a declared property: could be a dynamic property, which
            // is legal (if deprecated) PHP - not flagged here, only wrong
            // types on properties we do know about.
            return;
        };
        let bindings = self.class_template_bindings(&class, &type_args);
        let expected = types::substitute_template(&prop_sig.ty, &bindings);
        let expected = self.resolve_pseudo(&expected, &class);
        let actual = self.infer_expr(rhs, locals);
        if actual == Type::Mixed || self.registry.is_assignable(&actual, &expected) {
            return;
        }
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!("{class}::${prop} expects {expected}, {actual} given"),
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
        });
    }

    fn check_args(
        &mut self,
        callee_label: &str,
        sig: &FunctionSig,
        args: &[Expr],
        locals: &HashMap<String, Type>,
        label: &str,
        defining_class: Option<&str>,
    ) {
        let required = sig
            .params
            .iter()
            .filter(|p| !p.variadic && !p.has_default)
            .count();
        if args.len() < required {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!(
                    "{callee_label}() expects at least {required} argument(s), {} given",
                    args.len()
                ),
            });
        }

        for (i, arg) in args.iter().enumerate() {
            let param = match sig.params.get(i) {
                Some(p) => p,
                None => match sig.params.last() {
                    Some(p) if p.variadic => p,
                    _ => break,
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
                });
            }
        }
    }

    fn infer_expr(&self, e: &Expr, locals: &HashMap<String, Type>) -> Type {
        match e {
            Expr::Literal(lit) => match lit {
                Literal::Int(n) => Type::IntLiteral(*n),
                Literal::Float(_) => Type::Float,
                Literal::String(s) => Type::StringLiteral(s.clone()),
                Literal::Bool(true) => Type::True,
                Literal::Bool(false) => Type::False,
                Literal::Null => Type::Null,
            },
            Expr::Variable(name) => locals.get(name).cloned().unwrap_or(Type::Mixed),
            Expr::Grouping(inner) | Expr::Clone(inner, _) => self.infer_expr(inner, locals),
            Expr::FirstClassCallable(_) => Type::Callable,
            // A throw expression never actually produces a value - Never
            // is exactly the right type for it, the same way it's used for
            // a function that always throws or exits.
            Expr::Throw(_) => Type::Never,
            // Never given a stable nominal type, since multiple such
            // expressions in the same file could share the placeholder
            // name - Mixed is the honest answer, not an oversight.
            Expr::NewAnonymousClass(..) => Type::Mixed,
            Expr::Cast(ty, _) => match ty.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "string" => Type::String,
                "bool" => Type::Bool,
                "array" => Type::Array(Box::new(Type::Mixed)),
                "object" => Type::Object("object".to_string(), Vec::new()),
                "unset" => Type::Null,
                _ => Type::Mixed,
            },
            Expr::Unary(UnaryOp::Not, _) => Type::Bool,
            Expr::Unary(_, inner) => self.infer_expr(inner, locals),
            Expr::Binary(op, l, r) => self.infer_binary(op, l, r, locals),
            Expr::Assign(_, rhs) => self.infer_expr(rhs, locals),
            // `self`/`parent`/`static` aren't lexed as keywords (see the
            // parser: they're indistinguishable from any other identifier),
            // so a literal `Expr::Name("self")` has to be special-cased
            // here rather than showing up as an Object of that name.
            Expr::New(class_expr, _) => match class_expr.as_ref() {
                Expr::Name(name) => match name.as_str() {
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
            Expr::Call(callee, args) => match callee.as_ref() {
                Expr::Name(name) if !is_language_construct(name) => {
                    let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                    self.registry
                        .lookup_function(&primary)
                        .or_else(|| {
                            fallback
                                .as_deref()
                                .and_then(|g| self.registry.lookup_function(g))
                        })
                        .map(|sig| self.infer_call_return_type(sig, args, locals))
                        .unwrap_or(Type::Mixed)
                }
                _ => Type::Mixed,
            },
            Expr::MethodCall(recv, method, args) => match self.infer_expr(recv, locals) {
                Type::Object(class, type_args) => self
                    .registry
                    .lookup_method(&class, method)
                    .map(|sig| {
                        // Class-level substitution (Collection<User>'s own
                        // `T`) runs first, then infer_call_return_type's
                        // existing per-call-argument binding handles any
                        // template the *method itself* separately declares
                        // - the two are independent parameter scopes that
                        // can both appear on the same signature.
                        let sig = self.substitute_class_templates(sig, &class, &type_args);
                        let resolved = self.infer_call_return_type(&sig, args, locals);
                        self.resolve_pseudo(&resolved, &class)
                    })
                    .unwrap_or(Type::Mixed),
                _ => Type::Mixed,
            },
            Expr::Ternary(_, then_, else_) => {
                let else_ty = self.infer_expr(else_, locals);
                match then_ {
                    Some(t) => {
                        let then_ty = self.infer_expr(t, locals);
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
            Expr::ArrayLiteral(_) => Type::Array(Box::new(Type::Mixed)),
            Expr::Interpolated(_) => Type::String,
            Expr::PropertyAccess(base, prop) => match self.infer_expr(base, locals) {
                Type::Object(class, type_args) => self
                    .registry
                    .lookup_property(&class, prop)
                    .map(|p| {
                        let bindings = self.class_template_bindings(&class, &type_args);
                        let resolved = types::substitute_template(&p.ty, &bindings);
                        self.resolve_pseudo(&resolved, &class)
                    })
                    .unwrap_or(Type::Mixed),
                _ => Type::Mixed,
            },
            // A homogeneous array's element type applies regardless of
            // which key is accessed; optimistically assumes the access is
            // in bounds/present rather than widening to `V|null` for a
            // possibly-missing element - consistent with this checker's
            // general preference for fewer false positives over catching
            // every possible runtime-undefined access.
            Expr::Index(base, Some(idx)) => match self.infer_expr(base, locals) {
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

    /// A generic function's return type may mention its own `@template`
    /// parameters (`TemplateParam`) - this binds each one against the
    /// actual argument at this call site (`identity(5)` binds `T` to
    /// `int`) and substitutes the result into the return type. A signature
    /// with no templates at all is untouched (the common case, cheap).
    fn infer_call_return_type(
        &self,
        sig: &FunctionSig,
        args: &[Expr],
        locals: &HashMap<String, Type>,
    ) -> Type {
        let mut bindings: HashMap<String, Type> = HashMap::new();
        for (param, arg) in sig.params.iter().zip(args.iter()) {
            let arg_ty = self.infer_expr(arg, locals);
            types::bind_template_param(&param.ty, &arg_ty, &mut bindings);
        }
        types::substitute_template(&sig.return_type, &bindings)
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
            | BinOp::LogicalXor
            | BinOp::Spaceship => Type::Bool,
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr => {
                let lt = self.infer_expr(l, locals);
                let rt = self.infer_expr(r, locals);
                if lt == Type::Float || rt == Type::Float {
                    Type::Float
                } else {
                    Type::Int
                }
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
/// `token.rs` - so `isset($x)` parses as `Expr::Call(Name("isset"), ...)`
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
    match e {
        Expr::Literal(Literal::String(s)) => Some(s.clone()),
        Expr::Literal(Literal::Int(n)) => Some(n.to_string()),
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

    fn diagnostics_for(src: &str) -> Vec<Diagnostic> {
        let tokens = Lexer::new(src).tokenize();
        let program = Parser::new(tokens).parse_program();
        let mut registry = Registry::build(&program);
        registry.merge_stubs(crate::php_stubs::generated_stubs());
        registry.merge_stub_classes(crate::php_stubs::generated_stub_classes());
        registry.merge_stubs(crate::stubs::builtin_stubs());
        let mut checker = Checker::new(&registry);
        checker.check_program(&program);
        checker.into_diagnostics()
    }

    #[test]
    fn flags_wrong_argument_type() {
        let d = diagnostics_for(r#"<?php strlen(42);"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("expects string"));
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
        // These parse as a synthetic Expr::Call (see parser.rs's own
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
}
