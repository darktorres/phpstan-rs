//! Replicates the real `pestphp/pest-plugin-phpstan` extension's `$this`
//! resolution inside Pest's `it`/`test`/`describe`/`beforeEach`/
//! `afterEach`/`beforeAll`/`afterAll` closures. Real PHP has no static way
//! to know `$this` is valid inside these top-level-file closures - Pest's
//! own runtime rebinds it via `Closure::bindTo()` before invoking them.
//! Real PHPStan only accepts this because of Pest's own first-party
//! PHPStan extension - this module's own resolution order is read
//! directly from that extension's real source
//! (`vendor/pestphp/pest-plugin-phpstan/src/Type/Pest/` in a real corpus,
//! not guessed): `TestClosureThisTypeExtension.php`'s own constant lists
//! are the authoritative function set (`is_this_binding_call` below);
//! `PestTestCaseType.php`/`PestConfigReader.php` give the resolution
//! order - a file-local `uses(...)` call (no `->in(...)`) written
//! directly inside the file being checked, else a directory-scoped
//! `uses(...)->in(dir1, dir2, ...)` call found in any `Pest.php` config
//! file whose target directory prefixes the file being checked, else
//! default to `PHPUnit\Framework\TestCase`.
//!
//! Deliberately narrower than the real extension in two disclosed ways,
//! neither with any real corpus evidence to justify building more:
//! trait arguments in `uses(...)` aren't distinguished from classes (real
//! Pest wraps them in its own `PestTestCaseWithTraitsType`); a `->in(...)`
//! argument must be a bare string literal, no glob-wildcard targets (real
//! Pest's own glob step is a no-op for a literal, already-existing
//! directory path anyway).
//!
//! The base scanner (`checker/expr.rs`) never spells out Pest's own
//! function names - it only calls `is_this_binding_call`, so a future
//! second PHPStan-extension-shaped plugin (a different testing
//! framework's own `$this`-binding convention, say) would add its own
//! sibling module with its own predicate and resolver, without touching
//! this one.

use std::path::{Path, PathBuf};

use crate::ast::{Arg, ExprKind, Stmt, StmtKind};
use crate::checker::{literal_string_value, unwrap_grouping};
use crate::registry::{NameContext, Registry};
use crate::types::Type;

/// The seven Pest functions whose closure argument gets `$this` rebound.
pub(crate) fn is_this_binding_call(name: &str) -> bool {
    matches!(
        name,
        "it" | "test" | "describe" | "beforeEach" | "afterEach" | "beforeAll" | "afterAll"
    )
}

/// Extracts class names from a `uses(...)` call's arguments - `X::class`
/// (resolved through `ctx`, since a `Pest.php` file imports classes via
/// an ordinary `use` statement the same as any other PHP file) or a plain
/// string literal, used as-is: a string's contents are never subject to
/// `use`-import short-name resolution the way a bare `Name`/`StaticAccess`
/// token is. Trait arguments aren't distinguished from classes - see the
/// module doc comment.
pub(crate) fn class_names_from_uses_call(args: &[Arg], ctx: &NameContext) -> Vec<String> {
    args.iter()
        .filter_map(|a| match &a.value.kind {
            ExprKind::StaticAccess(class_name, member) if member == "class" => {
                Some(ctx.resolve(class_name))
            }
            _ => literal_string_value(&a.value).map(|s| s.trim_start_matches('\\').to_string()),
        })
        .collect()
}

fn this_type_from_class_names(names: Vec<String>) -> Option<Type> {
    let mut iter = names.into_iter();
    let first = iter.next()?;
    let rest: Vec<String> = iter.collect();
    if rest.is_empty() {
        Some(Type::Object(first, Vec::new()))
    } else {
        let mut all = vec![first];
        all.extend(rest);
        Some(Type::Intersection(
            all.into_iter()
                .map(|n| Type::Object(n, Vec::new()))
                .collect(),
        ))
    }
}

/// A bare `uses(X::class);` written directly inside the file being
/// checked (no chained `->in(...)`) - real Pest's own file-local binding,
/// checked first, ahead of any directory-scoped `Pest.php` config.
pub(crate) fn file_local_this_type(program: &[Stmt], ctx: &NameContext) -> Option<Type> {
    program.iter().find_map(|stmt| {
        let StmtKind::Expression(e) = &stmt.kind else {
            return None;
        };
        let ExprKind::Call(callee, args) = &unwrap_grouping(e).kind else {
            return None;
        };
        let ExprKind::Name(name) = &callee.kind else {
            return None;
        };
        if name != "uses" {
            return None;
        }
        this_type_from_class_names(class_names_from_uses_call(args, ctx))
    })
}

/// Directory-scoped `$this` bindings collected from every `Pest.php`
/// config file in a project - `uses(X::class, ...)->in(dir1, dir2, ...)`,
/// each target directory recorded as a path prefix (resolved relative to
/// that `Pest.php` file's own parent directory, unless absolute).
/// Bindings accumulate as a union across every matching rule (real
/// Pest's own behavior - no "most specific wins" pruning).
#[derive(Debug, Default)]
pub struct PestConfig {
    bindings: Vec<(PathBuf, Vec<String>)>,
}

impl PestConfig {
    /// Built once from every currently-loaded file named `Pest.php`,
    /// reusing each one's already-parsed AST - no re-parsing needed.
    pub fn build<'a>(pest_files: impl Iterator<Item = (&'a Path, &'a [Stmt])>) -> PestConfig {
        let mut bindings = Vec::new();
        for (path, ast) in pest_files {
            if path.file_name().and_then(|n| n.to_str()) != Some("Pest.php") {
                continue;
            }
            let Some(dir) = path.parent() else {
                continue;
            };
            let ctx = NameContext::build(ast);
            for stmt in ast {
                let StmtKind::Expression(e) = &stmt.kind else {
                    continue;
                };
                let ExprKind::MethodCall(inner, method, in_args, _) = &unwrap_grouping(e).kind
                else {
                    continue;
                };
                if method != "in" {
                    continue;
                }
                let ExprKind::Call(uses_callee, uses_args) = &unwrap_grouping(inner).kind else {
                    continue;
                };
                let ExprKind::Name(uses_name) = &uses_callee.kind else {
                    continue;
                };
                if uses_name != "uses" {
                    continue;
                }
                let class_names = class_names_from_uses_call(uses_args, &ctx);
                if class_names.is_empty() {
                    continue;
                }
                for in_arg in in_args {
                    let Some(target) = literal_string_value(&in_arg.value) else {
                        continue;
                    };
                    let target_path = Path::new(target);
                    let prefix = if target_path.is_absolute() {
                        target_path.to_path_buf()
                    } else {
                        dir.join(target_path)
                    };
                    bindings.push((prefix, class_names.clone()));
                }
            }
        }
        PestConfig { bindings }
    }

    /// Resolves the `$this` type for one file being checked - every
    /// directory binding whose prefix matches `file_path` (unioned,
    /// deduped), else the default (`PHPUnit\Framework\TestCase`, only if
    /// actually known to `registry`), else `Type::Mixed` as the honest
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
        if registry.classes.contains_key(DEFAULT) {
            Type::Object(DEFAULT.to_string(), Vec::new())
        } else {
            Type::Mixed
        }
    }
}
