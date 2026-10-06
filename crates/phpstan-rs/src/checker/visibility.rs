//! `public`/`private`/`protected` access-control enforcement, using
//! `PropertyReflection`/`FunctionReflection`'s `visibility` field. Uses
//! `property.private`/`property.protected` (shared by read and plain
//! write - the asymmetric `private(set)`/`protected(set)` PHP 8.4 half
//! isn't modeled), `method.private`/`method.protected`, and
//! `staticMethod.private`/`staticMethod.protected`.

use crate::checker::{Checker, MemberLabel};
use syntax::ast::{Expr, ExprKind, Literal, Visibility};
use syntax::span::Span;

/// `check_member_visibility`'s own call bundle - 7 positional
/// parameters tripped clippy's `too_many_arguments` lint, and a named-
/// field bundle is clearer at each call site anyway (mirrors this
/// project's own `CheckArgsCall` precedent, `calls.rs`).
pub(crate) struct MemberVisibilityCheck<'a> {
    pub(crate) member: MemberLabel<'a>,
    pub(crate) visibility: &'a Visibility,
    pub(crate) span: Span,
}

/// `check_static_private_member_access`'s own call bundle - same
/// too-many-arguments reason as `MemberVisibilityCheck` above.
struct StaticPrivateAccessCheck<'a> {
    verb: &'a str,
    noun: &'a str,
    identifier: &'static str,
    owner: &'a str,
    member_display: &'a str,
    span: Span,
}

impl<'a> Checker<'a> {
    /// Real PHP's private/protected access-control rule, as far as this
    /// project can practically check it: `Visibility::Public` is always
    /// legal; `Private` only from within the exact declaring class;
    /// `Protected` from within the declaring class or anywhere in its
    /// inheritance chain. The direct check for `Protected` (checked both
    /// directions - `current_class` a subclass of `owner`, or `owner` a
    /// subclass of `current_class`) covers the common cases, but `owner`
    /// is whichever class's lookup actually *resolved* the member (the
    /// nearest concrete override, for an inherited `abstract protected`
    /// one), not necessarily the ancestor that first declared it - two
    /// sibling subclasses of a common `abstract class Base { abstract
    /// protected function f(); }`, each independently overriding `f()`,
    /// are still mutually protected-accessible in real PHP. `member_name`
    /// is the fallback for exactly this case: if `current_class`'s
    /// hierarchy independently resolves a member of the identical name
    /// (regardless of its own visibility - finding anything there is
    /// enough to prove a shared ancestor contract exists), access is
    /// allowed. Not perfectly precise (two same-named, genuinely
    /// unrelated protected members in unconnected hierarchies would also
    /// pass), but strictly more accurate than the direct check alone.
    pub(crate) fn is_member_access_visible(
        &self,
        owner: &str,
        visibility: &Visibility,
        member_name: &str,
        kind: &str,
    ) -> bool {
        match visibility {
            Visibility::Public => true,
            Visibility::Private => self.current_class.as_deref() == Some(owner),
            Visibility::Protected => match self.current_class.as_deref() {
                Some(c) => {
                    c == owner
                        || self.registry.is_subclass_of(c, owner)
                        || self.registry.is_subclass_of(owner, c)
                        || match kind {
                            // A static property shares the exact same
                            // `PropertyReflection`/`lookup_property` machinery an
                            // instance property already uses - only
                            // `PropertyReflection::is_static` (already checked by
                            // this arm's own caller before it ever reaches
                            // here) tells them apart.
                            "property" | "staticProperty" => {
                                self.registry.lookup_property(c, member_name).is_some()
                            }
                            // A class constant gets the identical
                            // sibling-shared-ancestor fallback as
                            // property/method access - a real constant
                            // can be overridden per-subclass exactly like
                            // a method can.
                            "classConstant" => self.registry.lookup_const(c, member_name).is_some(),
                            _ => self.registry.lookup_method(c, member_name).is_some(),
                        }
                }
                None => false,
            },
        }
    }

    /// `kind` is the real PHPStan identifier prefix (`"property"`,
    /// `"method"`, `"staticMethod"` - the latter two both count as
    /// `is_method` for `is_member_access_visible`'s own purposes);
    /// `member_name` is the bare member name (no `$`/`()`); `describe`
    /// is the human-facing member description (`"property Foo::$bar"`,
    /// `"method Foo::bar()"`), mirroring this project's own existing
    /// "access to undefined {describe}" convention (`ExprKind::
    /// PropertyAccess`'s own `property.notFound` message). Reports
    /// nothing when access is legal or `visibility` is `Public` - the
    /// overwhelmingly common case, checked first so every real call
    /// site can call this unconditionally without its own `if` guard.
    pub(crate) fn check_member_visibility(&mut self, check: MemberVisibilityCheck) {
        let MemberVisibilityCheck {
            member,
            visibility,
            span,
        } = check;
        let (owner, member_name) = (member.owner, member.name);
        let kind = member.kind.identifier_prefix();
        let word = match visibility {
            Visibility::Public => return,
            Visibility::Private => "private",
            Visibility::Protected => "protected",
        };
        if self.is_member_access_visible(owner, visibility, member_name, kind) {
            return;
        }
        self.report(
            format!("access to {word} {member}"),
            span,
            0,
            format!("{kind}.{word}"),
        );
    }

    /// A narrower, LSB-specific angle distinct from the ordinary
    /// visibility check just above. `static::CONST` performs late static
    /// binding, which can resolve to a different class at runtime than
    /// the one lexically containing this code - unlike `self::CONST`
    /// (always the literal declaring class) - so a `private` constant
    /// reached this way is unsafe even when access would otherwise be
    /// legal from here: `Base::make()` reading `static::SECRET`, called
    /// through a `Child` that redeclares its own `private const SECRET`,
    /// throws a real "Cannot access private constant Child::SECRET".
    /// Exempts a `final` class: it can never have a subclass, so
    /// `static::` there can never actually differ from the literal
    /// declaring class. The caller (`ExprKind::ClassConstFetch`
    /// arm in `checker/expr.rs`) already confirmed the class-part is the
    /// literal `static` keyword and that the constant is `private` before
    /// calling this.
    pub(crate) fn check_static_private_constant_access(
        &mut self,
        owner: &str,
        member: &str,
        span: Span,
    ) {
        self.check_static_private_member_access(StaticPrivateAccessCheck {
            verb: "access",
            noun: "constant",
            identifier: "staticClassAccess.privateConstant",
            owner,
            member_display: member,
            span,
        });
    }

    /// Same idea as `check_static_private_constant_access` just above,
    /// for a `private` static property instead - a static property
    /// shares the same late-static-binding unsafety: a subclass
    /// redeclaring the same `private static` property makes
    /// `static::$prop`, read from inside the ancestor's own method,
    /// throw a real "Cannot access private property" error at the call
    /// site.
    pub(crate) fn check_static_private_property_access(
        &mut self,
        owner: &str,
        member: &str,
        span: Span,
    ) {
        self.check_static_private_member_access(StaticPrivateAccessCheck {
            verb: "access",
            noun: "property",
            identifier: "staticClassAccess.privateProperty",
            owner,
            member_display: &format!("${member}"),
            span,
        });
    }

    /// Same idea as `check_static_private_constant_access`/
    /// `check_static_private_property_access` above, for a `private`
    /// static method call instead: a subclass redeclaring the same
    /// `private static` method makes `static::method()`, called from
    /// inside the ancestor's own method, throw a real "Call to private
    /// method ... from scope ..." error at the call site.
    pub(crate) fn check_static_private_method_access(
        &mut self,
        owner: &str,
        member: &str,
        span: Span,
    ) {
        self.check_static_private_member_access(StaticPrivateAccessCheck {
            verb: "call",
            noun: "method",
            identifier: "staticClassAccess.privateMethod",
            owner,
            member_display: &format!("{member}()"),
            span,
        });
    }

    /// Shared by the three `check_static_private_*_access` wrappers above.
    /// All three are the identical "skip inside a `final` class,
    /// otherwise report" shape, differing only in the verb
    /// (`"access"`/`"call"`), noun, identifier, and exactly-formatted
    /// member name (a constant's own bare name, a property's `$`-
    /// prefixed one, or a method's own `()`-suffixed one) - bundled into
    /// one struct for the same reason `MemberVisibilityCheck` above is.
    fn check_static_private_member_access(&mut self, check: StaticPrivateAccessCheck) {
        let StaticPrivateAccessCheck {
            verb,
            noun,
            identifier,
            owner,
            member_display,
            span,
        } = check;
        let is_final = self
            .current_class
            .as_deref()
            .and_then(|c| self.registry.class_by_name(c))
            .is_some_and(|info| info.is_final());
        if is_final {
            return;
        }
        self.report(
            format!("unsafe {verb} to private {noun} {owner}::{member_display} through static::"),
            span,
            0,
            identifier,
        );
    }

    /// Real PHP's `Closure::bind`/`->bindTo()` own third argument
    /// (`$newScope`): resolves the confidently-known-class shapes -
    /// `Foo::class` and a plain, already-registered string literal
    /// (`'Foo'`) - and nothing else (a variable, a computed expression,
    /// ... all correctly return `None`, "don't guess," rather than
    /// falling back to `$this`'s own type the way real PHP's own
    /// implicit default does, which this project has no way to recover
    /// once already checking the closure literal outside its original
    /// declaration site).
    pub(crate) fn literal_class_name_from_expr(&self, e: &Expr) -> Option<String> {
        match &e.kind {
            ExprKind::ClassNameFetch(target) => target
                .as_name()
                .and_then(|n| self.resolve_static_class_name(n)),
            ExprKind::Literal(Literal::String(s)) => {
                let s = s.as_str()?.trim_start_matches('\\');
                self.registry.has_class(s).then(|| s.to_string())
            }
            _ => None,
        }
    }
}
