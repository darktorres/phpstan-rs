//! Replicates the `pestphp/pest-plugin-phpstan` extension's `$this`
//! resolution inside Pest's `it`/`test`/`describe`/`beforeEach`/
//! `afterEach`/`beforeAll`/`afterAll` closures. PHP has no static way to
//! know `$this` is valid inside these top-level-file closures - Pest's
//! own runtime rebinds it via `Closure::bindTo()` before invoking them.
//! PHPStan only accepts this because of Pest's own first-party PHPStan
//! extension - this module's resolution order is read directly from
//! that extension's source
//! (`vendor/pestphp/pest-plugin-phpstan/src/Type/Pest/`):
//! `TestClosureThisTypeExtension.php`'s own constant lists are the
//! authoritative function set (`is_this_binding_call` below);
//! `PestTestCaseType.php`/`PestConfigReader.php` give the resolution
//! order - a file-local `uses(...)` call (no `->in(...)`) written
//! directly inside the file being checked, else a directory-scoped
//! `uses(...)->in(dir1, dir2, ...)` call found in any `Pest.php` config
//! file whose target directory prefixes the file being checked, else
//! default to `PHPUnit\Framework\TestCase`.
//!
//! Deliberately narrower than the real extension in two disclosed ways:
//! trait arguments in `uses(...)` aren't distinguished from classes (Pest
//! wraps them in its own `PestTestCaseWithTraitsType`); a `->in(...)`
//! argument must be a bare string literal, no glob-wildcard targets
//! (Pest's own glob step is a no-op for a literal, already-existing
//! directory path anyway).
//!
//! The base scanner (`checker/expr.rs`) never spells out Pest's own
//! function names - it only calls `is_this_binding_call`, so a future
//! second PHPStan-extension-shaped plugin (a different testing
//! framework's own `$this`-binding convention, say) would add its own
//! sibling module with its own predicate and resolver, without touching
//! this one.

use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use crate::registry::Registry;
use syntax::ast::literal_string_value;
use syntax::ast::{
    Arg, ArgRange, ClosurePayload, ExprId, ExprKind, NodeArenas, StaticTarget, StmtId, StmtKind,
};
use types::Type;

/// The seven Pest functions whose closure argument gets `$this` rebound.
pub(crate) fn is_this_binding_call(name: &str) -> bool {
    matches!(
        name,
        "it" | "test" | "describe" | "beforeEach" | "afterEach" | "beforeAll" | "afterAll"
    )
}

/// Extracts class names from a `uses(...)` call's arguments - `X::class`
/// (the class it resolves to, through the file's `use` imports) or a plain
/// string literal, used as-is: a string's contents are never subject to
/// `use`-import short-name resolution the way a bare `Name`/`ClassConstFetch`/`StaticPropertyFetch`
/// token is. Trait arguments aren't distinguished from classes - see the
/// module doc comment.
pub(crate) fn class_names_from_uses_call(arenas: &NodeArenas, args: &[Arg]) -> Vec<String> {
    args.iter()
        .filter_map(|a| match arenas.expr(a.value).map(|e| &e.kind) {
            Some(ExprKind::ClassNameFetch(StaticTarget::Name(class_name))) => {
                class_name.resolved_class().map(str::to_string)
            }
            _ => literal_string_value(arenas, a.value)
                .map(|s| s.trim_start_matches('\\').to_string()),
        })
        .collect()
}

fn this_type_from_class_names(names: Vec<String>) -> Option<Type> {
    let mut iter = names.into_iter();
    let first = iter.next()?;
    let rest: Vec<String> = iter.collect();
    if rest.is_empty() {
        Some(Type::object(first.into(), Vec::new()))
    } else {
        let mut all = vec![first];
        all.extend(rest);
        Some(Type::intersection_of(
            all.into_iter()
                .map(|n| Type::object(n.into(), Vec::new()))
                .collect(),
        ))
    }
}

/// A bare `uses(X::class);` written directly inside the file being
/// checked (no chained `->in(...)`) - real Pest's own file-local binding,
/// checked first, ahead of any directory-scoped `Pest.php` config.
pub(crate) fn file_local_this_type(arenas: &NodeArenas, program: &[StmtId]) -> Option<Type> {
    program.iter().find_map(|id| {
        let stmt = arenas.stmt(*id)?;
        let StmtKind::Expression(e) = &stmt.kind else {
            return None;
        };
        let e = arenas.expr(arenas.peeled(*e)?)?;
        let ExprKind::Call(callee, args) = &e.kind else {
            return None;
        };
        let ExprKind::Name(name) = &arenas.expr(*callee)?.kind else {
            return None;
        };
        let name = arenas.qualified_name(name)?;
        if name != "uses" {
            return None;
        }
        this_type_from_class_names(class_names_from_uses_call(
            arenas,
            arenas.args(args.clone()),
        ))
    })
}

/// Pest's own dynamic-property idiom, replicated from
/// `pestphp/pest-plugin-phpstan`'s `TestCaseDynamicPropertyTypeExtension`
/// and `PestHookPropertyReader`: every `beforeEach`/`test`/`afterEach`/
/// `describe` closure in a file shares the same `$this` instance, so a
/// property set in one `beforeEach(...)` closure is legitimately
/// readable from another closure in the same file. Narrower than the
/// real reader in the same two ways `file_local_this_type`'s own doc
/// comment discloses for class resolution:
///
/// - Only a **top-level, file-local** `beforeEach(...)` call is scanned
///   here (mirroring `file_local_this_type`'s own top-level-only shape) -
///   the real reader finds `beforeEach(...)` anywhere in the file via a
///   fully recursive AST search, which would also catch one nested
///   inside a `describe(...)` closure.
/// - Only a **direct, top-level statement** inside the closure body is
///   recognized as a property assignment (mirroring the real reader's
///   own `foreach ($closure->stmts as $stmt)`, no recursion into a
///   nested `if`/loop).
///
/// Property *names* only, not the real reader's full RHS-type inference
/// (resolving local-variable aliasing, `@var` doc-comment hints, ...) -
/// this only needs to know a name exists so a read of it isn't flagged
/// as undefined, matching this project's own established "safe default"
/// pattern for a definition whose real type isn't worth modeling
/// precisely (see `predefine_byref_or_unknown_args`).
pub(crate) fn file_local_hook_properties(
    arenas: &NodeArenas,
    program: &[StmtId],
) -> FxHashSet<String> {
    let mut props = FxHashSet::default();
    for &id in program {
        let Some(stmt) = arenas.stmt(id) else {
            continue;
        };
        let StmtKind::Expression(e) = &stmt.kind else {
            continue;
        };
        let Some(e) = arenas.peeled(*e).and_then(|id| arenas.expr(id)) else {
            continue;
        };
        let ExprKind::Call(callee, args) = &e.kind else {
            continue;
        };
        let Some(callee) = arenas.expr(*callee) else {
            continue;
        };
        let ExprKind::Name(name) = &callee.kind else {
            continue;
        };
        let Some(name) = arenas.qualified_name(name) else {
            continue;
        };
        if name != "beforeEach" {
            continue;
        }
        for arg in arenas.args(args.clone()) {
            collect_hook_closure_properties(arenas, arg.value, &mut props);
        }
    }
    props
}

/// The top-level-statements-only half of `file_local_hook_properties`'s
/// own doc comment: `$this->prop = ...` assignments found as direct
/// statements in a `beforeEach(...)` closure's own body, keying off the
/// literal `$this` expression only (unlike the real reader, which
/// resolves by the property base's *type* via PHPStan's own `Scope` -
/// an aliased `$self = $this; $self->prop` isn't covered here).
fn collect_hook_closure_properties(
    arenas: &NodeArenas,
    expr: ExprId,
    props: &mut FxHashSet<String>,
) {
    let Some(expr) = arenas.peeled(expr).and_then(|id| arenas.expr(id)) else {
        return;
    };
    let ExprKind::Closure(closure_id) = &expr.kind else {
        return;
    };
    let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure_id) else {
        return;
    };
    for &id in arenas.stmt_ids(&closure.body) {
        let Some(stmt) = arenas.stmt(id) else {
            continue;
        };
        let StmtKind::Expression(e) = &stmt.kind else {
            continue;
        };
        let Some(e) = arenas.peeled(*e).and_then(|id| arenas.expr(id)) else {
            continue;
        };
        let (ExprKind::Assign(lhs, _) | ExprKind::AssignRef(lhs, _)) = &e.kind else {
            continue;
        };
        let Some(lhs) = arenas.expr(*lhs) else {
            continue;
        };
        let ExprKind::PropertyAccess(base, prop, _) = &lhs.kind else {
            continue;
        };
        if matches!(arenas.expr(*base).map(|e| &e.kind), Some(ExprKind::Variable(name)) if **name == *"this")
        {
            props.insert(prop.to_string());
        }
    }
}

/// Walks a Pest fluent chain (`uses(...)->beforeEach(...)->afterEach(...)
/// ->in(...)`, any number/order of intermediate calls between `uses(...)`
/// and the terminal `.in(...)` real Pest requires to register a
/// directory-scoped rule) down to its base `uses(...)` call, collecting
/// every `beforeEach(...)` closure's own property assignments found along
/// the way into `hook_props`. `expr` is whatever the outermost `.in(...)`
/// call's own receiver is - returns the base `uses(...)` call's own
/// arguments, or `None` if the chain doesn't bottom out in a real
/// `uses(...)` call.
fn walk_uses_chain(
    arenas: &NodeArenas,
    expr: ExprId,
    hook_props: &mut FxHashSet<String>,
) -> Option<ArgRange> {
    syntax::stack::guard(|| {
        let expr = arenas.expr(arenas.peeled(expr)?)?;
        match &expr.kind {
            ExprKind::Call(callee, args) => {
                let ExprKind::Name(name) = &arenas.expr(*callee)?.kind else {
                    return None;
                };
                let name = arenas.qualified_name(name)?;
                (name == "uses").then(|| args.clone())
            }
            ExprKind::MethodCall(inner, method, call_args, _) => {
                if **method == *"beforeEach" {
                    for arg in arenas.args(call_args.clone()) {
                        collect_hook_closure_properties(arenas, arg.value, hook_props);
                    }
                }
                walk_uses_chain(arenas, *inner, hook_props)
            }
            _ => None,
        }
    })
}

/// Directory-scoped `$this` bindings *and* hook-property names collected
/// from every `Pest.php` config file in a project -
/// `uses(X::class, ...)->beforeEach(...)->in(dir1, dir2, ...)`, each
/// target directory recorded as a path prefix (resolved relative to that
/// `Pest.php` file's own parent directory, unless absolute). Both
/// accumulate as a union across every matching rule (real Pest's own
/// behavior - no "most specific wins" pruning).
#[derive(Debug, Default)]
pub struct PestConfig {
    bindings: Vec<(PathBuf, Vec<String>)>,
    hook_properties: Vec<(PathBuf, Vec<String>)>,
}

/// What one `Pest.php` file binds, read from its tree when it is parsed.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PestFacts {
    bindings: Vec<(PathBuf, Vec<String>)>,
    hook_properties: Vec<(PathBuf, Vec<String>)>,
}

impl PestFacts {
    /// The bindings of the file `path` with the tree `ast`; `None` for a
    /// file not named `Pest.php`.
    pub fn of(path: &Path, arenas: &NodeArenas, ast: &[StmtId]) -> Option<PestFacts> {
        if path.file_name().and_then(|n| n.to_str()) != Some("Pest.php") {
            return None;
        }
        let dir = path.parent()?;
        let mut bindings = Vec::new();
        let mut hook_properties = Vec::new();
        for &id in ast {
            let stmt = arenas.stmt(id)?;
            let StmtKind::Expression(e) = &stmt.kind else {
                continue;
            };
            let e = arenas.expr(arenas.peeled(*e)?)?;
            let ExprKind::MethodCall(inner, method, in_args, _) = &e.kind else {
                continue;
            };
            if **method != *"in" {
                continue;
            }
            let mut chain_props = FxHashSet::default();
            let Some(uses_args) = walk_uses_chain(arenas, *inner, &mut chain_props) else {
                continue;
            };
            let class_names = class_names_from_uses_call(arenas, arenas.args(uses_args));
            // A bare `uses()->beforeEach(...)->in(...)` (no class
            // argument to `uses()` at all - confirmed real, not
            // hypothetical: `tests/Pest.php`'s own `uses()->
            // beforeEach(...)->afterEach(...)->in('Unit')`) still has
            // real hook properties worth recording even though there's
            // no class binding to go with them - only skip when
            // *neither* is worth recording.
            if class_names.is_empty() && chain_props.is_empty() {
                continue;
            }
            let chain_props: Vec<String> = chain_props.into_iter().collect();
            for in_arg in arenas.args(in_args.clone()) {
                let Some(target) = literal_string_value(arenas, in_arg.value) else {
                    continue;
                };
                let target_path = Path::new(target.as_ref());
                let prefix = if target_path.is_absolute() {
                    target_path.to_path_buf()
                } else {
                    dir.join(target_path)
                };
                if !class_names.is_empty() {
                    bindings.push((prefix.clone(), class_names.clone()));
                }
                if !chain_props.is_empty() {
                    hook_properties.push((prefix, chain_props.clone()));
                }
            }
        }
        Some(PestFacts {
            bindings,
            hook_properties,
        })
    }
}

impl PestConfig {
    /// Built from the facts of every currently-loaded file named
    /// `Pest.php`.
    pub fn build<'a>(pest_files: impl Iterator<Item = &'a PestFacts>) -> PestConfig {
        let mut bindings = Vec::new();
        let mut hook_properties = Vec::new();
        for facts in pest_files {
            bindings.extend(facts.bindings.iter().cloned());
            hook_properties.extend(facts.hook_properties.iter().cloned());
        }
        PestConfig {
            bindings,
            hook_properties,
        }
    }

    /// Resolves the `$this` type for one file being checked - every
    /// directory binding whose prefix matches `file_path` (unioned,
    /// deduped), else the default (`PHPUnit\Framework\TestCase`, only if
    /// actually known to `registry`), else `mixed` as the honest
    /// last resort.
    pub fn resolve_this_type(&self, file_path: &Path, registry: &Registry) -> Type {
        let mut names: Vec<String> = self
            .bindings
            .iter()
            .filter(|(prefix, _)| file_path.starts_with(prefix))
            .flat_map(|(_, classes)| classes.iter().cloned())
            .collect();
        names.sort();
        names.dedup();
        if let Some(ty) = this_type_from_class_names(names) {
            return ty;
        }
        const DEFAULT: &str = "PHPUnit\\Framework\\TestCase";
        if registry.has_class(DEFAULT) {
            Type::object(DEFAULT.into(), Vec::new())
        } else {
            Type::mixed()
        }
    }

    /// Resolves the directory-scoped dynamic-property names for one file
    /// being checked - every `hook_properties` binding whose prefix
    /// matches `file_path`, unioned (unlike `resolve_this_type`'s
    /// single-value class slot, a property-name set has no "most
    /// specific wins" precedence to apply - the caller unions this with
    /// whatever `file_local_hook_properties` finds too).
    pub fn resolve_hook_properties(&self, file_path: &Path) -> FxHashSet<String> {
        self.hook_properties
            .iter()
            .filter(|(prefix, _)| file_path.starts_with(prefix))
            .flat_map(|(_, props)| props.iter().cloned())
            .collect()
    }
}

syntax::canonical_struct!(PestFacts {
    bindings,
    hook_properties,
});
