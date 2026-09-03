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
    is_real_variable_key, narrow_type_excluding, narrowing_key, purge_stale_narrowing_for,
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
        let program = Parser::new(tokens).parse_program().unwrap();
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
    fn appending_to_an_array_in_a_loop_then_reading_a_literal_index_is_not_flagged() {
        // Phase 22 Finding 2, real shape: RegenerateFixtureTest.php's own
        // `$tagIds = []; foreach ([...] as $name) { $tagIds[] = ...; }`
        // then `$tagIds[0]`/`[1]`/`[2]` read afterward.
        let d = diagnostics_for(
            r#"<?php
            function test(): void {
                $tagIds = [];
                foreach (['a', 'b', 'c'] as $name) {
                    $tagIds[] = strlen($name);
                }
                echo $tagIds[0];
                echo $tagIds[1];
                echo $tagIds[2];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn assigning_a_dynamic_key_in_a_loop_then_reading_a_literal_key_is_not_flagged() {
        // Phase 22 Finding 2, real shape: SearchFilterRenderer.php's own
        // `$displayFilters[$filtName] = ...;` inside a `foreach`, then
        // `$displayFilters['words']` read elsewhere.
        let d = diagnostics_for(
            r#"<?php
            function test(): void {
                $displayFilters = [];
                foreach (['words' => true, 'tags' => false] as $filtName => $filtConf) {
                    $displayFilters[$filtName] = $filtConf;
                }
                echo $displayFilters['words'];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_dynamic_write_disables_typo_catching_for_the_rest_of_that_variable() {
        // The real, deliberate cost of Finding 2's fix, pinned down as a
        // test rather than left as prose: once `$x` is widened away from
        // `Type::ArrayShape` by an append/dynamic-key write anywhere in
        // its lifetime, `check_shape_key_access` (which only fires on
        // `Type::ArrayShape`) stops catching a genuine typo on *any* key
        // of that variable from that point on - not just the new one.
        let d = diagnostics_for(
            r#"<?php
            function test(): void {
                $x = [];
                $x['a'] = 1;
                $x[] = 2;
                echo $x['typo'];
            }
        "#,
        );
        assert!(
            d.is_empty(),
            "expected the typo to go uncaught (the fix's known trade-off), got: {d:?}"
        );
    }

    // A round of Piwigo-corpus bucketing found the array-append-widening
    // path (Phase 22) built a plain `Array`, never a real `list<T>`, even
    // though real PHP's own append semantics always assign the next
    // sequential int key - verified via dumpType(): a single append onto
    // an already-list-like container (including a freshly-empty `[]`)
    // stays a real, now-provably-non-empty list.
    #[test]
    fn appending_onto_a_freshly_empty_array_produces_a_non_empty_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param non-empty-list<string> $x */
            function sink(array $x): void {}
            function f(string $a): void {
                $content = [];
                $content[] = $a;
                sink($content);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn appending_onto_an_already_typed_list_stays_a_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<string> $x
             *  @return non-empty-list<string>
             */
            function f(array $x): array {
                $x[] = 'y';
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_dynamic_key_write_does_not_claim_the_result_is_still_a_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<string> $x
             *  @return list<string>
             */
            function f(array $x, string $key): array {
                $x[$key] = 'y';
                return $x;
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected the list-ness mismatch to still be flagged: {d:?}"
        );
    }

    #[test]
    fn a_nested_write_through_a_dynamically_keyed_array_grows_its_element_shape() {
        // Real shape: CalendarMonthly::buildMonthCalendar builds $items
        // via a dynamic-key write ($items[(int) $row->period] = [...]),
        // then adds more fields per-element via a *nested* write
        // ($items[$day]['derivative'] = ...) - discovered as a genuine
        // new false positive while verifying Finding 2's own fix:
        // bind_assignment_target's Index arm required a bare Variable
        // base, so the nested write (base is itself $items[$day], an
        // Index, not a Variable) was silently ignored, leaving
        // $items[$day]'s inferred shape frozen at its initial literal -
        // missing 'derivative' entirely.
        let d = diagnostics_for(
            r#"<?php
            function test(int $period, string $file): void {
                $items = [];
                $items[$period] = ['nb_images' => 0];
                $items[$period]['derivative'] = $file;
                echo $items[$period]['derivative'];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 12 Finding E: a real, pre-existing gap found by making
    // check_shape_key_access Union-aware and re-checking the real
    // corpus - PiwigoInfosSender::send's own $piwigoInfos becomes a real
    // Union of two differently-shaped ArrayShapes across an if/else (one
    // branch's own nested field differs from the other's), and a later
    // flat write into that Union silently updated no member at all,
    // leaving a key written one line earlier still reported missing by a
    // very next nested write's own "does this key exist" check.
    // Positive precision check (not just "no false positive"): `Mixed` is
    // never flagged against anything (this project's own "don't guess"
    // leniency), so a test only asserting an empty diagnostic list here
    // would pass whether or not `$value` was ever given its real, precise
    // `string` type. Passing it to an `int`-only parameter instead makes
    // the fix's own effect directly observable: a real mismatch is only
    // reported once `$value` is actually inferred as `string`.
    #[test]
    fn foreach_over_a_homogeneous_array_shape_infers_the_shared_field_type() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): int { return $n; }
            function test(): void {
                $versions = ['current' => 'a', 'latest' => 'b'];
                foreach ($versions as $key => $value) {
                    needsInt($value);
                }
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected $value to be inferred as string (and flagged against needsInt): {d:?}"
        );
    }

    #[test]
    fn a_flat_write_into_a_union_of_array_shapes_updates_every_member() {
        let d = diagnostics_for(
            r#"<?php
            function test(bool $cond): void {
                $info = ['a' => 1];
                if ($cond) {
                    $info['b'] = 'x';
                } else {
                    $info['c'] = 'y';
                }
                $info['file_extensions'] = [];
                $info['file_extensions']['jpg'] = 3;
                echo $info['file_extensions']['jpg'];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 12 (unplanned follow-on): the real `PiwigoInfosSender::send`
    // shape - a `Union`-typed container (built across an `if`/`else`) whose
    // very first write to a brand-new key is itself a *nested* append
    // (`$info['updates'][] = ...`), never a flat `$info['updates'] = [];`
    // first. `check_assignment_target`'s `Index` arm used to route the
    // inner `$info['updates']` sub-expression through `check_expr`, which
    // wrongly checked it as a genuine read via `check_shape_key_access` -
    // masked before Finding E made that function actually inspect a
    // `Union` instead of bailing on it outright.
    #[test]
    fn a_nested_append_creating_a_brand_new_key_on_a_union_typed_container_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function test(bool $cond): void {
                if ($cond) {
                    $info = ['a' => 1];
                } else {
                    $info = ['b' => 2];
                }
                $info['updates'][] = 'x';
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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

    // Phase 31, real shape: `Piwigo\Admin\Install\InstallDataSeeder::seed`'s
    // own `$insert = ['id' => 1, 'galleries_url' => ...]; ...
    // massInsert('sites', array_keys($insert), [$insert]);` - the real
    // stub's own `@return int[]|string[]` (a whole-array either-or, not a
    // single homogeneous key type) always won when the argument's own
    // shape was knowable, printing as the confusing `array<int>|
    // array<string>` this cluster's diagnostics showed.
    #[test]
    fn array_keys_infers_string_keys_from_a_known_array_shape() {
        let d = diagnostics_for(
            r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            function f() {
                $insert = ['id' => 1, 'galleries_url' => 'x'];
                needsStrings(array_keys($insert));
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_keys_infers_int_keys_when_every_key_is_canonically_numeric() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function f() {
                $list = ['x', 'y', 'z'];
                needsInts(array_keys($list));
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_keys_unions_int_and_string_for_a_genuinely_mixed_shape() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function f() {
                $mixed = [0 => 'a', 'name' => 'b'];
                needsInts(array_keys($mixed));
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("needsInts"), "got: {d:?}");
    }

    // A round of Piwigo-corpus bucketing found the native stub's own
    // literal `@return int[]|string[]` became a real regression once
    // `array_element_type` gained a `Union` arm (below): a plain,
    // non-literal array<string, V> argument (not a literal shape, so the
    // dedicated case above doesn't apply) fell through to that stub
    // verbatim, surfacing `int|string` for every reader of a key -
    // opcache-preload.php's own `foreach (array_keys($classMap) as
    // $symbol) { str_starts_with($symbol, ...); }` was real corpus code
    // this broke. This project has no key-type tracking at all
    // (a known, tracked limitation) - falls back to its own usual
    // `Mixed` key-type default instead of the stub's own misleadingly
    // precise-sounding answer.
    #[test]
    fn array_keys_on_a_plain_array_falls_back_to_a_mixed_key_type_not_int_or_string() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array<string, string> $classMap */
            function f(array $classMap): void {
                foreach (array_keys($classMap) as $symbol) {
                    str_starts_with($symbol, 'x');
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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

    // Phase 23 Finding 3: str_replace/preg_replace's real stub return
    // types are unconditionally unioned with `array`, even though the
    // function only returns an array when $subject itself is one.
    #[test]
    fn str_replace_narrows_to_string_when_the_subject_is_a_string() {
        let d = diagnostics_for(
            r#"<?php
            function f(string $subject): string {
                return str_replace(['a', 'b'], ['c', 'd'], $subject);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_replace_narrows_to_string_or_null_when_the_subject_is_a_string() {
        let d = diagnostics_for(
            r#"<?php
            function f(string $subject): void {
                $result = preg_replace('/a/', 'b', $subject);
                if ($result !== null) {
                    strlen($result);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn str_replace_falls_back_to_the_generic_path_for_a_non_string_subject() {
        // Scoped to "subject is exactly Type::String" - a Mixed subject
        // (or a real array one) must keep today's existing, unaffected
        // generic-path behavior rather than the new special case firing.
        let d = diagnostics_for(
            r#"<?php
            function f($subject) {
                $result = str_replace('a', 'b', $subject);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 32 Finding E: same subject-dependent-return-type family as
    // `str_replace` above, missed by Phase 23/31 - `substr_replace`'s own
    // subject is argument index 0, not 2.
    #[test]
    fn substr_replace_narrows_to_string_when_the_subject_is_a_string() {
        let d = diagnostics_for(
            r#"<?php
            function f(string $subject): string {
                return substr_replace($subject, 'x', 0, 1);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn substr_replace_falls_back_to_the_generic_path_for_a_non_string_subject() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject) {
                $result = substr_replace($subject, 'x', 0, 1);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 13 Finding A: getenv()'s real stub is unconditionally
    // `array|string|false`, but a truly zero-argument call only ever
    // returns `array`, and a call with a real, non-null `$name` only
    // ever returns `string|false`.
    #[test]
    fn getenv_with_a_literal_name_narrows_to_string_or_false() {
        let d = diagnostics_for(
            r#"<?php
            function f(): string|false {
                return getenv('X');
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn getenv_with_no_arguments_narrows_to_a_plain_array() {
        let d = diagnostics_for(
            r#"<?php
            function f(): array {
                return getenv();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn getenv_with_an_explicit_null_name_keeps_the_broad_type() {
        // A real, live PHP fact confirmed against real PHPStan too:
        // `getenv(null)` returns the full environment array, exactly
        // like the zero-argument call - it must NOT get the
        // plain-`array` answer, and must NOT narrow to `string|false`.
        let d = diagnostics_for(
            r#"<?php
            function f(): string|false {
                return getenv(null);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "expected the broad type to still apply: {d:?}");
    }

    #[test]
    fn getenv_with_a_local_only_named_argument_keeps_the_broad_type() {
        // `getenv(local_only: true)` leaves `$name` at its own `null`
        // default (confirmed against real PHPStan's own dumpType()) -
        // `args.is_empty()` is false here, so this must not get the
        // plain-`array` answer either.
        let d = diagnostics_for(
            r#"<?php
            function f(): string|false {
                return getenv(local_only: true);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "expected the broad type to still apply: {d:?}");
    }

    #[test]
    fn getenv_with_a_possibly_null_name_keeps_the_broad_type() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $name): string|false {
                return getenv($name);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "expected the broad type to still apply: {d:?}");
    }

    // A round of Piwigo-corpus bucketing found the native stub's own
    // plain `string|false` was the single largest non-array-related
    // false-positive source: real PHPStan's own dynamic-return-type
    // extension always returns a real non-falsy-string on success
    // (verified via dumpType()), letting the common `$jar = tempnam(...);
    // if ($jar === false) { throw ...; }` idiom feed a
    // non-empty-string-typed sink afterward.
    #[test]
    fn tempnam_on_success_narrows_to_a_non_falsy_string() {
        let d = diagnostics_for(
            r#"<?php
            /** @param non-empty-string $path */
            function sink(string $path): void {}
            function f(): void {
                $jar = tempnam(sys_get_temp_dir(), 'x');
                if ($jar === false) {
                    throw new RuntimeException('x');
                }
                sink($jar);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Same family as tempnam() above: the native stub declares a bare
    // array|false, but real PHPStan's own dynamic-return-type extension
    // always returns a real list<string> on success, verified via
    // dumpType(). Real Piwigo test code (StructuralTest's own
    // findCallSitesInRootPhpFiles) narrows away false via `$paths !==
    // false ? $paths : []` and relies on the result being a real
    // list<string>.
    #[test]
    fn glob_on_success_is_a_real_list_of_strings() {
        let d = diagnostics_for(
            r#"<?php
            /** @return list<string> */
            function f(string $pattern): array {
                $paths = glob($pattern);
                return $paths !== false ? $paths : [];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // The vendored stub's own `@return string[]|false` reflects PHP 7's
    // behavior for an empty $separator - PHP 8+ throws a ValueError there
    // instead, so real PHPStan drops |false entirely and always answers
    // non-empty-list<string>, verified via dumpType() (even for a
    // non-literal $separator). Real Piwigo code
    // (GeneratedFileErrorMapper::lines) relies on exactly this.
    #[test]
    fn explode_always_returns_a_non_empty_list_never_false() {
        let d = diagnostics_for(
            r#"<?php
            /** @return non-empty-list<string> */
            function f(string $sep, string $s): array {
                return explode($sep, $s);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Only the array-stripping half of mb_convert_encoding()'s own real
    // (more involved) dynamic extension is implemented - evidenced via
    // `CharsetHelper::convertCharset`'s own real shape.
    #[test]
    fn mb_convert_encoding_narrows_to_string_or_false_when_the_subject_is_a_string() {
        let d = diagnostics_for(
            r#"<?php
            function f(string $subject): string|false {
                return mb_convert_encoding($subject, 'UTF-8', 'ISO-8859-1');
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 32 Finding D: microtime()'s real stub is unconditionally
    // `string|float` - real PHPStan narrows a literal `true`/`false`
    // argument via its own dynamic return type extension.
    #[test]
    fn microtime_true_infers_plain_float() {
        let d = diagnostics_for(
            r#"<?php
            function f(): float {
                return microtime(true);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn microtime_false_and_bare_infer_plain_string() {
        let d = diagnostics_for(
            r#"<?php
            function f(): string {
                return microtime(false);
            }
            function g(): string {
                return microtime();
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn microtime_with_a_non_literal_bool_keeps_todays_generic_union() {
        // Must not narrow when the argument isn't a literal true/false -
        // the generic string|float union should still apply, and a bare
        // `float` return should still be flagged as unsound.
        let d = diagnostics_for(
            r#"<?php
            function f(bool $b): float {
                return microtime($b);
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected the generic union to still apply: {d:?}"
        );
    }

    // Phase 32 Finding J: json_encode(...): string|false - passing
    // JSON_THROW_ON_ERROR makes a real failure throw instead of
    // returning false.
    #[test]
    fn json_encode_with_throw_on_error_infers_plain_string() {
        let d = diagnostics_for(
            r#"<?php
            function f($x): string {
                return json_encode($x, JSON_THROW_ON_ERROR);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Real shape: `UserListPageRenderer`'s own bitwise-OR'd multi-flag
    // call - the fix has to walk an arbitrary-depth `|`-chain, not just
    // match a bare constant.
    #[test]
    fn json_encode_with_throw_on_error_in_a_multi_flag_bitor_chain_infers_plain_string() {
        let d = diagnostics_for(
            r#"<?php
            function f($x): string {
                return json_encode($x, JSON_HEX_TAG | JSON_HEX_AMP | JSON_THROW_ON_ERROR);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 34 Finding A: curl_getinfo($ch, CONSTANT) has a real,
    // per-constant return type real PHPStan already models - the real
    // shape driving 16 corpus diagnostics (idcGet, identCurl, and every
    // other browser-test curl helper).
    #[test]
    fn curl_getinfo_response_code_infers_int() {
        let d = diagnostics_for(
            r#"<?php
            function f($ch): int {
                return curl_getinfo($ch, CURLINFO_RESPONSE_CODE);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn curl_getinfo_effective_url_infers_string() {
        let d = diagnostics_for(
            r#"<?php
            function f($ch): string {
                return curl_getinfo($ch, CURLINFO_EFFECTIVE_URL);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 12 Finding A: preg_match's by-ref $matches shape - a real
    // PCRE-structure parser (never a matching engine), scoped to the
    // common 3-argument, literal-pattern form.
    #[test]
    fn preg_match_required_group_is_accepted_as_a_string_parameter() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches);
                needsString($matches[1]);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Since Finding E, `$matches`'s own *unconditional* type (right after
    // the call, with no narrowing) is a `Union` with the empty (no-match)
    // shape - reading `$matches[1]` there collapses to `Mixed` (traced
    // through in the plan itself: the empty-shape member's own missing-
    // key fallback is `Mixed`, and `union_types` unconditionally absorbs
    // into it), so the test above alone no longer discriminates "typed as
    // a real `string`" from "leniently accepted because it's `Mixed`."
    // This one narrows first (Finding E's own `preg_match_condition`
    // recognizer), proving `$matches[1]` really is a precise `string` in
    // the one context that matters in practice - passing something
    // genuinely incompatible (an array) would be flagged if it weren't.
    #[test]
    fn preg_match_required_group_is_precisely_typed_once_the_match_is_confirmed() {
        let d = diagnostics_for(
            r#"<?php
            function needsArray(array $a): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?/', $subject, $matches)) {
                    needsArray($matches[1]);
                }
            }
        "#,
        );
        // Phase 12 (literal enumeration): group 1's own content is the
        // constant `a`, so it's now typed as the literal `'a'`, not
        // generic `string` - even more precise than this test's own
        // original intent (proving precise typing at all, not just
        // Mixed-leniency), so the check just needs to name the sharper
        // type.
        assert!(
            d.iter()
                .any(|x| x.message.contains("needsArray") && x.message.contains("'a'")),
            "expected an argument-type mismatch naming 'a', got: {d:?}"
        );
    }

    #[test]
    fn preg_match_reading_a_genuinely_absent_group_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches);
                echo $matches[5];
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("no key '5'")),
            "expected a 'no such key' diagnostic, got: {d:?}"
        );
    }

    // A "reads $matches[1]/[5] in the impossible branch" version of this
    // test wouldn't actually discriminate whether narrowing happened:
    // `infer_expr`'s `Index` arm falls back to `Mixed` for `Type::Never`
    // just as readily as for a real `ArrayShape`, so no diagnostic either
    // way. Passing `$matches` itself to a parameter requiring an
    // incompatible type is the real, discriminating test - `Never` is
    // vacuously assignable to anything (so no diagnostic proves the
    // branch really was narrowed to it), while an ordinary `ArrayShape`
    // is not (so a diagnostic there proves the optional case correctly
    // *isn't* over-optimistically narrowed).
    #[test]
    fn preg_match_array_key_exists_false_branch_narrows_a_required_group_to_never() {
        // Nested inside a truthy `preg_match(...)` check (Finding E's own
        // `preg_match_condition` narrowing) rather than tested in
        // isolation: since Finding E, $matches's own *unconditional* type
        // right after the call is a `Union` with the empty (no-match)
        // shape, and array_key_exists's own false-branch narrowing
        // correctly leaves that empty-shape Union member untouched (it's
        // entirely consistent with key '1' being absent) - so, without
        // first narrowing away the no-match possibility, the overall
        // result is genuinely `array{}`, not `Never`. The precise,
        // non-Union full shape (and therefore a real `Never` narrowing
        // for a required key) only exists once the match is confirmed.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?/', $subject, $matches)) {
                    if (! array_key_exists('1', $matches)) {
                        needsInt($matches);
                    }
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_array_key_exists_false_branch_does_not_narrow_an_optional_group() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches);
                if (! array_key_exists('2', $matches)) {
                    needsInt($matches);
                }
            }
        "#,
        );
        assert!(
            !d.is_empty(),
            "expected a diagnostic proving the optional group wasn't narrowed to Never, got none"
        );
    }

    // --- Task 1c: trailing-vs-interior optional field presence/value ---

    #[test]
    fn preg_match_an_interior_optional_group_is_always_present_not_optional() {
        // Group 2 is optional (via `?`) but *interior* - group 3, after
        // it, is required, so real PCRE can never omit $matches[2]
        // entirely (confirmed live: `preg_match('/(a)(b)?(c)/', 'ac', $m)`
        // gives `$m = [0=>'ac', 1=>'a', 2=>'', 3=>'c']`). Using the same
        // established technique as `preg_match_array_key_exists_false_
        // branch_narrows_a_required_group_to_never` above: if the false
        // branch of `array_key_exists('2', ...)` narrows `$matches` to
        // `Never` (vacuously assignable to anything), the field is
        // correctly modeled as always-present.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?(c)/', $subject, $matches)) {
                    if (! array_key_exists('2', $matches)) {
                        needsInt($matches);
                    }
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_a_trailing_optional_group_keeps_its_own_optional_field() {
        // Same shape as the interior case above, but group 2 here really
        // is the pattern's own last group (trailing) - today's existing,
        // unaffected-by-Task-1c behavior: the field can genuinely be
        // absent, so `array_key_exists`'s false branch must *not* narrow
        // `$matches` to `Never`.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?/', $subject, $matches)) {
                    if (! array_key_exists('2', $matches)) {
                        needsInt($matches);
                    }
                }
            }
        "#,
        );
        assert!(
            !d.is_empty(),
            "expected a diagnostic proving the trailing optional group wasn't narrowed to Never, got none"
        );
    }

    #[test]
    fn preg_match_named_group_is_readable_by_number_and_by_name() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match('/(?<year>\d+)/', $subject, $matches);
                needsString($matches[1]);
                needsString($matches['year']);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // See the identical reasoning on
    // `preg_match_required_group_is_precisely_typed_once_the_match_is_confirmed`
    // above - narrows first, so this actually proves precise typing by
    // both number and name, not just Mixed-leniency.
    #[test]
    fn preg_match_named_group_is_precisely_typed_by_number_and_name_once_confirmed() {
        let d = diagnostics_for(
            r#"<?php
            function needsArray(array $a): void {}
            function f($subject): void {
                if (preg_match('/(?<year>\d+)/', $subject, $matches)) {
                    needsArray($matches[1]);
                    needsArray($matches['year']);
                }
            }
        "#,
        );
        assert_eq!(
            d.iter()
                .filter(|x| x.message.contains("needsArray") && x.message.contains("string"))
                .count(),
            2,
            "expected two argument-type mismatches naming string, got: {d:?}"
        );
    }

    // --- Phase 12: literal enumeration for constant-only groups ---

    #[test]
    fn preg_match_a_constant_alternation_group_is_typed_as_a_literal_union() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(cat|dog)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("needsInt") && x.message.contains("'cat'|'dog'")),
            "expected an argument-type mismatch naming 'cat'|'dog', got: {d:?}"
        );
    }

    // `preg_match` itself can't be used to test this composition:
    // reading through a flagged call's `$matches` always collapses to
    // `Mixed` (Finding E's own narrowing is scoped to no-flags callers
    // only, so a flagged call's `$matches` stays the unconditional
    // `Union` even inside a confirmed-matched `if` - see
    // `preg_match_offset_capture_types_each_entry_as_a_string_int_tuple`'s
    // own comment above). `preg_match_all` never has that Union at all
    // (Finding D), so it's the only way to observe the composed tuple
    // shape directly.
    #[test]
    fn preg_match_all_a_constant_group_combined_with_offset_capture_only_narrows_the_text_slot() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match_all('/(cat)/', $subject, $matches, PREG_OFFSET_CAPTURE);
                needsInt($matches[1][0][0]);
                needsInt($matches[1][0][1]);
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected exactly one mismatch (the text slot only), got: {d:?}"
        );
        assert!(
            d[0].message.contains("needsInt") && d[0].message.contains("'cat'"),
            "expected the text slot to be flagged as 'cat', got: {d:?}"
        );
    }

    #[test]
    fn preg_match_a_non_constant_group_stays_plain_string() {
        // `\d+` is neither a literal sequence nor a top-level alternation -
        // stays today's plain `string`, not a wrong or guessed literal.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(\d+)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("needsInt") && x.message.ends_with("string given")),
            "expected a plain string mismatch, got: {d:?}"
        );
    }

    // --- Task 1b: tagged unions ---
    //
    // Reading `$matches[N]` directly from the tagged `Union` collapses to
    // `Mixed` (the "without" variant genuinely lacks key N, and a `Union`
    // read already correctly falls back to `Mixed` whenever any member
    // lacks the key - established, correct behavior, not something to
    // work around). `array_key_exists('N', $matches)` narrowing
    // (`narrow_array_shape_by_key_presence`) is what actually drops the
    // non-matching member first, leaving a plain (non-`Union`) shape
    // behind - the same mechanism these tests use to observe the tagged
    // union's own internal shape precisely.

    #[test]
    fn preg_match_single_optional_group_with_variant_keeps_the_nested_descendant() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a(b))?/', $subject, $matches)) {
                    if (array_key_exists('1', $matches)) {
                        needsInt($matches[1]);
                        needsInt($matches[2]);
                    }
                }
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("needsInt") && x.message.contains("'ab'")),
            "expected the with-variant's group 1 to be 'ab', got: {d:?}"
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("needsInt") && x.message.contains("'b'")),
            "expected the with-variant's nested group 2 to be 'b', got: {d:?}"
        );
    }

    #[test]
    fn preg_match_single_optional_group_without_variant_is_bare() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                if (preg_match('/(a(b))?/', $subject, $matches)) {
                    if (!array_key_exists('1', $matches)) {
                        echo $matches[1];
                        echo $matches[2];
                    }
                }
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("no key '1'")),
            "expected the without-variant to lack key 1, got: {d:?}"
        );
        assert!(
            d.iter().any(|x| x.message.contains("no key '2'")),
            "expected the without-variant to also lack the nested descendant's own key 2, got: {d:?}"
        );
    }

    #[test]
    fn preg_match_two_optional_top_level_groups_stay_on_the_flat_path() {
        // Two top-level groups fails the "exactly one" gate - stays flat
        // (an `optional` field), not tagged. `array_key_exists` on a
        // *flat* shape's own optional field narrows without ever
        // touching a `Union` at all, so a diagnostic here would only
        // appear if this pattern were wrongly treated as a single
        // optional group.
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match('/(a)?(b)?/', $subject, $matches);
                echo $matches[1];
                echo $matches[2];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_a_top_level_alternation_forces_earlier_siblings_and_removes_later_ones() {
        // `array_key_exists('2', ...)` alone isn't enough to isolate the
        // "dog" branch specifically: field 2 is *also* present (forced
        // `''`) in the "bird" branch's own variant, as an *earlier*
        // sibling of group 3 - real, confirmed behavior, not a test bug
        // to work around by accident. Combined with `!array_key_exists
        // ('3', ...)` (true only for "cat" and "dog", since "bird" is
        // the only variant with field 3 present), the pair together
        // isolates "dog" specifically.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(cat)|(dog)|(bird)/', $subject, $matches)) {
                    if (array_key_exists('2', $matches) && !array_key_exists('3', $matches)) {
                        needsInt($matches[1]); // earlier sibling - forced ''
                        needsInt($matches[2]); // this branch's own group
                        echo $matches[3]; // later sibling - removed entirely
                    }
                }
            }
        "#,
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("needsInt") && x.message.contains("''")),
            "expected the earlier sibling to be forced to '', got: {d:?}"
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("needsInt") && x.message.contains("'dog'")),
            "expected this branch's own group to be 'dog', got: {d:?}"
        );
        assert!(
            d.iter().any(|x| x.message.contains("no key '3'")),
            "expected the later sibling to be entirely removed, got: {d:?}"
        );
    }

    #[test]
    fn preg_match_a_zero_group_alternation_branch_adds_the_bare_variant() {
        // 2 top-level groups rules out the single-optional-group
        // dispatch; the alternation has 3 branches but only 2 groups
        // (`x` has none) - matching it produces $matches with only
        // field "0", a real possibility this test confirms is present
        // by narrowing array_key_exists('1', ...) to false and checking
        // the surviving shape is genuinely bare.
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                if (preg_match('/(cat)|(dog)|x/', $subject, $matches)) {
                    if (!array_key_exists('1', $matches) && !array_key_exists('2', $matches)) {
                        echo $matches[1];
                    }
                }
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("no key '1'")),
            "expected the bare (zero-group-branch) variant to lack key 1, got: {d:?}"
        );
    }

    #[test]
    fn preg_match_an_alternation_wrapped_in_an_optional_quantifier_still_adds_the_bare_variant() {
        // Every branch already has a group (no branch-count mismatch) -
        // the bare variant here can only come from
        // `alternation_wrapped_in_optional_quantifier`.
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                if (preg_match('/(?:(cat)|(dog))?/', $subject, $matches)) {
                    if (!array_key_exists('1', $matches) && !array_key_exists('2', $matches)) {
                        echo $matches[1];
                    }
                }
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("no key '1'")),
            "expected the bare (whole-alternation-optional) variant to lack key 1, got: {d:?}"
        );
    }

    #[test]
    fn preg_match_unmatched_as_null_stays_flat_for_the_single_optional_group_case() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match('/(a(b))?/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                echo $matches[1];
                echo $matches[2];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_unmatched_as_null_stays_flat_for_the_alternation_case() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match('/(cat)|(dog)/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                echo $matches[1];
                echo $matches[2];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_with_a_dynamic_pattern_stays_mixed() {
        let d = diagnostics_for(
            r#"<?php
            function f($pattern, $subject): void {
                preg_match($pattern, $subject, $matches);
                echo $matches[99];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_with_an_unrecognized_flags_expression_stays_mixed() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match('/(a)/', $subject, $matches, SOME_UNDEFINED_FLAG);
                echo $matches[99];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 12 Finding B: PREG_OFFSET_CAPTURE - confirmed against a real
    // PHP interpreter that every present entry becomes a real [string,
    // int] tuple (the match text and its byte offset), not a bare
    // string. Passes leniently either way - Finding E's own
    // `preg_match_condition` narrowing is deliberately scoped to
    // no-`$flags` callers only, so a flagged call's `$matches` stays a
    // `Union` even inside a confirmed-matched `if`, and reading through
    // it collapses to `Mixed` (this project's usual "don't know, don't
    // flag" leniency) rather than the real tuple shape. The genuinely
    // discriminating check for a flagged call is field *presence*
    // through the `Union` (Part 1's own real value), covered by the
    // dedicated test just below.
    #[test]
    fn preg_match_offset_capture_types_each_entry_as_a_string_int_tuple() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match('/(a)/', $subject, $matches, PREG_OFFSET_CAPTURE);
                needsString($matches[1][0]);
                needsInt($matches[1][1]);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_offset_capture_still_flags_a_genuinely_absent_group_through_the_union() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match('/(a)/', $subject, $matches, PREG_OFFSET_CAPTURE);
                echo $matches[99];
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("no key '99'")),
            "expected a 'no such key' diagnostic even through the Union, got: {d:?}"
        );
    }

    // Phase 12 Finding C: PREG_UNMATCHED_AS_NULL - confirmed against a
    // real PHP interpreter that a non-participating optional group
    // becomes `null` in the array rather than an entirely absent key.
    // Both tests below now pass leniently either way, the same reason as
    // `preg_match_offset_capture_types_each_entry_as_a_string_int_tuple`
    // just above: Finding E's own narrowing is scoped to no-`$flags`
    // callers, so a `PREG_UNMATCHED_AS_NULL`-flagged call's `$matches`
    // stays a `Union` with no narrowed, precise-typed context to read it
    // from - reading through it collapses to `Mixed` regardless of the
    // real nullable-vs-plain distinction this shape-building logic
    // actually produces. **A real, disclosed test gap, not a false
    // claim of coverage elsewhere**: `preg_match_matches_shape` itself
    // (`checker/expr.rs`) has no dedicated unit test of its own - this
    // project's established convention for this family of per-function
    // shape helpers (see `curl_getinfo_component_type`'s identical lack
    // of one) is to verify them only through `diagnostics_for`-based
    // integration tests like these, which is exactly what Finding E's
    // own narrowing scope now prevents from discriminating this one
    // specific distinction. Left open rather than papered over with a
    // test that would pass without truly exercising anything.
    #[test]
    fn preg_match_unmatched_as_null_makes_an_optional_group_nullable_but_always_present() {
        let d = diagnostics_for(
            r#"<?php
            function needsNullableString(?string $s): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                // Reading the field at all is fine (no "no such key") -
                // the field being present is still correctly preserved
                // through the Union.
                needsNullableString($matches[2]);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_unmatched_as_null_does_not_widen_a_required_group() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                needsString($matches[1]);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 12 Finding D: preg_match_all - both PREG_PATTERN_ORDER (the
    // real default) and PREG_SET_ORDER, both confirmed against a real
    // PHP interpreter before implementing.
    #[test]
    fn preg_match_all_pattern_order_types_each_group_as_an_array_of_strings() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches);
                needsInt($matches[1]);
            }
        "#,
        );
        // Phase 12 (literal enumeration): group 1's own content is the
        // constant `a`, so its per-match array element is now the
        // literal `'a'`, not generic `string`. Corpus round: each group's
        // own match array is itself always a real `list<T>` (verified via
        // dumpType()), not merely `array<T>`.
        assert!(
            d.iter()
                .any(|x| x.message.contains("needsInt") && x.message.contains("list<'a'>")),
            "expected an argument-type mismatch naming list<'a'>, got: {d:?}"
        );
    }

    #[test]
    fn preg_match_all_pattern_order_always_populates_every_key_never_optional() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches);
                // Group 2 is optional in a single match, but
                // PREG_PATTERN_ORDER always includes its own key
                // (possibly as an empty array) - reading it should never
                // be flagged as "no such key".
                echo count($matches[2]);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_all_set_order_reuses_the_single_match_shape_per_entry() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches, PREG_SET_ORDER);
                foreach ($matches as $oneMatch) {
                    needsString($oneMatch[1]);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_all_set_order_reading_a_genuinely_absent_group_is_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches, PREG_SET_ORDER);
                foreach ($matches as $oneMatch) {
                    echo $oneMatch[5];
                }
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("no key '5'")),
            "expected a 'no such key' diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn preg_match_all_set_order_composes_with_offset_capture() {
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match_all('/(a)/', $subject, $matches, PREG_SET_ORDER | PREG_OFFSET_CAPTURE);
                foreach ($matches as $oneMatch) {
                    needsInt($oneMatch[1][1]);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_all_with_a_pattern_this_parser_bails_on_stays_mixed() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match_all('/(?(1)a|b)/', $subject, $matches);
                echo $matches[99];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn preg_match_with_a_pattern_this_parser_bails_on_stays_mixed() {
        let d = diagnostics_for(
            r#"<?php
            function f($subject): void {
                preg_match('/(?(1)a|b)/', $subject, $matches);
                echo $matches[99];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn curl_getinfo_redirect_url_infers_string_or_false() {
        let d = diagnostics_for(
            r#"<?php
            function f($ch): string|false {
                return curl_getinfo($ch, CURLINFO_REDIRECT_URL);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn curl_getinfo_with_an_unrecognized_constant_keeps_todays_mixed() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            function f($ch): void {
                needsString(curl_getinfo($ch, CURLINFO_NOT_IN_THE_CURATED_TABLE));
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn curl_getinfo_with_no_option_argument_keeps_todays_mixed() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            function f($ch): void {
                needsString(curl_getinfo($ch));
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn json_encode_without_throw_on_error_keeps_todays_generic_union() {
        let d = diagnostics_for(
            r#"<?php
            function f($x): string {
                return json_encode($x);
            }
            function g($x): string {
                return json_encode($x, JSON_PRETTY_PRINT);
            }
        "#,
        );
        assert_eq!(
            d.len(),
            2,
            "expected the generic string|false union to still apply to both: {d:?}"
        );
    }

    // Phase 32 Finding K, real shape: `ImageDerivativeController::
    // parseRequest`'s `$pos = strrpos($req, '.'); $pos !== false ||
    // $this->ierror('Missing .', 400); $ext = substr($req, $pos);` -
    // `ierror(): never` is a real PHP `never` return type.
    #[test]
    fn a_bare_or_statement_with_a_never_returning_right_side_narrows_the_left() {
        let d = diagnostics_for(
            r#"<?php
            class C {
                function ierror(string $msg): never {
                    throw new Exception($msg);
                }
                function f(string $req): string {
                    $pos = strrpos($req, '.');
                    $pos !== false || $this->ierror('Missing .');
                    return substr($req, $pos);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_bare_or_statement_does_not_narrow_when_the_right_side_isnt_never() {
        let d = diagnostics_for(
            r#"<?php
            class C {
                function regular(): bool {
                    return true;
                }
                function f(string $req): string {
                    $pos = strrpos($req, '.');
                    $pos !== false || $this->regular();
                    return substr($req, $pos);
                }
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected $pos to stay unnarrowed (still int|false): {d:?}"
        );
    }

    // Phase 32 Finding L: exact type equality is the wrong tool for "is
    // this the kind of thing being narrowed" - both `narrow_type_to`/
    // `narrow_type_excluding` (member filtering) and `union_types`
    // (member dedup at construction time) used plain `==`/`!=`.

    #[test]
    fn union_types_collapses_a_string_literal_into_an_already_present_string() {
        assert_eq!(
            union_types(Type::String, Type::StringLiteral("".to_string())),
            Type::String
        );
    }

    #[test]
    fn union_types_collapses_an_int_literal_into_an_already_present_int() {
        assert_eq!(union_types(Type::Int, Type::IntLiteral(0)), Type::Int);
    }

    // The float-widening pitfall this fix deliberately avoids: `is_subtype
    // (IntLiteral(0), Float)` is `true` (PHP's int-to-float argument
    // coercion), but that's an *assignability* fact, not a "this value's
    // representation is already covered" one - an `IntLiteral` sitting
    // next to an unrelated `Float` member must not be silently dropped.
    #[test]
    fn union_types_does_not_collapse_an_int_literal_into_an_unrelated_float() {
        assert_eq!(
            union_types(Type::Float, Type::IntLiteral(0)),
            Type::Union(vec![Type::Float, Type::IntLiteral(0)])
        );
    }

    // Phase 14: the same absorption idea, for an accessory-refined
    // string/array and an IntRange - an unknown branch genuinely might
    // be empty/out of range, so the refined member can't survive next
    // to its own separately-present, unrefined base.
    #[test]
    fn union_types_collapses_a_non_empty_string_into_an_already_present_string() {
        let non_empty = Type::Intersection(vec![
            Type::String,
            Type::Accessory(types::AccessoryKind::NonEmptyString),
        ]);
        assert_eq!(union_types(Type::String, non_empty), Type::String);
    }

    #[test]
    fn union_types_collapses_a_list_into_an_already_present_array() {
        let list_ty = Type::Intersection(vec![
            Type::Array(Box::new(Type::Int)),
            Type::Accessory(types::AccessoryKind::ArrayList),
        ]);
        assert_eq!(
            union_types(Type::Array(Box::new(Type::Int)), list_ty),
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn union_types_collapses_an_int_range_into_an_already_present_int() {
        assert_eq!(
            union_types(Type::Int, Type::IntRange(Some(1), None)),
            Type::Int
        );
    }

    #[test]
    fn union_types_collapses_a_float_literal_into_an_already_present_float() {
        assert_eq!(
            union_types(Type::Float, Type::FloatLiteral(1.5)),
            Type::Float
        );
    }

    #[test]
    fn union_types_does_not_collapse_a_real_class_intersection() {
        // Guards Phase 14's own absorption fix: a genuine, unrelated
        // multi-class intersection (no Accessory member at all) must
        // never be treated as "more specific than its own base type" -
        // Countable&Iterator sitting next to bare Countable in the same
        // union are both real, independent possibilities.
        let intersection = Type::Intersection(vec![
            Type::Object("Countable".to_string(), Vec::new()),
            Type::Object("Iterator".to_string(), Vec::new()),
        ]);
        assert_eq!(
            union_types(
                Type::Object("Countable".to_string(), Vec::new()),
                intersection.clone()
            ),
            Type::Union(vec![
                Type::Object("Countable".to_string(), Vec::new()),
                intersection
            ])
        );
    }

    // Phase 12 (unplanned follow-on): a real hang, found via `gdb` on a
    // hung `check-project` process, not guessed - `merge_scopes` (one
    // `union_types` call per `if`/`else`) and `write_index_into`'s own
    // `Union` arm both feed into `union_types`, which had no cap on how
    // many distinct members it would accumulate. A real corpus function
    // with many sequential branches assigning genuinely distinct array
    // shapes to the same variable grew that variable's own union member
    // count combinatorially once `Type::Union` values stopped collapsing
    // to `Mixed` at the first read/write (Finding E's own `Union([matched,
    // empty])` unconditional `preg_match` shape). Simulates that same
    // shape directly: 20 pairwise-distinct one-field `ArrayShape`s folded
    // through `union_types` one at a time, exactly the access pattern
    // `merge_scopes` uses across many sequential branches.
    #[test]
    fn union_types_caps_runaway_member_growth_instead_of_hanging() {
        let result = (0..20)
            .map(|i| Type::ArrayShape(vec![(i.to_string(), Type::Int, false)]))
            .reduce(union_types)
            .unwrap();
        assert_eq!(
            result,
            Type::Mixed,
            "expected the cap to collapse a >12-member union to Mixed, got {result:?}"
        );
    }

    // The cap must not misfire on the common, small case real code
    // actually produces - a handful of genuinely distinct branch types
    // stays a precise `Union`, not `Mixed`.
    #[test]
    fn union_types_below_the_cap_still_builds_a_precise_union() {
        let result = [Type::Int, Type::String, Type::Bool, Type::Null]
            .into_iter()
            .reduce(union_types)
            .unwrap();
        assert_eq!(
            result,
            Type::Union(vec![Type::Int, Type::String, Type::Bool, Type::Null])
        );
    }

    // Task 2: array-shape generalization, tried before the hard
    // MAX_UNION_MEMBERS collapse - 13 shapes that all share the *same*
    // key set merge into one shape with unioned field types instead of
    // collapsing to `Mixed`. Tested directly against `generalize_by_key_
    // set` itself, not via sequential `union_types` calls: `.reduce()`
    // builds the union one pairwise call at a time, which triggers
    // generalization repeatedly at different *partial* states (and can
    // itself separately trip the per-field union's own `MAX_UNION_
    // MEMBERS` cap when the field values are too varied) - a real
    // property of how `union_types` is actually invoked, but not what
    // this test is trying to isolate, which is `generalize_by_key_set`'s
    // own one-shot merge behavior over an already-complete member list.
    #[test]
    fn generalize_by_key_set_merges_same_key_set_shapes_instead_of_collapsing() {
        // Only 2 distinct field values (alternating), so the per-field
        // union stays well under its own cap and the result is a clean,
        // precise `bool`, not `Mixed` - proving the merge actually folds
        // real field types together, not just picks one arbitrarily.
        let members: Vec<Type> = (0..13)
            .map(|i| {
                let value = if i % 2 == 0 { Type::True } else { Type::False };
                Type::ArrayShape(vec![("x".to_string(), value, false)])
            })
            .collect();
        let result = generalize_by_key_set(members);
        assert_eq!(
            result.len(),
            1,
            "expected every same-key-set shape to merge into one"
        );
        match &result[0] {
            Type::ArrayShape(fields) => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "x");
                assert!(!fields[0].2, "the merged field must not be optional");
                assert_eq!(
                    fields[0].1,
                    Type::Union(vec![Type::True, Type::False]),
                    "expected the alternating True/False values to fold into a real Union"
                );
            }
            other => panic!("expected a single merged ArrayShape, got {other:?}"),
        }
    }

    // A mix of same- and different-key-set shapes only merges the
    // matching subset - the different-key-set members pass through
    // untouched, exactly as many members as went in.
    #[test]
    fn generalize_by_key_set_only_merges_the_matching_subset() {
        let mut members: Vec<Type> = (0..13)
            .map(|i| {
                let value = if i % 2 == 0 { Type::True } else { Type::False };
                Type::ArrayShape(vec![("x".to_string(), value, false)])
            })
            .collect();
        members.extend((0..5).map(|i| Type::ArrayShape(vec![(format!("y{i}"), Type::Int, false)])));
        let result = generalize_by_key_set(members);
        // The 13 same-key-set ("x") shapes merge into 1; the 5
        // different-key-set ("y0".."y4") shapes each stay distinct and
        // untouched.
        assert_eq!(result.len(), 6, "expected 1 merged + 5 untouched members");
        let untouched_y_shapes = result
            .iter()
            .filter(|m| matches!(m, Type::ArrayShape(fields) if fields.len() == 1 && fields[0].0.starts_with('y')))
            .count();
        assert_eq!(
            untouched_y_shapes, 5,
            "expected all 5 y-shapes to pass through unmerged"
        );
    }

    // The two tests above exercise `generalize_by_key_set` directly;
    // this one confirms `union_types` itself actually calls it - built as
    // a single `union_types(pre_built_union_of_12, one_more_shape)` call
    // (not a `.reduce()` chain) specifically so only *one* generalization
    // pass happens, matching what `generalize_by_key_set`'s own
    // standalone tests already prove that pass does.
    #[test]
    fn union_types_itself_generalizes_before_collapsing_to_mixed() {
        let first_twelve = Type::Union(
            (0..12)
                .map(|i| {
                    let value = if i % 2 == 0 { Type::True } else { Type::False };
                    Type::ArrayShape(vec![("x".to_string(), value, false)])
                })
                .collect(),
        );
        // A genuinely new value, distinct from every one of the first 12
        // (which only alternate True/False) - `union_types`'s own
        // existing dedup (`!members.contains(&other)`) would otherwise
        // silently no-op adding a 13th member that already exists,
        // masking whether generalization ran at all.
        let thirteenth = Type::ArrayShape(vec![("x".to_string(), Type::Null, false)]);
        let result = union_types(first_twelve, thirteenth);
        match result {
            Type::ArrayShape(fields) => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "x");
            }
            other => panic!(
                "expected union_types to generalize down to one merged ArrayShape, got {other:?}"
            ),
        }
    }

    // Real shape: `Piwigo\Tag\TagService::getTagIds(string|array<string>
    // $rawTags, ...)`: `if (! is_array($rawTags)) { $rawTags =
    // explode(',', $rawTags); }` - `is_array()`'s registered target is
    // generic `Type::Array(Mixed)`; the specific-element `Array(String)`
    // member used to survive the exclusion untouched (`Array(String) !=
    // Array(Mixed)`), keeping the full `string|array<string>` type going
    // into `explode()`'s plain-`string` argument.
    #[test]
    fn is_array_false_branch_excludes_a_specific_element_array_member_too() {
        let d = diagnostics_for(
            r#"<?php
            /** @param string|array<string> $rawTags */
            function f($rawTags): int {
                if (! is_array($rawTags)) {
                    $rawTags = explode(',', $rawTags);
                }
                return count($rawTags);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Real shape: `Piwigo\Section\SectionInitializer::parse()`'s
    // `$rewritten = is_string($rewritten) ? $rewritten : '';` then
    // `str_replace('//', '/', $rewritten)` - the ternary's own
    // `union_types(String, StringLiteral(""))` used to keep both members,
    // so `$rewritten` never matched `str_replace`'s subject-narrowing
    // `== Type::String` check again.
    #[test]
    fn str_replace_narrows_a_subject_built_from_a_ternarys_string_and_string_literal_branches() {
        let d = diagnostics_for(
            r#"<?php
            function f($rewritten): string {
                $rewritten = is_string($rewritten) ? $rewritten : '';
                $rewritten = str_replace('//', '/', $rewritten);
                return strtoupper($rewritten);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // The false-branch side of Phase 28's `is_scalar` special case was
    // outright broken with no diagnostic in this corpus surfacing it:
    // `narrow_type_excluding(current, is_scalar's 4-member target)`
    // filtered via `*m != excluded`, and no real member is ever
    // structurally equal to the whole 4-member union, so `!is_scalar($x)`
    // never excluded anything, silently.
    #[test]
    fn is_scalar_false_branch_actually_excludes_the_scalar_members() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int|array $x */
            function f($x): array {
                if (! is_scalar($x)) {
                    return $x;
                }
                return [];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // `apply_phpunit_assertion_narrowing` (Phase 27) shares the exact same
    // bug as `is_array()` above, found via this finding's own caller
    // audit: `assertIsArray($x)` with `$x: array<string>|int` used to
    // fall back to the imprecise generic `array` for the same reason.
    #[test]
    fn assert_is_array_keeps_a_specific_element_type_not_the_generic_array() {
        let d = diagnostics_for(
            r#"<?php
            class T {
                /** @param array<string>|int $x */
                function f($x): array {
                    self::assertIsArray($x);
                    return $x;
                }
                static function assertIsArray($x): void {}
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
    fn stdclass_dynamic_property_reads_are_never_flagged_as_undefined() {
        // Phase 34 Finding D: real shape, FilesystemHelperTest/LangTest -
        // a Pest test double capturing an error message via a dynamic
        // property on a plain `new stdClass()`.
        let d = diagnostics_for(
            r#"<?php
            function f(): string {
                $capture = new stdClass();
                $capture->lastMessage = 'hi';
                return $capture->lastMessage;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn stdclass_undefined_method_calls_are_still_flagged() {
        // The control case proving Finding D's exemption is property-
        // access-only, not a blanket carve-out - an undefined *method*
        // call on stdClass is a real fatal error real PHPStan still
        // flags too.
        let d = diagnostics_for(
            r#"<?php
            function f(): void {
                $c = new stdClass();
                $c->someMethod();
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("someMethod"), "got: {d:?}");
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

    // Phase 28, real shape: `proc_open()`/`fsockopen()` return
    // `resource|false`, and this corpus's test helpers guard every one of
    // them with `if (! is_resource($x)) { throw ...; }` before
    // `fclose`/`fwrite`/`fread`/`proc_close`/etc. - `is_resource` was
    // simply missing from `narrowing_type_for_predicate`'s table.
    #[test]
    fn is_resource_narrowing_excludes_false() {
        let d = diagnostics_for(
            r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $proc = open();
                if (! is_resource($proc)) {
                    throw new Exception('failed');
                }
                fclose($proc);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn is_scalar_narrows_to_the_four_scalar_types() {
        // Unlike is_numeric (deliberately not narrowed at all - real
        // PHPStan's own int|float|numeric-string result has no
        // representation here), is_scalar's real set is exactly
        // int|float|string|bool, with no such approximation gap.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function process(mixed $value) {
                if (is_scalar($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("needsInt"), "got: {d:?}");
    }

    // Phase 13 Finding B: real shape from
    // `SectionPopulator::computeMetaRobots`'s own
    // `is_countable($page_tag_ids) and count($page_tag_ids) > 1` -
    // `count()` on anything but `array|Countable` is a real PHP 8+
    // `TypeError`, and `is_countable` wasn't in
    // `narrowing_type_for_predicate` at all.
    #[test]
    fn is_countable_narrows_a_possibly_null_value_before_count() {
        let d = diagnostics_for(
            r#"<?php
            function f(): int {
                $tags = null;
                if (is_countable($tags) and count($tags) > 1) {
                    return count($tags);
                }
                return 0;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn is_countable_false_branch_excludes_array_and_countable() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int|array<int> $x */
            function f($x): int {
                if (! is_countable($x)) {
                    return $x;
                }
                return count($x);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 14 Task 6: narrowing sources for the new refinement types,
    // each verified against real PHPStan's own dumpType().
    #[test]
    fn not_equal_empty_string_narrows_to_non_empty_string() {
        let d = diagnostics_for(
            r#"<?php
            function needsNonEmpty(): void {}
            /** @param non-empty-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if ($x !== '') {
                    sink($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // A round of Piwigo-corpus bucketing found `$x !== []` (the array
    // equivalent of the string idiom just above) was never handled at
    // all - `is_known_single_value` didn't recognize an empty array
    // literal as exhaustively single-valued the way it already does a
    // literal string/int - verified against real PHPStan's own
    // dumpType() for both a plain array and an already-list-typed value.
    #[test]
    fn not_equal_empty_array_narrows_to_non_empty_array() {
        let d = diagnostics_for(
            r#"<?php
            /** @param non-empty-array<int> $x */
            function sink(array $x): void {}
            /** @param array<int> $x */
            function f(array $x): void {
                if ($x !== []) {
                    sink($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn not_equal_empty_array_narrows_an_already_typed_list_to_non_empty() {
        let d = diagnostics_for(
            r#"<?php
            /** @param non-empty-list<int> $x */
            function sink(array $x): void {}
            /** @param list<int> $x */
            function f(array $x): void {
                if ($x !== []) {
                    sink($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn not_empty_on_a_string_narrows_to_non_falsy_string_not_merely_non_empty() {
        // empty() is also false for '0' - real PHPStan narrows !empty($x)
        // to non-falsy-string, not just non-empty-string.
        let d = diagnostics_for(
            r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!empty($x)) {
                    sink($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn not_empty_on_an_array_narrows_to_non_empty_array() {
        let d = diagnostics_for(
            r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (!empty($x)) {
                    sink($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn count_greater_than_zero_narrows_to_non_empty_array() {
        let d = diagnostics_for(
            r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (count($x) > 0) {
                    sink($x);
                }
            }
            function g(array $x): void {
                if (count($x) >= 1) {
                    sink($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn is_numeric_narrows_a_plain_string_to_numeric_string() {
        let d = diagnostics_for(
            r#"<?php
            /** @param numeric-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (is_numeric($x)) {
                    sink($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn is_numeric_false_branch_leaves_a_string_unchanged() {
        let d = diagnostics_for(
            r#"<?php
            /** @param numeric-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!is_numeric($x)) {
                    return;
                }
                sink($x);
            }
            function g(string $x): void {
                if (is_numeric($x)) {
                    return;
                }
                sink($x);
            }
        "#,
        );
        // g() must still be flagged: the false branch is plain string,
        // not numeric-string.
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn a_literal_argument_satisfies_a_literal_string_parameter_a_plain_string_does_not() {
        let d = diagnostics_for(
            r#"<?php
            /** @param literal-string $x */
            function sink($x): void {}
            function f(): void {
                sink("hello");
            }
            function g(string $x): void {
                sink($x);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    // Phase 14 Task 7: the array-destructuring regression risk gets its
    // own direct, dedicated coverage - the live repro this round's own
    // review found broken (foreach over a list<T>-typed variable
    // silently inferring Mixed) and its siblings.
    #[test]
    fn foreach_over_a_list_typed_variable_still_infers_the_real_element_type() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $x */
            function f($x): void {
                foreach ($x as $item) {
                    needsInt($item);
                }
            }
            function needsInt(int $x): int { return $x; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn adding_two_list_typed_arrays_still_uses_array_union_not_numeric_addition() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $a
             *  @param list<int> $b
             *  @return array<int>
             */
            function f($a, $b): array {
                return $a + $b;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_element_type_callers_still_work_given_a_list_typed_argument() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $x */
            function f($x): void {
                foreach (array_map(fn($v) => $v + 1, $x) as $v) {
                    needsInt($v);
                }
                foreach (array_chunk($x, 2) as $chunk) {
                    foreach ($chunk as $v) {
                        needsInt($v);
                    }
                }
                needsInt(array_reduce($x, fn($carry, $v) => $carry + $v, 0));
            }
            function needsInt(int $x): int { return $x; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn sprintf_percent_d_is_still_flagged_for_a_list_typed_argument() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $x */
            function f($x): void {
                sprintf('%d', $x);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("list<int>"), "got: {d:?}");
    }

    #[test]
    fn a_by_ref_call_with_a_list_typed_declared_parameter_still_widens_precisely() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $x */
            function mutates(array &$x): void {}
            function f(): void {
                $items = [];
                mutates($items);
                foreach ($items as $item) {
                    needsInt($item);
                }
            }
            function needsInt(int $x): int { return $x; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // A round of Piwigo-corpus bucketing found the generic by-ref-widening
    // rule above discarded real precision for the sort() family - each
    // fell into the generic "trust the declared param type" widening,
    // which is only the plain `array &$array` stub for every one of
    // these. Verified against real PHPStan's own dumpType(): sort/rsort/
    // usort/shuffle always reindex to a real list; asort/arsort/ksort/
    // krsort/uasort/uksort/natsort/natcasesort preserve the exact key
    // structure unchanged.
    #[test]
    fn sort_reindexes_an_already_typed_array_to_a_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array<string, int> $x
             *  @return list<int>
             */
            function f(array $x): array {
                sort($x);
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn usort_and_shuffle_also_reindex_to_a_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array<string, int> $a
             *  @return list<int>
             */
            function f(array $a): array {
                usort($a, static fn (int $x, int $y): int => $x <=> $y);
                return $a;
            }
            /** @param array<string, int> $b
             *  @return list<int>
             */
            function g(array $b): array {
                shuffle($b);
                return $b;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn asort_and_ksort_preserve_the_exact_type_unchanged() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{a: int, b: string} $x
             *  @return array{a: int, b: string}
             */
            function f(array $x): array {
                asort($x);
                return $x;
            }
            /** @param array{a: int, b: string} $y
             *  @return array{a: int, b: string}
             */
            function g(array $y): array {
                ksort($y);
                return $y;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_values_always_narrows_to_a_list_even_from_a_non_list_input() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array<string, int> $x
             *  @return list<int>
             */
            function f($x): array {
                return array_values($x);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_filter_array_combine_and_array_unique_no_longer_claim_the_result_is_still_a_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $x
             *  @return list<int>
             */
            function usingFilter($x): array {
                return array_filter($x);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function usingCombine($x): array {
                return array_combine($x, $x);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function usingUnique($x): array {
                return array_unique($x);
            }
        "#,
        );
        assert_eq!(d.len(), 3, "got: {d:?}");
    }

    // A round of Piwigo-corpus bucketing found real PHPStan's own
    // array_filter() extension recognizes an explicit null-check callback
    // (`fn ($v) => $v !== null`) and strips Null from the retained
    // element type, not just its usual accessory-marker stripping -
    // verified via dumpType(). Real Piwigo code (GalleryController)
    // builds `$page_items` exactly this way.
    #[test]
    fn array_filter_with_an_explicit_null_check_arrow_fn_strips_null_from_the_element_type() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array<int|null> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, static fn (?int $v): bool => $v !== null);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_filter_with_an_explicit_null_check_closure_strips_null_from_the_element_type() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array<int|null> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, static function (?int $v): bool {
                    return $v !== null;
                });
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_filter_with_an_unrelated_callback_does_not_strip_null() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array<int|null> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, static fn (?int $v): bool => $v !== 0);
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected null to still survive an unrelated filter: {d:?}"
        );
    }

    #[test]
    fn array_slice_and_array_reverse_preserve_list_ness_only_with_default_preserve_keys() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $x
             *  @return list<int>
             */
            function sliceDefault($x): array {
                return array_slice($x, 1);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function reverseDefault($x): array {
                return array_reverse($x);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function slicePreserveKeys($x): array {
                return array_slice($x, 1, null, true);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].location.contains("slicePreserveKeys"), "got: {d:?}");
    }

    // A new round of Piwigo-corpus bucketing surfaced `infer_array_literal_type`
    // giving up entirely (falling to bare `Array(Mixed)`) on any array
    // literal containing a spread element - the real, common shape being
    // `[...findA(), ...findB()]`, combining several `list<T>`-returning
    // helpers into one. Verified against real PHPStan's own dumpType():
    // spreading two `list<T>`s stays a `list<T>`; spreading a `list<T>`
    // alongside a plain `array<T>` degrades to plain `array<T>` (not a
    // list); a leading literal element makes the whole thing non-empty.
    #[test]
    fn spreading_two_lists_into_an_array_literal_still_produces_a_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $a
             *  @param list<int> $b
             *  @return list<int>
             */
            function f($a, $b): array {
                return [...$a, ...$b];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn spreading_a_list_and_a_plain_array_degrades_to_a_plain_array_not_a_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $a
             *  @param array<int> $b
             *  @return list<int>
             */
            function f($a, $b): array {
                return [...$a, ...$b];
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected the list-ness mismatch to still be flagged: {d:?}"
        );
    }

    // Found by the corpus recheck after the fix above landed: real Piwigo
    // code (`UserInfoWithThemeName::toArray()`) spreads another object's
    // own precisely-known `toArray()` shape and adds one more field -
    // `[...$this->userInfo->toArray(), 'theme_name' => $x]`. Verified
    // against real PHPStan's own dumpType(): spreading a shape with only
    // non-canonical (string) keys merges those keys in verbatim, keeping
    // the whole literal a precise shape rather than degrading to a
    // generic array.
    #[test]
    fn spreading_a_known_shape_and_adding_a_field_still_produces_a_precise_shape() {
        let d = diagnostics_for(
            r#"<?php
            /** @return array{a: int, b: string} */
            function inner(): array {
                return ['a' => 1, 'b' => 'x'];
            }
            /** @return array{a: int, b: string, c: bool} */
            function outer(): array {
                return [...inner(), 'c' => true];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // The same corpus recheck's largest remaining bucket: array_merge()
    // combining several list<T>-typed arrays, real Piwigo code
    // (`array_merge(['a', 'b'], someListReturningCall())`). Verified
    // against real PHPStan's own dumpType(): merging only list-like
    // arguments stays a list; merging in even one plain array<T> degrades
    // list-ness for the whole result but keeps the element-type union.
    #[test]
    fn array_merge_of_two_lists_stays_a_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<string> $a
             *  @param list<string> $b
             *  @return list<string>
             */
            function f($a, $b): array {
                return array_merge($a, $b);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_merge_of_a_list_and_a_plain_array_degrades_to_a_plain_array_not_a_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<string> $a
             *  @param array<string> $b
             *  @return list<string>
             */
            function f($a, $b): array {
                return array_merge($a, $b);
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected the list-ness mismatch to still be flagged: {d:?}"
        );
    }

    #[test]
    fn array_merge_with_a_literal_array_argument_produces_a_non_empty_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<string> $a
             *  @return non-empty-list<string>
             */
            function f($a): array {
                return array_merge(['x', 'y'], $a);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Found by a later round of corpus bucketing: array_merge(...$x) - a
    // spread *argument*, not a spread array-literal item - was treated as
    // if the whole array-of-arrays were one merge argument, giving
    // list<list<int>> instead of list<int>. Real Piwigo code
    // (SearchService::searchAllwords) flattens a word=>list<int> map's
    // own values this way. Verified via dumpType().
    #[test]
    fn array_merge_of_a_spread_argument_flattens_one_level() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array<string, list<int>> $byWord
             *  @return list<int>
             */
            function f(array $byWord): array {
                return array_merge(...array_values($byWord));
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Found by the corpus recheck after the fix above landed: real Piwigo
    // code (`Logger::__construct`) merges a property's own default
    // `array{...}` shape with a caller-supplied options shape via
    // array_merge(), then reassigns the property with the merged result -
    // `$this->options = array_merge($this->options, $options)`. Verified
    // against real PHPStan's own dumpType(): merging two known shapes
    // does the same key-renumbering/overwrite merge a spread array
    // literal does, producing a precise merged shape rather than
    // degrading to a generic array.
    #[test]
    fn array_merge_of_two_known_shapes_produces_a_precise_merged_shape() {
        let d = diagnostics_for(
            r#"<?php
            class C {
                /** @var array{a?: int, b?: string} */
                private array $opts = ['a' => 1, 'b' => 'x'];

                /** @param array{a?: int, b?: string} $x */
                public function __construct(array $x) {
                    $this->opts = array_merge($this->opts, $x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // A round of Piwigo-corpus bucketing found `array_element_type`/
    // `is_array_like`/`is_list_like`/`is_known_non_empty_array` had no
    // `Type::Union` arm at all - the single most common way a real
    // variable's type actually arrives here: a control-flow merge
    // (`$x = []; if (...) { foreach (...) { $x[] = ...; } }` leaves `$x`
    // a Union of the "0 iterations" `array{}` branch and the "≥1" real
    // list branch). Every one of `sort()`, `array_merge()`, `foreach`,
    // etc. silently stopped recognizing such a value as an array at all,
    // the same class of regression Phase 14's own Task 7 audit exists to
    // prevent - just for `Union` instead of `Intersection`.
    #[test]
    fn sort_still_reindexes_to_a_list_when_the_input_is_a_union_from_a_conditional_append() {
        let d = diagnostics_for(
            r#"<?php
            /** @return list<string> */
            function f(bool $flag): array {
                $x = [];
                if ($flag) {
                    foreach (['a', 'b'] as $s) {
                        $x[] = $s;
                    }
                }
                sort($x);
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_element_type_union_arm_does_not_collapse_to_mixed_via_an_empty_branch() {
        // The regression this fix could easily introduce: folding an
        // empty ArrayShape branch's own degenerate `Mixed` element type
        // into the union would collapse the *whole* merged element type
        // down to `Mixed`, discarding the real element type the
        // non-empty branch actually has.
        let d = diagnostics_for(
            r#"<?php
            /** @return list<int> */
            function f(bool $flag): array {
                $x = [];
                if ($flag) {
                    $x[] = 1;
                }
                sort($x);
                return $x;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_union_with_a_non_array_member_does_not_get_wrongly_upgraded_to_a_list() {
        // `array_element_type`'s new Union arm must require *every*
        // member to be array-like - a `list<int>|null` has no sensible
        // single "element type," so sort() falls back to its own
        // ordinary (imprecise, but safe) declared-parameter-type
        // widening instead of wrongly claiming the result is a list.
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int>|null $x
             *  @return list<int>
             */
            function f($x): array {
                sort($x);
                return $x;
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected the fallback array(mixed) to still mismatch list<int>: {d:?}"
        );
    }

    #[test]
    fn a_literal_element_before_a_spread_list_makes_the_result_a_non_empty_list() {
        let d = diagnostics_for(
            r#"<?php
            /** @param list<int> $a
             *  @return non-empty-list<int>
             */
            function f($a): array {
                return [1, ...$a];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 13 Finding C: verified against real PHPStan's own dumpType() -
    // `/` gives `int|float` unconditionally whenever it can't prove
    // `float` is involved, `float` whenever it can.
    #[test]
    fn division_of_two_ints_infers_int_or_float() {
        let d = diagnostics_for(
            r#"<?php
            function f(int $a, int $b): int|float {
                return $a / $b;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn division_involving_a_float_infers_plain_float() {
        let d = diagnostics_for(
            r#"<?php
            function f(int $a, float $b): float {
                return $a / $b;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn division_of_two_mixed_values_infers_int_or_float_not_mixed() {
        // Real PHPStan's own unconditional `int|float` applies even for
        // two fully unknown operands - a declared `int` return must
        // still be rejected (proving this isn't just falling back to the
        // old, universally-compatible Mixed).
        let d = diagnostics_for(
            r#"<?php
            function f($a, $b): int {
                return $a / $b;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
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
    fn instanceof_false_branch_drops_the_checked_class_from_a_union() {
        // Phase 33 Finding 1: BrowserTestHelpers::dbEscape's real shape.
        let d = diagnostics_for(
            r#"<?php
            class Mysqli {}
            class PgConnection {}
            function needsPgConnection(PgConnection $c): void {}
            function dbEscape(Mysqli|PgConnection $db): void {
                if ($db instanceof Mysqli) {
                    return;
                }
                needsPgConnection($db);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn instanceof_false_branch_of_the_exact_declared_class_is_dead_code() {
        let d = diagnostics_for(
            r#"<?php
            class Cat {}
            function needsInt(int $n): void {}
            function handle(Cat $c): void {
                if (! ($c instanceof Cat)) {
                    needsInt($c);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn instanceof_false_branch_against_an_unrelated_class_leaves_the_type_untouched() {
        let d = diagnostics_for(
            r#"<?php
            class Cat {}
            class Dog {}
            function needsCat(Cat $c): void {}
            function handle(Cat|Dog $animal): void {
                if ($animal instanceof Dog) {
                    return;
                }
                needsCat($animal);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn in_array_strict_narrows_the_true_branch_to_the_haystacks_literal_union() {
        // Phase 33 Finding 2: CatPermSubmitRequest::fromArray's real
        // shape - $post['status'] ?? null infers Type::Null (not Mixed),
        // so this exercises narrow_type_to's non-Union fallback too.
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param array<array-key, mixed> $post
             */
            function fromArray(array $post): string {
                $status_raw = $post['status'] ?? null;
                $status = in_array($status_raw, ['public', 'private'], true) ? $status_raw : '';
                return $status;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn in_array_without_the_strict_flag_narrows_nothing() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param array<array-key, mixed> $post
             */
            function fromArray(array $post): string {
                $status_raw = $post['status'] ?? null;
                $status = in_array($status_raw, ['public', 'private']) ? $status_raw : '';
                return $status;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("null"), "got: {d:?}");
    }

    #[test]
    fn in_array_strict_with_a_non_literal_haystack_element_narrows_nothing() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param array<array-key, mixed> $post
             */
            function fromArray(array $post, string $other): string {
                $status_raw = $post['status'] ?? null;
                $status = in_array($status_raw, [$other, 'public'], true) ? $status_raw : '';
                return $status;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("null"), "got: {d:?}");
    }

    #[test]
    fn array_key_exists_true_branch_keeps_only_shapes_declaring_the_key() {
        let d = diagnostics_for(
            r#"<?php
            function needsBValue(string $b): void {}
            /**
             * @param array{a: int} $withA
             * @param array{b: string} $withB
             */
            function handle(array $withA, array $withB, bool $cond): void {
                $x = $cond ? $withA : $withB;
                if (array_key_exists('b', $x)) {
                    needsBValue($x['b']);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_key_exists_false_branch_keeps_only_shapes_lacking_the_key() {
        let d = diagnostics_for(
            r#"<?php
            function needsAValue(int $a): void {}
            /**
             * @param array{a: int} $withA
             * @param array{b: string} $withB
             */
            function handle(array $withA, array $withB, bool $cond): void {
                $x = $cond ? $withA : $withB;
                if (! array_key_exists('b', $x)) {
                    needsAValue($x['a']);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn array_key_exists_on_a_merely_optional_key_narrows_neither_branch() {
        let shape = Type::ArrayShape(vec![("a".to_string(), Type::Int, true)]);
        assert_eq!(
            narrowing::narrow_array_shape_by_key_presence(&shape, "a", true),
            shape
        );
        assert_eq!(
            narrowing::narrow_array_shape_by_key_presence(&shape, "a", false),
            shape
        );
    }

    #[test]
    fn array_key_exists_on_a_plain_array_leaves_the_type_untouched() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param array<string, int> $x
             */
            function handle(array $x): void {
                if (array_key_exists('a', $x)) {
                    $y = $x;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn is_string_narrows_a_variable_subscript_array_index_expression() {
        // Phase 33 Finding 5: Translator::translate's real shape.
        let d = diagnostics_for(
            r#"<?php
            class Translator {
                /** @var array<string, string|array<int, string>> */
                private array $mirror = [];

                public function translate(string $key): string {
                    $val = $key;
                    if (is_string($this->mirror[$key])) {
                        $val = $this->mirror[$key];
                    }
                    return $val;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn reassigning_the_subscript_variable_invalidates_the_narrowed_entry() {
        // The soundness case this finding exists for. `is_int(...)`'s own
        // non-`Union` narrowing fallback (`narrow_type_to`'s `_ =>
        // target.clone()`) makes the *stale* entry a confidently-wrong
        // `int` rather than the correct, unprovable `mixed` a fresh read
        // would give - `needsString` (not `needsInt`) exposes the
        // difference: a surviving stale `int` would wrongly fail it,
        // while a correctly-purged `mixed` is leniently never flagged at
        // all (this project's own "don't guess" rule for `Mixed`
        // arguments), so the fix is confirmed by the diagnostic
        // *disappearing*, not by one appearing.
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr, string $key, string $other): void {
                if (is_int($arr[$key])) {
                    $key = $other;
                    needsString($arr[$key]);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn reassigning_an_intermediate_property_link_invalidates_a_deeper_narrowed_entry() {
        // Reassigning $foo->bar to a fresh literal that *keeps* key 'k'
        // (just with a different value type) sidesteps the unrelated
        // "array{...} has no key" check (`check_shape_key_access` runs
        // unconditionally, independent of narrowing) while still
        // exercising this fix directly: without it, the stale
        // `int`-narrowed `"foo->bar[k]"` entry would still win over the
        // real, freshly-reassigned `'newstring'`, wrongly reporting no
        // mismatch at all against `needsInt`.
        let d = diagnostics_for(
            r#"<?php
            class Foo {
                /** @var array<string, mixed> */
                public array $bar = [];
            }
            function needsInt(int $n): void {}
            function handle(Foo $foo): void {
                if (is_int($foo->bar['k'])) {
                    $foo->bar = ['k' => 'newstring'];
                    needsInt($foo->bar['k']);
                }
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("newstring"), "got: {d:?}");
    }

    #[test]
    fn a_by_ref_call_argument_invalidates_a_stale_narrowed_index_entry() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param array<string, mixed> $arr
             */
            function widenIt(array &$arr): void {}
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr): void {
                if (is_int($arr['k'])) {
                    widenIt($arr);
                    needsString($arr['k']);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn reassigning_an_unrelated_variable_does_not_purge_narrowing() {
        // The mirror-image control for the three tests above: since a
        // correctly-purged `mixed` is leniently never flagged, "no
        // diagnostic" alone wouldn't prove anything was preserved -
        // asserting the mismatch *still fires* is what proves the real
        // `int` narrowing survived the unrelated assignment intact.
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr, string $key): void {
                if (is_int($arr[$key])) {
                    $unrelated = 5;
                    needsString($arr[$key]);
                }
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("int"), "got: {d:?}");
    }

    #[test]
    fn or_of_same_key_predicates_narrows_the_true_branch_to_their_union() {
        // Phase 34 Finding B: CalendarMonthly::buildMonthCalendar's real
        // shape - $year infers Type::Null before this fix
        // (widen_coalesce_result's own fallback for a Mixed left side).
        let d = diagnostics_for(
            r#"<?php
            function needsIntOrString(int|string $x): void {}
            /**
             * @param array<string, mixed> $chronology_date
             */
            function handle(array $chronology_date): void {
                $year = $chronology_date['year'] ?? null;
                $year = is_int($year) || is_string($year) ? $year : 0;
                needsIntOrString($year);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_three_way_or_of_same_key_predicates_still_narrows() {
        let d = diagnostics_for(
            r#"<?php
            function needsIntOrStringOrBool(int|string|bool $x): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr): void {
                $x = $arr['x'] ?? null;
                $x = is_int($x) || is_string($x) || is_bool($x) ? $x : 0;
                needsIntOrStringOrBool($x);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn or_of_predicates_on_different_keys_narrows_neither() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr): void {
                $x = $arr['x'] ?? null;
                $y = $arr['y'] ?? null;
                $x = is_int($x) || is_string($y) ? $x : 0;
                needsString($x);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
    }

    #[test]
    fn a_backed_enum_s_value_satisfies_a_matching_literal_union_parameter() {
        // Phase 34 Finding C: Piwigo\Db\SortRenderer's real shape -
        // OrderByClause::__construct(string $column, 'ASC'|'DESC' $dir).
        let d = diagnostics_for(
            r#"<?php
            enum SortOrder: string {
                case Asc = 'ASC';
                case Desc = 'DESC';
            }
            class OrderByClause {
                /** @param 'ASC'|'DESC' $dir */
                public function __construct(string $column, string $dir) {}
            }
            function render(SortOrder $order): OrderByClause {
                return new OrderByClause('col', $order->value);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn pest_it_closure_this_is_not_flagged_as_undefined_variable() {
        // Real shape: tests/Browser/AboutControllerTest.php-style Pest
        // test files - `it('...', function (): void { ... $this ... })`
        // at true top-level file scope, no enclosing class/method. No
        // `uses()` call at all here, so `$this` resolves to the default
        // fallback (`Type::Mixed`, since `PHPUnit\Framework\TestCase`
        // isn't registered in this bare snippet) - the core fix under
        // test is that it's recognized as *defined* at all, not flagged
        // as an undefined variable the way it would be with no special
        // handling.
        let d = diagnostics_for(
            r#"<?php
            function it($description, $callback) {}
            it('does something', function (): void {
                $this->assertTrue(true);
            });
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn pest_file_local_uses_resolves_this_to_the_bound_class() {
        // Real Pest's own file-local `uses(X::class);` (no `->in(...)`) -
        // checked first, ahead of any directory-scoped Pest.php config.
        // Calling a real method on the bound class (not just tolerated
        // via Mixed) proves `$this` actually resolved to `RealTestBase`.
        let d = diagnostics_for(
            r#"<?php
            function uses($class) {}
            function it($description, $callback) {}
            class RealTestBase {
                public function realMethod(): int { return 1; }
            }
            uses(RealTestBase::class);
            it('does something', function (): int {
                return $this->realMethod();
            });
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn pest_file_local_uses_still_flags_a_real_undefined_method() {
        // Regression guard: Finding 1's fix must not blanket-suppress
        // every diagnostic inside a Pest closure - only the "undefined
        // variable $this" one. A genuinely undefined method call on the
        // bound class should still be flagged.
        let d = diagnostics_for(
            r#"<?php
            function uses($class) {}
            function it($description, $callback) {}
            class RealTestBase {
                public function realMethod(): int { return 1; }
            }
            uses(RealTestBase::class);
            it('does something', function (): int {
                return $this->totallyUndefinedMethod();
            });
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("undefined method")),
            "expected an undefined-method diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn pest_before_each_property_is_readable_from_another_closure_in_the_same_file() {
        let d = diagnostics_for(
            r#"<?php
            function uses($class) {}
            function beforeEach($callback) {}
            function it($description, $callback) {}
            class RealTestBase {}
            uses(RealTestBase::class);
            beforeEach(function (): void { $this->root = 'x'; });
            it('does something', function (): void {
                echo $this->root;
            });
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn pest_a_property_never_assigned_in_any_before_each_is_still_flagged() {
        let d = diagnostics_for(
            r#"<?php
            function uses($class) {}
            function beforeEach($callback) {}
            function it($description, $callback) {}
            class RealTestBase {}
            uses(RealTestBase::class);
            beforeEach(function (): void { $this->root = 'x'; });
            it('does something', function (): void {
                echo $this->totallyUndefinedProp;
            });
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("totallyUndefinedProp")),
            "expected an undefined-property diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn pest_before_each_property_assigned_inside_a_nested_if_is_not_recognized() {
        // Matches the real `PestHookPropertyReader`'s own narrower-than-
        // "anywhere in the closure" scan (top-level statements only) -
        // see `pest_config::collect_hook_closure_properties`'s own doc
        // comment. Deliberately still flagged, not a bug.
        let d = diagnostics_for(
            r#"<?php
            function uses($class) {}
            function beforeEach($callback) {}
            function it($description, $callback) {}
            class RealTestBase {}
            uses(RealTestBase::class);
            beforeEach(function (): void { if (true) { $this->root = 'x'; } });
            it('does something', function (): void {
                echo $this->root;
            });
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("$root")),
            "expected an undefined-property diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn generator_falling_off_the_end_with_no_return_is_not_a_null_mismatch() {
        // Real shape: vendor/composer-unused/symbol-parser's own
        // SymbolList::getIterator(): Generator { yield from
        // $this->items; } - a generator always returns a real Generator
        // object, regardless of whether it has an explicit `return`.
        let d = diagnostics_for(
            r#"<?php
            function gen(): Generator {
                yield 1;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn generator_with_a_bare_early_return_guard_is_not_a_null_mismatch() {
        // A bare `return;` inside a generator sets Generator::getReturn(),
        // not the function's own return value - a different call site
        // (check_stmt's StmtKind::Return(None) arm) than the
        // falls-off-the-end test above, and a common real idiom
        // (`if ($done) { return; } yield $x;`).
        let d = diagnostics_for(
            r#"<?php
            function gen(bool $done): Generator {
                if ($done) {
                    return;
                }
                yield 1;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn generator_closure_with_a_bare_early_return_guard_is_not_a_null_mismatch() {
        // Same idiom as above, but for a closure literal - check_block
        // never routes through check_function, so this needs its own
        // save/set/restore of current_function_is_generator.
        let d = diagnostics_for(
            r#"<?php
            $gen = function (bool $done): Generator {
                if ($done) {
                    return;
                }
                yield 1;
            };
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_non_generator_falling_off_the_end_is_still_a_real_null_mismatch() {
        // Regression guard: Finding 3's fix must not blanket-suppress the
        // real, pre-existing diagnostic for an ordinary (non-generator)
        // function that falls off the end with no return.
        let d = diagnostics_for(
            r#"<?php
            function notAGenerator(): int {
                $x = 1;
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("got null")),
            "expected a return type mismatch diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn assert_instanceof_narrows_for_the_rest_of_the_block() {
        // Real shape: vendor/pestphp/pest-plugin-mutate's own
        // AbstractFunctionCallUnwrapMutator::mutate() does exactly this -
        // `assert($node instanceof FuncCall); $node->args` - real PHPStan
        // documents support for narrowing on assert() the same as `if`.
        let d = diagnostics_for(
            r#"<?php
            class Node {}
            class FuncCall extends Node { public array $args = []; }
            function mutate(Node $node) {
                assert($node instanceof FuncCall);
                return $node->args;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn assert_without_a_matching_narrowing_shape_is_a_harmless_no_op() {
        // apply_narrowing already no-ops on condition shapes it doesn't
        // understand - this just confirms assert()'s own new call site
        // doesn't crash or misbehave for one (a plain boolean variable).
        let d = diagnostics_for(
            r#"<?php
            function check(bool $ok) {
                assert($ok);
                return $ok;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 27, real shape: real PHPUnit tests throughout this corpus
    // narrow via assertion calls instead of an `if`-guard - `$output =
    // ob_get_clean(); self::assertIsString($output);
    // self::assertStringContainsString(...);` - real PHPStan documents
    // support for exactly this via its own `phpstan/phpstan-phpunit`
    // extension, which this project doesn't depend on, so it's
    // replicated directly. Matches the same "statement narrows
    // unconditionally" shape `assert($cond)` already gets above.
    #[test]
    fn phpunit_assert_is_string_narrows_for_the_rest_of_the_block() {
        let d = diagnostics_for(
            r#"<?php
            class TestCase {
                public static function assertIsString($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): string {
                    $output = fetch();
                    self::assertIsString($output);
                    return $output;
                }
            }
            /** @return string|false */
            function fetch() {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn phpunit_assert_not_false_narrows_a_resource_for_the_rest_of_the_block() {
        let d = diagnostics_for(
            r#"<?php
            class TestCase {
                public static function assertNotFalse($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): void {
                    $handle = open();
                    self::assertNotFalse($handle);
                    fclose($handle);
                }
            }
            /** @return resource|false */
            function open() {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn phpunit_assert_not_null_narrows_via_an_instance_method_call_too() {
        // Same idiom via `$this->assertNotNull(...)` (PHPUnit's methods
        // are static, but PHP allows calling a static method through an
        // instance too, and real test code does this constantly).
        let d = diagnostics_for(
            r#"<?php
            class TestCase {
                public static function assertNotNull($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): string {
                    $value = fetch();
                    $this->assertNotNull($value);
                    return $value;
                }
            }
            /** @return ?string */
            function fetch() {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn phpunit_assert_instance_of_narrows_to_the_expected_class() {
        let d = diagnostics_for(
            r#"<?php
            class Node {}
            class FuncCall extends Node { public array $args = []; }
            class TestCase {
                public static function assertInstanceOf($expected, $actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(Node $node): array {
                    self::assertInstanceOf(FuncCall::class, $node);
                    return $node->args;
                }
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
    fn an_already_defined_variable_passed_by_ref_is_widened_not_left_stale() {
        // Phase 22 Finding 3, real shape: Piwigo\Admin\Install\
        // InstallService::installDbConnect(array &$infos, array &$errors,
        // ...) called as installDbConnect($infos, $errors, ...) where
        // $errors was already `$errors = [];` - the by-ref leniency above
        // only ever applied to an *undefined* variable
        // (`locals.contains_key(name)` skipped anything already tracked),
        // so `$errors` kept its stale, narrow `array{}` shape through the
        // call even though the callee is known to mutate it by reference,
        // and `$errors[0]` afterward was flagged as "no such key".
        let d = diagnostics_for(
            r#"<?php
            function fillErrors(array &$errors): void {}
            function test(): void {
                $errors = [];
                fillErrors($errors);
                echo $errors[0];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_by_ref_parameter_declared_wider_than_array_falls_back_to_mixed_not_the_full_union() {
        // Real corpus example that would otherwise regress:
        // Piwigo\Core\ArrayHelper::prependAppendArrayItems passes its own
        // already-array-typed `array $array` parameter into
        // `array_walk($array, ...)`, whose real phpstorm-stubs signature
        // is `array_walk(array|object &$array, ...)` - a union wider than
        // what an already-array-typed argument could ever actually
        // become (array_walk never turns an array into an object).
        // Blindly widening to the *full* declared type here regresses
        // real precision (the return type below would start looking
        // possibly-object); only a plain `Type::Array`/`Type::ArrayShape`
        // declared type is trusted, everything else falls back to the
        // same safe `Mixed` the undefined-variable case already uses.
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param array<int|string, mixed> $array
             * @return array<int|string, string>
             */
            function test(array $array): array {
                array_walk($array, function (mixed &$value): void {
                    $value = (string) $value;
                });
                return $array;
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
    fn anonymous_class_method_resolves_a_use_imported_return_type() {
        // Phase 22 Finding 5, real shape: a `use`-imported interface
        // return type on an anonymous class method - confirmed directly
        // from a real diagnostic's own wording before this fix: "expected
        // ResponseInterface, got Nyholm\Psr7\Response" (the expected side
        // printing bare/unqualified, proving it was never resolved
        // against the file's own `use` imports the way a top-level
        // class's method signature already is).
        let d = diagnostics_for(
            r#"<?php
            namespace App\Other;
            interface ResponseInterface {}

            namespace App\Impl;
            use App\Other\ResponseInterface;
            class RealResponse implements ResponseInterface {}

            namespace App;
            use App\Other\ResponseInterface;
            use App\Impl\RealResponse;
            function make() {
                return new class {
                    public function handle(): ResponseInterface {
                        return new RealResponse();
                    }
                };
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn anonymous_class_method_resolves_a_static_return_type() {
        // The sibling gap found alongside the use-import one: the
        // `Some(sig)` branch resolves self/static/parent pseudo-types via
        // `resolve_pseudo` whenever `class_name` is known; the anonymous-
        // class fallback branch did this for neither params nor the
        // return type. Before the fix, `: static` stayed the literal,
        // unresolved pseudo-type `Type::Object("static")`, which could
        // never match `$this`'s real (synthetic anonymous-class) type -
        // a return-type-mismatch false positive on the most ordinary
        // possible body (`return $this;`).
        let d = diagnostics_for(
            r#"<?php
            function make() {
                return new class {
                    public function self(): static {
                        return $this;
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

    // A round of Piwigo-corpus bucketing found this check compared the
    // *docblock-refined* return type on both sides, wrongly flagging an
    // override that faithfully keeps the ancestor's own native `array`
    // while the ancestor's docblock separately narrowed it further
    // (`list<...>`) - real PHP itself only enforces override-
    // compatibility on the native signature, and by default so does real
    // PHPStan (`checkPhpDocMethodSignatures` is off unless a project
    // opts in - confirmed directly against real PHPStan with this exact
    // shape). Real corpus shape: several `HtmlRenderingInterface` test
    // fakes each declaring `getCatBreadcrumb(): array`, exactly matching
    // the interface's own native signature, while the interface's
    // *docblock* separately says `@return list<array{...}>`.
    #[test]
    fn an_override_matching_only_the_ancestors_native_return_type_is_not_flagged() {
        let d = diagnostics_for(
            r#"<?php
            interface HasBreadcrumb13 {
                /** @return list<array{id: string, name: string}> */
                public function getCatBreadcrumb(string $x): array;
            }
            class FakeRenderer13 implements HasBreadcrumb13 {
                public function getCatBreadcrumb(string $x): array {
                    return [];
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Companion to the fix above: a genuine *native*-level incompatible
    // override (widening `int` to `string`, a real PHP fatal error) must
    // still be caught - this check has no other positive-case coverage
    // confirming it fires at all.
    #[test]
    fn an_override_genuinely_widening_the_native_return_type_is_still_flagged() {
        let d = diagnostics_for(
            r#"<?php
            class Base14 { public function make(): int { return 1; } }
            class Sub14 extends Base14 { public function make(): string { return "x"; } }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("is not compatible"), "got: {d:?}");
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

    // A round of Piwigo-corpus bucketing found real PHP 8.4 property hooks
    // (`CurrentConfig`'s own `apiKeyDuration`/`chmodValue`/etc.) whose
    // `set(Type $value)` hook deliberately accepts a wider type than the
    // property's own backing/read type, coercing internally before the
    // real assignment - a write must be checked against the hook's own
    // parameter type, not the property's declared type.
    #[test]
    fn a_property_hooks_wider_set_parameter_type_is_the_real_write_sink() {
        let d = diagnostics_for(
            r#"<?php
            class Config1 {
                /** @var list<string> */
                public array $names = ['a'] {
                    set(array $value) {
                        $this->names = array_values($value);
                    }
                }
            }
            function f(Config1 $c, array $raw): void {
                $c->names = $raw;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_property_hooks_narrower_set_parameter_type_is_still_enforced() {
        let d = diagnostics_for(
            r#"<?php
            class Config2 {
                public int $level = 0 {
                    set(int $value) {
                        $this->level = $value;
                    }
                }
            }
            function f(Config2 $c, string $bad): void {
                $c->level = $bad;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
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

    // Phase 13 Finding E: a trait method calling $this->hostOnlyMethod()
    // is valid PHP (the trait's author expects any using class to
    // provide it) - checked standalone (current_class bound to the
    // trait's own FQN), this always false-positived as "undefined
    // method".
    #[test]
    fn a_trait_method_calling_a_host_only_method_is_not_flagged_when_the_host_declares_it() {
        let d = diagnostics_for(
            r#"<?php
            trait Greeter {
                public function greet(): string { return $this->name(); }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_trait_method_calling_a_host_only_method_is_flagged_for_a_sibling_host_missing_it() {
        // Proves per-host re-checking, not "checked once (against
        // whichever host happens to run first) and reused" - the same
        // trait, used by two classes, must be checked once per host.
        let d = diagnostics_for(
            r#"<?php
            trait Greeter {
                public function greet(): string { return $this->name(); }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
            class Robot {
                use Greeter;
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert!(d[0].message.contains("undefined method"), "got: {d:?}");
    }

    #[test]
    fn a_host_independent_trait_bug_is_reported_once_not_once_per_host() {
        let d = diagnostics_for(
            r#"<?php
            trait Broken {
                public function boom(): int { return thisFunctionDoesNotExist(); }
            }
            class A { use Broken; }
            class B { use Broken; }
        "#,
        );
        assert_eq!(d.len(), 1, "expected the duplicate to be deduped: {d:?}");
    }

    #[test]
    fn an_unused_trait_is_still_checked_standalone() {
        let d = diagnostics_for(
            r#"<?php
            trait Orphaned {
                public function boom(): int { return thisFunctionDoesNotExist(); }
            }
        "#,
        );
        assert_eq!(d.len(), 1, "an unused trait must still be checked: {d:?}");
    }

    #[test]
    fn a_trait_used_only_by_another_trait_is_checked_against_the_real_final_host() {
        // `Inner` is used only by `Outer` (itself a trait), never
        // directly by any real class - `Inner`'s own $this->name() must
        // still resolve against Concrete, the real, final host, not
        // against `Outer` (which is itself just as host-unaware).
        let d = diagnostics_for(
            r#"<?php
            trait Inner {
                public function greet(): string { return $this->name(); }
            }
            trait Outer {
                use Inner;
            }
            class Concrete {
                use Outer;
                public function name(): string { return "x"; }
            }
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
        // A plain empty class, not `stdClass` - Phase 34 Finding D
        // exempts `stdClass` specifically from undefined-property
        // checking (real PHP's own "arbitrary property bag" class), so
        // this test needs an ordinary class to still prove `$this`-
        // rebinding is actually checked.
        let d = diagnostics_for(
            r#"<?php
            class Empty4841 {}
            function test(): void {
                Closure::bind(function () { return $this->totallyUndefinedVar; }, new Empty4841());
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
    fn a_by_ref_out_param_defined_on_the_right_of_and_is_readable_afterward() {
        // Phase 22 Finding 6, real shape: Piwigo\Admin\AdminShell::
        // runDispatch's `if (isset(...) and is_string(...) and (bool)
        // preg_match(..., $matches)) { ... reads $matches ... }` -
        // `$matches` is `preg_match`'s by-ref out-param, defined as a
        // side effect of evaluating the right operand of a chained
        // `and`/`&&`. `check_expr`'s `Binary(And, l, r)` arm used to
        // check `r` against a *clone* of `locals` that got discarded
        // once the expression finished, so this definition never
        // reached the real `locals` the enclosing `if`'s body sees.
        let d = diagnostics_for(
            r#"<?php
            function f(string $subject): void {
                if (is_string($subject) and (bool) preg_match('/(\d+)/', $subject, $matches)) {
                    echo $matches[0];
                    echo $matches[1];
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn instanceof_narrowing_inside_and_does_not_leak_into_the_else_branch() {
        // The critical regression test for Finding 6's own fix: a naive
        // "merge every newly-inserted key back into locals" (rather than
        // only real variable names) would insert `apply_narrowing`'s own
        // synthetic `"arg->value"` narrowing key into the real `locals`
        // *before* the `if`'s then/else split even runs - contaminating
        // the `else` branch (where the condition was false, so
        // `$arg->value` is NOT known to be a `Variable`) with the
        // narrowed-to type. A test that only covers the by-ref-in-`&&`
        // shape above would still pass under that broken version and
        // would not catch this.
        let d = diagnostics_for(
            r#"<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                if ($arg->value instanceof Variable && $arg->value->name !== '') {
                    echo 'narrowed';
                } else {
                    echo $arg->value->name;
                }
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("Base::$name")),
            "expected the else branch to still see $arg->value as Base (undefined $name), got: {d:?}"
        );
    }

    #[test]
    fn is_string_narrows_a_literal_array_index_expression() {
        // Phase 22, real shape: Piwigo\Template\Template::__construct's
        // `if (isset($lang_info['jquery_code']) and is_string($lang_info
        // ['jquery_code']) and ...) { ... str_replace('-', '_',
        // $lang_info['jquery_code']) ... }` - `narrowing_key` (and every
        // `apply_narrowing` call site consuming it) used to require a
        // bare `ExprKind::Variable` directly, so `is_string(...)` never
        // narrowed a literal-key array-index expression the way it
        // already narrowed a property chain.
        let d = diagnostics_for(
            r#"<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if (is_string($info['code'])) {
                    return strtoupper($info['code']);
                }
                return '';
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Found while implementing/testing Finding F above: `check_expr`'s own
    // `Index` arm never consulted the narrowed `locals` state at all -
    // unlike `infer_expr`'s `Index` arm, and unlike `check_expr`'s own
    // `PropertyAccess` arm just above it in the same match. A narrowed
    // array-index expression passed as a *function argument* (checked via
    // `infer_expr` for the argument-type comparison, the shape the test
    // just above this one exercises) already worked; a bare `return
    // $x['k'];` reading the same narrowed expression directly did not.
    #[test]
    fn is_string_narrows_a_literal_array_index_expression_read_back_via_a_bare_return() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if (is_string($info['code'])) {
                    return $info['code'];
                }
                return '';
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 32 Finding G, real shape: `PasswordController::resetPassword`'s
    // `$user_id = (bool) $reset_password_key_result ? ... :
    // $this->resetPasswordCode();` (both branches `false|int`) then `if (!
    // is_numeric($user_id)) { return false; }` before `UserId::from
    // ($user_id)` (expects plain `int`).
    #[test]
    fn is_numeric_narrows_the_true_branch_by_excluding_bool_and_null() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int|false $x */
            function f($x): int {
                if (is_numeric($x)) {
                    return $x;
                }
                return 0;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // The unsoundness a shared-target table entry would have introduced
    // (see this arm's own doc comment): `is_numeric`'s true branch must
    // *not* drop `String`, since a numeric-looking string still satisfies
    // it.
    #[test]
    fn is_numeric_leaves_string_untouched_in_the_true_branch() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int|string $x */
            function f($x): int|string {
                if (is_numeric($x)) {
                    return $x;
                }
                return 0;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn is_numeric_false_branch_narrows_by_excluding_int_and_float() {
        let d = diagnostics_for(
            r#"<?php
            /** @param int|string $x */
            function f($x): string {
                if (! is_numeric($x)) {
                    return $x;
                }
                return '';
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 32 Finding H, corpus-level check: `end()`'s own real stub
    // (`function end(object|array &$array): mixed {}` with `@return
    // mixed|false`) used to resolve to the literal `mixed|false` - a
    // stray `False` member fails `strtoupper()`'s plain `string` argument
    // check on its own, even though the whole union is vacuously
    // satisfiable via `Mixed`.
    #[test]
    fn strtoupper_accepts_ends_own_still_mixed_return_type() {
        let d = diagnostics_for(
            r#"<?php
            /** @param string[] $items */
            function f(array $items): string {
                return strtoupper(end($items));
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn equality_narrows_a_literal_array_index_expression() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if ($info['code'] !== null) {
                    return strtoupper($info['code']);
                }
                return '';
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn is_string_narrows_an_assignment_nested_in_its_own_argument() {
        // Phase 23, real shape: Piwigo\Admin\Extensions\
        // CoreUpdateService::checkPiwigoUpgrade's
        // `is_string($result = @HttpClientService::fetch(...))` -
        // `narrowing_key` used to return None for an Assign expression, so
        // this extremely common "assign-then-check" idiom was never
        // narrowed at all, even though the assignment itself already
        // defined `$result` correctly.
        let d = diagnostics_for(
            r#"<?php
            function fetch(): string|false { return 'x'; }
            function f(): string {
                if (is_string($result = fetch())) {
                    return $result;
                }
                return '';
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 26, real shape: Piwigo\Admin\Extensions\
    // ExtensionScanner::scanDirectoryEntries's `while (($file =
    // readdir($dir)) !== false) { ... preg_match(..., $file) ... }` -
    // real PHP *requires* the parens here (`=` binds looser than `!==`,
    // so `$file = readdir($dir) !== false` would parse as `$file =
    // (readdir($dir) !== false)` instead), so the extremely common
    // "assign-then-check" idiom is almost always wrapped in a `Grouping`
    // node - `narrowing_key` never saw through it to reach the `Assign`
    // arm Phase 23 added underneath.
    #[test]
    fn is_string_narrows_a_parenthesized_assignment_in_a_while_condition() {
        let d = diagnostics_for(
            r#"<?php
            function readdir_(): string|false { return 'x'; }
            function f(): void {
                while (($file = readdir_()) !== false) {
                    preg_match('/x/', $file);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 26, real shape: Piwigo\Admin\Image\ImageBackend::webpInfo's
    // `$fp = fopen(...); if (! (bool) $fp) { throw ...; } fread($fp,
    // 25);` - a common, explicit style for a truthiness guard. `(bool)`
    // doesn't change which values are truthy/falsy, but `apply_narrowing`
    // had no arm for `Cast` at all, so the guard was never recognized.
    #[test]
    fn a_bool_cast_truthiness_guard_narrows_like_a_bare_variable_check() {
        let d = diagnostics_for(
            r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $fp = open();
                if (! (bool) $fp) {
                    throw new Exception('failed');
                }
                fclose($fp);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 32 Finding F, real shape: `BrowserTestHelpers::apiFetch`'s
    // `is_int($decoded['status'] ?? null)` - `narrowing_key` had no arm
    // for a `??`-wrapped expression at all, so this common
    // "narrow-an-array-element-that-might-be-absent" idiom never narrowed
    // `$decoded['status']` itself.
    #[test]
    fn is_int_narrows_through_a_null_coalesce_fallback() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{status?: int|null} $decoded */
            function f(array $decoded): int {
                if (is_int($decoded['status'] ?? null)) {
                    return $decoded['status'];
                }
                return 0;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // The unsoundness this fix's own scoping avoids: unlike `?? null`,
    // `is_int($decoded['status'] ?? 0)` can be true purely because the
    // fallback `0` itself is an int, even when `$decoded['status']` is
    // absent/`null` - narrowing it to `int` there would be a real, new
    // false negative, so the coalesce arm must not fire for this
    // fallback.
    #[test]
    fn is_int_does_not_narrow_through_a_non_null_coalesce_fallback() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{status?: int|string} $decoded */
            function f(array $decoded): int {
                if (is_int($decoded['status'] ?? 0)) {
                    return $decoded['status'];
                }
                return 0;
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected $decoded['status'] to stay unnarrowed (still int|string): {d:?}"
        );
    }

    // The same boundary, checked against a non-literal fallback too - not
    // just "a fallback that happens to be non-zero."
    #[test]
    fn is_int_does_not_narrow_through_a_variable_coalesce_fallback() {
        let d = diagnostics_for(
            r#"<?php
            /** @param array{status?: int|string} $decoded */
            function f(array $decoded, int $default): int {
                if (is_int($decoded['status'] ?? $default)) {
                    return $decoded['status'];
                }
                return 0;
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected $decoded['status'] to stay unnarrowed (still int|string): {d:?}"
        );
    }

    // Real, general unsoundness bug found via a Phase 12 corpus re-check
    // (`CategoryUpdateController::__invoke`, minimized here) that first
    // looked like a cross-file registry-timing mystery but turned out to
    // be a plain, always-reproducible narrowing bug: `apply_equality_
    // narrowing`'s exclusion branch was using the *other side's own
    // static type* (`Bool`, a general type covering both `true`/`false`,
    // not one specific value) the same way it uses a genuine single-value
    // exclusion (`Null`/`True`/`False`/a literal) - wrongly collapsing an
    // already-known `bool` to `Type::Never` the moment it was compared
    // `!==` against another `bool`-typed (but not literally known) value.
    #[test]
    fn not_identical_to_a_non_literal_bool_does_not_collapse_the_narrowed_type_to_never() {
        let d = diagnostics_for(
            r#"<?php
            function test(?bool $a, bool $b): void {
                $update = [];
                if ($a !== null && $a !== $b) {
                    $update['flag'] = $a;
                }
                echo $update['flag'];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Real, general bug found via a Phase 12 corpus re-check (minimized
    // from `HtmlServiceTestFatalErrorCaller::call`): `check_expr`'s own
    // `Ternary` arm wrote `merge_scopes(then_locals, else_locals)` back
    // into the *outer* locals, unlike a `Ternary` used as a value inside
    // a larger expression ever should - a ternary's own condition
    // narrowing a *param* (`$collected` here) down to the literal empty
    // shape on its true branch then got permanently unioned into
    // `$collected`'s real, tracked type for everything checked
    // afterward, including a second, independent evaluation of the exact
    // same sub-expression - here, `preg_replace`'s own subject-type
    // check saw the corrupted union instead of `$collected[0]`'s real
    // `string` type, losing its precise return type.
    #[test]
    fn a_ternarys_own_condition_narrowing_does_not_leak_past_the_ternary() {
        let d = diagnostics_for(
            r#"<?php
            class Holder {
                public ?string $captured = null;

                /** @param list<string> $collected */
                public function run(array $collected): void {
                    $this->captured = $collected === [] ? null : preg_replace('/^x/', '', $collected[0]);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn reassigning_a_property_after_narrowing_it_does_not_leave_the_stale_narrowed_type_behind() {
        // Phase 22, real shape: a lazy-init idiom
        // (Piwigo\Asset\ViteManifest::entries) - `if ($this->entries ===
        // null) { $this->entries = [...]; } return $this->entries;`.
        // `apply_equality_narrowing` correctly narrows `$this->entries`
        // to `null` for the *condition's own* branch, but
        // `bind_assignment_target` had no `PropertyAccess` arm at all, so
        // the reassignment inside the `if` body never updated that same
        // synthetic narrowing-key entry - `infer_expr`'s own "check
        // narrowed key first" fast path kept reading back the stale
        // `null` instead of the real, newly-assigned array, so the
        // return type looked nullable when it never could be.
        let d = diagnostics_for(
            r#"<?php
            class Cache {
                /** @var array<string>|null */
                private ?array $entries = null;

                /** @return array<string> */
                private function entries(): array {
                    if ($this->entries === null) {
                        $this->entries = ['a', 'b'];
                    }
                    return $this->entries;
                }
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

    // Phase 23 Finding 6, real shape: Piwigo\Core\Logger declares eight
    // non-negative severity constants plus `OFF = -1` - the one negative
    // one. `const int OFF = -1;` parses as a literal `1` wrapped in a
    // unary negation, which `infer_const_type` used to fall back to
    // `Mixed` for, widening `['severity' => Logger::OFF]` to
    // `array{severity: mixed}` and failing the constructor's declared
    // `array{severity?: int|string, ...}` shape.
    #[test]
    fn a_negative_typed_class_constant_infers_its_real_int_type() {
        let d = diagnostics_for(
            r#"<?php
            final class Logger {
                public const int EMERGENCY = 0;
                public const int OFF = -1;
                /** @param array{severity?: int|string} $options */
                public function __construct(array $options) {}
            }
            function f(): void {
                new Logger(['severity' => Logger::OFF]);
                new Logger(['severity' => Logger::EMERGENCY]);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 23 Finding 7, real shape: phpstorm-stubs'
    // `imagecreatefromstring` has a native return hint updated for PHP
    // 8's GD object migration (`: GdImage|false`) but a stale `@return
    // resource|GdImage|false` doc comment above it, never revised -
    // `resolve_type` used to always prefer the docblock, keeping the
    // stale `resource` member even though the native hint (present,
    // and not itself mentioning `resource`) is authoritative.
    #[test]
    fn a_stale_resource_doc_comment_does_not_override_a_narrower_native_hint() {
        let d = diagnostics_for(
            r#"<?php
            class GdImage {}
            /** @return resource|GdImage|false */
            function imagecreatefromstring(string $data): GdImage|false {}
            class ImageGd {
                private GdImage $image;
                public function __construct(string $data) {
                    $img = imagecreatefromstring($data);
                    if ($img === false) {
                        throw new Exception('failed');
                    }
                    $this->image = $img;
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 23 Finding 7, real shape: Piwigo\Core\Logger::$fileHandle is
    // declared `private mixed $fileHandle = null;` deliberately - PHP has
    // no native `resource` type to declare, so the property leans on its
    // own `@var resource|null` docblock to carry the real type. An
    // earlier, unconditional version of Finding 7's fix incorrectly
    // treated `mixed` as an authoritative native hint that contradicts
    // the docblock's `resource` member, stripping it down to bare `null`
    // (a `!== null` guard, not the real code's `(bool)`-cast guard - a
    // cast-wrapped property-access truthy check is a separate, pre-
    // existing narrowing gap unrelated to this finding).
    #[test]
    fn a_mixed_native_hint_does_not_override_a_docblocks_resource_member() {
        let d = diagnostics_for(
            r#"<?php
            class Logger {
                /** @var resource|null */
                private mixed $fileHandle = null;
                private function open(): void {
                    $handle = fopen('/tmp/x', 'a');
                    if ($handle !== false) {
                        $this->fileHandle = $handle;
                    }
                }
                public function __destruct() {
                    if ($this->fileHandle !== null) {
                        fclose($this->fileHandle);
                    }
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 24, real shape: `substr()`/`explode()`/`pg_connect()` have no
    // native return-type hint at all, so their only source of a return
    // type is a stale `@return` doc comment, gated by
    // `#[LanguageLevelTypeAware(['V.V' => 'MODERN'], default: 'LEGACY')]`
    // immediately above `function` - previously unparsed entirely, so
    // the checker read the pre-8.0 legacy type unconditionally.
    #[test]
    fn language_level_type_aware_return_type_is_used_over_the_stale_docblock() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param string $string
             * @param int $offset
             * @param ?int $length
             * @return string|false
             */
            #[LanguageLevelTypeAware(["8.0" => "string"], default: "string|false")]
            function substr($string, $offset, $length) {}
            function f(string $s): string {
                return substr($s, 0, -1);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn language_level_type_aware_still_returns_an_array_when_the_default_would_have() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @param string $separator
             * @param string $string
             * @return string[]|false
             */
            #[LanguageLevelTypeAware(["8.0" => "string[]"], default: "string[]|false")]
            function explode($separator, $string) {}
            function f(string $s): array {
                return explode(',', $s);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Real shape: `finfo_file`'s own first parameter has no native hint
    // at all, only `#[LanguageLevelTypeAware(['8.1' => 'finfo'], default:
    // 'resource')] $finfo` - fixing `finfo_open`'s modernized `finfo`
    // return type alone (without this) left every downstream
    // `finfo_file($handle, ...)` call newly flagged, since `$handle`'s
    // now-correct `finfo` type no longer matched `finfo_file`'s still-
    // stale `resource` parameter type.
    #[test]
    fn language_level_type_aware_on_a_parameter_is_used_over_the_stale_docblock() {
        let d = diagnostics_for(
            r#"<?php
            class finfo {}
            /** @return finfo|false */
            #[LanguageLevelTypeAware(["8.1" => "finfo|false"], default: "resource|false")]
            function finfo_open() {}
            /**
             * @param resource $finfo
             * @return string|false
             */
            function finfo_file(#[LanguageLevelTypeAware(['8.1' => 'finfo'], default: 'resource')] $finfo, string $filename) {}
            function f(string $path): string {
                $handle = finfo_open();
                if ($handle === false) {
                    return '';
                }
                $result = finfo_file($handle, $path);
                return $result === false ? '' : $result;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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

    // Phase 23 Finding 5, real shape:
    // tests/Unit/Mail/MailServiceTestFakeSmtpServer.php's `$conn =
    // stream_socket_accept(...); if ($conn === false) { fwrite(STDERR,
    // ...); exit(1); } stream_set_timeout($conn, 5);` - `exit`/`die`
    // never inferred as `Type::Never`, so this guard clause wasn't
    // recognized as terminating and the then-branch's narrowing leaked
    // into the surrounding scope past the `if`.
    #[test]
    fn a_guard_clause_built_on_exit_terminates_like_return() {
        let d = diagnostics_for(
            r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $conn = open();
                if ($conn === false) {
                    exit(1);
                }
                fclose($conn);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_guard_clause_built_on_die_terminates_like_return() {
        let d = diagnostics_for(
            r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $conn = open();
                if ($conn === false) {
                    die('failed');
                }
                fclose($conn);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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
        // annotation on an actual property/param/return declaration - a
        // separate family of tests (see
        // `an_inline_var_annotation_...` below) covers the inline
        // `@var` form on an arbitrary statement.
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
    fn class_string_template_binds_the_concrete_subclass_from_a_static_factory_call() {
        // Phase 35 Finding E: `TypedRepository::narrow`-shaped repro -
        // `class-string<R>` needs to survive from the argument
        // (`Derived::class`) all the way through `@template R of Base`
        // binding to the `@return R` result, not collapse to plain
        // `string` (which would carry no `R` to bind at all).
        let d = diagnostics_for(
            r#"<?php
            class Base {}
            class Derived extends Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R> $repositoryClass
                 * @return R
                 */
                public static function narrow(string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            function f(): int {
                return TypedRepository::narrow(Derived::class);
            }
        "#,
        );
        assert!(
            d.iter().any(
                |x| x.message.contains("return type mismatch") && x.message.contains("Derived")
            ),
            "expected a return-type mismatch naming Derived, got: {d:?}"
        );
    }

    #[test]
    fn a_class_literal_still_satisfies_an_ordinary_string_parameter() {
        // Backward compatibility: `Foo::class` used to infer plain
        // `String`; now it's `ClassString(Object("Foo"))`, which must stay
        // freely assignable to an ordinary `string`-typed parameter - the
        // extremely common logging/error-message idiom.
        let d = diagnostics_for(
            r#"<?php
            class Foo {}
            function needsString(string $s): void {}
            function f(): void {
                needsString(Foo::class);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_class_string_template_parameter_still_accepts_a_plain_string_argument() {
        // The other, deliberately lenient direction: this project can't
        // verify a plain string's actual runtime content names a real
        // class, so a `class-string<R>`-typed parameter must still accept
        // an ordinary, non-literal `string` argument rather than
        // false-positive on it.
        let d = diagnostics_for(
            r#"<?php
            class Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R> $repositoryClass
                 * @return R
                 */
                public static function narrow(string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            function f(string $repositoryClass): void {
                TypedRepository::narrow($repositoryClass);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn get_class_of_a_known_object_infers_a_class_string_binding_the_same_template() {
        let d = diagnostics_for(
            r#"<?php
            class Base {}
            class Derived extends Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R> $repositoryClass
                 * @return R
                 */
                public static function narrow(string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            function f(Derived $d): int {
                return TypedRepository::narrow(get_class($d));
            }
        "#,
        );
        assert!(
            d.iter().any(
                |x| x.message.contains("return type mismatch") && x.message.contains("Derived")
            ),
            "expected a return-type mismatch naming Derived, got: {d:?}"
        );
    }

    #[test]
    fn get_class_of_an_unknown_typed_value_stays_plain_string() {
        let d = diagnostics_for(
            r#"<?php
            function needsString(string $s): void {}
            function f($x): void {
                needsString(get_class($x));
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn an_inline_var_annotation_on_a_later_statement_narrows_a_generically_typed_read() {
        // Phase 35 Finding F: the real `StructuralTest::findCallSites`
        // shape, reduced - `$file` comes back from `current()`'s own
        // generic (untyped, so `mixed`) return, an inline `/** @var
        // FakeFileInfo9921 $file */` sits on an unrelated `if` statement that
        // merely reads it, and code after that `if` calls a method only
        // `FakeFileInfo9921` has. Without the annotation, `$file` stays
        // `mixed` and this method call is never checked at all (this
        // project's usual mixed-argument/mixed-receiver leniency) - so a
        // real "undefined method" diagnostic here proves the annotation
        // actually took effect, not just that nothing was flagged.
        let d = diagnostics_for(
            r#"<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): void {
                $file = $it->current();
                /** @var FakeFileInfo9921 $file */
                if (true) {
                }
                $file->missingMethod();
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("undefined method")),
            "expected an undefined-method diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn a_bare_inline_var_annotation_overrides_the_immediately_following_assignment() {
        // The bug found during adversarial validation: applying the
        // assertion only *before* dispatch would have this immediately
        // clobbered by the assignment's own natural (mixed) inference,
        // since `bind_assignment_target` runs inside the very same
        // `check_stmt` call. A `return type mismatch` naming FakeFileInfo9921
        // proves the after-dispatch reapplication actually won.
        let d = diagnostics_for(
            r#"<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): string {
                /** @var FakeFileInfo9921 */
                $file = $it->current();
                return $file;
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("return type mismatch")
                && x.message.contains("FakeFileInfo9921")),
            "expected a return-type mismatch naming FakeFileInfo9921, got: {d:?}"
        );
    }

    #[test]
    fn a_named_inline_var_annotation_on_its_own_assignment_also_overrides_it() {
        // Same bug, explicitly-named form - proving the fix isn't
        // bare-form-specific.
        let d = diagnostics_for(
            r#"<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): string {
                /** @var FakeFileInfo9921 $file */
                $file = $it->current();
                return $file;
            }
        "#,
        );
        assert!(
            d.iter().any(|x| x.message.contains("return type mismatch")
                && x.message.contains("FakeFileInfo9921")),
            "expected a return-type mismatch naming FakeFileInfo9921, got: {d:?}"
        );
    }

    #[test]
    fn an_inline_var_override_is_invalidated_by_a_later_real_reassignment() {
        // The override correctly gets invalidated by a later real
        // reassignment of the same variable - Phase 33 Finding 5's own
        // purge-on-reassignment, exercised through this new call site.
        let d = diagnostics_for(
            r#"<?php
            class FakeFileInfo9921 {}
            function needsInt(int $n): void {}
            function f($it): void {
                $file = $it;
                /** @var FakeFileInfo9921 $file */
                if (true) {
                }
                $file = 5;
                needsInt($file);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_bare_inline_var_annotation_preceding_a_foreach_binding_two_names_is_a_no_op() {
        // Scope guard for `statement_defined_variable_names`'s own
        // restriction: a bare (nameless) tag can only infer its target
        // when the statement it precedes has exactly one candidate -
        // a `foreach` binding both a key and a value has two, so this is
        // left unguessed rather than assuming either one.
        let d = diagnostics_for(
            r#"<?php
            class FakeFileInfo9921 {}
            function f(array $items): string {
                /** @var FakeFileInfo9921 */
                foreach ($items as $k => $v) {
                    return $v;
                }
                return "fallback";
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_doc_comment_with_no_var_tag_is_a_no_op() {
        let d = diagnostics_for(
            r#"<?php
            function f(): int {
                /** Just a description, no @var tag here. */
                $x = "not an int";
                return 1;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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

    // Phase 23 Finding 1: `isset($x)`'s truthy branch narrows away `Null`,
    // the single most common PHP nullability idiom - previously a no-op.
    #[test]
    fn isset_guard_narrows_null_away_in_the_truthy_branch() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): int {
                if (isset($x)) {
                    return strlen($x);
                }
                return 0;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn isset_guard_narrows_a_property_access_too() {
        let d = diagnostics_for(
            r#"<?php
            class Box { public ?string $value = null; }
            function f(Box $box): int {
                if (isset($box->value)) {
                    return strlen($box->value);
                }
                return 0;
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // A round of Piwigo-corpus bucketing found a static property
    // (`self::$prop !== null`) had no narrowing key at all - only an
    // instance property's `->` chain was handled. Real Piwigo test code
    // (BrowserTestHelpers::adminSessionCookies) relies on exactly this.
    #[test]
    fn a_static_property_is_narrowed_by_an_equality_check_too() {
        let d = diagnostics_for(
            r#"<?php
            class Cache1 {
                private static ?string $value = null;
                public static function get(): string {
                    if (self::$value !== null) {
                        return self::$value;
                    }
                    return self::$value = 'computed';
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn reassigning_a_static_property_after_narrowing_it_does_not_leave_the_stale_narrowed_type_behind()
     {
        let d = diagnostics_for(
            r#"<?php
            class Cache2 {
                private static ?string $value = null;
                public static function reset(): int {
                    if (self::$value !== null) {
                        self::$value = null;
                        return strlen(self::$value);
                    }
                    return 0;
                }
            }
        "#,
        );
        assert_eq!(
            d.len(),
            1,
            "expected the stale narrowed type to be purged: {d:?}"
        );
    }

    #[test]
    fn isset_false_branch_is_not_narrowed() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $x): int {
                if (isset($x)) {
                    return 0;
                }
                return strlen($x);
            }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
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

    // Phase 23 Finding 4, real shape: `$handle = $filename !== null ?
    // @fopen($filename, 'wb') : false;` - fopen()'s own type is already a
    // Union (`resource|false`); combining it with the ternary's other
    // branch via a raw `Type::Union(vec![then_ty, else_ty])` nested that
    // whole union unflattened, so a later `!== false` narrowing on the
    // outer union couldn't reach the `false` still hiding inside it.
    #[test]
    fn ternary_flattens_a_branch_that_is_itself_already_a_union() {
        let d = diagnostics_for(
            r#"<?php
            function f(?string $filename): void {
                $handle = $filename !== null ? @fopen($filename, 'wb') : false;
                if ($handle !== false) {
                    fclose($handle);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
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
    fn coalesce_widens_to_the_left_side_s_array_type_when_the_fallback_is_a_narrower_shape() {
        // Phase 22 Finding 4, real shape: Piwigo\Activity\Projection\
        // SystemActivityLogEntry::$details is `public ?array $details`;
        // `$details = $row->details ?? [];` used to collapse the whole
        // expression's type to the literal empty `[]` shape from the
        // fallback alone, discarding that `$row->details`'s own real,
        // non-empty-capable `?array` type could hold arbitrary keys -
        // every later `$details['key']` read then looked like a typo.
        let d = diagnostics_for(
            r#"<?php
            class Row { public ?array $details; }
            function f(Row $row): void {
                $details = $row->details ?? [];
                echo $details['config_section'];
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn coalesce_still_collapses_to_the_fallback_for_an_ordinary_mixed_left_side() {
        // The common, currently-precise case Finding 4's own fix must
        // not regress: when the left side's real type isn't an
        // informative `Type::Array`, `??`'s result stays exactly the
        // fallback's own (here, literal string) type - not blanket
        // `Mixed` from unioning in `$x`'s own type.
        let d = diagnostics_for(
            r#"<?php
            function needsInt(int $x): int { return $x; }
            function f($x): int {
                return needsInt($x ?? 0);
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

    // Phase 29, real shape: Piwigo\Core\Lang::load's own `$source_file =
    // ''; foreach ($languages as $language) { $f = ...; if (found) {
    // $source_file = $f; break; } } if ($source_file === '') { return
    // false; } preg_replace(..., $source_file);` - a `break` reached
    // inside an `if` had its own state (a real string) silently
    // discarded from what flows past the loop, since `always_terminates`
    // correctly stops it from leaking into the *rest of the same block*
    // but nothing captured it for the *loop's own* exit state. Left
    // `$source_file` permanently typed as the literal `''` after the
    // loop, which then collapsed to `Type::Never` at the `=== ''` guard -
    // vacuously assignable to anything, masking every real check after it.
    #[test]
    fn a_break_reached_inside_an_if_is_seen_by_code_after_its_foreach_loop() {
        let d = diagnostics_for(
            r#"<?php
            function f(array $items): string {
                $found = '';
                foreach ($items as $item) {
                    if (is_string($item)) {
                        $found = $item;
                        break;
                    }
                }
                if ($found === '') {
                    return 'none';
                }
                return strtoupper($found);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_break_reached_inside_an_if_is_seen_by_code_after_its_while_loop() {
        let d = diagnostics_for(
            r#"<?php
            function f(array $items): string {
                $found = '';
                $i = 0;
                while ($i < count($items)) {
                    if (is_string($items[$i])) {
                        $found = $items[$i];
                        break;
                    }
                    $i++;
                }
                if ($found === '') {
                    return 'none';
                }
                return strtoupper($found);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_break_reached_inside_an_if_is_seen_by_code_after_its_for_loop() {
        let d = diagnostics_for(
            r#"<?php
            function f(array $rows): string {
                $found = '';
                for ($i = 0; $i < count($rows); $i++) {
                    if (is_string($rows[$i])) {
                        $found = $rows[$i];
                        break;
                    }
                }
                if ($found === '') {
                    return 'none';
                }
                return strtoupper($found);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_multi_level_break_reaches_the_correct_outer_loop() {
        let d = diagnostics_for(
            r#"<?php
            function f(array $matrix): string {
                $found = '';
                foreach ($matrix as $row) {
                    foreach ($row as $cell) {
                        if (is_string($cell)) {
                            $found = $cell;
                            break 2;
                        }
                    }
                }
                if ($found === '') {
                    return 'none';
                }
                return strtoupper($found);
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_bare_break_inside_a_switch_inside_a_loop_does_not_leak_to_the_loop() {
        // The switch's own `break` targets the switch itself, not the
        // enclosing loop - confirming the frame this fix pushes around a
        // `Switch` (to intercept exactly this) doesn't over-eagerly
        // propagate the switch's own internal break into the loop's own
        // post-loop merge (which, here, has no break of its own at all,
        // so it should merge exactly as if this fix didn't exist).
        let d = diagnostics_for(
            r#"<?php
            function f(array $items): void {
                $x = null;
                foreach ($items as $item) {
                    switch (true) {
                        case is_string($item):
                            $x = 'from switch';
                            break;
                    }
                }
                if ($x !== null) {
                    strtoupper($x);
                }
            }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    // Phase 29, real shape: found and reverted mid-fix -
    // `Piwigo\Calendar\CalendarMonthly::buildMonthCalendar`'s own
    // `foreach ($items as $day => $data) { ...; $items[$day]['derivative']
    // = ...; $items[$day]['file'] = ...; }` mutates the very same array
    // it iterates, via a nested write keyed by the loop variable itself.
    // An earlier version of this fix also merged `Foreach` with its own
    // pre-loop state (matching `While`/`For`'s existing "may run zero
    // times" treatment, for consistency) - but every key iterated over
    // here really does get the loop's own treatment, so unioning in the
    // *stale pre-loop* shape as an "also possible" alternative produced a
    // false "array{...} has no key 'derivative'" the real code never
    // risks. `Foreach` deliberately keeps checking its body once directly
    // against the real `locals`, gaining only the `break`-state capture.
    #[test]
    fn foreach_mutating_its_own_subject_via_the_loop_key_does_not_regress() {
        let d = diagnostics_for(
            r#"<?php
            function f(array $rows): void {
                $items = [];
                foreach ($rows as $row) {
                    $items[(int) $row] = ['nb_images' => 1];
                }
                foreach ($items as $day => $data) {
                    $items[$day]['derivative'] = 'x';
                    $items[$day]['file'] = 'y';
                }
                foreach ($items as $day => $data) {
                    echo $items[$day]['derivative'];
                    echo $items[$day]['file'];
                }
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

    // A round of Piwigo-corpus bucketing found a catch clause's own
    // ending state was discarded entirely after the whole try/catch -
    // `locals` only ever kept the try body's own final state, matching
    // this project's own documented "no scope merge back after try/
    // catch" limitation. Real shape: `$body = null; try { ...; } catch
    // (...) { $body = "x"; }` then using `$body` afterward.
    #[test]
    fn a_variable_assigned_only_inside_a_catch_clause_is_visible_after_the_try_catch() {
        let d = diagnostics_for(
            r#"<?php
            /** @param non-empty-string $x */
            function sink(string $x): void {}
            function f(): void {
                $body = 'default';
                try {
                    riskyCall();
                } catch (\Throwable $e) {
                    $body = 'from catch';
                }
                sink($body);
            }
            function riskyCall(): void {}
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_catch_clause_that_always_returns_is_excluded_from_the_post_try_merge() {
        let d = diagnostics_for(
            r#"<?php
            /** @param non-empty-string $x */
            function sink(string $x): void {}
            function f(): string {
                $body = 'default';
                try {
                    riskyCall();
                } catch (\Throwable $e) {
                    return 'error';
                }
                sink($body);
                return $body;
            }
            function riskyCall(): void {}
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

    // Phase 13 Finding D: real shape from `vendor/php-openapi/openapi`'s
    // own `OpenApi` class, which declares `@property Paths $paths` (a
    // magic property backed by `__get`/`__set`, no real declared field).
    #[test]
    fn a_class_level_property_tag_lets_the_magic_property_be_read() {
        let d = diagnostics_for(
            r#"<?php
            class Paths {}
            /**
             * @property Paths $paths
             */
            class OpenApi {}
            function f(OpenApi $o): Paths { return $o->paths; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_class_level_property_tag_is_inherited_by_a_subclass() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @property int $magic
             */
            class Base {}
            class Sub extends Base {}
            function f(Sub $s): int { return $s->magic; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_real_declared_property_wins_over_a_same_named_property_tag() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @property string $x
             */
            class A { public int $x = 0; }
            function f(A $a): int { return $a->x; }
        "#,
        );
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
    }

    #[test]
    fn a_property_read_tag_flags_an_attempted_write() {
        let d = diagnostics_for(
            r#"<?php
            /**
             * @property-read int $x
             */
            class A {}
            function f(A $a): void { $a->x = 1; }
        "#,
        );
        assert_eq!(d.len(), 1, "got: {d:?}");
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
