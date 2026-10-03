//! Parts of a diagnostic's message, as values that print themselves: what a
//! rule that reports rarely holds is kept as its parts and formatted only
//! when the diagnostic is made.

use std::fmt;

/// The kind of class member an access is to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MemberKind {
    Method,
    StaticMethod,
    Property,
    StaticProperty,
    Constant,
}

impl MemberKind {
    /// The prefix of PHPStan's identifiers for the member: `method`,
    /// `staticMethod`, `property`, `staticProperty`, `classConstant`.
    pub(crate) fn identifier_prefix(self) -> &'static str {
        match self {
            MemberKind::Method => "method",
            MemberKind::StaticMethod => "staticMethod",
            MemberKind::Property => "property",
            MemberKind::StaticProperty => "staticProperty",
            MemberKind::Constant => "classConstant",
        }
    }
}

/// A member of a class, as a message names it: `method Foo::bar()`,
/// `static method Foo::bar()`, `property Foo::$bar`, `static property
/// Foo::$bar`, `constant Foo::BAR`.
#[derive(Clone, Copy)]
pub(crate) struct MemberLabel<'a> {
    pub(crate) kind: MemberKind,
    pub(crate) owner: &'a str,
    pub(crate) name: &'a str,
}

impl fmt::Display for MemberLabel<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (owner, name) = (self.owner, self.name);
        match self.kind {
            MemberKind::Method => write!(f, "method {owner}::{name}()"),
            MemberKind::StaticMethod => write!(f, "static method {owner}::{name}()"),
            MemberKind::Property => write!(f, "property {owner}::${name}"),
            MemberKind::StaticProperty => write!(f, "static property {owner}::${name}"),
            MemberKind::Constant => write!(f, "constant {owner}::{name}"),
        }
    }
}

/// The callee of a call, as this project's own opt-in messages name it.
#[derive(Clone, Copy)]
pub(crate) enum CalleeLabel<'a> {
    /// A function, or text that says what is called.
    Text(&'a str),
    /// A method of a class, as the call wrote them: `Foo::bar`.
    Method { class: &'a str, method: &'a str },
    /// A class's constructor: `Foo::__construct`.
    Constructor(&'a str),
}

impl fmt::Display for CalleeLabel<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalleeLabel::Text(text) => f.write_str(text),
            CalleeLabel::Method { class, method } => write!(f, "{class}::{method}"),
            CalleeLabel::Constructor(class) => write!(f, "{class}::__construct"),
        }
    }
}
