//! Phase 57: `public`/`private`/`protected` access-control enforcement -
//! `PropertySig`/`FunctionSig` now carry a real `visibility` field (see
//! their own doc comments), but nothing consulted it until this file.
//! Real PHPStan's own identifiers: `property.private`/`property.protected`
//! (`AccessPropertiesCheck`, shared by read and plain write - the
//! asymmetric `private(set)`/`protected(set)` PHP 8.4 half this project
//! doesn't model gets its own separate `assign.property{Private,Protected}Set`
//! identifiers real PHPStan uses, not attempted here), `method.private`/
//! `method.protected` (`MethodCallCheck`), `staticMethod.private`/
//! `staticMethod.protected` (`StaticMethodCallCheck`).

use crate::ast::{Expr, ExprKind, Literal, Visibility};
use crate::checker::Checker;
use crate::token::Span;

/// `check_member_visibility`'s own call bundle - 7 positional
/// parameters tripped clippy's `too_many_arguments` lint, and a named-
/// field bundle is clearer at each call site anyway (mirrors this
/// project's own `CheckArgsCall` precedent, `calls.rs`).
pub(crate) struct MemberVisibilityCheck<'a> {
    pub(crate) kind: &'a str,
    pub(crate) owner: &'a str,
    pub(crate) member_name: &'a str,
    pub(crate) visibility: &'a Visibility,
    pub(crate) describe: &'a str,
    pub(crate) span: Span,
    pub(crate) label: &'a str,
}

impl<'a> Checker<'a> {
    /// Real PHP's own private/protected access-control rule, as far as
    /// this project can practically check it: `Visibility::Public` is
    /// always legal; `Private` only from within the exact declaring
    /// class; `Protected` from within the declaring class or anywhere in
    /// its inheritance chain. The direct check for `Protected` (checked
    /// both directions - `current_class` a subclass of `owner`, or
    /// `owner` a subclass of `current_class`) covers the common cases,
    /// but real corpus verification found a real gap: `owner` is
    /// whichever class's own lookup actually *resolved* the member (the
    /// nearest concrete override, for an inherited `abstract protected`
    /// one), not necessarily the ancestor that first declared it - two
    /// sibling subclasses of a common `abstract class Base { abstract
    /// protected function f(); }`, each independently overriding `f()`,
    /// are still mutually protected-accessible in real PHP (real shape:
    /// `Http1Driver`/`Http2Driver`, siblings under `AbstractHttpDriver`,
    /// `Http1Driver::write()` calling `$this->http2Driver->write()`).
    /// `member_name`/`is_method` is the fallback for exactly this case:
    /// if `current_class`'s own hierarchy independently resolves a
    /// member of the identical name (regardless of its own visibility -
    /// finding *anything* there is enough to prove a shared ancestor
    /// contract exists), access is allowed. Not perfectly precise (two
    /// same-named, genuinely unrelated protected members in unconnected
    /// hierarchies would also pass), but strictly more accurate than
    /// the direct check alone, and consistent with this project's own
    /// "don't guess toward a false positive" precedent elsewhere.
    fn is_member_access_visible(
        &self,
        owner: &str,
        visibility: &Visibility,
        member_name: &str,
        is_method: bool,
    ) -> bool {
        match visibility {
            Visibility::Public => true,
            Visibility::Private => self.current_class.as_deref() == Some(owner),
            Visibility::Protected => match self.current_class.as_deref() {
                Some(c) => {
                    c == owner
                        || self.registry.is_subclass_of(c, owner)
                        || self.registry.is_subclass_of(owner, c)
                        || if is_method {
                            self.registry.lookup_method(c, member_name).is_some()
                        } else {
                            self.registry.lookup_property(c, member_name).is_some()
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
            kind,
            owner,
            member_name,
            visibility,
            describe,
            span,
            label,
        } = check;
        let word = match visibility {
            Visibility::Public => return,
            Visibility::Private => "private",
            Visibility::Protected => "protected",
        };
        let is_method = kind != "property";
        if self.is_member_access_visible(owner, visibility, member_name, is_method) {
            return;
        }
        self.report(
            label.to_string(),
            format!("access to {word} {describe}"),
            span,
            0,
            &format!("{kind}.{word}"),
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
            ExprKind::StaticAccess(class_name, member) if member == "class" => {
                self.resolve_static_class_name(class_name)
            }
            ExprKind::Literal(Literal::String(s)) => {
                let s = s.trim_start_matches('\\');
                self.registry.classes.contains_key(s).then(|| s.to_string())
            }
            _ => None,
        }
    }
}
