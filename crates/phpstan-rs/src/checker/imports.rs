use rustc_hash::{FxHashMap, FxHashSet};

use crate::checker::{Checker, class_kind_identifier};
use crate::registry::class_decl_name;
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;

/// Real PHPStan's own `INTERNAL_CLASS_PREFIXES`
/// (`ClassForbiddenNameCheck`), transcribed - a Rector/PHP-Scoper/PHPUnit-
/// PHAR/Box-prefixed vendor class name almost always leaked into a
/// reference by mistake. `pub(crate)` so `checker::resolve.rs`'s own
/// `check_type_class_names_exist` (every one of that function's call
/// sites, not just the `use`-import site this module's own
/// `check_use_class_imports` already covers) can share the identical
/// list rather than drifting out of sync with a second copy.
pub(crate) const INTERNAL_CLASS_PREFIXES: &[(&str, &str)] = &[
    ("PHPStan", "_PHPStan_"),
    ("Rector", "RectorPrefix"),
    ("PHP-Scoper", "_PhpScoper"),
    ("PHPUnit", "PHPUnitPHAR"),
    ("Box", "_HumbugBox"),
];

impl<'a> Checker<'a> {
    /// Real PHPStan's own `ExistingNamesInUseRule`
    /// only ever existence-checks the `use function foo;` and `use const FOO;`
    /// forms - a plain, class-shaped `use Foo;` goes through
    /// `ClassNameCheck::checkClassNames` with a `null` usage location,
    /// which (per its own source) only runs case-sensitivity/
    /// forbidden-name checking in that case and never calls `hasClass`
    /// at all; a class reference is only ever existence-checked at the
    /// point it's actually *used* (a typehint, `new`, `instanceof`, ...),
    /// not merely imported. `use const`'s own half
    /// (`check_use_const_imports`, below) is a separate function; this
    /// one is the `use function` half, also covering its own
    /// `function.nameCase` check (opt-in, `RulesSettings::
    /// check_function_name_case`, mirroring real PHPStan's own
    /// `checkFunctionNameCase`-gated half of the same rule, default
    /// `false` at every level).
    pub(crate) fn check_use_function_imports(&mut self, program: &[Stmt]) {
        for stmt in program {
            let StmtKind::Use(imports) = &stmt.kind else {
                continue;
            };
            for import in imports {
                if import.kind != UseKind::Function {
                    continue;
                }
                // A `use`-imported path is always already fully qualified
                // from the global namespace (real PHP requires this,
                // leading `\` optional but implied) - unlike an ordinary
                // reference elsewhere in the file, it never goes through
                // `NameContext::resolve`'s own use-map/namespace-relative
                // logic, so a bare `trim_start_matches` is the whole of
                // it.
                let fqn = import.path.trim_start_matches('\\');
                match self.registry.lookup_function_with_name(fqn) {
                    None => {
                        self.report(
                            format!("used function {fqn} not found"),
                            stmt.span,
                            0,
                            "function.notFound",
                        );
                    }
                    // Opt-in (`RulesSettings::check_function_name_case`,
                    // default `false`), mirroring real PHPStan's own
                    // `checkFunctionNameCase`-gated half of
                    // `ExistingNamesInUseRule::checkFunctions` -
                    // case-insensitive-equal but not identical, matching
                    // that rule's own condition exactly.
                    Some((real_name, _))
                        if self.settings.rules.check_function_name_case
                            && real_name.eq_ignore_ascii_case(fqn)
                            && real_name != fqn =>
                    {
                        let mut builder = DiagnosticBuilder::new(
                            format!("Function {real_name} used with incorrect case: {fqn}."),
                            stmt.span,
                            0,
                            "function.nameCase",
                        );
                        if let Some((span, replacement, description)) =
                            Self::name_case_fix(&import.written, real_name, import.written.span)
                        {
                            builder = builder.fix(span, replacement, description);
                        }
                        self.report_builder(builder);
                    }
                    Some(_) => {}
                }
            }
        }
    }

    /// `use const`'s own existence half of `ExistingNamesInUseRule` (see
    /// `check_use_function_imports`'s own doc comment) - unconditional,
    /// not opt-in: PHP constants are case-sensitive at the language
    /// level, so there's no "resolves fine but wrong case" scenario the
    /// way there is for a function/class name, and real PHPStan has no
    /// `constant.nameCase` concept at all. Backed by `Registry::global_constants`,
    /// populated for every project-level `const`/`define()` declaration
    /// (`registry/index.rs`'s `build_declarations_with_context`) *and*
    /// every real built-in/extension constant the vendored phpstorm-
    /// stubs corpus declares via its own top-level `define()` calls
    /// (`registry::stubs::generated_stub_constants`, merged in at `Project::
    /// new` alongside `generated_stubs`/`generated_stub_classes`) - a
    /// disclosed residual: `Registry::global_constants` only ever sees a
    /// `define()` reachable from a file's own top level (through
    /// nested `if`/`else`/`{}` blocks, the same polyfill-idiom recursion
    /// `use function`'s own registry already gets) - a `define()` nested
    /// inside a function/method body is invisible to it, so a `use
    /// const` importing one stays unflagged either way rather than
    /// risking a false positive on a shape this project can't see.
    pub(crate) fn check_use_const_imports(&mut self, program: &[Stmt]) {
        for stmt in program {
            let StmtKind::Use(imports) = &stmt.kind else {
                continue;
            };
            for import in imports {
                if import.kind != UseKind::Const {
                    continue;
                }
                let fqn = import.path.trim_start_matches('\\');
                if !self.registry.global_constants.contains_key(fqn) {
                    self.report(
                        format!("used constant {fqn} not found"),
                        stmt.span,
                        0,
                        "constant.notFound",
                    );
                }
            }
        }
    }

    /// `ClassForbiddenNameCheck`: a `use`-imported
    /// class-shaped name matching one of a small, fixed set of internal
    /// build-tool-mangled prefixes (transcribed from real PHPStan's own
    /// `INTERNAL_CLASS_PREFIXES`) almost always means a Rector/PHP-Scoper/
    /// PHPUnit-PHAR/Box-prefixed vendor class leaked into a reference by
    /// mistake, not a deliberate one - real PHPStan flags this
    /// unconditionally (`nonIgnorable()`), matching the identical
    /// `class.prefixed` identifier and wording here. Deliberately scoped
    /// to the `use`-import site only, not every class reference in the
    /// file (real PHPStan's own equally broad application would need
    /// hooking into every existing class-reference checkpoint in this
    /// project - a real, disclosed scope trim, not attempted here).
    /// `ClassCaseSensitivityCheck`'s own half of this same real rule
    /// (`class.nameCase` on a `use`-import site specifically) is
    /// deliberately NOT built here: unlike `check_use_function_imports`'s
    /// sibling `function.nameCase` check (opt-in, since real PHPStan's
    /// own `checkFunctionNameCase` config defaults `false` at every
    /// level), the class case-check's own gating flag defaults active
    /// in real PHPStan - but this project's own class-name resolution
    /// does already have a real case-insensitive fallback
    /// (`Registry::lookup_class`, `registry/hierarchy.rs`).
    /// `class.nameCase` itself is already built and wired at other
    /// class-reference sites project-wide (`checker/resolve.rs`'s
    /// `check_type_class_names_exist` pipeline).
    pub(crate) fn check_use_class_imports(&mut self, program: &[Stmt]) {
        for stmt in program {
            let StmtKind::Use(imports) = &stmt.kind else {
                continue;
            };
            for import in imports {
                if import.kind != UseKind::Class {
                    continue;
                }
                let path = import.path.trim_start_matches('\\');
                for (project, prefix) in INTERNAL_CLASS_PREFIXES {
                    if path.starts_with(prefix) {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                format!("Referencing prefixed {project} class: {path}."),
                                stmt.span,
                                0,
                                "class.prefixed".to_string(),
                            )
                            .non_ignorable(),
                        );
                        break;
                    }
                }
                self.check_restricted_class_use_import(path, stmt.span);
            }
        }
    }

    /// `UsedNamesRule`: a same-file, case-insensitive
    /// collision between a class-like declaration's own name and a
    /// `use`-imported (class-shaped only - `use function`/`use const`
    /// are exempt, matching real PHPStan's own `shouldBeIgnored`) alias,
    /// in either direction - `use Foo\Bar; class Bar {}` and `class Bar
    /// {} use Foo\Bar;` are both real, unconditional PHP fatal errors
    /// ("Cannot declare class Bar because the name is already in use"/
    /// "Cannot use Foo\Bar as Bar because the name is already in use").
    /// A single left-to-right pass over
    /// `program`'s own top-level statements, mirroring `check_program`'s
    /// own identical positional `namespace`-tracking walk exactly (a
    /// `use`/class-like declaration is only ever legal at file top
    /// level, and this project's own parser already flattens both real
    /// PHP `namespace` forms - braced and semicolon - into one flat
    /// statement list, so there's no separate nested-block case to
    /// handle here) - `used_names` is keyed by the lowercased namespace
    /// active at each point, matching real PHPStan's own per-namespace
    /// scoping (and real PHP's: two same-named classes in two different
    /// namespaces never collide).
    pub(crate) fn check_used_names(&mut self, program: &[Stmt]) {
        // Original-case (for display in a message) and lowercased (the
        // real map key, since PHP namespace/name comparison here is
        // case-insensitive) kept side by side.
        let mut current_namespace = String::new();
        let mut current_namespace_lower = String::new();
        let mut used_names: FxHashMap<String, FxHashSet<String>> = FxHashMap::default();
        for stmt in program {
            match &stmt.kind {
                StmtKind::Namespace(name) => {
                    current_namespace = name.clone();
                    current_namespace_lower = name.to_ascii_lowercase();
                }
                StmtKind::Use(imports) => {
                    for import in imports {
                        if import.kind != UseKind::Class {
                            continue;
                        }
                        let path = import.path.trim_start_matches('\\');
                        let bare = path.rsplit('\\').next().unwrap_or(path);
                        let alias = import.alias.as_deref().unwrap_or(bare);
                        let scope = used_names
                            .entry(current_namespace_lower.clone())
                            .or_default();
                        if !scope.insert(alias.to_ascii_lowercase()) {
                            self.report_builder(
                                DiagnosticBuilder::new(format!(
                                        "Cannot use {path} as {alias} because the name is already in use."
                                    ),
                                    stmt.span,
                                    0,
                                    "use.nameInUse".to_string(),
                                )
                                .non_ignorable(),
                            );
                        }
                    }
                }
                StmtKind::ClassDecl(c) => {
                    let kind = class_kind_identifier(&c.kind);
                    let scope = used_names
                        .entry(current_namespace_lower.clone())
                        .or_default();
                    if !scope.insert(class_decl_name(c).to_ascii_lowercase()) {
                        let qualified = if current_namespace.is_empty() {
                            class_decl_name(c).to_string()
                        } else {
                            format!("{current_namespace}\\{}", class_decl_name(c))
                        };
                        self.report_builder(
                            DiagnosticBuilder::new(format!(
                                    "Cannot declare {kind} {qualified} because the name is already in use."
                                ),
                                c.span,
                                0,
                                format!("{kind}.nameInUse"),
                            )
                            .non_ignorable(),
                        );
                    }
                }
                StmtKind::EnumDecl(e) => {
                    let scope = used_names
                        .entry(current_namespace_lower.clone())
                        .or_default();
                    if !scope.insert(e.name.to_ascii_lowercase()) {
                        let qualified = if current_namespace.is_empty() {
                            e.name.to_string()
                        } else {
                            format!("{current_namespace}\\{}", e.name)
                        };
                        self.report_builder(
                            DiagnosticBuilder::new(format!(
                                    "Cannot declare enum {qualified} because the name is already in use."
                                ),
                                e.span,
                                0,
                                "enum.nameInUse".to_string(),
                            )
                            .non_ignorable(),
                        );
                    }
                }
                _ => {}
            }
        }
    }
}
