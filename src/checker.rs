use std::collections::HashSet;

use crate::ast::*;
use crate::pest_config;
use crate::registry::{FunctionSig, NameContext, Registry, resolve_names_in_type};
use crate::token::Span;
use crate::types::{self, Type};

/// A checked scope's variable-name-to-inferred-type bindings - repeated
/// across nearly every checker method's own parameter/return type before
/// this alias existed.
///
/// Phase 20 Finding B: backed by `imbl::HashMap`, a persistent,
/// structurally-shared map (a HAMT), not `std::collections::HashMap` -
/// cloned at ~22 call sites across `checker/stmt.rs`/`checker/expr.rs`
/// (once per branch of every `if`/`while`/ternary/`match`/closure, in
/// every function, in every file), profiling identified this as the
/// single biggest allocation source in the whole checker: a plain
/// `HashMap` clone rehashes every key and deep-clones every `Type`
/// value's own nested `String`s/`Vec`s. `imbl::HashMap::clone` is O(1) -
/// "really only a matter of copying a pointer and increasing a reference
/// counter" - a value only gets deep-copied lazily, on the *next*
/// mutation of whichever clone diverges from the other. `insert`/
/// `remove`/`entry`/`get`/`contains_key`/`iter`/`Default`/`IntoIterator`/
/// `FromIterator`/`PartialEq` all mirror `std::collections::HashMap`'s
/// own signatures closely enough that every existing call site needed
/// little to no change - confirmed directly against `imbl`/`im`'s own
/// docs before making this change, not assumed. `rustc_hash::
/// FxBuildHasher` (Finding C) as the hasher, since `imbl::HashMap` is
/// generic over it the same way `std::collections::HashMap` is.
pub(crate) type Locals = imbl::GenericHashMap<
    String,
    Type,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

/// A generic `@template` name -> concrete `Type` substitution map (class-
/// hierarchy template binding, function-call argument-to-template
/// binding) - built once per call/hierarchy-walk and consumed
/// immediately by `types::substitute_template`/`bind_template_param`,
/// never cloned per-branch the way `Locals` is. Used to reuse `Locals`'s
/// own type alias opportunistically (both were `HashMap<String, Type>`
/// underneath) before Phase 20 Finding B gave `Locals` its own,
/// persistent-map-backed identity - kept as a plain
/// `std::collections::HashMap` here since this use case has none of
/// `Locals`'s own "cloned once per branch" cost profile that motivated
/// the swap.
pub(crate) type TemplateBindings = std::collections::HashMap<String, Type>;

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
    pub(crate) registry: &'a Registry,
    pub(crate) diagnostics: Vec<Diagnostic>,
    // Namespace + `use`-import map, built once per file. Needed because
    // almost all real code refers to an imported class/function by its
    // short name, not a fully-qualified path - without resolving it here
    // first, undefined-class/function detection would flag the
    // overwhelming majority of legitimately-imported symbols. Also must
    // resolve to exactly the same fully-qualified names
    // `Registry::index` keys its entries under (Phase 9.5's fix for the
    // namespace-collision bug documented above), or the two would
    // silently disagree with each other.
    pub(crate) name_ctx: NameContext,
    // The class whose method body is currently being checked, if any -
    // needed to resolve self/parent/static to a concrete class name.
    pub(crate) current_class: Option<String>,
    // The name of the method whose body is currently being checked, if
    // any - needed to tell whether a `readonly` property write is
    // happening inside its own declaring class's constructor (Finding
    // 47's pragmatic first cut: any write outside that exact spot is
    // flagged).
    pub(crate) current_method: Option<String>,
    // Set for the duration of one function/method (or the top-level script)
    // body when it calls extract()/eval() - both can define variables this
    // checker has no way to see, so undefined-variable detection would be
    // pure noise for the rest of that body. Rather than track precisely
    // which diagnostics came from that check, the whole body's diagnostics
    // are discarded if this ends up true (see check_function).
    pub(crate) saw_dynamic_var_construct: bool,
    // Set for the duration of one function/method/closure body that
    // contains a yield/yield from anywhere (Finding 3) - a real
    // generator, whose implicit `return null` at the end (or an explicit
    // bare `return;`) never mismatches its declared Generator/Iterator/
    // Traversable return type, unlike an ordinary function. Consulted by
    // both check_function's own falls-off-the-end check and check_stmt's
    // StmtKind::Return(None) arm, which has no direct access to the
    // enclosing function's body to answer this itself.
    pub(crate) current_function_is_generator: bool,
    // Finding 1: the `$this` type inside a Pest `it`/`test`/`describe`/
    // `beforeEach`/`afterEach`/`beforeAll`/`afterAll` closure - set via
    // `set_pest_this_type` (by `Project::check_file`, from its own
    // `PestConfig::resolve_this_type`) before `check_program` runs, then
    // possibly overridden by a file-local `uses(...)` call
    // `check_program` itself finds (real Pest's own resolution order:
    // file-local wins over directory-scoped). Defaults to `Type::Mixed`
    // so every existing call site untouched by Finding 1 (the ~300+
    // direct `Checker::new` calls in this project's own tests, the `demo`
    // subcommand, ...) is completely unaffected - this field is only ever
    // consulted inside an `is_this_binding_call`-recognized closure
    // argument, which none of those exercise.
    pub(crate) pest_this_type: Type,
    // Phase 22 Finding 1: dynamic property names a Pest `beforeEach(...)`
    // closure assigns onto the shared `$this` (see `pest_config`'s own
    // `file_local_hook_properties`/`PestConfig::resolve_hook_properties`)
    // - set via `set_pest_hook_properties` (by `Project::check_file`, from
    // `PestConfig::resolve_hook_properties`) before `check_program` runs,
    // then unioned (not overridden - a property-name set has no "most
    // specific wins" precedence the way a single class type does) with
    // whatever file-local `beforeEach(...)` calls `check_program` itself
    // finds. Defaults empty, so every existing call site untouched by
    // this is completely unaffected.
    pub(crate) pest_hook_properties: HashSet<String>,
    // Phase 29: one frame per currently-open loop or switch, collecting
    // the `locals` snapshot at every `break` reached inside it - a stack,
    // since loops/switches nest. `StmtKind::Foreach`/`While`/`For`/
    // `DoWhile` each fold every collected state into what flows past the
    // loop once its body is done being checked (a real `break` inside an
    // `if` was otherwise silently discarded from post-loop state
    // entirely - `always_terminates` correctly treats it as "doesn't
    // fall through to the next statement in this block", but nothing
    // separately captured it for the *enclosing loop's* own exit state,
    // the same two-question split `Switch`'s own pre-existing
    // `exit_states` mechanism already draws for itself). `Switch` also
    // pushes/pops a frame here - not to consume it (its own `exit_states`
    // computation is entirely self-contained) but so a bare `break;`
    // written directly inside a case attaches to *that* frame (discarded
    // on pop) instead of leaking into whichever enclosing loop happens to
    // be open, matching real PHP's "innermost loop-or-switch" rule for an
    // unqualified `break;`.
    pub(crate) loop_break_states: Vec<Vec<Locals>>,
}

mod assignment;
mod calls;
mod control_flow;
mod expr;
mod generics;
mod narrowing;
mod oop;
mod resolve;
mod stmt;

pub(crate) use assignment::{is_superglobal, literal_string_key, literal_string_value};
pub(crate) use calls::{CheckArgsCall, is_language_construct, resolve_call_args};
pub(crate) use control_flow::{body_contains_yield, merge_scopes};
pub(crate) use expr::{build_matched_shape, union_types, unwrap_grouping};
// Only ever called by `union_types` itself in real code (`checker/expr.rs`) -
// this crate-level re-export exists solely so this module's own test
// suite can exercise it directly, the same reason `preg_match_matches_shape`
// isn't re-exported either.
#[cfg(test)]
pub(crate) use expr::generalize_by_key_set;
pub(crate) use narrowing::{
    is_real_variable_key, narrow_type_excluding, narrow_type_excluding_falsy, narrow_type_to,
    narrowing_key, purge_stale_narrowing_for,
};

impl<'a> Checker<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Checker {
            registry,
            diagnostics: Vec::new(),
            name_ctx: NameContext::default(),
            current_class: None,
            current_method: None,
            saw_dynamic_var_construct: false,
            current_function_is_generator: false,
            pest_this_type: Type::Mixed,
            pest_hook_properties: HashSet::new(),
            loop_break_states: Vec::new(),
        }
    }

    /// Sets the `$this` type Finding 1's Pest support seeds an
    /// `it`/`test`/`describe`/`beforeEach`/`afterEach`/`beforeAll`/
    /// `afterAll` closure argument with - a setter rather than a
    /// `Checker::new` parameter, so the ~300+ existing call sites that
    /// don't care about Pest at all don't need to change.
    pub fn set_pest_this_type(&mut self, ty: Type) {
        self.pest_this_type = ty;
    }

    /// Seeds the directory-scoped half of the dynamic-`$this`-property
    /// name set (`PestConfig::resolve_hook_properties`) - a setter for
    /// the same reason `set_pest_this_type` is one. `check_program`
    /// unions this with whatever file-local `beforeEach(...)` calls it
    /// finds itself, rather than one overriding the other.
    pub fn set_pest_hook_properties(&mut self, props: HashSet<String>) {
        self.pest_hook_properties = props;
    }

    /// Every diagnostic this checker emits goes through here - replaces
    /// 38 hand-written `Diagnostic { location, message, span, level }`
    /// literals (all sharing this exact 4-field shape) with one call
    /// site.
    pub(crate) fn report(&mut self, location: String, message: String, span: Span, level: u8) {
        self.diagnostics.push(Diagnostic {
            location,
            message,
            span,
            level,
        });
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

    #[hotpath::measure]
    pub fn check_program(&mut self, program: &[Stmt]) {
        // A simple whole-file pass (last `namespace` seen wins), unlike
        // `Registry::index`'s positional tracking - brace-delimited
        // multi-namespace-per-file is rare enough in real application
        // code that the extra precision isn't worth it here; a mismatch
        // in such a rare file only risks a missed diagnostic, not the
        // silent method-table corruption the registry itself had to
        // guard against.
        self.name_ctx = NameContext::build(program);
        // Finding 1: a file-local `uses(...)` call (no `->in(...)`) wins
        // over whatever `Project::check_file` already set via
        // `set_pest_this_type` from directory-scoped Pest.php config -
        // matches real Pest's own resolution order (file-local checked
        // first). A file with no such call leaves `pest_this_type`
        // exactly as `Project::check_file` set it.
        if let Some(local_ty) = pest_config::file_local_this_type(program, &self.name_ctx) {
            self.pest_this_type = local_ty;
        }
        // Unlike `pest_this_type` just above, file-local and
        // directory-scoped hook properties are unioned, not one
        // overriding the other - a property-name set has no "file-local
        // wins" precedence to apply.
        self.pest_hook_properties
            .extend(pest_config::file_local_hook_properties(program));

        let mut top_locals: Locals = Locals::default();
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
                    if c.kind == ClassKind::Trait {
                        self.check_trait_methods(&fqn, c);
                    } else {
                        for m in &c.methods {
                            self.check_function(&m.function, Some(&fqn), m.is_static);
                        }
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

    /// Phase 13 Finding E: a trait's own method bodies are checked once
    /// per class that `use`s it, not standalone - `flatten_traits`
    /// (`registry.rs`) already merges a trait's own methods/properties
    /// into every using class's own `ClassInfo` before checking starts,
    /// so re-checking with `current_class` set to a real host's FQN
    /// means every existing lookup (`lookup_method`/`lookup_property`)
    /// transparently sees that host's true member set - a trait method
    /// calling `$this->hostOnlyMethod()` is valid PHP (the trait's
    /// author expects any using class to provide it), but false-
    /// positived as "undefined method" when checked standalone. A trait
    /// with no user anywhere in the project (an orphaned/library trait)
    /// still gets checked standalone exactly as before - a real
    /// regression otherwise, since today's behavior checks every
    /// trait's body unconditionally.
    fn check_trait_methods(&mut self, fqn: &str, c: &ClassDecl) {
        let hosts = self.registry.classes_using_trait(fqn);
        let contexts: Vec<String> = if hosts.is_empty() {
            vec![fqn.to_string()]
        } else {
            hosts
        };
        let start = self.diagnostics.len();
        for host in contexts {
            for m in &c.methods {
                self.check_function(&m.function, Some(&host), m.is_static);
            }
        }
        // A trait used by 2+ classes gets checked 2+ times - a
        // genuinely host-independent bug in the trait's own body (no
        // `$this->...` involved at all) would otherwise be reported
        // once per host, since diagnostics are only ever sorted, never
        // deduplicated elsewhere. A real host-specific finding (missing
        // on one host but not another) survives this, since its own
        // message genuinely differs per host. This local sort exists
        // only to bring duplicates adjacent for `dedup_by` (which only
        // ever removes consecutive matches) - the final, whole-program
        // sort downstream of `check_program` reorders everything
        // correctly regardless of this block's own local order.
        self.diagnostics[start..].sort_by(|a, b| {
            (a.span.start.line, a.span.start.col, &a.message).cmp(&(
                b.span.start.line,
                b.span.start.col,
                &b.message,
            ))
        });
        self.diagnostics
            .dedup_by(|a, b| a.span == b.span && a.message == b.message);
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
    pub(crate) fn lookup_own_signature(
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

    #[hotpath::measure]
    pub(crate) fn check_function(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        is_static: bool,
    ) {
        let sig = self.lookup_own_signature(f, class_name);

        let mut locals: Locals = Locals::default();
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
            // Meant as a "shouldn't happen, but degrade rather than
            // panic" fallback (this function had to be indexed to be
            // reachable here at all) - except it's exactly what every
            // anonymous class method hits, unconditionally: `lookup_own_
            // signature` looks up `self.registry.classes.get(c)` for
            // `class_name = Some(&decl.name)`, and an anonymous class's
            // synthetic name is never indexed into the registry (see
            // `ExprKind::NewAnonymousClass`'s own comment) - so `sig` is
            // always `None` there. Unlike the `Some(sig)` branch above,
            // this one never resolved a `use`-imported type-hint name to
            // its FQN (Phase 22 Finding 5 - confirmed directly from a
            // real diagnostic's own wording: "expected ResponseInterface,
            // got Nyholm\Psr7\Response", the expected side printing bare/
            // unqualified where a resolved one would be fully qualified),
            // nor resolved a `self`/`static`/`parent` pseudo-type against
            // the enclosing (possibly anonymous) class the way line 385
            // above does. Both fixed the same way, using `self.name_ctx` -
            // the exact per-file context real class declarations get
            // resolved against - since this branch (unlike the `Some`
            // one) builds each `Type` fresh from a raw string on every
            // call anyway, with no immutable-AST obstacle to work around.
            None => {
                for p in &f.params {
                    let ty = p
                        .type_hint
                        .as_deref()
                        .map(types::parse_type_string)
                        .map(|ty| resolve_names_in_type(ty, &self.name_ctx))
                        .unwrap_or(Type::Mixed);
                    let ty = match class_name {
                        Some(c) => self.resolve_pseudo(&ty, c),
                        None => ty,
                    };
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
        let declared_return = sig.map(|s| s.return_type.clone()).or_else(|| {
            let ty = f
                .return_type
                .as_deref()
                .map(types::parse_type_string)
                .map(|ty| resolve_names_in_type(ty, &self.name_ctx))?;
            Some(match class_name {
                Some(c) => self.resolve_pseudo(&ty, c),
                None => ty,
            })
        });
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
        let previous_generator_flag = self.current_function_is_generator;
        self.current_function_is_generator = body_contains_yield(&f.body);

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
        // A real generator (Finding 3) is exempt: falling off the end (or
        // an explicit bare `return;`, handled in check_stmt) implicitly
        // ends the *generator's* iteration, not the function's own return
        // value - real PHPStan never checks either against the function's
        // declared Generator/Iterator/Traversable return type.
        if let Some(expected) = &declared_return
            && let Some(last) = f.body.last()
            && !self.always_terminates(last, &locals)
            && !self.current_function_is_generator
        {
            let expected = match &self.current_class {
                Some(c) => self.resolve_pseudo(expected, c),
                None => expected.clone(),
            };
            if expected != Type::Void
                && expected != Type::Mixed
                && !self.registry.is_assignable(&Type::Null, &expected)
            {
                self.report(
                    label.clone(),
                    format!("return type mismatch: expected {expected}, got null"),
                    f.span,
                    3,
                );
            }
        }

        if self.saw_dynamic_var_construct {
            self.diagnostics.truncate(diagnostics_before);
        }
        self.saw_dynamic_var_construct = previous_dynamic_flag;
        self.current_function_is_generator = previous_generator_flag;

        self.current_class = previous_class;
        self.current_method = previous_method;
    }
}

#[cfg(test)]
mod tests;
