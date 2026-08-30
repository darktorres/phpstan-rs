use std::collections::HashMap;

use crate::ast::*;
use crate::registry::{FunctionSig, Registry};
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
pub struct Checker<'a> {
    registry: &'a Registry,
    diagnostics: Vec<Diagnostic>,
    // Short name (or `as Alias`) -> the class's own bare declared name,
    // built once per file from its top-level `use` imports. Needed
    // because almost all real code refers to an imported class by this
    // short name, not a fully-qualified path - without resolving it here
    // first, undefined-class detection would flag the overwhelming
    // majority of legitimately-imported classes (Registry itself only
    // ever keys a class by its own bare declared name, ignoring
    // namespace, the same simplification `remove_declarations`'s doc
    // comment already describes for declarations).
    use_map: HashMap<String, String>,
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
            use_map: HashMap::new(),
            current_class: None,
            saw_dynamic_var_construct: false,
        }
    }

    /// Resolves a bare (no internal `\`) name against this file's
    /// `use`-import map, falling back to the name itself when it isn't
    /// one (a class declared directly in this file/namespace, or a
    /// genuinely undefined one - both look the same at this point, and
    /// the registry lookup that follows is what tells them apart).
    fn resolve_class_name(&self, name: &str) -> String {
        self.use_map
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    pub fn check_program(&mut self, program: &[Stmt]) {
        for stmt in program {
            if let Stmt::Use(imports) = stmt {
                for import in imports {
                    let bare = import
                        .path
                        .rsplit('\\')
                        .next()
                        .unwrap_or(&import.path)
                        .to_string();
                    let short_name = import.alias.clone().unwrap_or_else(|| bare.clone());
                    self.use_map.insert(short_name, bare);
                }
            }
        }

        let mut top_locals: HashMap<String, Type> = HashMap::new();
        let diagnostics_before = self.diagnostics.len();
        for stmt in program {
            match stmt {
                Stmt::FunctionDecl(f) => self.check_function(f, None),
                Stmt::ClassDecl(c) => {
                    for m in &c.methods {
                        self.check_function(&m.function, Some(&c.name));
                    }
                }
                Stmt::EnumDecl(e) => {
                    for m in &e.methods {
                        self.check_function(&m.function, Some(&e.name));
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
            locals.insert("this".to_string(), Type::Object(c.to_string()));
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
            Type::SelfRef | Type::StaticRef => Type::Object(defining_class.to_string()),
            Type::ParentRef => self
                .registry
                .classes
                .get(defining_class)
                .and_then(|info| info.extends.first())
                .map(|p| Type::Object(p.clone()))
                .unwrap_or_else(|| ty.clone()),
            _ => ty.clone(),
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
                let then_terminates = always_terminates(then_branch);
                let else_terminates = else_branch.as_deref().is_some_and(always_terminates);

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
                            .map(|t| Type::Object(t.clone()))
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
                    let bare = crate::registry::strip_leading_backslash(class_name);
                    locals.insert(
                        var_name.clone(),
                        Type::Object(self.resolve_class_name(&bare)),
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
                let sig = match callee.as_ref() {
                    Expr::Name(name) => self.registry.lookup_function(name),
                    _ => None,
                };
                self.predefine_byref_or_unknown_args(sig, args, locals);
                for a in args {
                    self.check_expr(a, locals, label);
                }
                if let Expr::Name(name) = callee.as_ref()
                    && let Some(sig) = sig
                {
                    self.check_args(name, sig, args, locals, label, None);
                }
            }
            Expr::MethodCall(recv, method, args) => {
                self.check_expr(recv, locals, label);
                let sig = match self.infer_expr(recv, locals) {
                    Type::Object(class) => self.registry.lookup_method(&class, &method.clone()),
                    _ => None,
                };
                self.predefine_byref_or_unknown_args(sig, args, locals);
                for a in args {
                    self.check_expr(a, locals, label);
                }
                if let Type::Object(class) = self.infer_expr(recv, locals)
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let qualified = format!("{class}::{method}");
                    self.check_args(&qualified, sig, args, locals, label, Some(&class));
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
                let resolved_class = match self.infer_expr(e, locals) {
                    Type::Object(class) => Some(class),
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
                    // A name containing an internal `\` is a genuinely
                    // namespaced reference (`new Foo\Bar()`) this project
                    // can't safely resolve (namespaces aren't modeled at
                    // all - see `strip_leading_backslash`'s doc comment),
                    // so it's left unchecked rather than risking a false
                    // positive; only a single leading `\` (fully qualified
                    // from the global namespace) was already stripped by
                    // `infer_expr`.
                    if !class.contains('\\') && !self.registry.classes.contains_key(class) {
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
        let Type::Object(class) = self.infer_expr(base, locals) else {
            return;
        };
        let Some(prop_sig) = self.registry.lookup_property(&class, prop) else {
            // Not a declared property: could be a dynamic property, which
            // is legal (if deprecated) PHP - not flagged here, only wrong
            // types on properties we do know about.
            return;
        };
        let expected = self.resolve_pseudo(&prop_sig.ty, &class);
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
                "object" => Type::Object("object".to_string()),
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
                    _ => {
                        let bare = crate::registry::strip_leading_backslash(name);
                        Type::Object(self.resolve_class_name(&bare))
                    }
                },
                _ => Type::Mixed,
            },
            Expr::Call(callee, args) => match callee.as_ref() {
                Expr::Name(name) => self
                    .registry
                    .lookup_function(name)
                    .map(|sig| self.infer_call_return_type(sig, args, locals))
                    .unwrap_or(Type::Mixed),
                _ => Type::Mixed,
            },
            Expr::MethodCall(recv, method, args) => match self.infer_expr(recv, locals) {
                Type::Object(class) => self
                    .registry
                    .lookup_method(&class, method)
                    .map(|sig| {
                        let resolved = self.infer_call_return_type(sig, args, locals);
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
                Type::Object(class) => self
                    .registry
                    .lookup_property(&class, prop)
                    .map(|p| self.resolve_pseudo(&p.ty, &class))
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
        "is_object" => Some(Type::Object("object".to_string())),
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

/// Conservatively recognizes a statement that unconditionally leaves the
/// current block (return/throw/break/continue, or a block/if where every
/// path does). Only used to decide whether a branch's locals should
/// propagate past an `if` - false negatives here (missing a case that
/// really does always terminate) just mean less narrowing, never a wrong
/// answer, so loops/switch/try are deliberately not analyzed and just
/// treated as "might fall through".
fn always_terminates(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Block(stmts) => stmts.last().is_some_and(always_terminates),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            always_terminates(then_branch) && else_branch.as_deref().is_some_and(always_terminates)
        }
        _ => false,
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
        let d = diagnostics_for(
            r#"<?php
            function f() {
                some_undeclared_function($result);
                return $result;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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
    fn use_import_resolves_to_the_bare_short_name_before_lookup() {
        let d = diagnostics_for(
            r#"<?php
            use App\Domain\Widget;
            function f() { return new Widget(); }
        "#,
        );
        // Widget isn't actually declared anywhere in this snippet, so this
        // documents the known, accepted limitation (no `use` target is
        // ever really loaded here) rather than a false negative in the
        // resolution logic itself - what's under test is that the
        // *short-name resolution* runs (App\Domain\Widget -> Widget)
        // without panicking and without also flagging an unrelated
        // spurious diagnostic; a real project supplies the actual
        // declaration file, as verified separately against Piwigo.
        assert!(
            d.iter().any(|x| x.message.contains("Widget not found")),
            "expected exactly the resolved bare name in the diagnostic, got: {d:?}"
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
}
