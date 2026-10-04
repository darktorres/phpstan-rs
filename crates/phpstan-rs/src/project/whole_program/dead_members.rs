//! Flags a protected/public method/
//! property/const never referenced anywhere in the project. Real
//! PHPStan's own dead-member rule gates on `isPrivate()` specifically,
//! since verifying no *other file* reaches a broader-visibility member
//! needs its cross-process collector machinery - this project's own
//! closed-world `Registry`/Tier-2 `use_sites` index already has that
//! fact for free, the same infrastructure `project::whole_program::
//! dead_code` already reads.
//!
//! `record_member_ref` already fires from every ordinary access arm
//! (instance/static property, instance/static method call, class
//! constant - `checker/expr.rs`'s own `PropertyAccess`/`MethodCall`/
//! `ClassConstFetch`/`StaticPropertyFetch`/`StaticCall` arms), so a ordinary reference needs no
//! new instrumentation at all. Two real gaps did:
//!
//! - A genuinely dynamic member access (`$obj->{$expr}`/`$obj->$name`)
//!   used to have no escape-hatch tracking at all (unlike `check_dead_
//!   enum_cases`'s own `{dynamic}`/`::cases()` hooks) - fixed by
//!   `Checker::escape_targets_for_receiver_type`: when the dynamic
//!   name itself is a literal string, `check_expr`'s own `Dynamic
//!   PropertyAccess`/`DynamicMethodCall` arms now record a real
//!   reference instead (PHP-semantically, `$obj->{'foo'}` *is*
//!   `$obj->foo`); when it's genuinely computed, every class the
//!   receiver's own resolved type could reach (itself, its ancestors,
//!   and - for an interface/non-final-abstract receiver - every real
//!   subclass/implementor anywhere in the registry, from its
//!   descendants index, `Registry::inheritors`) is escaped
//!   conservatively, all its members
//!   treated as used. A receiver this project can't resolve at all
//!   (`Mixed`) contributes no escape protection - a real, disclosed
//!   false-positive risk this project can't close without real type
//!   information.
//! - PHP invokes some methods implicitly, with no source-level
//!   reference this project could ever see: the engine's own magic
//!   method names (`__construct`, `__toString`, `__invoke`, ...) -
//!   hard-excluded below by name.
//! - A method overriding *any* real ancestor's own same-named method -
//!   an interface's contract method (`Countable::count()`, `Iterator::
//!   current()`, a third-party library's own callback interface like
//!   `PhpParser\NodeVisitor::enterNode()`), or a regular (non-interface)
//!   ancestor's method reached only through *polymorphic* dispatch on a
//!   base-typed variable - is excluded the same way, walking the full
//!   ancestor chain (an interface implemented only through a parent
//!   still counts). This was originally a small hand-maintained table
//!   of well-known SPL interfaces, replaced after measuring a real
//!   corpus (Piwigo) and finding that table's own real blind spot in
//!   the flesh: several genuinely-used `PhpParser\NodeVisitor`
//!   implementations (`enterNode`/`leaveNode`), never called explicitly
//!   anywhere in the analyzed source (php-parser's own traverser calls
//!   them polymorphically, and lives in vendor code this project's
//!   batch mode never walks for Tier-2 collection at all - see this
//!   module's own disclosure list below), were flagged dead under the
//!   old, narrower table. The generic override check catches every
//!   such interface automatically, with no library-specific knowledge
//!   at all, and also closes the regular-abstract-method-override case
//!   the old table never covered.
//! - A PHPUnit test method or lifecycle callback (`setUp`, `test*`) on
//!   a real `TestCase` subclass - the *actual* dominant source of noise
//!   the Piwigo measurement above surfaced (thousands of `test*`
//!   methods, each only ever invoked by PHPUnit's own reflection-based
//!   discovery, no explicit call anywhere). Recognized the exact same
//!   way `checker::risky_tests::check_risky_test_methods` already does
//!   (`Registry::is_subclass_of` + a plain `test`-prefix name check,
//!   with that check's own identical, disclosed scope limit: a
//!   differently-named `#[Test]`/`@test`-tagged method isn't
//!   recognized).
//!
//! Real, disclosed false-positive risks left unfixed (ship opt-in): the
//! whole-program `check-project`'s own primary Tier-2-collecting pass
//! only ever runs over *analyzed* (first-party) files, never vendor/
//! context ones (`main/check_project.rs`'s own primary loop iterates
//! `analyze_files_by_size` only) - a real call made entirely from
//! within vendor code (a third-party library's own internal traverser
//! calling back into a first-party interface implementation, the exact
//! shape measured on Piwigo above) is invisible to this check for that
//! architectural reason, not a bug specific to it; a member reachable
//! only through framework magic/reflection (a template calling a
//! method by name, a DI container definition, PHPUnit attribute
//! discovery); a member called only through a
//! `[$obj, 'methodName']` array-callable (`usort`/`array_map`/
//! `call_user_func` - a real, known-name reference this project doesn't
//! yet resolve, unlike the literal-string-dynamic-access case above,
//! which it does); a property and a method sharing the exact same name
//! on one class (legal PHP - `$this->x` and `$this->x()` are
//! independently dispatched) share one Tier-2 usage signal here, so
//! using one can mask the other's own real deadness - a real, but only
//! ever false-*negative* limitation, the safe direction.
//!
//! `$obj->$name(...)` (a bare variable right after `->`) used to be a
//! real gap here too - `parser/postfix.rs` mis-modeled it as an
//! ordinary, statically-named `MethodCall`/`PropertyAccess` (the
//! variable's own *declared name*, never its runtime value), so this
//! check's own escape hatch above never saw it. Fixed at the parser
//! level: it now parses exactly like the braced `$obj->{$name}(...)`
//! form, so both get identical escape-hatch treatment.

use crate::project::use_sites::{RefTarget, UseSiteIndex};
use crate::registry::Registry;
use syntax::ast::{ClassKind, Visibility};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberKind {
    Method,
    Property,
    Const,
}

/// PHP's own implicitly-invoked method names - the engine calls these
/// itself, so no explicit source-level reference to any of them ever
/// exists for `use_sites` to record.
const IMPLICITLY_INVOKED_METHODS: &[&str] = &[
    "__construct",
    "__destruct",
    "__call",
    "__callstatic",
    "__get",
    "__set",
    "__isset",
    "__unset",
    "__sleep",
    "__wakeup",
    "__serialize",
    "__unserialize",
    "__tostring",
    "__invoke",
    "__set_state",
    "__clone",
    "__debuginfo",
];

fn is_implicitly_invoked_method(name: &str) -> bool {
    IMPLICITLY_INVOKED_METHODS
        .iter()
        .any(|m| name.eq_ignore_ascii_case(m))
}

/// PHPUnit's own lifecycle callbacks - called by the test runner on
/// every `TestCase` subclass regardless of whether it declares any
/// `test*` method at all, by fixed name, the same "the framework calls
/// this implicitly" shape as `IMPLICITLY_INVOKED_METHODS` above, just
/// PHPUnit's own convention instead of PHP's.
const PHPUNIT_LIFECYCLE_METHODS: &[&str] = &[
    "setup",
    "teardown",
    "setupbeforeclass",
    "teardownafterclass",
    "onnotsuccessfultest",
    "assertpreconditions",
    "assertpostconditions",
];

/// A PHPUnit test method (`check_risky_test_methods`'s own identical
/// `test`-prefix convention and disclosed scope note - `#[Test]`/
/// `@test`-annotated methods with a different name are not recognized)
/// or lifecycle callback on a real `TestCase` subclass - the single
/// largest real-corpus source of noise measured on Piwigo before this
/// exclusion existed (PHPUnit's own reflection-based test discovery
/// calls every `test*` method itself; no explicit `->testFoo()` call
/// ever exists anywhere in the analyzed source for `use_sites` to see).
fn is_phpunit_test_method(registry: &Registry, fqn: &str, method_name: &str) -> bool {
    if !registry.is_subclass_of(fqn, "PHPUnit\\Framework\\TestCase") {
        return false;
    }
    method_name.starts_with("test")
        || PHPUNIT_LIFECYCLE_METHODS
            .iter()
            .any(|m| method_name.eq_ignore_ascii_case(m))
}

/// Whether `fqn` overrides `method_name` from any real ancestor - an
/// interface's own contract method (which every implementor must
/// override, and which real callers may only ever reach by dispatching
/// through the interface's own type, never an explicit call this
/// project could see attributed to *this* class specifically), or a
/// regular ancestor's method reached the same polymorphic way. `fqn`
/// itself is never in its own `all_ancestor_names`, so this only ever
/// matches a genuine override, never a method's own first declaration.
fn overrides_an_ancestor_method(registry: &Registry, fqn: &str, method_name: &str) -> bool {
    registry.all_ancestor_names(fqn).iter().any(|ancestor| {
        registry
            .classes
            .get(ancestor)
            .is_some_and(|info| info.methods().contains_key(method_name))
    })
}

/// Every `(owner FQN, member name, kind)` of `classes`, by their
/// registry names, with no recorded usage anywhere in the project - see
/// this module's own doc comment for what counts as "used" and what's
/// disclosed, not fixed. `classes` are the class-likes the analysed
/// files declare and the classes using a trait one declares: every class
/// a member an analysed file declares is resolved into. Callers resolve
/// each pair's own reportable location via `Project::decl_locations`
/// themselves (this function stays unaware of files/spans/`Diagnostic`
/// entirely - see `Project::check_whole_program`'s own doc comment for
/// why).
pub(crate) fn find_dead_members<'a>(
    classes: impl IntoIterator<Item = &'a str>,
    registry: &Registry,
    use_sites: &UseSiteIndex,
    escaped_classes: &rustc_hash::FxHashSet<String>,
) -> Vec<(String, String, MemberKind)> {
    let mut dead = Vec::new();
    for fqn in classes {
        let Some(info) = registry.classes.get(fqn) else {
            continue;
        };
        // An interface/trait is never itself instantiable - its own
        // method declarations are pure contracts for whatever implements
        // (interface) or hosts (trait) them, never something callable in
        // their own right, so they're never candidates here (they still
        // count as real ancestors for `overrides_an_ancestor_method`
        // above - only their own candidacy as a *source* of findings is
        // excluded).
        if !matches!(info.kind(), ClassKind::Class | ClassKind::Enum)
            || escaped_classes.contains(fqn)
        {
            continue;
        }
        let is_visible = |v: &Visibility| matches!(v, Visibility::Protected | Visibility::Public);
        let is_used = |name: &str| {
            !use_sites
                .references(&RefTarget::Member(fqn.to_string(), name.to_string()))
                .is_empty()
        };

        for (name, sig) in info.methods().iter() {
            if !is_visible(&sig.visibility())
                || sig.is_magic()
                || is_implicitly_invoked_method(name)
                || overrides_an_ancestor_method(registry, fqn, name)
                || is_phpunit_test_method(registry, fqn, name)
                || is_used(name)
            {
                continue;
            }
            dead.push((fqn.to_string(), name.to_string(), MemberKind::Method));
        }
        for (name, sig) in info.properties() {
            let is_enum_builtin = info.kind() == ClassKind::Enum
                && (name.as_ref() == "name" || name.as_ref() == "value");
            if !is_visible(&sig.visibility()) || is_enum_builtin || is_used(name) {
                continue;
            }
            dead.push((fqn.to_string(), name.to_string(), MemberKind::Property));
        }
        for (name, sig) in info.consts() {
            if sig.ty().is_enum_case() || !is_visible(&sig.visibility()) || is_used(name) {
                continue;
            }
            dead.push((fqn.to_string(), name.to_string(), MemberKind::Const));
        }
    }
    dead
}
