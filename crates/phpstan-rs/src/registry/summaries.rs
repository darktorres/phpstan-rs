//! What the finishing passes read of a function or method body, in symbolic
//! form: made once, for every body of a file, by one walk over the body
//! (on whichever thread parses the file), and resolved against the
//! registry by each pass. A pass reads a summary and not the statements, so
//! nothing but the summary has to be kept of a body to settle its verdicts.
//!
//! - Purity (`registry::purity`): whether a construct that makes a body
//!   impure occurs in it, and the calls whose own purity decides the rest.
//! - Effects (`registry::effects`): the places the body writes, what its
//!   variables are bound to, and the calls it inherits writes from.
//! - Assigned properties (`checker::uninitialized`): the properties the body
//!   assigns anywhere, on `$this` or on a variable it made an instance of.

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use syntax::ast::*;
use syntax::name::Name;
use syntax::visit::arena::{self, Visitor};
use types::Type;

use crate::registry::name_context::resolve_names_in_type;

use crate::registry::fresh::{FreshCall, FreshReturns};
use crate::registry::retention::{ParamUse, ParamUses, ThisCall, ThisUse};

/// A call a body's purity is conditional on, in the form the body wrote it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum PurityCall {
    /// A function called by name.
    Function(QualifiedName),
    /// A method called on `$this`.
    ThisMethod(Name),
    /// A static call on a class named by `self`, `static`, `parent` or a
    /// name, of a method.
    Static(QualifiedName, Name),
    /// `new` of a class named by `self`, `static` or a name.
    New(QualifiedName),
}

/// Where a write lands, as the body wrote it: a root, and the properties
/// and elements walked from it, the root's first.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct Place {
    pub(crate) root: Root,
    pub(crate) steps: Box<[Step]>,
    /// What a guard around the place shows of the variable it starts from.
    pub(crate) narrowing: Option<Narrowing>,
}

/// What a guard shows of a variable in the code it guards.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Narrowing {
    /// The variable holds no object: `is_array($x)`, `$x === null`.
    NoObject,
    /// The variable holds, when it holds an object, one of these classes:
    /// `$x instanceof C`.
    Instance(Box<[QualifiedName]>),
    /// The variable holds a value of this type, as an inline `@var` says.
    Declared(Type),
}

/// What a place starts from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Root {
    /// `$this`.
    This,
    /// A variable other than `$this` and the superglobals; `None` for one
    /// whose name is computed (`$$n`, `${…}`) or not written (`extract`).
    Variable(Option<Name>),
    /// `X::$p`, the class as written; `None` for a computed property name.
    Static(QualifiedName, Option<Name>),
    /// A static property of a class computed at run time (`$c::$p`).
    DynamicStatic(Option<Name>),
    /// A global: `$GLOBALS['x']` or a superglobal, by name; `None` for
    /// `$GLOBALS[$k]`.
    Global(Option<Name>),
    /// Any other value: a call's result, a new object, a literal.
    Value,
}

/// One step from a place's root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Step {
    /// `->p`; `None` for a computed name.
    Property(Option<Name>),
    /// `[k]`.
    Element,
}

impl Place {
    fn root(root: Root) -> Place {
        Place {
            root,
            steps: Box::new([]),
            narrowing: None,
        }
    }

    /// Any other value: a call's result, a new object, a literal.
    pub(crate) fn value() -> Place {
        Place::root(Root::Value)
    }

    /// This place, then `step`.
    pub(crate) fn then(self, step: Step) -> Place {
        let mut steps = self.steps.into_vec();
        steps.push(step);
        Place {
            root: self.root,
            steps: steps.into_boxed_slice(),
            narrowing: self.narrowing,
        }
    }

    /// The place `e` names, when it names one: a variable, a property, an
    /// element, a static property or a global, through any nesting.
    pub(crate) fn of(arenas: &NodeArenas, id: ExprId) -> Option<Place> {
        let e = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
        syntax::stack::guard(|| {
            Some(match &e.kind {
                ExprKind::Variable(v) if **v == *"this" => Place::root(Root::This),
                ExprKind::Variable(v) if is_superglobal(v) && **v != *"GLOBALS" => {
                    Place::root(Root::Global(Some(v.clone())))
                }
                ExprKind::Variable(v) => Place::root(Root::Variable(Some(v.clone()))),
                ExprKind::DynamicVariable(_) => Place::root(Root::Variable(None)),
                ExprKind::PropertyAccess(base, prop, _) => Place::of(arenas, *base)
                    .unwrap_or_else(|| Place::root(Root::Value))
                    .then(Step::Property(Some(prop.name.clone()))),
                ExprKind::DynamicPropertyAccess(base, _, _) => Place::of(arenas, *base)
                    .unwrap_or_else(|| Place::root(Root::Value))
                    .then(Step::Property(None)),
                ExprKind::Index(base, key) => {
                    let base_node = arenas.peeled(*base).and_then(|id| arenas.expr(id))?;
                    if matches!(&base_node.kind, ExprKind::Variable(v) if **v == *"GLOBALS") {
                        let name = (*key)
                            .and_then(|id| arenas.peeled(id).and_then(|id| arenas.expr(id)))
                            .and_then(|node| literal_string_value(arenas, node.id))
                            .map(|name| Name::from(&*name));
                        return Some(Place::root(Root::Global(name)));
                    }
                    Place::of(arenas, *base)
                        .unwrap_or_else(|| Place::root(Root::Value))
                        .then(Step::Element)
                }
                ExprKind::StaticPropertyFetch(StaticTarget::Name(class), prop) => {
                    Place::root(Root::Static((**class).clone(), Some(prop.name.clone())))
                }
                ExprKind::StaticPropertyFetch(StaticTarget::Expr(_), prop) => {
                    Place::root(Root::DynamicStatic(Some(prop.name.clone())))
                }
                ExprKind::DynamicStaticPropertyFetch(StaticTarget::Name(class), _) => {
                    Place::root(Root::Static((**class).clone(), None))
                }
                ExprKind::DynamicStaticPropertyFetch(StaticTarget::Expr(_), _) => {
                    Place::root(Root::DynamicStatic(None))
                }
                _ => return None,
            })
        })
    }
}

/// What a variable of a body is bound to.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Binding {
    /// `global $x`: the global of that name; `None` for `global $$n`.
    Global(Option<Name>),
    /// `static $x`: the declaration's static variable.
    Static,
    /// `$x = &place`, `foreach (place as &$x)`: a reference to the place.
    Reference(Place),
}

/// What a variable only ever holds, when it holds nothing the body did not
/// make.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Made {
    /// An object the body made: by `new` of the class named (`self`,
    /// `static` and `parent` as written), or of a class not named (`clone`,
    /// `new $c`, `new class`).
    Object(Option<QualifiedName>),
    /// A closure, an arrow function or a first-class callable the body
    /// wrote: one of those its summary lists at these indices.
    Closure(Box<[u32]>),
}

impl Made {
    /// What a variable holds that holds what `self` or `other` says: `None`
    /// when that is not only what the body made.
    pub(crate) fn join(&self, other: &Made) -> Option<Made> {
        match (self, other) {
            (Made::Closure(a), Made::Closure(b)) => {
                let mut ids: Vec<u32> = a.iter().chain(b.iter()).copied().collect();
                ids.sort_unstable();
                ids.dedup();
                Some(Made::Closure(ids.into_boxed_slice()))
            }
            (Made::Object(a), Made::Object(b)) => Some(Made::Object(
                a.as_ref()
                    .zip(b.as_ref())
                    .filter(|(a, b)| a.written.eq_ignore_ascii_case(&b.written))
                    .map(|(a, _)| a.clone()),
            )),
            _ => None,
        }
    }
}

/// The object a method is called on, as far as the body shows it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Receiver {
    /// `$this`.
    This,
    /// A property of `$this`.
    ThisProperty(Name),
    /// Another variable.
    Variable(Name),
    /// A static property, the class as written.
    Static(QualifiedName, Name),
    /// A value whose origins the body shows: a call's result, an element,
    /// a `new`, a constant.
    Origins(Arc<[Origin]>),
    /// Any other value.
    Unknown,
}

/// What a call is to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Callee {
    Function(QualifiedName),
    /// `$receiver->method()`; `None` for a computed method name.
    Method {
        receiver: Receiver,
        method: Option<Name>,
    },
    /// `Class::method()`, the class as written; `None` for a class computed
    /// at run time, or a computed method name.
    Static {
        class: Option<QualifiedName>,
        method: Option<Name>,
    },
    /// `new Class`; `None` for a class computed at run time.
    New(Option<QualifiedName>),
    /// `new class(...)`, whose class has this name.
    Anonymous(Option<Name>),
    /// A value called: the variable holding it, or `None` for any other
    /// expression.
    Value(Option<Name>),
    /// A closure, an arrow function or a first-class callable the body
    /// wrote, called where it is written: one of those its summary lists at
    /// these indices.
    Written(Box<[u32]>),
    /// What PHP runs for an operation on a value, no call written: on a
    /// value from `on`, or from anywhere when `None`.
    Implicit {
        on: Option<Arc<[Origin]>>,
        operation: Operation,
    },
    /// `parent::$property::get()`, or `::set()` when `set`: the parent
    /// class's hook of the property, on `$this`.
    ParentHook {
        property: Name,
        set: bool,
    },
    /// An `include` or `require`, by the id of its expression in the file:
    /// the top level of the file it names, run in the scope it is written in.
    Include(u32),
}

/// An operation on a value through which PHP may run the value's code.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Operation {
    /// An access to the property of the name, `None` for a computed one:
    /// `__get`, `__set`, `__isset` or `__unset` where it is magic.
    Property(Option<Name>, Access),
    /// An access to an element: `offsetGet`, `offsetSet`, `offsetExists` or
    /// `offsetUnset` of an `ArrayAccess`.
    Element(Access),
    /// A conversion to a string: `__toString`.
    String,
    /// `clone`, with properties given when `with`: `__clone`, and a write
    /// of each property given.
    Clone { with: bool },
    /// A `foreach` over the value, its keys taken when `keys`: an
    /// `Iterator`'s methods, or an `IteratorAggregate`'s `getIterator` and
    /// those of the iterator it gives.
    Iterate { keys: bool },
    /// The value at the place stops being held, by an `unset` or an
    /// assignment over it: `__destruct` of an object nothing else holds.
    Destroy,
}

/// What an access does with a property or an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Access {
    Read,
    Write,
    /// `isset`, `empty` and `??` testing it.
    Isset,
    Unset,
}

/// Where a value comes from, as the body wrote it: what it is taken to be,
/// by the declarations, with no flow.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Origin {
    /// A value that is no object: a literal, a string, a boolean, an array.
    NoObject,
    /// What an arithmetic operator gives: a number, or an object of a class
    /// overloading the operator (`GMP`, `BcMath\Number`).
    Arithmetic,
    /// The value at the place.
    Place(Place),
    /// What the call returns.
    Returned(#[serde(with = "super::callee_table")] Arc<Callee>),
    /// An object `new` made of the class named (`self`, `static` and
    /// `parent` as written); `None` for a class computed at run time.
    New(Option<QualifiedName>),
    /// An object of the anonymous class of this name.
    Anonymous(Option<Name>),
    /// A closure, an arrow function or a first-class callable.
    Closure,
    /// An exception a `catch` caught, of one of the classes.
    Caught(Box<[QualifiedName]>),
    /// A global constant.
    Constant(QualifiedName),
    /// A class constant (an enum case too), the class as written.
    ClassConstant(QualifiedName, Name),
    /// A value of this type, as an inline `@var` says.
    Declared(Type),
    /// The objects held in the elements, at any depth, of an array from the
    /// origin.
    Elements(Arc<Origin>),
    /// What a call of the function takes out of the arrays from the first
    /// origins, and out of the objects the second hold: an element, or an
    /// array of elements, when the function is the builtin that does so
    /// (`takes_elements`); what the callee returns otherwise.
    Taken(
        #[serde(with = "super::callee_table")] Arc<Callee>,
        Arc<[Origin]>,
        Arc<[Origin]>,
    ),
    /// What the body writes the variable, not the argument a parameter taken
    /// by reference started as.
    Written(Name),
}

impl PartialEq for Narrowing {
    fn eq(&self, other: &Narrowing) -> bool {
        match (self, other) {
            (Narrowing::NoObject, Narrowing::NoObject) => true,
            (Narrowing::Instance(a), Narrowing::Instance(b)) => a == b,
            (Narrowing::Declared(a), Narrowing::Declared(b)) => {
                a.canonical_text() == b.canonical_text()
            }
            _ => false,
        }
    }
}

impl Eq for Narrowing {}

impl Hash for Narrowing {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Narrowing::NoObject => {}
            Narrowing::Instance(classes) => classes.hash(state),
            Narrowing::Declared(ty) => ty.canonical_text().hash(state),
        }
    }
}

impl PartialEq for Origin {
    fn eq(&self, other: &Origin) -> bool {
        match (self, other) {
            (Origin::NoObject, Origin::NoObject)
            | (Origin::Arithmetic, Origin::Arithmetic)
            | (Origin::Closure, Origin::Closure) => true,
            (Origin::Place(a), Origin::Place(b)) => a == b,
            (Origin::Returned(a), Origin::Returned(b)) => a == b,
            (Origin::New(a), Origin::New(b)) => a == b,
            (Origin::Anonymous(a), Origin::Anonymous(b)) => a == b,
            (Origin::Caught(a), Origin::Caught(b)) => a == b,
            (Origin::Constant(a), Origin::Constant(b)) => a == b,
            (Origin::ClassConstant(a, x), Origin::ClassConstant(b, y)) => a == b && x == y,
            (Origin::Declared(a), Origin::Declared(b)) => a.canonical_text() == b.canonical_text(),
            (Origin::Elements(a), Origin::Elements(b)) => a == b,
            (Origin::Taken(a, x, p), Origin::Taken(b, y, q)) => a == b && x == y && p == q,
            (Origin::Written(a), Origin::Written(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Origin {}

impl Hash for Origin {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Origin::NoObject | Origin::Arithmetic | Origin::Closure => {}
            Origin::Place(place) => place.hash(state),
            Origin::Returned(callee) => callee.hash(state),
            Origin::New(class) => class.hash(state),
            Origin::Anonymous(name) => name.hash(state),
            Origin::Caught(classes) => classes.hash(state),
            Origin::Constant(name) => name.hash(state),
            Origin::ClassConstant(class, constant) => {
                class.hash(state);
                constant.hash(state);
            }
            Origin::Declared(ty) => ty.canonical_text().hash(state),
            Origin::Elements(inner) => inner.hash(state),
            Origin::Taken(callee, from, held) => {
                callee.hash(state);
                from.hash(state);
                held.hash(state);
            }
            Origin::Written(name) => name.hash(state),
        }
    }
}

fn hash_of(value: &impl Hash) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

impl Origin {
    /// Whether it is the same from any scope: no variable of the scope it is
    /// written in decides it.
    fn context_free(&self) -> bool {
        match self {
            Origin::Place(_) | Origin::Returned(_) | Origin::Written(_) => false,
            Origin::Elements(inner) => inner.context_free(),
            Origin::Taken(..) => false,
            _ => true,
        }
    }

    /// Whether it is, or is the elements of, what a call takes out of an
    /// array.
    fn mentions_taken(&self) -> bool {
        match self {
            Origin::Taken(..) => true,
            Origin::Elements(inner) => inner.mentions_taken(),
            _ => false,
        }
    }

    /// Where the objects held in the elements of an array from here come
    /// from; `None` when it holds none a destructor is run for: no array, or
    /// an object whose properties are not followed.
    fn elements(self) -> Option<Origin> {
        match self {
            Origin::NoObject
            | Origin::Arithmetic
            | Origin::Closure
            | Origin::New(_)
            | Origin::Anonymous(_)
            | Origin::Caught(_) => None,
            origin => Some(Origin::Elements(Arc::new(origin))),
        }
    }

    /// Whether a value from here is no object PHP runs code of for an
    /// operation.
    fn runs_nothing(&self) -> bool {
        matches!(
            self,
            Origin::NoObject | Origin::Arithmetic | Origin::Closure
        )
    }
}

/// Whether the value `id` is no object and holds none at any depth.
fn holds_no_object(arenas: &NodeArenas, id: ExprId) -> bool {
    let Some(e) = arenas.peeled(id).and_then(|id| arenas.expr(id)) else {
        return false;
    };
    !matches!(e.kind, ExprKind::ArrayLiteral(_))
        && origins_of(arenas, id).is_some_and(|origins| origins.iter().all(Origin::runs_nothing))
}

/// Where the value `e` evaluates to comes from; `None` when that is not
/// known.
fn origins_of(arenas: &NodeArenas, id: ExprId) -> Option<Vec<Origin>> {
    let e = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    syntax::stack::guard(|| {
        Some(match &e.kind {
            ExprKind::Literal(_)
            | ExprKind::Interpolated(_)
            | ExprKind::ArrayLiteral(_)
            | ExprKind::ClassNameFetch(_)
            | ExprKind::Isset(_)
            | ExprKind::Empty(_)
            | ExprKind::Instanceof(..)
            | ExprKind::ShellExec(_)
            | ExprKind::Print(_)
            | ExprKind::Exit(..)
            | ExprKind::Throw(_)
            | ExprKind::Unary(UnaryOp::Not, _) => vec![Origin::NoObject],
            ExprKind::Unary(_, _) => vec![Origin::Arithmetic],
            // An object stays itself; anything else becomes a `stdClass`.
            ExprKind::Cast(CastSpelling::Object, inner) => {
                let mut out = origins_of(arenas, *inner)?;
                out.push(Origin::New(Some(QualifiedName {
                    written: Name::from("stdClass"),
                    resolved: ResolvedName::Exact(Name::from("stdClass")),
                    span: arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                })));
                out
            }
            ExprKind::Cast(..) => vec![Origin::NoObject],
            ExprKind::Binary(op, l, r) => match op {
                BinOp::Coalesce => {
                    let mut out = origins_of(arenas, *l)?;
                    out.extend(origins_of(arenas, *r)?);
                    out
                }
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::Pow
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr => vec![Origin::Arithmetic],
                BinOp::Pipe => return None,
                BinOp::Concat
                | BinOp::Eq
                | BinOp::Identical
                | BinOp::NotEq
                | BinOp::NotIdentical
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::LtEq
                | BinOp::GtEq
                | BinOp::Spaceship
                | BinOp::And
                | BinOp::Or
                | BinOp::LogicalAnd
                | BinOp::LogicalOr
                | BinOp::LogicalXor => vec![Origin::NoObject],
            },
            ExprKind::Assign(_, rhs) | ExprKind::AssignRef(_, rhs) => origins_of(arenas, *rhs)?,
            ExprKind::CompoundAssign(op, lhs, rhs) => match op {
                BinOp::Concat => vec![Origin::NoObject],
                BinOp::Coalesce => {
                    let mut out = origins_of(arenas, *lhs)?;
                    out.extend(origins_of(arenas, *rhs)?);
                    out
                }
                _ => vec![Origin::Arithmetic],
            },
            ExprKind::Ternary(cond, then, otherwise) => {
                let mut out = origins_of(arenas, (*then).unwrap_or(*cond))?;
                out.extend(origins_of(arenas, *otherwise)?);
                out
            }
            ExprKind::Match { arms, .. } => {
                let mut out = Vec::new();
                for arm in arenas.match_arms(arms.clone()) {
                    out.extend(origins_of(arenas, arm.body)?);
                }
                out
            }
            ExprKind::Clone(inner, _) => origins_of(arenas, *inner)?,
            ExprKind::Variable(_)
            | ExprKind::DynamicVariable(_)
            | ExprKind::PropertyAccess(..)
            | ExprKind::DynamicPropertyAccess(..)
            | ExprKind::Index(..)
            | ExprKind::StaticPropertyFetch(..)
            | ExprKind::DynamicStaticPropertyFetch(..) => {
                vec![Origin::Place(Place::of(arenas, e.id)?)]
            }
            ExprKind::Call(callee, args) => match arenas
                .peeled(*callee)
                .and_then(|id| arenas.expr(id))
                .map(|callee| &callee.kind)
            {
                Some(ExprKind::Name(name)) => {
                    let name = arenas.qualified_name(name)?.clone();
                    let taken = takes_elements(&name.written)
                        .then(|| arenas.args(args.clone()))
                        .and_then(|args| match args {
                            [first, ..] if !first.spread && first.name.is_none() => {
                                origins_of(arenas, first.value)
                            }
                            _ => None,
                        });
                    match taken {
                        Some(from) => vec![Origin::Taken(
                            Arc::new(Callee::Function(name)),
                            from.into(),
                            Arc::from([]),
                        )],
                        None => vec![Origin::Returned(Arc::new(Callee::Function(name)))],
                    }
                }
                Some(ExprKind::Variable(name)) if **name != *"this" => {
                    vec![Origin::Returned(Arc::new(Callee::Value(Some(
                        name.clone(),
                    ))))]
                }
                _ => return None,
            },
            ExprKind::MethodCall(recv, method, _, _) => {
                vec![Origin::Returned(Arc::new(Callee::Method {
                    receiver: receiver(arenas, *recv),
                    method: Some(method.name.clone()),
                }))]
            }
            ExprKind::StaticCall(target, method, _) => {
                vec![Origin::Returned(Arc::new(static_callee(
                    arenas, target, method,
                )))]
            }
            ExprKind::New(callee, _) => vec![Origin::New(
                match arenas
                    .peeled(*callee)
                    .and_then(|id| arenas.expr(id))
                    .map(|callee| &callee.kind)
                {
                    Some(ExprKind::Name(name)) => Some(arenas.qualified_name(name)?.clone()),
                    _ => None,
                },
            )],
            ExprKind::NewAnonymousClass(class_id) => vec![Origin::Anonymous(
                arenas.anonymous_class_name(*class_id).cloned(),
            )],
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
                vec![Origin::Closure]
            }
            ExprKind::Name(name) => {
                let name = arenas.qualified_name(name)?;
                let written = name.written.trim_start_matches('\\');
                // A magic constant is a string or an integer.
                if written.len() > 4 && written.starts_with("__") && written.ends_with("__") {
                    vec![Origin::NoObject]
                } else {
                    vec![Origin::Constant(name.clone())]
                }
            }
            ExprKind::ClassConstFetch(StaticTarget::Name(class), constant) => {
                vec![Origin::ClassConstant(
                    (**class).clone(),
                    constant.name.clone(),
                )]
            }
            ExprKind::Grouping(_)
            | ExprKind::ErrorSuppress(_)
            | ExprKind::Destructure(..)
            | ExprKind::DynamicMethodCall(..)
            | ExprKind::DynamicStaticCall(..)
            | ExprKind::ClassConstFetch(..)
            | ExprKind::DynamicClassConstFetch(..)
            | ExprKind::Eval(_)
            | ExprKind::Include(..)
            | ExprKind::Yield(..)
            | ExprKind::YieldFrom(_) => return None,
        })
    })
}

/// Whether the expression `id` is a value made where it is written and held
/// by nothing: an object a `new`, a `clone` or a call gives.
fn is_temporary(arenas: &NodeArenas, id: ExprId) -> bool {
    matches!(
        arenas
            .peeled(id)
            .and_then(|id| arenas.expr(id))
            .map(|e| &e.kind),
        Some(
            ExprKind::New(..)
                | ExprKind::Clone(..)
                | ExprKind::Call(..)
                | ExprKind::MethodCall(..)
                | ExprKind::DynamicMethodCall(..)
                | ExprKind::StaticCall(..)
                | ExprKind::DynamicStaticCall(..)
        )
    )
}

/// Whether the expression `id` is an array made where it is written: an array
/// literal, or what a call gives.
fn holds_elements_made_here(arenas: &NodeArenas, id: ExprId) -> bool {
    matches!(
        arenas
            .peeled(id)
            .and_then(|id| arenas.expr(id))
            .map(|e| &e.kind),
        Some(
            ExprKind::ArrayLiteral(..)
                | ExprKind::Call(..)
                | ExprKind::MethodCall(..)
                | ExprKind::DynamicMethodCall(..)
                | ExprKind::StaticCall(..)
                | ExprKind::DynamicStaticCall(..)
        )
    )
}

/// Where the elements of a value from `origins` come from: an element of
/// each place; `None` when one is not a place.
fn element_origins(origins: Vec<Origin>) -> Option<Vec<Origin>> {
    origins
        .into_iter()
        .map(|origin| match origin {
            Origin::Place(place) => Some(Origin::Place(place.then(Step::Element))),
            _ => None,
        })
        .collect()
}

/// Whether `name`, as written, is a builtin that returns an element of its
/// first argument, an array of them, or the elements it removes from it.
pub(crate) fn takes_elements(name: &str) -> bool {
    let name = name.strip_prefix('\\').unwrap_or(name);
    [
        "array_pop",
        "array_shift",
        "array_splice",
        "reset",
        "end",
        "next",
        "prev",
        "current",
    ]
    .iter()
    .any(|taker| name.eq_ignore_ascii_case(taker))
}

/// Whether the value `e` evaluates to may be a string, as an operand of a
/// loose comparison.
fn may_be_string(arenas: &NodeArenas, id: ExprId) -> bool {
    let Some(e) = arenas.peeled(id).and_then(|id| arenas.expr(id)) else {
        return true;
    };
    !matches!(
        &e.kind,
        ExprKind::Literal(Literal::Int(_) | Literal::Float(_) | Literal::Bool(_) | Literal::Null)
            | ExprKind::ArrayLiteral(_)
            | ExprKind::Isset(_)
            | ExprKind::Empty(_)
            | ExprKind::Instanceof(..)
            | ExprKind::Unary(..)
            | ExprKind::Cast(
                CastSpelling::Int
                    | CastSpelling::Integer
                    | CastSpelling::Float
                    | CastSpelling::Real
                    | CastSpelling::Double
                    | CastSpelling::Bool
                    | CastSpelling::Boolean
                    | CastSpelling::Array
                    | CastSpelling::Object
                    | CastSpelling::Unset
                    | CastSpelling::Void,
                _
            )
            | ExprKind::Binary(
                BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Mod
                    | BinOp::Pow
                    | BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor
                    | BinOp::Shl
                    | BinOp::Shr
                    | BinOp::Eq
                    | BinOp::Identical
                    | BinOp::NotEq
                    | BinOp::NotIdentical
                    | BinOp::Lt
                    | BinOp::Gt
                    | BinOp::LtEq
                    | BinOp::GtEq
                    | BinOp::Spaceship
                    | BinOp::And
                    | BinOp::Or
                    | BinOp::LogicalAnd
                    | BinOp::LogicalOr
                    | BinOp::LogicalXor,
                _,
                _
            )
    )
}

/// Where an argument is passed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ArgAt {
    Position(u32),
    Named(Name),
    /// `...$args`, and every argument after one.
    Spread,
}

/// A callable an argument names, as the body wrote it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Callable {
    /// A string: a function's name, or `Class::method`.
    Text(Name),
    /// `[target, 'method']`.
    Pair(PairTarget, Name),
    /// A closure, an arrow function or a first-class callable the body
    /// wrote, by the index its summary lists it at.
    Written(u32),
}

/// The first element of an array callable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum PairTarget {
    /// An object: `$this`, a property of it, a variable or a static
    /// property, as a method's receiver.
    Object(Receiver),
    /// A class, by a string or `Class::class`.
    Class(QualifiedName),
}

impl Summarizer<'_> {
    /// The callable `e` names, when the body wrote one.
    fn callable(&mut self, arenas: &NodeArenas, id: ExprId) -> Option<Callable> {
        let id = arenas.peeled(id)?;
        let e = arenas.expr(id)?;
        match &e.kind {
            ExprKind::Literal(Literal::String(text)) => Some(Callable::Text(Name::from(
                std::str::from_utf8(arenas.string_bytes(text)?).ok()?,
            ))),
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
                Some(Callable::Written(self.closure_id(id)))
            }
            ExprKind::ArrayLiteral(items) => {
                let items = arenas.array_items(items.clone());
                let [first, second] = items else {
                    return None;
                };
                let positional = |item: &ArrayItem, at: i64| {
                    !item.spread
                        && !item.by_ref
                        && match &item.key {
                            None => true,
                            Some(key) => arenas
                                .peeled(*key)
                                .and_then(|key| arenas.expr(key))
                                .is_some_and(|key| matches!(&key.kind, ExprKind::Literal(Literal::Int(n)) if *n == at)),
                        }
                };
                if !positional(first, 0) || !positional(second, 1) {
                    return None;
                }
                let method = literal_string_value(
                    arenas,
                    arenas
                        .peeled(second.value)
                        .and_then(|id| arenas.expr(id))?
                        .id,
                )?;
                let first_value = arenas.peeled(first.value).and_then(|id| arenas.expr(id))?;
                let target = match &first_value.kind {
                    // A class a string names is fully qualified.
                    ExprKind::Literal(Literal::String(text)) => {
                        let name = Name::from(
                            std::str::from_utf8(arenas.string_bytes(text)?)
                                .ok()?
                                .trim_start_matches('\\'),
                        );
                        PairTarget::Class(QualifiedName {
                            written: name.clone(),
                            resolved: ResolvedName::Exact(name),
                            span: arenas.expr_span(first.value)?,
                        })
                    }
                    ExprKind::ClassNameFetch(StaticTarget::Name(class)) => {
                        PairTarget::Class((**class).clone())
                    }
                    _ => PairTarget::Object(receiver(arenas, first.value)),
                };
                Some(Callable::Pair(target, Name::from(&*method)))
            }
            ExprKind::Assign(_, rhs) => syntax::stack::guard(|| self.callable(arenas, *rhs)),
            _ => None,
        }
    }
}

/// The destroy a call result makes at the end of its statement, held on the
/// call's record in `CallWrites::dies`: the result's own, and its contents'.
pub(crate) const DIES_TEMPORARY: u8 = 1;
pub(crate) const DIES_CONTENTS: u8 = 2;
/// Marks, in a debug build, the operation the summary would have recorded for
/// a held destroy, for the classifier to check the held one against.
pub(crate) const DIES_SHADOW: u8 = 4;

/// A call in a body, with its arguments that are places, where a
/// by-reference parameter writes them, and those that name a callable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CallWrites {
    pub(crate) callee: Callee,
    /// Where each argument is passed.
    pub(crate) arguments: Box<[ArgAt]>,
    pub(crate) places: Box<[(ArgAt, Place)]>,
    /// The arguments that are no place and hold no object at any depth: a
    /// literal, a constant, a `::class` name.
    pub(crate) objectless: Box<[ArgAt]>,
    pub(crate) callables: Box<[(ArgAt, Callable)]>,
    /// The arguments the call writes, if it is the builtin it is written as,
    /// in a way the summary already applied to the variable (`array_push`,
    /// `sort`, `preg_match`, see `filler`): they are not lost to the call.
    pub(crate) fills: Box<[ArgAt]>,
    /// The destroys the call's result makes, `DIES_TEMPORARY` and
    /// `DIES_CONTENTS`, which the classifier reads as operations of the call.
    #[serde(default)]
    pub(crate) dies: u8,
}

/// What a body or a closure in it writes, before any declaration is looked
/// up. A closure, an arrow function or a first-class callable it writes is
/// not run by being written: what it writes is its own.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WriteFacts {
    /// The variables the body assigns, which a declared type of a parameter
    /// then no longer describes.
    pub(crate) reassigned: Box<[Name]>,
    /// The variables the body assigns over, or an element of: the value one
    /// held when the body started, if it is a parameter taken by reference,
    /// stops being held there.
    pub(crate) overwritten: Box<[Name]>,
    /// Every place the body writes.
    pub(crate) writes: Box<[Place]>,
    /// What its variables are bound to; `None` for a variable whose name is
    /// computed (`global $$n`), which may be any.
    pub(crate) bindings: Box<[(Option<Name>, Binding)]>,
    /// The variables written, each with what it only ever holds when that is
    /// what the body made, `None` otherwise. A body's own lists only those
    /// that hold what it made; a closure's every variable it writes, since
    /// one it takes from the code that wrote it holds what that code made
    /// only while the closure writes it nothing else.
    pub(crate) made: Box<[(Name, Option<Made>)]>,
    /// The variables written whose every value written comes from where
    /// the body shows, each with where; one written a value not known is
    /// not listed.
    pub(crate) origins: Box<[(Name, Box<[Origin]>)]>,
    /// The variables whose origins die when the scope ends: what their
    /// origins name is destroyed then, the classifier reads it from `origins`.
    #[serde(default)]
    pub(crate) dying_locals: Box<[Name]>,
    /// A variable whose name is computed is written (`$$n = …`,
    /// `extract()`): any variable may hold anything.
    pub(crate) computed: bool,
    /// A construct that may write anything: `eval` or `include`.
    pub(crate) anything: bool,
    /// Every call, in the order the body makes them, and each operation
    /// through which PHP may run code, once.
    pub(crate) calls: Box<[CallWrites]>,
}

/// A closure, an arrow function or a first-class callable a body wrote.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClosureFacts {
    /// The closure it is written in, by its index; `None` when the body
    /// wrote it itself.
    pub(crate) within: Option<u32>,
    /// Its parameters, in order.
    pub(crate) params: Box<[ClosureParam]>,
    /// The variables it takes from the code that wrote it, each with whether
    /// it takes it by reference; `None` for every variable but its
    /// parameters, by value, as an arrow function and a first-class callable
    /// take them.
    pub(crate) captures: Option<Box<[(Name, bool)]>>,
    /// The classes the objects it returns may be of, when its declared return
    /// type says no more than classes by name and what is no object; `None`
    /// when it returns anything else, or declares nothing.
    pub(crate) returns: Option<Box<[Name]>>,
    pub(crate) facts: WriteFacts,
}

/// A closure's or an arrow function's parameter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClosureParam {
    pub(crate) name: Name,
    /// The classes its declared type admits objects of (`self`, `static` and
    /// `parent` as written); `None` for an object of any class.
    pub(crate) classes: Option<Box<[QualifiedName]>>,
    /// Taken by reference, or variadic: its value is not its declared type's
    /// all through the body.
    pub(crate) untyped: bool,
}

impl ClosureParam {
    fn of(param: &Param, arenas: &NodeArenas) -> ClosureParam {
        ClosureParam {
            name: param.name.name.clone(),
            classes: hint_classes(param.type_hint.as_ref(), arenas),
            untyped: param.by_ref || param.variadic,
        }
    }
}

/// The classes a value of the declared type `hint` may be an object of;
/// `None` for any class.
fn hint_classes(hint: Option<&TypeHint>, arenas: &NodeArenas) -> Option<Box<[QualifiedName]>> {
    fn add(hint: &TypeHint, arenas: &NodeArenas, out: &mut Vec<QualifiedName>) -> Option<()> {
        let keyword = |name: &str| QualifiedName {
            written: Name::from(name),
            resolved: ResolvedName::Exact(Name::from(name)),
            span: hint.span,
        };
        match &hint.kind {
            TypeHintKind::Keyword(native, _) => match native {
                NativeType::Object | NativeType::Mixed | NativeType::Callable => return None,
                NativeType::Iterable => out.push(keyword("Traversable")),
                NativeType::SelfType => out.push(keyword("self")),
                NativeType::Static => out.push(keyword("static")),
                NativeType::Parent => out.push(keyword("parent")),
                NativeType::Int
                | NativeType::Float
                | NativeType::String
                | NativeType::Bool
                | NativeType::Array
                | NativeType::Void
                | NativeType::Never
                | NativeType::Null
                | NativeType::True
                | NativeType::False => {}
            },
            TypeHintKind::Name(names) => out.extend(arenas.qualified_names(names).iter().cloned()),
            TypeHintKind::Nullable(inner) => add(inner, arenas, out)?,
            TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => {
                for member in arenas.type_hints(members) {
                    add(member, arenas, out)?;
                }
            }
        }
        Some(())
    }
    let mut out = Vec::new();
    add(hint?, arenas, &mut out)?;
    Some(out.into_boxed_slice())
}

/// The classes the objects of a value of the declared type `hint` may be of,
/// when it admits no value that holds objects without being one: `None`
/// for an array, `iterable`, `mixed`, `object`, `callable`, a type relative to
/// the class, and what is not declared.
fn return_classes(hint: Option<&TypeHint>, arenas: &NodeArenas) -> Option<Box<[Name]>> {
    fn add(hint: &TypeHint, arenas: &NodeArenas, out: &mut Vec<Name>) -> Option<()> {
        match &hint.kind {
            TypeHintKind::Keyword(native, _) => match native {
                NativeType::Int
                | NativeType::Float
                | NativeType::String
                | NativeType::Bool
                | NativeType::Void
                | NativeType::Never
                | NativeType::Null
                | NativeType::True
                | NativeType::False => Some(()),
                _ => None,
            },
            TypeHintKind::Name(names) => {
                for name in arenas.qualified_names(names) {
                    out.push(name.resolved_class_name()?.clone());
                }
                Some(())
            }
            TypeHintKind::Nullable(inner) => add(inner, arenas, out),
            TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => {
                for member in arenas.type_hints(members) {
                    add(member, arenas, out)?;
                }
                Some(())
            }
        }
    }
    let mut out = Vec::new();
    add(hint?, arenas, &mut out)?;
    Some(out.into_boxed_slice())
}

/// A body, read by the passes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BodySummary {
    /// The calls the body's purity is conditional on, or `None` when the body
    /// holds a construct that makes it impure whatever they are.
    pub(crate) purity: Option<Box<[PurityCall]>>,
    pub(crate) writes: WriteFacts,
    /// The closures, arrow functions and first-class callables the body
    /// wrote, those written in them too, each by its index; one is listed
    /// after the one it is written in.
    pub(crate) closures: Box<[ClosureFacts]>,
    /// Whether the body has no statements.
    pub(crate) empty: bool,
    /// The properties the body assigns, when it assigns any.
    pub(crate) assigned: Option<Box<AssignedProperties>>,
    /// What the body does with its parameters, when it has any.
    pub(crate) parameters: Option<Box<ParamUses>>,
    /// What the body returns, when it returns only objects it made.
    pub(crate) fresh: Option<Box<FreshReturns>>,
}

impl BodySummary {
    /// The summary of code that only writes `e`, a closure, an arrow
    /// function or a first-class callable: the first closure it lists.
    pub(crate) fn of_closure(arenas: &NodeArenas, id: ExprId) -> BodySummary {
        let mut walk = Summarizer::default();
        walk.visit_expr(arenas, id);
        walk.finish()
    }

    /// The summary of the statements of one body.
    pub fn of(arenas: &NodeArenas, body: &[StmtId]) -> BodySummary {
        BodySummary::of_function(arenas, &[], body)
    }

    /// The summary of the top level of a file: the statements that run when
    /// it is included, which are all but the declarations it makes.
    pub fn of_file(arenas: &NodeArenas, stmts: &[StmtId]) -> BodySummary {
        let running: Vec<StmtId> = stmts
            .iter()
            .copied()
            .filter(|id| {
                !matches!(
                    arenas.stmt(*id).map(|stmt| &stmt.kind),
                    Some(
                        StmtKind::FunctionDecl(_)
                            | StmtKind::ClassDecl(_)
                            | StmtKind::Namespace(_)
                            | StmtKind::Use(_)
                    )
                )
            })
            .collect();
        BodySummary::summarize(arenas, &[], &running, Some(stmts), false)
    }

    /// The summary of the body of a function with the parameters `params`:
    /// a constructor's assigns each property it promotes, first.
    pub fn of_function(arenas: &NodeArenas, params: &[Param], body: &[StmtId]) -> BodySummary {
        BodySummary::of_function_in(arenas, params, body, None)
    }

    /// `of_function` for a body in a file whose statements are `file`, which
    /// gives the names its inline `@var` tags are written in.
    pub fn of_function_in(
        arenas: &NodeArenas,
        params: &[Param],
        body: &[StmtId],
        file: Option<&[StmtId]>,
    ) -> BodySummary {
        BodySummary::summarize(arenas, params, body, file, true)
    }

    /// The summary of `body`; the variables it wrote die with it when
    /// `ends`, which a file's top level does not (they are globals).
    fn summarize(
        arenas: &NodeArenas,
        params: &[Param],
        body: &[StmtId],
        file: Option<&[StmtId]>,
        ends: bool,
    ) -> BodySummary {
        let mut walk = Summarizer {
            names: file,
            by_ref_params: params
                .iter()
                .filter(|param| param.by_ref)
                .map(|param| param.name.name.clone())
                .collect(),
            ..Summarizer::default()
        };
        for param in params.iter().filter(|param| param.promotion.is_some()) {
            walk.implicit_on(
                Some(vec![Origin::Place(Place::root(Root::This))]),
                Operation::Property(Some(param.name.name.clone()), Access::Write),
            );
            walk.write_this_property(&param.name.name);
        }
        arena::walk_stmts(&mut walk, arenas, body);
        walk.flush_dies();
        walk.merge_references();
        if ends {
            let by_ref = params
                .iter()
                .filter(|param| param.by_ref)
                .map(|param| param.name.name.clone())
                .collect();
            walk.destroy_locals(&by_ref, &FxHashSet::default());
        }
        let mut summary = walk.finish();
        summary.empty = body.is_empty();
        let yields = arena::body_contains_yield(arenas, body);
        (summary.parameters, summary.fresh) =
            crate::registry::body_passes::summarize(arenas, params, body, yields);
        summary
    }

    /// The summary of the body of `hook`, a hook of `property`, `set` when
    /// it is its `set` hook; `None` when it has none. A `set` hook written
    /// as an expression assigns its value to the property.
    pub fn of_hook(
        arenas: &NodeArenas,
        hook: &PropertyHook,
        property: &Name,
        set: bool,
    ) -> Option<BodySummary> {
        match &hook.body {
            PropertyHookBody::Block(stmts) => Some(BodySummary::of(arenas, arenas.stmt_ids(stmts))),
            PropertyHookBody::Arrow(e) => {
                let mut walk = Summarizer::default();
                walk.visit_expr(arenas, *e);
                if set {
                    walk.write_this_property(property);
                }
                Some(walk.finish())
            }
            PropertyHookBody::Abstract => None,
        }
    }
}

/// The properties a body assigns anywhere in its statements, however deeply
/// nested and however conditional, outside closures: `$this->p = ...`,
/// `$this->p ??= ...`, and the same on a local variable the body
/// assigned a `new` expression.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AssignedProperties {
    /// The properties assigned on `$this`.
    on_this: Box<[Name]>,
    /// The variable and the property assigned on it, for each assignment.
    on_variables: Box<[(Name, Name)]>,
    /// The variable and the class named by the `new` it was assigned (a
    /// leading backslash trimmed; `self` and `static` as written).
    new_instances: Box<[(Name, Name)]>,
}

/// Collects what the expression statements of a body assign.
#[derive(Default)]
struct AssignedBuilder {
    on_this: Vec<Name>,
    on_variables: Vec<(Name, Name)>,
    new_instances: Vec<(Name, Name)>,
}

impl AssignedBuilder {
    /// The expression of a statement of the body, outside its closures.
    fn statement(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        match &e.kind {
            ExprKind::Assign(lhs, rhs) => {
                if let Some(ExprNode {
                    kind: ExprKind::Variable(v),
                    ..
                }) = arenas.expr(*lhs)
                    && let Some(class) = new_instance_class(arenas, *rhs)
                {
                    self.new_instances.push((v.clone(), class));
                }
                record_property_assigned(arenas, *lhs, &mut self.on_this, &mut self.on_variables);
            }
            ExprKind::AssignRef(lhs, _) | ExprKind::CompoundAssign(BinOp::Coalesce, lhs, _) => {
                record_property_assigned(arenas, *lhs, &mut self.on_this, &mut self.on_variables);
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Option<Box<AssignedProperties>> {
        if self.on_this.is_empty() && self.on_variables.is_empty() {
            return None;
        }
        self.on_this.sort();
        self.on_this.dedup();
        self.on_variables.sort();
        self.on_variables.dedup();
        self.new_instances.sort();
        self.new_instances.dedup();
        Some(Box::new(AssignedProperties {
            on_this: self.on_this.into_boxed_slice(),
            on_variables: self.on_variables.into_boxed_slice(),
            new_instances: self.new_instances.into_boxed_slice(),
        }))
    }
}

impl AssignedProperties {
    /// Whether the body assigns `prop` on `$this` or on a variable it made
    /// an instance of `class_name` (or of `self` or `static`).
    pub(crate) fn assigns(&self, prop: &str, class_name: &str) -> bool {
        if self.on_this.iter().any(|name| **name == *prop) {
            return true;
        }
        self.on_variables.iter().any(|(variable, assigned)| {
            **assigned == *prop
                && self
                    .new_instances
                    .iter()
                    .any(|(v, class)| v == variable && is_self_instance_class(class, class_name))
        })
    }
}

/// The class a `new` names, at the end of a chain of assignments
/// (`$a = $b->p = new C`).
fn new_instance_class(arenas: &NodeArenas, id: ExprId) -> Option<Name> {
    let e = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    syntax::stack::guard(|| match &e.kind {
        ExprKind::New(callee, _) => {
            let ExprKind::Name(name) = &arenas.peeled(*callee).and_then(|id| arenas.expr(id))?.kind
            else {
                return None;
            };
            let name = arenas.qualified_name(name)?;
            Some(Name::from(name.trim_start_matches('\\')))
        }
        ExprKind::Assign(_, rhs) => new_instance_class(arenas, *rhs),
        _ => None,
    })
}

fn is_self_instance_class(class: &str, class_name: &str) -> bool {
    class.eq_ignore_ascii_case("self")
        || class.eq_ignore_ascii_case("static")
        || class == class_name
        || class.ends_with(&format!("\\{class_name}"))
}

fn record_property_assigned(
    arenas: &NodeArenas,
    id: ExprId,
    on_this: &mut Vec<Name>,
    on_variables: &mut Vec<(Name, Name)>,
) {
    let Some(lhs) = arenas.expr(id) else { return };
    let ExprKind::PropertyAccess(base, property, _) = &lhs.kind else {
        return;
    };
    let Some(base) = arenas.expr(*base) else {
        return;
    };
    let ExprKind::Variable(v) = &base.kind else {
        return;
    };
    if **v == *"this" {
        on_this.push(property.name.clone());
    } else {
        on_variables.push((v.clone(), property.name.clone()));
    }
}

/// Origins, not known when `None`.
type MaybeOrigins = Option<Vec<Origin>>;

/// `known` with `more` added, not known once either is not.
fn extend_known(known: &mut MaybeOrigins, more: MaybeOrigins) {
    *known = match (known.take(), more) {
        (Some(mut all), Some(more)) => {
            all.extend(more);
            Some(all)
        }
        _ => None,
    };
}

/// A variable a `foreach` by reference bound to the elements of another.
#[derive(Debug)]
struct ElementRef {
    subject: Name,
    item: Name,
    // Where what the loop writes to the variable comes from, or `None` once
    // that is not known.
    written: MaybeOrigins,
    // Whether the body assigns the variable, which destroys the element it
    // is the reference to.
    overwritten: bool,
    // Whether the variable is bound to the other itself, not to its elements.
    whole: bool,
}

/// What one scope writes, as the walk finds it: the body's own, or a
/// closure's.
#[derive(Default)]
struct Scope {
    reassigned: FxHashSet<Name>,
    overwritten: FxHashSet<Name>,
    writes: Vec<Place>,
    bindings: Vec<(Option<Name>, Binding)>,
    // What each variable written holds: what the body made, or `None` once
    // it is written anything else.
    made: FxHashMap<Name, Option<Made>>,
    // Where what each variable written holds comes from, or `None` once it
    // is written a value not known.
    origins: FxHashMap<Name, Option<Vec<Origin>>>,
    // Where the objects held in the arrays each variable written holds come
    // from, or `None` once they are not known.
    contents: FxHashMap<Name, Option<Vec<Origin>>>,
    // The variables a `foreach` by reference bound to the elements of the
    // variable, in that order: what they hold, the elements hold.
    element_refs: Vec<ElementRef>,
    computed: bool,
    anything: bool,
    calls: Vec<CallWrites>,
    // The implicit operations listed in `calls`, each with the key it was
    // recorded under. A later rewrite of a listed call's `on` leaves the key
    // as it was, which is what the table and the printed set both hold.
    implicit_keys: Vec<(Option<Arc<[Origin]>>, Operation)>,
    // The positions in `implicit_keys` of the implicit operations, found by
    // their structure.
    implicit: hashbrown::HashTable<u32>,
    // The destroys of call results waiting for their call's record.
    pending_dies: Vec<PendingDies>,
    // The variables whose origins die when the scope ends, in the order
    // `destroy_locals` takes them.
    dying_locals: Vec<Name>,
    #[cfg(debug_assertions)]
    implicit_printed: FxHashSet<String>,
}

impl Scope {
    /// The scope's facts; `all_written` keeps every variable written in
    /// `made`, not only those holding what the body made.
    fn finish(self, all_written: bool) -> WriteFacts {
        let mut reassigned: Vec<Name> = self.reassigned.into_iter().collect();
        reassigned.sort();
        let mut overwritten: Vec<Name> = self.overwritten.into_iter().collect();
        overwritten.sort();
        let mut made: Vec<(Name, Option<Made>)> = self
            .made
            .into_iter()
            .filter(|(_, made)| all_written || made.is_some())
            .collect();
        made.sort_by(|a, b| a.0.cmp(&b.0));
        let mut origins: Vec<(Name, Box<[Origin]>)> = self
            .origins
            .into_iter()
            .filter_map(|(name, origins)| Some((name, distinct_origins(origins?))))
            .collect();
        origins.sort_by(|a, b| a.0.cmp(&b.0));
        WriteFacts {
            reassigned: reassigned.into_boxed_slice(),
            overwritten: overwritten.into_boxed_slice(),
            writes: self.writes.into_boxed_slice(),
            bindings: self.bindings.into_boxed_slice(),
            made: made.into_boxed_slice(),
            origins: origins.into_boxed_slice(),
            dying_locals: self.dying_locals.into_boxed_slice(),
            computed: self.computed,
            anything: self.anything,
            calls: self.calls.into_boxed_slice(),
        }
    }
}

/// A destroy a call result makes, waiting for the record of its call.
struct PendingDies {
    id: u32,
    bit: u8,
    source: HeldSource,
}

/// What a held destroy's operand is: the callee of a call, or the class of a
/// `new`.
enum HeldSource {
    Call(Arc<Callee>),
    New(Option<syntax::ast::QualifiedName>),
}

/// The call whose result `id` holds: `id` itself, or the call a grouping, a
/// suppressed call or a clone of a call holds.
fn result_call(arenas: &NodeArenas, id: ExprId) -> Option<ExprId> {
    let mut at = id;
    loop {
        match &arenas.expr(at)?.kind {
            ExprKind::Call(..)
            | ExprKind::MethodCall(..)
            | ExprKind::StaticCall(..)
            | ExprKind::New(..) => return Some(at),
            ExprKind::Grouping(inner)
            | ExprKind::ErrorSuppress(inner)
            | ExprKind::Clone(inner, _) => {
                at = *inner;
            }
            _ => return None,
        }
    }
}

/// `origins` without the repeats, in order.
fn distinct_origins(origins: Vec<Origin>) -> Box<[Origin]> {
    let mut kept: Vec<Origin> = Vec::with_capacity(origins.len());
    let mut seen: hashbrown::HashTable<u32> = hashbrown::HashTable::with_capacity(origins.len());
    #[cfg(debug_assertions)]
    let mut printed: FxHashSet<String> = FxHashSet::default();
    for origin in origins {
        let hash = hash_of(&origin);
        let new = seen.find(hash, |&at| kept[at as usize] == origin).is_none();
        #[cfg(debug_assertions)]
        assert_eq!(new, printed.insert(syntax::canonical::print(&origin)));
        if new {
            seen.insert_unique(hash, kept.len() as u32, |&at| hash_of(&kept[at as usize]));
            kept.push(origin);
        }
    }
    kept.into_boxed_slice()
}

/// A scope the walk is inside of, while it walks a closure written in it.
struct Enclosing {
    scope: Scope,
    // The variables the closure written in it takes from it by reference.
    shared: FxHashSet<Name>,
}

#[derive(Default)]
struct Summarizer<'a> {
    // How many closures and arrow functions the walk is inside: they are
    // not run by being made, so their statements are no part of the body's
    // purity.
    closures: u32,
    impure: bool,
    purity_calls: Vec<PurityCall>,
    assigned: AssignedBuilder,
    scope: Scope,
    // The scopes around the one the walk is in, the outermost first.
    enclosing: Vec<Enclosing>,
    // The closure the walk is in, by its index; `None` in the body's own
    // scope.
    current: Option<u32>,
    // The call the walk is recording, by its expression, for `call` to take.
    call_id: Option<u32>,
    // Each closure's index, by its file-local expression id.
    ids: FxHashMap<ExprId, u32>,
    written: Vec<ClosureFacts>,
    // What an access does with a property or an element, by its expression
    // id, when not only a read (`READ`, `WRITE`, `ISSET`,
    // `UNSET`).
    accesses: FxHashMap<ExprId, u8>,
    // What the guards around the code the walk is in show of variables of
    // the scope it is in, the innermost last.
    narrowed: Vec<(Name, Narrowing)>,
    // The statements of the file the body is in, to find the names its
    // inline `@var` tags are written in; `None` where they are not read.
    names: Option<&'a [StmtId]>,
    // The inline `@var` tags of the statement the walk is in that assign
    // their variable there: the type it is taken to hold.
    declared: Vec<(Name, Type)>,
    // The body's parameters taken by reference.
    by_ref_params: FxHashSet<Name>,
}

const READ: u8 = 1;
const WRITE: u8 = 2;
const ISSET: u8 = 4;
const UNSET: u8 = 8;

impl Summarizer<'_> {
    fn finish(mut self) -> BodySummary {
        self.flush_dies();
        BodySummary {
            purity: (!self.impure).then(|| self.purity_calls.into_boxed_slice()),
            writes: self.scope.finish(false),
            closures: self.written.into_boxed_slice(),
            empty: false,
            assigned: self.assigned.finish(),
            parameters: None,
            fresh: None,
        }
    }

    /// The index of the closure, arrow function or first-class callable
    /// `e`, given it when it is first met.
    fn closure_id(&mut self, id: ExprId) -> u32 {
        let next = self.written.len() as u32;
        let closure = *self.ids.entry(id).or_insert(next);
        if closure == next {
            // Until the walk reaches it, it may write anything.
            self.written.push(ClosureFacts {
                within: None,
                params: Box::new([]),
                captures: None,
                returns: None,
                facts: WriteFacts {
                    anything: true,
                    ..WriteFacts::default()
                },
            });
        }
        closure
    }

    /// Walks the closure, arrow function or first-class callable `e` in a
    /// scope of its own, which takes `captures` from the one the walk is in.
    fn closure(
        &mut self,
        arenas: &NodeArenas,
        id: ExprId,
        params: &[Param],
        captures: Option<Box<[(Name, bool)]>>,
        returns: Option<Box<[Name]>>,
        walk: impl FnOnce(&mut Summarizer),
    ) {
        let id = self.closure_id(id);
        let shared = captures
            .iter()
            .flatten()
            .filter(|(_, by_ref)| *by_ref)
            .map(|(name, _)| name.clone())
            .collect();
        self.enclosing.push(Enclosing {
            scope: std::mem::take(&mut self.scope),
            shared,
        });
        let within = self.current.replace(id);
        let narrowed = std::mem::take(&mut self.narrowed);
        walk(self);
        let by_ref = params
            .iter()
            .filter(|param| param.by_ref)
            .map(|param| param.name.name.clone())
            .collect();
        let shared = self
            .enclosing
            .last()
            .map(|enclosing| enclosing.shared.clone())
            .unwrap_or_default();
        self.flush_dies();
        self.merge_references();
        self.destroy_locals(&by_ref, &shared);
        self.narrowed = narrowed;
        let Some(enclosing) = self.enclosing.pop() else {
            unreachable!("pushed above");
        };
        let scope = std::mem::replace(&mut self.scope, enclosing.scope);
        self.current = within;
        self.written[id as usize] = ClosureFacts {
            within,
            params: params
                .iter()
                .map(|param| ClosureParam::of(param, arenas))
                .collect(),
            captures,
            returns,
            facts: scope.finish(true),
        };
    }

    /// The scope the walk is in, and each around it sharing the variable
    /// `name` with it by reference.
    fn scopes_sharing(&mut self, name: &Name) -> Vec<&mut Scope> {
        let mut out = vec![&mut self.scope];
        for enclosing in self.enclosing.iter_mut().rev() {
            if !enclosing.shared.contains(name) {
                break;
            }
            out.push(&mut enclosing.scope);
        }
        out
    }

    /// A construct that makes the body impure, outside a closure.
    fn impure_unless_in_closure(&mut self, impure: bool) {
        if impure && self.closures == 0 {
            self.impure = true;
        }
    }

    fn purity_call(&mut self, call: PurityCall) {
        if self.closures == 0 && !self.impure {
            self.purity_calls.push(call);
        }
    }

    /// An assignment to `target`: each variable it names is reassigned.
    fn reassign(&mut self, arenas: &NodeArenas, target: ExprId) {
        let mut names = FxHashSet::default();
        collect_assigned(arenas, target, &mut names);
        for name in names {
            for scope in self.scopes_sharing(&name) {
                scope.reassigned.insert(name.clone());
            }
        }
    }

    /// A write to `target` of a value from `origins`, not known when
    /// `None`: to each target of a destructuring, of an element of it, to
    /// the place `target` names otherwise. A variable written holds what
    /// the body made when `made` says what.
    fn write(
        &mut self,
        arenas: &NodeArenas,
        target: ExprId,
        made: Option<Made>,
        origins: Option<Vec<Origin>>,
    ) {
        let Some(target_node) = arenas.peeled(target).and_then(|id| arenas.expr(id)) else {
            return;
        };
        if let ExprKind::Destructure(items, _) = &target_node.kind {
            let element = origins.and_then(element_origins);
            for item in arenas.destructure_items(items) {
                let Some(target) = &item.target else {
                    continue;
                };
                let Some(target_node) = arenas.peeled(*target).and_then(|id| arenas.expr(id))
                else {
                    continue;
                };
                match &target_node.kind {
                    // `[&$x] = …`: a reference to the element.
                    ExprKind::Variable(name) if item.by_ref && **name != *"this" => {
                        let place = match element.as_deref() {
                            Some([Origin::Place(place)]) => place.clone(),
                            _ => Place::value(),
                        };
                        self.bind(Some(name.clone()), Binding::Reference(place));
                    }
                    _ => self.write(arenas, *target, None, element.clone()),
                }
            }
            return;
        }
        let Some(place) = Place::of(arenas, target) else {
            return;
        };
        let place = if place.steps.is_empty() {
            place
        } else {
            self.narrow_place(place)
        };
        match (&place.root, &*place.steps) {
            (Root::Variable(Some(name)), []) => self.note_variable(name.clone(), made, origins),
            (Root::Variable(None), []) => self.write_computed(),
            _ => {}
        }
        self.scope.writes.push(place);
    }

    /// That the variable `name` is written what `made` says, or anything
    /// else when `made` is `None`, of a value from `origins`, not known when
    /// `None`: in the scope the walk is in, and in those sharing it by
    /// reference.
    fn note_variable(&mut self, name: Name, made: Option<Made>, origins: Option<Vec<Origin>>) {
        let contents = origins
            .as_deref()
            .and_then(|origins| self.contents_from_origins(origins));
        for (depth, scope) in self.scopes_sharing(&name).into_iter().enumerate() {
            let held = scope
                .made
                .entry(name.clone())
                .or_insert_with(|| made.clone());
            *held = match (held.take(), &made) {
                (Some(a), Some(b)) => a.join(b),
                _ => None,
            };
            // Where a value a closure writes comes from is the same to the
            // code around it only where none of the closure's variables
            // decides it.
            let origins = origins
                .as_ref()
                .filter(|origins| depth == 0 || origins.iter().all(Origin::context_free));
            let known = scope
                .origins
                .entry(name.clone())
                .or_insert_with(|| Some(Vec::new()));
            match (known.as_mut(), origins) {
                (Some(known), Some(origins)) => known.extend(origins.iter().cloned()),
                _ => *known = None,
            }
            for element_ref in scope.element_refs.iter_mut().filter(|r| r.item == name) {
                element_ref.written = match (element_ref.written.take(), origins) {
                    (Some(mut all), Some(origins)) => {
                        all.extend(origins.iter().cloned());
                        Some(all)
                    }
                    _ => None,
                };
            }
        }
        self.note_contents(&name, contents);
    }

    /// That the variable `name` holds, in the arrays it holds, objects from
    /// `contents`, not known when `None`: in the scope the walk is in, and
    /// in those sharing it by reference.
    fn note_contents(&mut self, name: &Name, contents: Option<Vec<Origin>>) {
        for (depth, scope) in self.scopes_sharing(name).into_iter().enumerate() {
            let contents = contents
                .as_ref()
                .filter(|contents| depth == 0 || contents.iter().all(Origin::context_free));
            let known = scope
                .contents
                .entry(name.clone())
                .or_insert_with(|| Some(Vec::new()));
            match (known.as_mut(), contents) {
                (Some(known), Some(contents)) => {
                    for origin in contents {
                        if !known.contains(origin) {
                            known.push(origin.clone());
                        }
                    }
                }
                _ => *known = None,
            }
        }
    }

    /// Where the objects held in the elements, at any depth, of an array
    /// from `origins` come from; `None` when that is not known.
    fn contents_from_origins(&self, origins: &[Origin]) -> Option<Vec<Origin>> {
        let mut out = Vec::new();
        for origin in origins {
            match origin {
                Origin::Place(place) => match (&place.root, &*place.steps) {
                    (Root::Variable(Some(name)), []) if self.scope.contents.contains_key(name) => {
                        out.extend(self.scope.contents[name].as_ref()?.iter().cloned());
                    }
                    _ => out.push(Origin::Elements(Arc::new(origin.clone()))),
                },
                other => out.extend(other.clone().elements()),
            }
        }
        Some(out)
    }

    /// Where the objects held in the elements, at any depth, of the array
    /// the value `id` is come from, and those it holds directly when it is
    /// an array literal; `None` when that is not known.
    fn contents_of(&self, arenas: &NodeArenas, id: ExprId) -> Option<Vec<Origin>> {
        let e = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
        syntax::stack::guard(|| match &e.kind {
            ExprKind::ArrayLiteral(items) => {
                let mut out = Vec::new();
                for item in arenas.array_items(items.clone()) {
                    if item.spread {
                        out.extend(self.contents_from_origins(&self.origins(arenas, item.value)?)?);
                    } else {
                        out.extend(self.origins(arenas, item.value)?);
                        out.extend(self.contents_of(arenas, item.value)?);
                    }
                }
                Some(out)
            }
            ExprKind::Ternary(cond, then, otherwise) => {
                let mut out = self.contents_of(arenas, (*then).unwrap_or(*cond))?;
                out.extend(self.contents_of(arenas, *otherwise)?);
                Some(out)
            }
            ExprKind::Binary(BinOp::Coalesce, l, r) => {
                let mut out = self.contents_of(arenas, *l)?;
                out.extend(self.contents_of(arenas, *r)?);
                Some(out)
            }
            ExprKind::Match { arms, .. } => {
                let mut out = Vec::new();
                for arm in arenas.match_arms(arms.clone()) {
                    out.extend(self.contents_of(arenas, arm.body)?);
                }
                Some(out)
            }
            ExprKind::Assign(_, rhs) => self.contents_of(arenas, *rhs),
            _ => self.contents_from_origins(&self.origins(arenas, id)?),
        })
    }

    /// That the value `rhs`, from `origins`, was written to the variable
    /// `lhs`, or to an element or a property of it: the variable holds the
    /// objects it holds.
    fn note_stored(
        &mut self,
        arenas: &NodeArenas,
        lhs: ExprId,
        rhs: ExprId,
        origins: Option<&[Origin]>,
    ) {
        let Some(Place {
            root: Root::Variable(Some(name)),
            steps,
            ..
        }) = Place::of(arenas, lhs)
        else {
            return;
        };
        if *name == *"this" {
            return;
        }
        let stored = if steps.is_empty() {
            self.contents_of(arenas, rhs)
        } else {
            origins.map(<[Origin]>::to_vec).and_then(|mut stored| {
                stored.extend(self.contents_of(arenas, rhs)?);
                Some(stored)
            })
        };
        self.note_contents(&name, stored);
    }

    /// A write to a variable whose name is computed: any variable of the
    /// scope may hold anything, and any it shares by reference with the
    /// scopes around it.
    fn write_computed(&mut self) {
        self.scope.computed = true;
        let shared: Vec<Name> = self
            .enclosing
            .last()
            .map(|enclosing| enclosing.shared.iter().cloned().collect())
            .unwrap_or_default();
        for name in shared {
            self.note_variable(name.clone(), None, None);
            for scope in self.scopes_sharing(&name) {
                scope.reassigned.insert(name.clone());
            }
        }
    }

    /// The value the variable `name` holds dies when it is bound to a place:
    /// what it was written, and the objects in the arrays it held.
    fn destroy_held(&mut self, name: &Name) {
        let Some(origins) = self.scope.origins.get(name).cloned() else {
            return;
        };
        let contents = self
            .scope
            .contents
            .get(name)
            .cloned()
            .unwrap_or(Some(Vec::new()));
        self.implicit_on(origins, Operation::Destroy);
        self.implicit_on(contents, Operation::Destroy);
    }

    fn bind(&mut self, variable: Option<Name>, binding: Binding) {
        match &variable {
            Some(name) => {
                self.scope.made.insert(name.clone(), None);
                self.scope.origins.insert(name.clone(), None);
                for element_ref in self
                    .scope
                    .element_refs
                    .iter_mut()
                    .filter(|r| r.item == *name)
                {
                    element_ref.written = None;
                }
            }
            None => self.write_computed(),
        }
        self.scope.bindings.push((variable, binding));
    }

    /// That the expression `target` does what `access` says with what it
    /// accesses.
    fn mark(&mut self, arenas: &NodeArenas, target: ExprId, access: u8) {
        let Some(target) = arenas.peeled(target) else {
            return;
        };
        *self.accesses.entry(target).or_default() |= access;
    }

    /// A write to the property `property` of `$this`.
    fn write_this_property(&mut self, property: &Name) {
        self.scope
            .writes
            .push(Place::root(Root::This).then(Step::Property(Some(property.clone()))));
    }

    /// The narrowing of the variable `name` where the walk is, if any.
    fn narrowing_of(&self, name: &Name) -> Option<&Narrowing> {
        self.narrowed
            .iter()
            .rev()
            .find(|(narrowed, _)| narrowed == name)
            .map(|(_, narrowing)| narrowing)
    }

    /// `place`, with what the guards around the walk show of its variable.
    fn narrow_place(&self, mut place: Place) -> Place {
        if let Root::Variable(Some(name)) = &place.root
            && place.narrowing.is_none()
        {
            place.narrowing = self.narrowing_of(name).cloned();
        }
        place
    }

    fn narrow_origin(&self, origin: Origin) -> Origin {
        match origin {
            Origin::Place(place) => Origin::Place(self.narrow_place(place)),
            Origin::Returned(callee) => Origin::Returned(self.narrow_callee(callee)),
            other => other,
        }
    }

    fn narrow_callee(&self, callee: Arc<Callee>) -> Arc<Callee> {
        match &*callee {
            Callee::Method { receiver, method } => Arc::new(Callee::Method {
                receiver: self.narrow_receiver(receiver.clone()),
                method: method.clone(),
            }),
            Callee::Implicit { on, operation } => Arc::new(Callee::Implicit {
                on: on.as_ref().map(|on| {
                    on.iter()
                        .map(|origin| self.narrow_origin(origin.clone()))
                        .collect()
                }),
                operation: operation.clone(),
            }),
            _ => callee,
        }
    }

    fn narrow_receiver(&self, receiver: Receiver) -> Receiver {
        match receiver {
            Receiver::Variable(name) if self.narrowing_of(&name).is_some() => {
                Receiver::Origins(Arc::new([Origin::Place(
                    self.narrow_place(Place::root(Root::Variable(Some(name)))),
                )]))
            }
            Receiver::Origins(origins) => Receiver::Origins(
                origins
                    .iter()
                    .map(|origin| self.narrow_origin(origin.clone()))
                    .collect(),
            ),
            other => other,
        }
    }

    /// Where the value `id` comes from, as far as the guards around the
    /// walk show.
    fn origins(&self, arenas: &NodeArenas, id: ExprId) -> Option<Vec<Origin>> {
        let origins = origins_of(arenas, id)?;
        Some(if self.narrowed.is_empty() {
            origins
        } else {
            origins
                .into_iter()
                .map(|origin| self.narrow_origin(origin))
                .collect()
        })
    }

    /// Every `Taken` origin the scope holds, taking from the variables it
    /// tracks the contents of what they hold at the end of the walk.
    fn resolve_taken(&mut self) {
        let has = |origins: &[Origin]| origins.iter().any(Origin::mentions_taken);
        let names: Vec<Name> = self.scope.origins.keys().cloned().collect();
        for name in names {
            for contents in [false, true] {
                let map = if contents {
                    &self.scope.contents
                } else {
                    &self.scope.origins
                };
                let Some(Some(origins)) = map.get(&name) else {
                    continue;
                };
                if !has(origins) {
                    continue;
                }
                let resolved = self.take_known_contents(origins.clone());
                let map = if contents {
                    &mut self.scope.contents
                } else {
                    &mut self.scope.origins
                };
                map.insert(name.clone(), Some(resolved));
            }
        }
        for index in 0..self.scope.calls.len() {
            let Callee::Implicit {
                on: Some(on),
                operation,
            } = &self.scope.calls[index].callee
            else {
                continue;
            };
            if !has(on) {
                continue;
            }
            let operation = operation.clone();
            let on: Arc<[Origin]> = self.take_known_contents(on.to_vec()).into();
            self.scope.calls[index].callee = Callee::Implicit {
                on: Some(on),
                operation,
            };
        }
    }

    /// The arrays a `Taken` origin takes from that are variables the walk
    /// tracks the contents of hold what it takes: the origins of those.
    fn take_known_contents(&self, origins: Vec<Origin>) -> Vec<Origin> {
        origins
            .into_iter()
            .map(|origin| self.take_known_contents_of(origin))
            .collect()
    }

    fn take_known_contents_of(&self, origin: Origin) -> Origin {
        match origin {
            Origin::Elements(inner) if inner.mentions_taken() => {
                Origin::Elements(Arc::new(self.take_known_contents_of((*inner).clone())))
            }
            Origin::Taken(callee, from, held) => {
                let mut arrays = Vec::new();
                let mut held = held.to_vec();
                for array in from.iter() {
                    let tracked = match array {
                        Origin::Place(Place {
                            root: Root::Variable(Some(name)),
                            steps,
                            ..
                        }) if steps.is_empty() => self.scope.contents.get(name),
                        _ => None,
                    };
                    match tracked {
                        Some(Some(contents)) => held.extend(contents.iter().cloned()),
                        _ => arrays.push(array.clone()),
                    }
                }
                Origin::Taken(callee, arrays.into(), held.into())
            }
            origin => origin,
        }
    }

    /// The object `id` is, as far as the guards around the walk show.
    fn receiver(&self, arenas: &NodeArenas, id: ExprId) -> Receiver {
        let receiver = receiver(arenas, id);
        if self.narrowed.is_empty() {
            receiver
        } else {
            self.narrow_receiver(receiver)
        }
    }

    /// The place `id` names, read, as far as the guards around the walk
    /// show.
    fn place_read(&self, arenas: &NodeArenas, id: ExprId) -> Option<Place> {
        let place = Place::of(arenas, id)?;
        Some(if self.narrowed.is_empty() {
            place
        } else {
            self.narrow_place(place)
        })
    }

    /// Takes `facts` as shown of the variables they name, for as long as the
    /// walk is in `region`, but for those written in it; the length to cut
    /// `narrowed` back to when it leaves.
    fn narrow(
        &mut self,
        arenas: &NodeArenas,
        facts: Vec<(Name, Narrowing)>,
        region: &[Node],
    ) -> usize {
        let mark = self.narrowed.len();
        if !facts.is_empty() {
            let written = written_in(arenas, region);
            self.narrowed.extend(
                facts
                    .into_iter()
                    .filter(|(name, _)| !written.contains(name)),
            );
        }
        mark
    }

    /// The variables the inline `@var` tags of the statement `id` give a
    /// type, with it.
    fn inline_var_tags(&self, arenas: &NodeArenas, id: StmtId) -> Vec<(Name, Type)> {
        let Some(file) = self.names else {
            return Vec::new();
        };
        let (Some(stmt), Some(span)) = (arenas.stmt(id), arenas.stmt_span(id)) else {
            return Vec::new();
        };
        if matches!(
            stmt.kind,
            StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) | StmtKind::GlobalConst(_)
        ) {
            return Vec::new();
        }
        let Some(site) = arenas.doc_site(span.start) else {
            return Vec::new();
        };
        if !site.text.contains("var") {
            return Vec::new();
        }
        let tags = phpdoc::var_tags::var_tag_nodes(&site.text);
        if tags.is_empty() {
            return Vec::new();
        }
        let names = syntax::name_resolution::context_at_ids(arenas, file, span.start);
        let unnamed = tags.iter().filter(|(name, _)| name.is_none()).count();
        let target = assigned_variable(arenas, id);
        let mut out = Vec::new();
        for (name, node) in tags.iter() {
            let name = match name {
                Some(name) => Name::from(name.trim_start_matches('$')),
                None if unnamed == 1 => match &target {
                    Some(target) => target.clone(),
                    None => continue,
                },
                None => continue,
            };
            let ty = resolve_names_in_type(crate::registry::parse_type_node(node), &names);
            out.push((name, ty));
        }
        out
    }

    /// The statements of a list in order: what an inline `@var` tag gives a
    /// variable holds from its statement on, and what a statement that
    /// leaves unless a condition holds shows of the rest of the list.
    fn walk_list(&mut self, arenas: &NodeArenas, stmts: &[StmtId]) {
        let mark = self.narrowed.len();
        for (at, &id) in stmts.iter().enumerate() {
            let rest: Vec<Node> = stmts[at + 1..]
                .iter()
                .map(|&stmt| Node::Stmt(stmt))
                .collect();
            let mut from_here = Vec::new();
            let mut after = Vec::new();
            let tags = self.inline_var_tags(arenas, id);
            if !tags.is_empty() {
                let written = written_in(arenas, &[Node::Stmt(id)]);
                let target = assigned_variable(arenas, id);
                for (name, ty) in tags {
                    if !written.contains(&name) {
                        from_here.push((name, Narrowing::Declared(ty)));
                    } else if target.as_ref() == Some(&name) {
                        self.declared.push((name.clone(), ty.clone()));
                        after.push((name, Narrowing::Declared(ty)));
                    }
                }
            }
            let mut region = vec![Node::Stmt(id)];
            region.extend(rest.iter().copied());
            self.narrow(arenas, from_here, &region);
            self.visit_stmt(arenas, id);
            self.declared.clear();
            self.narrow(arenas, after, &rest);
            let Some(StmtKind::If(branch)) = arenas.stmt(id).map(|stmt| &stmt.kind) else {
                continue;
            };
            let mut region = vec![Node::Stmt(id)];
            region.extend(rest);
            self.narrow(arenas, after_if(arenas, branch), &region);
        }
        self.narrowed.truncate(mark);
    }

    /// An `if` chain: each condition and body sees what the earlier
    /// conditions being false and its own being true show.
    fn walk_if(&mut self, arenas: &NodeArenas, branch: &IfStmt) {
        let mark = self.narrowed.len();
        let chain = branches(arenas, branch);
        for (at, &(cond, body)) in chain.iter().enumerate() {
            self.visit_expr(arenas, cond);
            let inner = self.narrow(
                arenas,
                guards(arenas, cond, true),
                &[Node::Expr(cond), Node::Stmt(body)],
            );
            self.visit_stmt(arenas, body);
            self.narrowed.truncate(inner);
            let mut later: Vec<Node> = vec![Node::Expr(cond)];
            later.extend(
                chain[at + 1..]
                    .iter()
                    .flat_map(|&(cond, body)| [Node::Expr(cond), Node::Stmt(body)]),
            );
            later.extend(branch.else_branch.map(Node::Stmt));
            self.narrow(arenas, guards(arenas, cond, false), &later);
        }
        if let Some(body) = branch.else_branch {
            self.visit_stmt(arenas, body);
        }
        self.narrowed.truncate(mark);
    }

    /// An operation PHP may run code of the value `operand` for.
    fn implicit(&mut self, arenas: &NodeArenas, operand: ExprId, operation: Operation) {
        self.implicit_on(self.origins(arenas, operand), operation);
    }

    /// The value `id` dies where it is written when nothing holds it: a
    /// temporary is destroyed at the end of the statement that made it.
    fn dies(&mut self, arenas: &NodeArenas, id: ExprId) {
        if is_temporary(arenas, id) && !self.hold_result_dies(arenas, id, DIES_TEMPORARY, None) {
            self.implicit(arenas, id, Operation::Destroy);
        }
        if holds_elements_made_here(arenas, id) {
            self.hold_contents(arenas, id);
        }
    }

    /// The contents of `id` die. An array literal's items are held one by
    /// one, as their own results, and the rest is recorded.
    fn hold_contents(&mut self, arenas: &NodeArenas, id: ExprId) {
        if let Some(items) = self.plain_literal_items(arenas, id) {
            let parts: Option<Vec<(Vec<Origin>, Vec<Origin>)>> = items
                .iter()
                .map(|&item| Some((self.origins(arenas, item)?, self.contents_of(arenas, item)?)))
                .collect();
            if let Some(parts) = parts {
                let mut rest = Vec::new();
                for (&item, (origins, contents)) in items.iter().zip(parts) {
                    if !self.hold_result_dies(
                        arenas,
                        item,
                        DIES_TEMPORARY,
                        Some(origins.as_slice()),
                    ) {
                        rest.extend(origins);
                    }
                    if !self.hold_result_dies(
                        arenas,
                        item,
                        DIES_CONTENTS,
                        Some(contents.as_slice()),
                    ) {
                        rest.extend(contents);
                    }
                }
                self.implicit_on(Some(rest), Operation::Destroy);
                return;
            }
        }
        if !self.hold_result_dies(arenas, id, DIES_CONTENTS, None) {
            let contents = self.contents_of(arenas, id);
            self.implicit_on(contents, Operation::Destroy);
        }
    }

    /// The items of the array literal `id`, through its grouping, when it has
    /// no spread item.
    fn plain_literal_items(&self, arenas: &NodeArenas, id: ExprId) -> Option<Vec<ExprId>> {
        let e = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
        let ExprKind::ArrayLiteral(items) = &e.kind else {
            return None;
        };
        let mut out = Vec::new();
        for item in arenas.array_items(items.clone()) {
            if item.spread {
                return None;
            }
            out.push(item.value);
        }
        Some(out)
    }

    /// The call a result's destroy (`bit`) of `id` belongs to, and its
    /// operand, when `id`'s value is that one call's result alone.
    fn held_source(
        &self,
        arenas: &NodeArenas,
        id: ExprId,
        bit: u8,
        known: Option<&[Origin]>,
    ) -> Option<(u32, HeldSource)> {
        let call = result_call(arenas, id)?;
        let kind = &arenas.expr(call)?.kind;
        let is_call = matches!(
            kind,
            ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::StaticCall(..)
        );
        let source = if bit == DIES_TEMPORARY {
            let owned;
            let origins = match known {
                Some(known) => known,
                None => {
                    owned = self.origins(arenas, id)?;
                    &owned
                }
            };
            match origins {
                [Origin::Returned(callee)] if is_call => HeldSource::Call(callee.clone()),
                [Origin::New(class)] if matches!(kind, ExprKind::New(..)) => {
                    HeldSource::New(class.clone())
                }
                _ => return None,
            }
        } else {
            let owned;
            let contents = match known {
                Some(known) => known,
                None => {
                    owned = self.contents_of(arenas, id)?;
                    &owned
                }
            };
            match contents {
                [Origin::Elements(inner)] if is_call => match &**inner {
                    Origin::Returned(callee) => HeldSource::Call(callee.clone()),
                    _ => return None,
                },
                _ => return None,
            }
        };
        Some((u32::from(call.into_raw()), source))
    }

    /// Holds the destroy of `id`'s call result (`bit`) for the call's record;
    /// false when `id` holds no such result.
    fn hold_result_dies(
        &mut self,
        arenas: &NodeArenas,
        id: ExprId,
        bit: u8,
        known: Option<&[Origin]>,
    ) -> bool {
        let Some((call, source)) = self.held_source(arenas, id, bit, known) else {
            return false;
        };
        #[cfg(debug_assertions)]
        {
            let list = if bit == DIES_TEMPORARY {
                self.origins(arenas, id)
            } else {
                self.contents_of(arenas, id)
            };
            self.scope.calls.push(CallWrites {
                callee: Callee::Implicit {
                    on: Some(Arc::from(list.unwrap_or_default())),
                    operation: Operation::Destroy,
                },
                arguments: Box::new([]),
                places: Box::new([]),
                objectless: Box::new([]),
                callables: Box::new([]),
                fills: Box::new([]),
                dies: DIES_SHADOW,
            });
        }
        // The walk holds a result's destroy before it records the call, so
        // the call's record takes it (`attach_pending`).
        self.scope.pending_dies.push(PendingDies {
            id: call,
            bit,
            source,
        });
        true
    }

    /// Adds `bit` to the call record at `at`, when that record is the call
    /// the destroy is of.
    fn attach(&mut self, at: u32, bit: u8, source: &HeldSource) -> bool {
        let call = &mut self.scope.calls[at as usize];
        let same = match source {
            HeldSource::Call(callee) => call.callee == **callee,
            HeldSource::New(class) => matches!(&call.callee, Callee::New(c) if c == class),
        };
        if !same {
            return false;
        }
        call.dies |= bit;
        true
    }

    /// The operation a held destroy stands for, when no record takes it.
    fn held_operation(bit: u8, source: &HeldSource) -> Vec<Origin> {
        let origin = match source {
            HeldSource::Call(callee) => Origin::Returned(callee.clone()),
            HeldSource::New(class) => Origin::New(class.clone()),
        };
        if bit == DIES_CONTENTS {
            vec![Origin::Elements(Arc::new(origin))]
        } else {
            vec![origin]
        }
    }

    /// Gives the destroys waiting for the call recorded at `at` to that record.
    fn attach_pending(&mut self, raw: u32, at: u32) {
        if self.scope.pending_dies.is_empty() {
            return;
        }
        for held in std::mem::take(&mut self.scope.pending_dies) {
            if held.id != raw {
                self.scope.pending_dies.push(held);
            } else if !self.attach(at, held.bit, &held.source) {
                let on = Self::held_operation(held.bit, &held.source);
                self.implicit_on(Some(on), Operation::Destroy);
            }
        }
    }

    /// The destroys still waiting for their call's record are recorded as
    /// operations, as they always were.
    fn flush_dies(&mut self) {
        for held in std::mem::take(&mut self.scope.pending_dies) {
            let on = Self::held_operation(held.bit, &held.source);
            self.implicit_on(Some(on), Operation::Destroy);
        }
    }

    /// What the variables bound to elements of another, or to the other
    /// itself, were written is held by the other: its origins, when the
    /// binding is to the variable itself, and the objects in the arrays it
    /// holds.
    fn merge_references(&mut self) {
        self.merge_element_refs();
        self.resolve_taken();
    }

    fn merge_element_refs(&mut self) {
        for index in 0..self.scope.element_refs.len() {
            let element_ref = &self.scope.element_refs[index];
            let (subject, item, whole) = (
                element_ref.subject.clone(),
                element_ref.item.clone(),
                element_ref.whole,
            );
            let written = element_ref.written.clone();
            let item_contents = self
                .scope
                .contents
                .get(&item)
                .cloned()
                .unwrap_or(Some(Vec::new()));
            let (origins, contents) = if whole {
                (written, item_contents)
            } else {
                let both = written.zip(item_contents).map(|(mut all, held)| {
                    all.extend(held);
                    all
                });
                (Some(Vec::new()), both)
            };
            let held = self
                .scope
                .origins
                .entry(subject.clone())
                .or_insert_with(|| Some(Vec::new()));
            extend_known(held, origins);
            let held = self
                .scope
                .contents
                .entry(subject)
                .or_insert_with(|| Some(Vec::new()));
            extend_known(held, contents);
        }
        // A variable bound to the whole of another holds what the other does.
        for index in 0..self.scope.element_refs.len() {
            let element_ref = &self.scope.element_refs[index];
            if !element_ref.whole {
                continue;
            }
            let (subject, item) = (element_ref.subject.clone(), element_ref.item.clone());
            let origins = self.scope.origins.get(&subject).cloned();
            let contents = self.scope.contents.get(&subject).cloned();
            if let Some(origins) = origins {
                self.scope.origins.insert(item.clone(), origins);
            }
            if let Some(contents) = contents {
                self.scope.contents.insert(item, contents);
            }
        }
    }

    /// The variables the scope wrote die when it ends, but for the
    /// parameters taken by reference, the variables bound to a place,
    /// global or static, and those `shared` with the scope around it.
    fn destroy_locals(&mut self, by_ref: &FxHashSet<Name>, shared: &FxHashSet<Name>) {
        let bound: FxHashSet<Name> = self
            .scope
            .bindings
            .iter()
            .filter_map(|(name, _)| name.clone())
            .collect();
        let mut locals: Vec<(Name, MaybeOrigins, MaybeOrigins)> = self
            .scope
            .origins
            .iter()
            .filter(|(name, _)| {
                **name != *"this"
                    && !by_ref.contains(*name)
                    && !shared.contains(*name)
                    && !bound.contains(*name)
            })
            .map(|(name, origins)| {
                let contents = self
                    .scope
                    .contents
                    .get(name)
                    .cloned()
                    .unwrap_or(Some(Vec::new()));
                (name.clone(), origins.clone(), contents)
            })
            .collect();
        locals.sort_by(|a, b| a.0.cmp(&b.0));
        let overwritten: Vec<MaybeOrigins> = self
            .scope
            .element_refs
            .iter()
            .filter(|r| r.overwritten)
            .map(|r| {
                let mut all = r.written.clone()?;
                if r.whole {
                    all.extend(
                        self.scope
                            .origins
                            .get(&r.subject)
                            .cloned()
                            .unwrap_or(Some(Vec::new()))?,
                    );
                }
                for name in [&r.subject, &r.item] {
                    all.extend(
                        self.scope
                            .contents
                            .get(name)
                            .cloned()
                            .unwrap_or(Some(Vec::new()))?,
                    );
                }
                Some(all)
            })
            .collect();
        for held in overwritten {
            self.implicit_on(held, Operation::Destroy);
        }
        for (name, origins, contents) in locals {
            let dies = origins
                .as_ref()
                .is_some_and(|origins| !origins.iter().all(Origin::runs_nothing));
            if dies {
                // The classifier reads what the variable holds from the
                // variable's origins in the facts, which are these.
                self.scope.dying_locals.push(name.clone());
                #[cfg(debug_assertions)]
                {
                    let list = distinct_origins(origins.clone().unwrap_or_default());
                    self.scope.calls.push(CallWrites {
                        callee: Callee::Implicit {
                            on: Some(Arc::from(list)),
                            operation: Operation::Destroy,
                        },
                        arguments: Box::new([]),
                        places: Box::new([]),
                        objectless: Box::new([]),
                        callables: Box::new([]),
                        fills: Box::new([]),
                        dies: DIES_SHADOW,
                    });
                }
            } else {
                self.implicit_on(origins, Operation::Destroy);
            }
            self.implicit_on(contents, Operation::Destroy);
        }
    }

    /// An operation PHP may run code of a value from `on` for, from
    /// anywhere when `None`: none on a value that is no object it runs code
    /// of, and one of each in a scope.
    fn implicit_on(&mut self, on: Option<Vec<Origin>>, operation: Operation) {
        if on
            .as_ref()
            .is_some_and(|on| on.iter().all(Origin::runs_nothing))
        {
            return;
        }
        let hash = hash_of(&(on.as_deref(), &operation));
        let keys = &self.scope.implicit_keys;
        let new = self
            .scope
            .implicit
            .find(hash, |&at| {
                let (held, held_operation) = &keys[at as usize];
                held.as_deref() == on.as_deref() && *held_operation == operation
            })
            .is_none();
        #[cfg(debug_assertions)]
        {
            let callee = Callee::Implicit {
                on: on.clone().map(Arc::from),
                operation: operation.clone(),
            };
            assert_eq!(
                new,
                self.scope
                    .implicit_printed
                    .insert(syntax::canonical::print(&callee))
            );
        }
        if !new {
            return;
        }
        let on: Option<Arc<[Origin]>> = on.map(Arc::from);
        let at = self.scope.implicit_keys.len() as u32;
        self.scope
            .implicit_keys
            .push((on.clone(), operation.clone()));
        let keys = &self.scope.implicit_keys;
        self.scope.implicit.insert_unique(hash, at, |&at| {
            let (held, held_operation) = &keys[at as usize];
            hash_of(&(held.as_deref(), held_operation))
        });
        self.scope.calls.push(CallWrites {
            callee: Callee::Implicit { on, operation },
            arguments: Box::new([]),
            places: Box::new([]),
            objectless: Box::new([]),
            callables: Box::new([]),
            fills: Box::new([]),
            dies: 0,
        });
    }

    /// An access to a property, `name` (`None` for a computed one), or an
    /// element, when `element`, of `base`, doing what `access` says; `base`
    /// is read on the way, tested too when the access tests.
    fn member_access(
        &mut self,
        arenas: &NodeArenas,
        base: ExprId,
        name: Option<&Name>,
        access: u8,
        element: bool,
    ) {
        let on = self.origins(arenas, base);
        for (bit, kind) in [
            (READ, Access::Read),
            (WRITE, Access::Write),
            (ISSET, Access::Isset),
            (UNSET, Access::Unset),
        ] {
            if access & bit == 0 {
                continue;
            }
            let operation = if element {
                Operation::Element(kind)
            } else {
                Operation::Property(name.cloned(), kind)
            };
            self.implicit_on(on.clone(), operation);
        }
        let link = if access & ISSET != 0 {
            ISSET | READ
        } else {
            READ
        };
        self.mark(arenas, base, link);
    }

    /// The targets of a destructuring of a value from `source`, written:
    /// each element taken is read from it.
    fn destructure(
        &mut self,
        arenas: &NodeArenas,
        items: &DestructureItemRange,
        source: Option<Vec<Origin>>,
    ) {
        self.implicit_on(source.clone(), Operation::Element(Access::Read));
        let element = source.and_then(element_origins);
        for item in arenas.destructure_items(items) {
            let Some(target) = &item.target else {
                continue;
            };
            let Some(target_node) = arenas.peeled(*target).and_then(|id| arenas.expr(id)) else {
                continue;
            };
            match &target_node.kind {
                ExprKind::Destructure(inner, _) => self.destructure(arenas, inner, element.clone()),
                _ if item.by_ref => {}
                _ => self.mark(arenas, *target, WRITE),
            }
        }
    }

    /// A call, with the arguments that are places and those that name a
    /// callable.
    fn call(&mut self, arenas: &NodeArenas, callee: Callee, args: ArgRange) {
        let call_id = self.call_id.take();
        let args = arenas.args(args);
        let mut spread = false;
        let mut arguments = Vec::with_capacity(args.len());
        let mut places: SmallVec<[_; 4]> = SmallVec::new();
        let mut objectless: SmallVec<[_; 4]> = SmallVec::new();
        let mut callables: SmallVec<[_; 4]> = SmallVec::new();
        for (at, arg) in args.iter().enumerate() {
            spread |= arg.spread;
            let at = match &arg.name {
                _ if spread => ArgAt::Spread,
                Some(name) => ArgAt::Named(name.name.clone()),
                None => ArgAt::Position(at as u32),
            };
            arguments.push(at.clone());
            if let Some(callable) = self.callable(arenas, arg.value) {
                callables.push((at.clone(), callable));
            }
            if let Some(place) = self.place_read(arenas, arg.value) {
                places.push((at, place));
            } else if holds_no_object(arenas, arg.value)
                || self.array_holds_no_object(arenas, arg.value)
            {
                objectless.push(at);
            }
        }
        let fills = self.fills(arenas, &callee, args);
        self.scope.calls.push(CallWrites {
            callee,
            arguments: arguments.into_boxed_slice(),
            places: places.into_iter().collect(),
            objectless: objectless.into_iter().collect(),
            callables: callables.into_iter().collect(),
            fills,
            dies: 0,
        });
        if let Some(raw) = call_id {
            let at = self.scope.calls.len() as u32 - 1;
            self.attach_pending(raw, at);
        }
    }

    /// Whether the value `id` is an array literal that holds no object at any
    /// depth.
    fn array_holds_no_object(&self, arenas: &NodeArenas, id: ExprId) -> bool {
        matches!(
            arenas
                .peeled(id)
                .and_then(|id| arenas.expr(id))
                .map(|e| &e.kind),
            Some(ExprKind::ArrayLiteral(_))
        ) && self
            .contents_of(arenas, id)
            .is_some_and(|contents| contents.iter().all(Origin::runs_nothing))
    }

    /// What dies where `lhs` is assigned over: the value it held and the
    /// arrays that held, unless it is the item of a by-reference loop, which
    /// only notes that it was overwritten.
    fn destroy_overwritten(&mut self, arenas: &NodeArenas, lhs: ExprId) {
        let item = plain_variable(arenas, lhs).and_then(|name| {
            self.scope
                .element_refs
                .iter_mut()
                .rev()
                .find(|r| r.item == name)
        });
        match item {
            Some(element_ref) => element_ref.overwritten = true,
            None => {
                match self.by_ref_param_of(arenas, lhs) {
                    // The argument it started as is destroyed in the code
                    // that passed it.
                    Some(name) => {
                        if plain_variable(arenas, lhs).is_some() {
                            self.implicit_on(
                                Some(vec![Origin::Written(name.clone())]),
                                Operation::Destroy,
                            );
                        }
                        self.scope.overwritten.insert(name);
                    }
                    None => self.implicit(arenas, lhs, Operation::Destroy),
                }
                // The arrays it held die with it.
                if let Some(contents) =
                    plain_variable(arenas, lhs).and_then(|name| self.contents_held(&name))
                {
                    self.implicit_on(contents, Operation::Destroy);
                }
            }
        }
    }

    /// The parameter taken by reference of the body whose value, or an
    /// element of it, an assignment or `unset` of `target` overwrites.
    fn by_ref_param_of(&self, arenas: &NodeArenas, target: ExprId) -> Option<Name> {
        overwritten_variable(arenas, target)
            .filter(|name| self.current.is_none() && self.by_ref_params.contains(name))
    }

    /// What the arrays the variable `name` holds hold, with what the
    /// variables bound to the whole of it have written so far; `None` when
    /// it is not tracked.
    fn contents_held(&self, name: &Name) -> Option<MaybeOrigins> {
        let mut held = self.scope.contents.get(name).cloned()?;
        for element_ref in self
            .scope
            .element_refs
            .iter()
            .filter(|r| r.whole && r.subject == *name)
        {
            let through = self
                .scope
                .contents
                .get(&element_ref.item)
                .cloned()
                .unwrap_or(Some(Vec::new()));
            extend_known(&mut held, through);
        }
        Some(held)
    }

    /// The arguments of a call written as a builtin that writes what it
    /// takes by reference in a way `filler` describes, with the writes
    /// applied to the variables; empty when the call is not one, or an
    /// argument it writes is not a variable the walk can follow.
    fn fills(&mut self, arenas: &NodeArenas, callee: &Callee, args: &[Arg]) -> Box<[ArgAt]> {
        let Callee::Function(name) = callee else {
            return Box::new([]);
        };
        let written = name.written.as_str();
        let written = written.strip_prefix('\\').unwrap_or(written);
        let Some(filler) = filler(written) else {
            return Box::new([]);
        };
        if args.iter().any(|arg| arg.spread || arg.name.is_some()) {
            return Box::new([]);
        }
        let mut writes = Vec::new();
        for (position, arg) in args.iter().enumerate() {
            if position < filler.first || position > filler.last {
                continue;
            }
            let fill = match (filler.fill, position) {
                (Fill::Exec, 1) => Fill::Appends,
                (Fill::Exec, _) => Fill::Overwrites,
                (fill, _) => fill,
            };
            let Some(variable) = plain_variable(arenas, arg.value) else {
                if filler.last == usize::MAX && holds_no_object(arenas, arg.value) {
                    continue;
                }
                return Box::new([]);
            };
            let tracked = self.scope.contents.contains_key(&variable);
            let followed = match fill {
                Fill::Overwrites | Fill::Appends => true,
                Fill::Settype => settype_to(arenas, args).is_some(),
                Fill::Keeps | Fill::Stores(_) | Fill::Splice => tracked,
                Fill::Walk => tracked && walk_takes_by_value(arenas, args),
                Fill::Exec => unreachable!(),
            };
            if !followed {
                return Box::new([]);
            }
            writes.push((position, variable, fill, arg.value));
        }
        let mut at = Vec::with_capacity(writes.len());
        for (position, variable, fill, value) in writes {
            match fill {
                Fill::Keeps | Fill::Walk => {}
                Fill::Stores(from) => {
                    let stored = self.stored_by(arenas, &args[from.min(args.len())..], false);
                    self.note_contents(&variable, stored);
                }
                Fill::Splice => {
                    let stored = self.stored_by(arenas, &args[3.min(args.len())..], true);
                    self.note_contents(&variable, stored);
                }
                Fill::Settype if settype_to(arenas, args) == Some(SettypeTo::Object) => {
                    // An object stays itself; an array becomes a `stdClass`
                    // holding its elements, which the variable still holds.
                    let class = arenas.expr_span(value).map(|span| QualifiedName {
                        written: Name::from("stdClass"),
                        resolved: ResolvedName::Exact(Name::from("stdClass")),
                        span,
                    });
                    self.write(arenas, value, None, Some(vec![Origin::New(class)]));
                    self.reassign(arenas, value);
                }
                Fill::Overwrites | Fill::Settype => {
                    self.destroy_overwritten(arenas, value);
                    self.write(arenas, value, None, Some(vec![Origin::NoObject]));
                    self.reassign(arenas, value);
                }
                Fill::Appends => {
                    self.write(arenas, value, None, Some(vec![Origin::NoObject]));
                    self.reassign(arenas, value);
                }
                Fill::Exec => unreachable!(),
            }
            at.push(ArgAt::Position(position as u32));
        }
        at.into_boxed_slice()
    }

    /// What the arguments `args`, stored as elements of an array, hold: the
    /// objects of their origins and contents; `None` when that is not known,
    /// or, with `arrays`, when one of them may be an object rather than an
    /// array whose elements are stored.
    fn stored_by(&mut self, arenas: &NodeArenas, args: &[Arg], arrays: bool) -> MaybeOrigins {
        args.iter().try_fold(Vec::new(), |mut all, arg| {
            let origins = self.origins(arenas, arg.value)?;
            if arrays && origins.iter().any(|origin| *origin != Origin::NoObject) {
                return None;
            }
            all.extend(origins);
            all.extend(self.contents_of(arenas, arg.value)?);
            Some(all)
        })
    }

    /// What the body made that `e` evaluates to, when it is that.
    fn made_by(&mut self, arenas: &NodeArenas, id: ExprId) -> Option<Made> {
        let e = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
        match &e.kind {
            ExprKind::New(callee, _) => Some(Made::Object(
                match arenas
                    .peeled(*callee)
                    .and_then(|id| arenas.expr(id))
                    .map(|callee| &callee.kind)
                {
                    Some(ExprKind::Name(name)) => Some(arenas.qualified_name(name)?.clone()),
                    _ => None,
                },
            )),
            ExprKind::NewAnonymousClass(_) | ExprKind::Clone(..) => Some(Made::Object(None)),
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
                Some(Made::Closure(Box::new([self.closure_id(e.id)])))
            }
            ExprKind::Assign(_, rhs) => syntax::stack::guard(|| self.made_by(arenas, *rhs)),
            _ => None,
        }
    }
}

impl Summarizer<'_> {
    /// Records the include `id` as a call of the file it names. The file
    /// runs in this scope, so it may assign any variable of it.
    fn record_include(&mut self, id: ExprId) {
        self.write_computed();
        self.scope.calls.push(CallWrites {
            callee: Callee::Include(u32::from(id.into_raw())),
            arguments: Box::new([]),
            places: Box::new([]),
            objectless: Box::new([]),
            callables: Box::new([]),
            fills: Box::new([]),
            dies: 0,
        });
    }

    /// Records `e` when it is a call.
    fn record_call(&mut self, arenas: &NodeArenas, id: ExprId) {
        let outer = self.call_id.replace(u32::from(id.into_raw()));
        self.record_call_inner(arenas, id);
        self.call_id = outer;
    }

    fn record_call_inner(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        match &e.kind {
            ExprKind::Call(callee, args) => match arenas
                .peeled(*callee)
                .and_then(|id| arenas.expr(id))
                .map(|callee| &callee.kind)
            {
                Some(ExprKind::Name(name)) => {
                    let name = arenas
                        .qualified_name(name)
                        .expect("call name range belongs to its retained file owner");
                    self.purity_call(PurityCall::Function(name.clone()));
                    self.call(arenas, Callee::Function(name.clone()), args.clone());
                }
                _ => {
                    self.impure_unless_in_closure(true);
                    let callee = match self.made_by(arenas, *callee) {
                        Some(Made::Closure(ids)) => Callee::Written(ids),
                        _ => Callee::Value(
                            match arenas
                                .peeled(*callee)
                                .and_then(|id| arenas.expr(id))
                                .map(|callee| &callee.kind)
                            {
                                Some(ExprKind::Variable(name)) => Some(name.clone()),
                                _ => None,
                            },
                        ),
                    };
                    self.call(arenas, callee, args.clone());
                }
            },
            ExprKind::MethodCall(recv, method, args, _) => {
                let on_this = arenas.expr(*recv).is_some_and(
                    |recv| matches!(&recv.kind, ExprKind::Variable(v) if *v == "this"),
                );
                if on_this {
                    self.purity_call(PurityCall::ThisMethod(method.name.clone()));
                } else {
                    self.impure_unless_in_closure(true);
                }
                self.call(
                    arenas,
                    Callee::Method {
                        receiver: self.receiver(arenas, *recv),
                        method: Some(method.name.clone()),
                    },
                    args.clone(),
                );
            }
            ExprKind::DynamicMethodCall(recv, _, args, _) => self.call(
                arenas,
                Callee::Method {
                    receiver: self.receiver(arenas, *recv),
                    method: None,
                },
                args.clone(),
            ),
            ExprKind::StaticCall(target, method, args) => {
                match target.as_name() {
                    Some(name) => {
                        self.purity_call(PurityCall::Static(name.clone(), method.name.clone()));
                    }
                    None => self.impure_unless_in_closure(true),
                }
                self.call(arenas, static_callee(arenas, target, method), args.clone());
            }
            ExprKind::DynamicStaticCall(target, _, args) => self.call(
                arenas,
                Callee::Static {
                    class: target.as_name().cloned(),
                    method: None,
                },
                args.clone(),
            ),
            ExprKind::New(callee, args) => match arenas
                .peeled(*callee)
                .and_then(|id| arenas.expr(id))
                .map(|callee| &callee.kind)
            {
                Some(ExprKind::Name(name)) => {
                    let name = arenas
                        .qualified_name(name)
                        .expect("constructor name range belongs to its retained file owner");
                    self.purity_call(PurityCall::New(name.clone()));
                    self.call(arenas, Callee::New(Some(name.clone())), args.clone());
                }
                _ => {
                    self.impure_unless_in_closure(true);
                    self.call(arenas, Callee::New(None), args.clone());
                }
            },
            ExprKind::NewAnonymousClass(class_id) => {
                if let Some(class) = arenas.anonymous_class(*class_id) {
                    self.call(
                        arenas,
                        Callee::Anonymous(arenas.anonymous_class_name(*class_id).cloned()),
                        class.args.clone(),
                    );
                }
            }
            _ => {}
        }
    }
}

/// Whether `e` is a call writing variables whose names it computes:
/// `extract()`, and `parse_str()` and `mb_parse_str()` with no array to
/// write.
pub(crate) fn writes_computed_variables(arenas: &NodeArenas, id: ExprId) -> bool {
    let Some(e) = arenas.expr(id) else {
        return false;
    };
    let ExprKind::Call(callee, args) = &e.kind else {
        return false;
    };
    let Some(ExprNode {
        kind: ExprKind::Name(name),
        ..
    }) = arenas.peeled(*callee).and_then(|id| arenas.expr(id))
    else {
        return false;
    };
    let Some(name) = arenas.qualified_name(name) else {
        return false;
    };
    let args = arenas.args(args.clone());
    let name = name.written.trim_start_matches('\\');
    name.eq_ignore_ascii_case("extract")
        || ((name.eq_ignore_ascii_case("parse_str") || name.eq_ignore_ascii_case("mb_parse_str"))
            && args.len() < 2
            && !args.iter().any(|arg| arg.spread || arg.name.is_some()))
}

/// Whether a destructuring takes a reference to an element, at any depth.
pub(crate) fn destructures_by_ref(arenas: &NodeArenas, items: &DestructureItemRange) -> bool {
    arenas.destructure_items(items).iter().any(|item| {
        item.by_ref
            || item.target.as_ref().is_some_and(|target| {
                match arenas
                    .peeled(*target)
                    .and_then(|id| arenas.expr(id))
                    .map(|target| &target.kind)
                {
                    Some(ExprKind::Destructure(inner, _)) => destructures_by_ref(arenas, inner),
                    _ => false,
                }
            })
    })
}

/// The call `e` is, as a body's summary records it.
pub(crate) fn call_of(arenas: &NodeArenas, id: ExprId) -> Option<CallWrites> {
    let mut walk = Summarizer::default();
    walk.record_call(arenas, id);
    walk.scope.calls.pop()
}

/// `$obj->prop = ...`/`unset($obj->prop)`: a write to a property of any
/// object, through any depth of array-index nesting.
fn property_write_target(arenas: &NodeArenas, id: ExprId) -> bool {
    let Some(e) = arenas.peeled(id).and_then(|id| arenas.expr(id)) else {
        return false;
    };
    syntax::stack::guard(|| match &e.kind {
        ExprKind::PropertyAccess(..) | ExprKind::DynamicPropertyAccess(..) => true,
        ExprKind::Index(base, _) => property_write_target(arenas, *base),
        _ => false,
    })
}

/// What `target::method()` calls: a method, or the parent class's hook of a
/// property (`parent::$property::get()`).
fn static_callee(arenas: &NodeArenas, target: &StaticTarget, method: &Ident) -> Callee {
    if let Some(property) = target.parent_hook_property(arenas) {
        for (name, set) in [("get", false), ("set", true)] {
            if method.name.eq_ignore_ascii_case(name) {
                return Callee::ParentHook {
                    property: Name::from(property),
                    set,
                };
            }
        }
    }
    Callee::Static {
        class: target.as_name().cloned(),
        method: Some(method.name.clone()),
    }
}

/// The object `recv` is, as far as the body shows it.
fn receiver(arenas: &NodeArenas, id: ExprId) -> Receiver {
    let Some(recv) = arenas.peeled(id).and_then(|id| arenas.expr(id)) else {
        return Receiver::Unknown;
    };
    match &recv.kind {
        ExprKind::Variable(v) if **v == *"this" => Receiver::This,
        ExprKind::Variable(v) => Receiver::Variable(v.clone()),
        ExprKind::PropertyAccess(base, prop, _)
            if arenas
                .peeled(*base)
                .and_then(|id| arenas.expr(id))
                .is_some_and(
                    |base| matches!(&base.kind, ExprKind::Variable(v) if **v == *"this"),
                ) =>
        {
            Receiver::ThisProperty(prop.name.clone())
        }
        ExprKind::StaticPropertyFetch(StaticTarget::Name(class), prop) => {
            Receiver::Static((**class).clone(), prop.name.clone())
        }
        _ => match origins_of(arenas, id) {
            Some(origins) => Receiver::Origins(origins.into()),
            None => Receiver::Unknown,
        },
    }
}

/// A node of code a narrowing is looked for writes in.
#[derive(Clone, Copy)]
enum Node {
    Expr(ExprId),
    Stmt(StmtId),
}

/// The variables written anywhere in `nodes`, closures and declarations
/// nested in them too.
fn written_in(arenas: &NodeArenas, nodes: &[Node]) -> FxHashSet<Name> {
    let mut written = FxHashSet::default();
    let mut walk = Reassigned(&mut written);
    for node in nodes {
        match *node {
            Node::Expr(id) => walk.visit_expr(arenas, id),
            Node::Stmt(id) => walk.visit_stmt(arenas, id),
        }
    }
    written
}

/// The variable the expression `id` is, when it is a plain one.
/// What a builtin writes into an argument it takes by reference.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fill {
    /// The elements it holds, in another order or fewer: nothing new.
    Keeps,
    /// The arguments from the position on, as elements.
    Stores(usize),
    /// The elements of the argument at 3, which must be an array.
    Splice,
    /// A new value that holds no object: strings, numbers, arrays of them.
    Overwrites,
    /// The same, added to what is there.
    Appends,
    /// `exec`: the output is added to, the result code overwritten.
    Exec,
    /// `settype`, which writes no object unless it is given `object` or
    /// `array`.
    Settype,
    /// `array_walk`, which writes what its callback assigns to the elements
    /// it takes by reference, so nothing when it takes none.
    Walk,
}

/// The by-reference arguments of a builtin, positions `first` to `last`, and
/// what it writes in them.
#[derive(Clone, Copy)]
pub(crate) struct Filler {
    first: usize,
    last: usize,
    fill: Fill,
}

/// How the builtin `name`, as written, writes the arguments it takes by
/// reference, when the summary can say.
pub(crate) fn filler(name: &str) -> Option<Filler> {
    let mut buffer = [0u8; 32];
    let lower = buffer.get_mut(..name.len())?;
    lower.copy_from_slice(name.as_bytes());
    lower.make_ascii_lowercase();
    let at = |first, last, fill| Some(Filler { first, last, fill });
    match std::str::from_utf8(lower).ok()? {
        "array_push" | "array_unshift" => at(0, 0, Fill::Stores(1)),
        "sort" | "rsort" | "usort" | "uasort" | "uksort" | "asort" | "arsort" | "ksort"
        | "krsort" | "natsort" | "natcasesort" | "shuffle" | "array_pop" | "array_shift"
        | "reset" | "end" | "next" | "prev" => at(0, 0, Fill::Keeps),
        "array_multisort" => at(0, usize::MAX, Fill::Keeps),
        "array_walk" | "array_walk_recursive" => at(0, 0, Fill::Walk),
        "array_splice" => at(0, 0, Fill::Splice),
        "preg_match" | "preg_match_all" | "similar_text" | "getopt" | "is_callable" => {
            at(2, 2, Fill::Overwrites)
        }
        "preg_replace" | "preg_replace_callback" => at(4, 4, Fill::Overwrites),
        "preg_replace_callback_array" | "str_replace" | "str_ireplace" => {
            at(3, 3, Fill::Overwrites)
        }
        "parse_str" | "mb_parse_str" | "system" | "passthru" => at(1, 1, Fill::Overwrites),
        "exec" => at(1, 2, Fill::Exec),
        "sscanf" | "fscanf" => at(2, usize::MAX, Fill::Overwrites),
        "settype" => at(0, 0, Fill::Settype),
        _ => None,
    }
}

/// Whether the second argument of an `array_walk` call is a closure or an
/// arrow function that takes none of its parameters by reference.
fn walk_takes_by_value(arenas: &NodeArenas, args: &[Arg]) -> bool {
    let Some(callback) = args
        .get(1)
        .and_then(|arg| arenas.peeled(arg.value))
        .and_then(|id| arenas.expr(id))
    else {
        return false;
    };
    let params = match &callback.kind {
        ExprKind::Closure(id) => match arenas.closure(*id) {
            Some(ClosurePayload::Closure(closure)) => closure.params.clone(),
            _ => return false,
        },
        ExprKind::ArrowFn(id) => match arenas.closure(*id) {
            Some(ClosurePayload::ArrowFn(arrow)) => arrow.params.clone(),
            _ => return false,
        },
        _ => return false,
    };
    arenas.params(params).iter().all(|param| !param.by_ref)
}

/// What a `settype` call turns its variable into.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettypeTo {
    Scalar,
    Object,
}

/// What the second argument of a `settype` call, a literal, names.
fn settype_to(arenas: &NodeArenas, args: &[Arg]) -> Option<SettypeTo> {
    let Some(ExprKind::Literal(Literal::String(payload))) = args
        .get(1)
        .and_then(|arg| arenas.peeled(arg.value))
        .and_then(|id| arenas.expr(id))
        .map(|e| &e.kind)
    else {
        return None;
    };
    let bytes = arenas.string_bytes(payload)?;
    let is = |names: &[&str]| {
        names
            .iter()
            .any(|n| bytes.eq_ignore_ascii_case(n.as_bytes()))
    };
    if is(&[
        "int", "integer", "float", "double", "string", "bool", "boolean", "null",
    ]) {
        Some(SettypeTo::Scalar)
    } else if is(&["object"]) {
        Some(SettypeTo::Object)
    } else {
        None
    }
}

fn plain_variable(arenas: &NodeArenas, id: ExprId) -> Option<Name> {
    let e = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    match &e.kind {
        ExprKind::Variable(v) if **v != *"this" && !is_superglobal(v) => Some(v.clone()),
        _ => None,
    }
}

/// The variable whose value, or an element of it, an assignment to `id`
/// overwrites.
fn overwritten_variable(arenas: &NodeArenas, id: ExprId) -> Option<Name> {
    let mut id = id;
    loop {
        let e = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
        match &e.kind {
            ExprKind::Index(base, Some(_)) => id = *base,
            _ => return plain_variable(arenas, id),
        }
    }
}

/// The variable the statement `id` assigns, when it is an expression
/// statement doing only that.
fn assigned_variable(arenas: &NodeArenas, id: StmtId) -> Option<Name> {
    let StmtKind::Expression(e) = &arenas.stmt(id)?.kind else {
        return None;
    };
    let e = arenas.peeled(*e).and_then(|id| arenas.expr(id))?;
    match &e.kind {
        ExprKind::Assign(lhs, _) => plain_variable(arenas, *lhs),
        _ => None,
    }
}

/// The global function a call names, lowercased, with its arguments.
fn called_function(arenas: &NodeArenas, id: ExprId) -> Option<(String, &[Arg])> {
    let e = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    let ExprKind::Call(callee, args) = &e.kind else {
        return None;
    };
    let callee = arenas.peeled(*callee).and_then(|id| arenas.expr(id))?;
    let ExprKind::Name(name) = &callee.kind else {
        return None;
    };
    let name = arenas.qualified_name(name)?;
    let written = name.written.trim_start_matches('\\');
    Some((written.to_ascii_lowercase(), arenas.args(args.clone())))
}

/// What the condition `id` shows of the variables it tests when it is true,
/// or when it is false for `!positive`.
fn guards(arenas: &NodeArenas, id: ExprId, positive: bool) -> Vec<(Name, Narrowing)> {
    let Some(e) = arenas.peeled(id).and_then(|id| arenas.expr(id)) else {
        return Vec::new();
    };
    syntax::stack::guard(|| match &e.kind {
        ExprKind::Unary(UnaryOp::Not, inner) => guards(arenas, *inner, !positive),
        ExprKind::Binary(BinOp::And | BinOp::LogicalAnd, left, right) if positive => {
            let mut out = guards(arenas, *left, true);
            out.extend(guards(arenas, *right, true));
            out
        }
        ExprKind::Binary(BinOp::Or | BinOp::LogicalOr, left, right) if !positive => {
            let mut out = guards(arenas, *left, false);
            out.extend(guards(arenas, *right, false));
            out
        }
        ExprKind::Instanceof(subject, InstanceofTarget::Name(range)) if positive => {
            match plain_variable(arenas, *subject) {
                Some(name) => vec![(
                    name,
                    Narrowing::Instance(arenas.qualified_names(range).to_vec().into_boxed_slice()),
                )],
                None => Vec::new(),
            }
        }
        // Equal in kind and value to a literal, which is no object.
        ExprKind::Binary(op @ (BinOp::Identical | BinOp::NotIdentical), left, right)
            if positive == (*op == BinOp::Identical) =>
        {
            let literal = |id: ExprId| {
                arenas
                    .peeled(id)
                    .and_then(|id| arenas.expr(id))
                    .is_some_and(|e| matches!(e.kind, ExprKind::Literal(_)))
            };
            let variable = match (literal(*left), literal(*right)) {
                (true, false) => plain_variable(arenas, *right),
                (false, true) => plain_variable(arenas, *left),
                _ => None,
            };
            variable.map_or_else(Vec::new, |name| vec![(name, Narrowing::NoObject)])
        }
        ExprKind::Call(..) => {
            let Some((function, args)) = called_function(arenas, e.id) else {
                return Vec::new();
            };
            let subject = args
                .first()
                .filter(|arg| !arg.spread && arg.name.is_none())
                .and_then(|arg| plain_variable(arenas, arg.value));
            let Some(subject) = subject else {
                return Vec::new();
            };
            match function.as_str() {
                "is_array" | "is_string" | "is_int" | "is_integer" | "is_long" | "is_float"
                | "is_double" | "is_bool" | "is_null" | "is_numeric" | "is_scalar"
                | "is_resource"
                    if positive =>
                {
                    vec![(subject, Narrowing::NoObject)]
                }
                "is_object" if !positive => vec![(subject, Narrowing::NoObject)],
                "is_a" | "is_subclass_of" if positive => {
                    let class = args
                        .get(1)
                        .filter(|arg| !arg.spread && arg.name.is_none())
                        .and_then(|arg| literal_string_value(arenas, arg.value));
                    let Some(class) = class else {
                        return Vec::new();
                    };
                    let class = class.trim_start_matches('\\');
                    if class.is_empty() {
                        return Vec::new();
                    }
                    let name = QualifiedName {
                        written: Name::from(class),
                        resolved: ResolvedName::Exact(Name::from(class)),
                        span: arenas
                            .expr_span(e.id)
                            .expect("expression has a parallel span"),
                    };
                    vec![(subject, Narrowing::Instance(Box::new([name])))]
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    })
}

/// Whether control never goes on past the statement `id`.
fn leaves(arenas: &NodeArenas, id: StmtId) -> bool {
    let Some(stmt) = arenas.stmt(id) else {
        return false;
    };
    syntax::stack::guard(|| match &stmt.kind {
        StmtKind::Return(_)
        | StmtKind::Throw(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Goto(_) => true,
        StmtKind::Expression(e) => arenas
            .peeled(*e)
            .and_then(|id| arenas.expr(id))
            .is_some_and(|e| matches!(e.kind, ExprKind::Exit(..) | ExprKind::Throw(_))),
        StmtKind::Block(stmts) => arenas
            .stmt_ids(stmts)
            .iter()
            .any(|&stmt| leaves(arenas, stmt)),
        StmtKind::If(branch) => {
            branch
                .else_branch
                .is_some_and(|branch| leaves(arenas, branch))
                && leaves(arenas, branch.then_branch)
                && arenas
                    .elseifs(branch.elseifs.clone())
                    .iter()
                    .all(|elseif| leaves(arenas, elseif.body))
        }
        _ => false,
    })
}

/// The `(condition, body)` pairs of an `if` chain, in order.
fn branches(arenas: &NodeArenas, branch: &IfStmt) -> Vec<(ExprId, StmtId)> {
    std::iter::once((branch.cond, branch.then_branch))
        .chain(
            arenas
                .elseifs(branch.elseifs.clone())
                .iter()
                .map(|elseif| (elseif.cond, elseif.body)),
        )
        .collect()
}

/// What the code after the `if` statement `branch` may take as shown,
/// whichever way control gets there: what every way that does not leave
/// shows.
fn after_if(arenas: &NodeArenas, branch: &IfStmt) -> Vec<(Name, Narrowing)> {
    let mut ways: Vec<Vec<(Name, Narrowing)>> = Vec::new();
    let mut before: Vec<(Name, Narrowing)> = Vec::new();
    for (cond, body) in branches(arenas, branch) {
        if !leaves(arenas, body) {
            let mut way = before.clone();
            way.extend(guards(arenas, cond, true));
            ways.push(way);
        }
        before.extend(guards(arenas, cond, false));
    }
    match branch.else_branch {
        Some(body) if leaves(arenas, body) => {}
        _ => ways.push(before),
    }
    let mut ways = ways.into_iter();
    let Some(first) = ways.next() else {
        return Vec::new();
    };
    let others: Vec<FxHashSet<(&Name, &Narrowing)>> = ways
        .as_slice()
        .iter()
        .map(|way| {
            way.iter()
                .map(|(name, narrowing)| (name, narrowing))
                .collect()
        })
        .collect();
    first
        .into_iter()
        .filter(|(name, narrowing)| others.iter().all(|way| way.contains(&(name, narrowing))))
        .collect()
}

impl Visitor for Summarizer<'_> {
    fn visit_function_decl(&mut self, arenas: &NodeArenas, function: &FunctionDecl) {
        let mut nested = Reassigned(&mut self.scope.reassigned);
        arena::walk_function_decl(&mut nested, arenas, function);
    }

    fn visit_class_def(&mut self, arenas: &NodeArenas, class: &ClassDef) {
        let mut nested = Reassigned(&mut self.scope.reassigned);
        arena::walk_class_def(&mut nested, arenas, class);
    }

    fn visit_arg(&mut self, arenas: &NodeArenas, arg: &Arg) {
        if arg.spread {
            self.implicit(arenas, arg.value, Operation::Iterate { keys: true });
        }
        self.dies(arenas, arg.value);
        arena::walk_arg(self, arenas, arg);
    }

    fn visit_catch_clause(&mut self, arenas: &NodeArenas, clause: &CatchClause) {
        if let Some(var) = &clause.var {
            let name = var.name.clone();
            let caught = Origin::Caught(
                arenas
                    .qualified_names(&clause.types)
                    .to_vec()
                    .into_boxed_slice(),
            );
            self.note_variable(name.clone(), None, Some(vec![caught]));
            for scope in self.scopes_sharing(&name) {
                scope.reassigned.insert(name.clone());
            }
            self.scope
                .writes
                .push(Place::root(Root::Variable(Some(name))));
        }
        arena::walk_catch_clause(self, arenas, clause);
    }

    fn visit_stmts(&mut self, arenas: &NodeArenas, stmts: &[StmtId]) {
        self.walk_list(arenas, stmts);
    }

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        syntax::stack::guard(|| {
            let impure = match &stmt.kind {
                StmtKind::Echo(_)
                | StmtKind::Throw(_)
                | StmtKind::Global(_)
                | StmtKind::StaticVar(_)
                | StmtKind::InlineHtml(_) => true,
                StmtKind::Unset(targets) => arenas
                    .expr_ids(targets)
                    .iter()
                    .any(|target| property_write_target(arenas, *target)),
                _ => false,
            };
            self.impure_unless_in_closure(impure);
            if self.closures == 0
                && let StmtKind::Expression(expr) = &stmt.kind
            {
                self.assigned.statement(arenas, *expr);
            }
            if let StmtKind::Expression(expr) = &stmt.kind {
                self.dies(arenas, *expr);
            }
            match &stmt.kind {
                StmtKind::Unset(targets) => {
                    for &target in arenas.expr_ids(targets) {
                        match self.by_ref_param_of(arenas, target) {
                            // Unsetting the parameter leaves what it
                            // refers to held by the code that passed it.
                            Some(name) => {
                                if plain_variable(arenas, target).is_none() {
                                    self.scope.overwritten.insert(name);
                                }
                            }
                            None => self.implicit(arenas, target, Operation::Destroy),
                        }
                        self.write(arenas, target, None, Some(vec![Origin::NoObject]));
                        self.mark(arenas, target, UNSET);
                    }
                }
                StmtKind::Echo(values) => {
                    for &value in arenas.expr_ids(values) {
                        self.implicit(arenas, value, Operation::String);
                    }
                }
                StmtKind::Switch { subject, cases } => {
                    let cases = arenas.switch_cases(cases.clone());
                    let tests = || cases.iter().filter_map(|case| case.test);
                    if tests().any(|test| may_be_string(arenas, test)) {
                        self.implicit(arenas, *subject, Operation::String);
                    }
                    if may_be_string(arenas, *subject) {
                        for test in tests() {
                            self.implicit(arenas, test, Operation::String);
                        }
                    }
                }
                StmtKind::Foreach {
                    subject,
                    key,
                    value,
                    by_ref,
                    ..
                } => {
                    let source = self.origins(arenas, *subject);
                    self.implicit_on(
                        source.clone(),
                        Operation::Iterate {
                            keys: key.is_some(),
                        },
                    );
                    if let Some(key) = key {
                        self.implicit(arenas, *key, Operation::Destroy);
                        self.write(arenas, *key, None, None);
                        self.mark(arenas, *key, WRITE);
                    }
                    // An array the body made, or a variable whose contents it
                    // knows, yields the objects it holds.
                    let tracked = plain_variable(arenas, *subject)
                        .is_some_and(|name| self.scope.contents.contains_key(&name));
                    let element = if tracked || holds_elements_made_here(arenas, *subject) {
                        self.contents_of(arenas, *subject)
                    } else {
                        source.and_then(element_origins)
                    };
                    if !*by_ref {
                        self.implicit(arenas, *value, Operation::Destroy);
                    }
                    if is_temporary(arenas, *subject) {
                        self.implicit_on(element.clone(), Operation::Destroy);
                    }
                    let value_node = arenas.peeled(*value).and_then(|id| arenas.expr(id));
                    match value_node.map(|node| &node.kind) {
                        Some(ExprKind::Variable(name)) if *by_ref => {
                            let element = Place::of(arenas, *subject)
                                .unwrap_or_else(|| Place::root(Root::Value))
                                .then(Step::Element);
                            self.destroy_held(name);
                            self.bind(Some(name.clone()), Binding::Reference(element));
                        }
                        Some(ExprKind::Destructure(items, _)) => {
                            self.write(arenas, *value, None, element.clone());
                            self.destructure(arenas, items, element);
                        }
                        _ => {
                            self.write(arenas, *value, None, element);
                            if !*by_ref {
                                self.mark(arenas, *value, WRITE);
                            }
                        }
                    }
                    if *by_ref {
                        match (
                            plain_variable(arenas, *subject).filter(|_| tracked),
                            plain_variable(arenas, *value),
                        ) {
                            // The loop writes the elements through the
                            // reference, which holds what they will.
                            (Some(name), Some(item)) => {
                                self.write(arenas, *subject, None, Some(Vec::new()));
                                self.scope.element_refs.push(ElementRef {
                                    subject: name,
                                    item,
                                    written: Some(Vec::new()),
                                    overwritten: false,
                                    whole: false,
                                });
                            }
                            _ => self.write(arenas, *subject, None, None),
                        }
                    }
                    for target in key.iter().copied().chain([*value]) {
                        self.reassign(arenas, target);
                    }
                }
                StmtKind::Global(vars) => {
                    for var in arenas.global_vars(vars.clone()) {
                        match var {
                            GlobalVar::Name(name) => self.bind(
                                Some(name.name.clone()),
                                Binding::Global(Some(name.name.clone())),
                            ),
                            GlobalVar::Dynamic(name) => {
                                self.implicit(arenas, *name, Operation::String);
                                self.bind(None, Binding::Global(None));
                            }
                        }
                    }
                }
                StmtKind::StaticVar(vars) => {
                    for variable in arenas.static_vars(vars.clone()) {
                        let name = variable.name.name.clone();
                        let (origins, contents) = match variable.default {
                            Some(default) => (
                                self.origins(arenas, default),
                                self.contents_of(arenas, default),
                            ),
                            None => (Some(vec![Origin::NoObject]), Some(Vec::new())),
                        };
                        self.destroy_held(&name);
                        self.bind(Some(name.clone()), Binding::Static);
                        // What it holds is what the body writes it, which the
                        // calls before wrote the same.
                        self.scope.origins.insert(name.clone(), origins);
                        self.scope.contents.insert(name.clone(), contents);
                    }
                }
                _ => {}
            }
            match &stmt.kind {
                StmtKind::If(branch) => self.walk_if(arenas, branch),
                StmtKind::While { cond, body } => {
                    self.visit_expr(arenas, *cond);
                    let mark = self.narrow(
                        arenas,
                        guards(arenas, *cond, true),
                        &[Node::Expr(*cond), Node::Stmt(*body)],
                    );
                    self.visit_stmt(arenas, *body);
                    self.narrowed.truncate(mark);
                }
                // Control may come in at a label from where no guard held.
                StmtKind::Label(_) => self.narrowed.clear(),
                _ => arena::walk_stmt(self, arenas, id),
            }
        })
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        syntax::stack::guard(|| {
            let impure = match &e.kind {
                ExprKind::ClassConstFetch(..)
                | ExprKind::ClassNameFetch(_)
                | ExprKind::StaticPropertyFetch(..)
                | ExprKind::DynamicStaticPropertyFetch(..) => true,
                ExprKind::Variable(name) => is_superglobal(name),
                ExprKind::Yield(..)
                | ExprKind::YieldFrom(_)
                | ExprKind::ShellExec(_)
                | ExprKind::Print(_)
                | ExprKind::Exit(..)
                | ExprKind::Eval(_)
                | ExprKind::Include(..) => true,
                ExprKind::Assign(lhs, _)
                | ExprKind::AssignRef(lhs, _)
                | ExprKind::CompoundAssign(_, lhs, _) => property_write_target(arenas, *lhs),
                ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..) => true,
                _ => false,
            };
            self.impure_unless_in_closure(impure);
            let peeled = arenas.peeled(id).unwrap_or(id);
            let access = self.accesses.remove(&peeled).unwrap_or(READ);
            if let ExprKind::MethodCall(base, ..)
            | ExprKind::DynamicMethodCall(base, ..)
            | ExprKind::PropertyAccess(base, ..)
            | ExprKind::DynamicPropertyAccess(base, ..)
            | ExprKind::Index(base, _) = &e.kind
            {
                self.dies(arenas, *base);
            }
            match &e.kind {
                ExprKind::Assign(lhs, rhs) => {
                    let made = self.made_by(arenas, *rhs);
                    let mut origins = self.origins(arenas, *rhs);
                    if let Some(name) = plain_variable(arenas, *lhs)
                        && let Some((_, ty)) =
                            self.declared.iter().find(|(tagged, _)| *tagged == name)
                    {
                        origins = Some(vec![Origin::Declared(ty.clone())]);
                    }
                    if matches!(
                        arenas
                            .peeled(*lhs)
                            .and_then(|id| arenas.expr(id))
                            .map(|e| &e.kind),
                        Some(
                            ExprKind::Variable(_)
                                | ExprKind::PropertyAccess(..)
                                | ExprKind::StaticPropertyFetch(..)
                                | ExprKind::Index(_, Some(_))
                        )
                    ) {
                        self.destroy_overwritten(arenas, *lhs);
                    }
                    self.write(arenas, *lhs, made, origins.clone());
                    self.note_stored(arenas, *lhs, *rhs, origins.as_deref());
                    if let Some(ExprNode {
                        kind: ExprKind::Destructure(items, _),
                        ..
                    }) = arenas.peeled(*lhs).and_then(|id| arenas.expr(id))
                    {
                        if destructures_by_ref(arenas, items) {
                            self.write(arenas, *rhs, None, None);
                        }
                        self.destructure(arenas, items, origins);
                    } else {
                        self.mark(arenas, *lhs, WRITE);
                    }
                    self.reassign(arenas, *lhs);
                }
                ExprKind::CompoundAssign(op, lhs, rhs) => {
                    let origins = match op {
                        BinOp::Concat => Some(vec![Origin::NoObject]),
                        BinOp::Coalesce => self.origins(arenas, *rhs),
                        _ => Some(vec![Origin::Arithmetic]),
                    };
                    match op {
                        BinOp::Coalesce => self.note_stored(arenas, *lhs, *rhs, origins.as_deref()),
                        BinOp::Add => self.note_stored(arenas, *lhs, *rhs, Some(&[])),
                        _ => {}
                    }
                    self.write(arenas, *lhs, None, origins);
                    self.reassign(arenas, *lhs);
                    self.mark(
                        arenas,
                        *lhs,
                        if *op == BinOp::Coalesce {
                            ISSET | READ | WRITE
                        } else {
                            READ | WRITE
                        },
                    );
                    if *op == BinOp::Concat {
                        self.implicit(arenas, *lhs, Operation::String);
                        self.implicit(arenas, *rhs, Operation::String);
                    }
                }
                ExprKind::AssignRef(lhs, rhs) => {
                    match arenas
                        .peeled(*lhs)
                        .and_then(|id| arenas.expr(id))
                        .map(|lhs| &lhs.kind)
                    {
                        Some(ExprKind::Variable(name)) if **name != *"this" => {
                            let place =
                                Place::of(arenas, *rhs).unwrap_or_else(|| Place::root(Root::Value));
                            self.destroy_held(name);
                            self.bind(Some(name.clone()), Binding::Reference(place));
                            match plain_variable(arenas, *rhs)
                                .filter(|of| self.scope.contents.contains_key(of))
                            {
                                Some(subject) => self.scope.element_refs.push(ElementRef {
                                    subject,
                                    item: name.clone(),
                                    written: Some(Vec::new()),
                                    overwritten: false,
                                    whole: true,
                                }),
                                None => self.write(arenas, *rhs, None, None),
                            }
                        }
                        _ => {
                            self.write(arenas, *lhs, None, None);
                            self.write(arenas, *rhs, None, None);
                        }
                    }
                    self.reassign(arenas, *lhs);
                }
                ExprKind::Unary(
                    UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                    target,
                ) => {
                    self.write(arenas, *target, None, Some(vec![Origin::Arithmetic]));
                    self.mark(arenas, *target, READ | WRITE);
                }
                ExprKind::Eval(_) => self.scope.anything = true,
                ExprKind::Include(..) => self.record_include(id),
                _ => {
                    self.record_call(arenas, id);
                    if writes_computed_variables(arenas, id) {
                        self.write_computed();
                    }
                }
            }
            match &e.kind {
                ExprKind::Isset(items) => {
                    for &item in arenas.expr_ids(items) {
                        self.mark(arenas, item, ISSET);
                    }
                }
                ExprKind::Empty(item) => self.mark(arenas, *item, ISSET | READ),
                ExprKind::Binary(BinOp::Coalesce, left, _) => {
                    self.mark(arenas, *left, ISSET | READ)
                }
                ExprKind::PropertyAccess(base, prop, _) => {
                    self.member_access(arenas, *base, Some(&prop.name), access, false);
                }
                ExprKind::DynamicPropertyAccess(base, name, _) => {
                    self.member_access(arenas, *base, None, access, false);
                    self.implicit(arenas, *name, Operation::String);
                }
                ExprKind::Index(base, _) => self.member_access(arenas, *base, None, access, true),
                ExprKind::DynamicVariable(name)
                | ExprKind::DynamicStaticPropertyFetch(_, name)
                | ExprKind::Cast(CastSpelling::String | CastSpelling::Binary, name)
                | ExprKind::Print(name) => self.implicit(arenas, *name, Operation::String),
                ExprKind::Binary(BinOp::Concat, left, right) => {
                    self.implicit(arenas, *left, Operation::String);
                    self.implicit(arenas, *right, Operation::String);
                }
                ExprKind::Binary(
                    BinOp::Eq
                    | BinOp::NotEq
                    | BinOp::Lt
                    | BinOp::Gt
                    | BinOp::LtEq
                    | BinOp::GtEq
                    | BinOp::Spaceship,
                    left,
                    right,
                ) => {
                    if may_be_string(arenas, *right) {
                        self.implicit(arenas, *left, Operation::String);
                    }
                    if may_be_string(arenas, *left) {
                        self.implicit(arenas, *right, Operation::String);
                    }
                }
                ExprKind::Interpolated(parts) => {
                    for &part in arenas.expr_ids(parts) {
                        if !arenas
                            .expr(part)
                            .is_some_and(|part| matches!(&part.kind, ExprKind::Literal(_)))
                        {
                            self.implicit(arenas, part, Operation::String);
                        }
                    }
                }
                ExprKind::Exit(_, Some(args)) => {
                    if let Some(arg) = arenas.args(args.clone()).first() {
                        self.implicit(arenas, arg.value, Operation::String);
                    }
                }
                ExprKind::Clone(inner, with) => self.implicit(
                    arenas,
                    *inner,
                    Operation::Clone {
                        with: with.is_some(),
                    },
                ),
                ExprKind::YieldFrom(inner) => {
                    self.implicit(arenas, *inner, Operation::Iterate { keys: true });
                }
                ExprKind::ArrayLiteral(items) => {
                    for item in arenas
                        .array_items(items.clone())
                        .iter()
                        .filter(|item| item.spread)
                    {
                        self.implicit(arenas, item.value, Operation::Iterate { keys: true });
                    }
                }
                _ => {}
            }
            match &e.kind {
                ExprKind::Closure(closure_id) => {
                    let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure_id) else {
                        return;
                    };
                    let captures = arenas
                        .closure_uses(&closure.uses)
                        .iter()
                        .map(|u| (u.name.name.clone(), u.by_ref))
                        .collect();
                    self.closures += 1;
                    self.closure(
                        arenas,
                        id,
                        arenas.params(closure.params.clone()),
                        Some(captures),
                        return_classes(closure.return_type.as_deref(), arenas),
                        |walk| {
                            arena::walk_expr(walk, arenas, id);
                        },
                    );
                    self.closures -= 1;
                }
                ExprKind::ArrowFn(closure_id) => {
                    let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*closure_id) else {
                        return;
                    };
                    self.closures += 1;
                    self.closure(
                        arenas,
                        id,
                        arenas.params(arrow.params.clone()),
                        None,
                        return_classes(arrow.return_type.as_deref(), arenas),
                        |walk| arena::walk_expr(walk, arenas, id),
                    );
                    self.closures -= 1;
                }
                ExprKind::FirstClassCallable(call) => {
                    let dynamic = arenas.expr(*call).is_some_and(|call| {
                        matches!(
                            &call.kind,
                            ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..)
                        )
                    });
                    self.impure_unless_in_closure(dynamic);
                    self.closure(arenas, id, &[], None, None, |walk| {
                        walk.record_call(arenas, *call);
                        if let Some(own) = walk.scope.calls.last_mut() {
                            own.arguments = Box::new([ArgAt::Spread]);
                        }
                    });
                    arena::walk_expr(self, arenas, *call);
                }
                ExprKind::Binary(
                    op @ (BinOp::And | BinOp::LogicalAnd | BinOp::Or | BinOp::LogicalOr),
                    left,
                    right,
                ) => {
                    self.visit_expr(arenas, *left);
                    let and = matches!(op, BinOp::And | BinOp::LogicalAnd);
                    let mark = self.narrow(
                        arenas,
                        guards(arenas, *left, and),
                        &[Node::Expr(*left), Node::Expr(*right)],
                    );
                    self.visit_expr(arenas, *right);
                    self.narrowed.truncate(mark);
                }
                ExprKind::Ternary(cond, then, otherwise) => {
                    self.visit_expr(arenas, *cond);
                    for (branch, positive) in [(*then, true), (Some(*otherwise), false)] {
                        let Some(branch) = branch else { continue };
                        let mark = self.narrow(
                            arenas,
                            guards(arenas, *cond, positive),
                            &[Node::Expr(*cond), Node::Expr(branch)],
                        );
                        self.visit_expr(arenas, branch);
                        self.narrowed.truncate(mark);
                    }
                }
                _ => arena::walk_expr(self, arenas, id),
            }
        })
    }
}

/// Collects the variables a body assigns, in the declarations nested in it
/// too.
struct Reassigned<'a>(&'a mut FxHashSet<Name>);

impl Visitor for Reassigned<'_> {
    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        match &e.kind {
            ExprKind::Assign(lhs, _)
            | ExprKind::AssignRef(lhs, _)
            | ExprKind::CompoundAssign(_, lhs, _)
            | ExprKind::Unary(
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                lhs,
            ) => collect_assigned(arenas, *lhs, self.0),
            _ => {}
        }
        arena::walk_expr(self, arenas, id);
    }

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        match &stmt.kind {
            StmtKind::Foreach { key, value, .. } => {
                for target in key.iter().copied().chain([*value]) {
                    collect_assigned(arenas, target, self.0);
                }
            }
            StmtKind::Unset(targets) => {
                for &target in arenas.expr_ids(targets) {
                    collect_assigned(arenas, target, self.0);
                }
            }
            _ => {}
        }
        arena::walk_stmt(self, arenas, id);
    }

    fn visit_catch_clause(&mut self, arenas: &NodeArenas, clause: &CatchClause) {
        if let Some(var) = &clause.var {
            self.0.insert(var.name.clone());
        }
        arena::walk_catch_clause(self, arenas, clause);
    }
}

fn collect_assigned(arenas: &NodeArenas, id: ExprId, out: &mut FxHashSet<Name>) {
    let Some(target) = arenas.expr(id) else {
        return;
    };
    match &target.kind {
        ExprKind::Variable(name) => {
            out.insert(name.clone());
        }
        ExprKind::Destructure(items, _) => {
            for item in arenas.destructure_items(items) {
                if let Some(target) = &item.target {
                    collect_assigned(arenas, *target, out);
                }
            }
        }
        _ => {}
    }
}

syntax::canonical_enum!(PurityCall {
    Function(name),
    ThisMethod(method),
    Static(class, method),
    New(class),
});

syntax::canonical_struct!(Place {
    root,
    steps,
    narrowing
});

syntax::canonical_enum!(Narrowing {
    NoObject,
    Instance(classes),
    Declared(ty),
});

syntax::canonical_enum!(Root {
    This,
    Variable(name),
    Static(class, prop),
    DynamicStatic(prop),
    Global(name),
    Value,
});

syntax::canonical_enum!(Step {
    Property(prop),
    Element,
});

syntax::canonical_enum!(Binding {
    Global(name),
    Static,
    Reference(place),
});

syntax::canonical_enum!(Made {
    Object(class),
    Closure(ids),
});

syntax::canonical_enum!(Receiver {
    This,
    ThisProperty(prop),
    Variable(name),
    Static(class, prop),
    Origins(origins),
    Unknown,
});

syntax::canonical_enum!(Callee {
    Function(name),
    Method { receiver, method },
    Static { class, method },
    New(class),
    Anonymous(name),
    Value(name),
    Written(ids),
    Implicit { on, operation },
    ParentHook { property, set },
    Include(at),
});

syntax::canonical_enum!(Operation {
    Property(name, access),
    Element(access),
    String,
    Clone { with },
    Iterate { keys },
    Destroy,
});

syntax::canonical_enum!(Access {
    Read,
    Write,
    Isset,
    Unset,
});

syntax::canonical_enum!(Origin {
    NoObject,
    Arithmetic,
    Place(place),
    Returned(callee),
    New(class),
    Anonymous(name),
    Closure,
    Caught(classes),
    Constant(name),
    ClassConstant(class, constant),
    Declared(ty),
    Elements(inner),
    Taken(callee, from, held),
    Written(name),
});

syntax::canonical_enum!(ArgAt {
    Position(at),
    Named(name),
    Spread,
});

syntax::canonical_enum!(Callable {
    Text(name),
    Pair(target, method),
    Written(id),
});

syntax::canonical_enum!(PairTarget {
    Object(receiver),
    Class(class),
});

syntax::canonical_struct!(CallWrites {
    callee,
    arguments,
    places,
    objectless,
    callables,
    fills,
    dies,
});

syntax::canonical_struct!(WriteFacts {
    reassigned,
    overwritten,
    writes,
    bindings,
    made,
    origins,
    dying_locals,
    computed,
    anything,
    calls,
});

syntax::canonical_struct!(ClosureFacts {
    within,
    params,
    captures,
    returns,
    facts,
});

syntax::canonical_struct!(ClosureParam {
    name,
    classes,
    untyped,
});

syntax::canonical_struct!(BodySummary {
    purity,
    writes,
    closures,
    empty,
    assigned,
    parameters,
    fresh,
});

syntax::canonical_enum!(FreshCall {
    Function(name),
    Static(class, method),
    New(class),
});

syntax::canonical_struct!(FreshReturns { calls });

syntax::canonical_struct!(ParamUses { params, this });

syntax::canonical_struct!(ThisUse {
    kept,
    passed,
    calls,
    accesses_properties,
});

syntax::canonical_enum!(ThisCall {
    Method(name),
    SelfMethod(name),
    ParentMethod(name),
});

syntax::canonical_struct!(ParamUse { kept, passed });

syntax::canonical_struct!(AssignedProperties {
    on_this,
    on_variables,
    new_instances,
});

#[cfg(test)]
mod tests {
    use super::*;

    fn place(name: &str) -> Origin {
        Origin::Place(Place::root(Root::Variable(Some(Name::from(name)))))
    }

    #[test]
    fn the_elements_of_an_origin_are_not_the_origin() {
        let inner = place("list");
        let elements = Origin::Elements(Arc::new(inner.clone()));
        assert_ne!(inner, elements);
        assert_ne!(hash_of(&inner), hash_of(&elements));
        assert_eq!(elements, Origin::Elements(Arc::new(inner.clone())));
        assert_ne!(
            syntax::canonical::print(&inner),
            syntax::canonical::print(&elements)
        );
        assert_ne!(elements, Origin::Elements(Arc::new(place("other"))));
    }

    #[test]
    fn elements_of_an_origin_that_holds_no_array_are_none() {
        for origin in [Origin::NoObject, Origin::Arithmetic, Origin::Closure] {
            assert!(origin.elements().is_none());
        }
        assert!(Origin::New(None).elements().is_none());
        assert_eq!(
            place("list").elements(),
            Some(Origin::Elements(Arc::new(place("list"))))
        );
    }

    #[test]
    fn the_elements_of_a_place_depend_on_the_scope_as_the_place_does() {
        assert!(!Origin::Elements(Arc::new(place("list"))).context_free());
        assert!(
            Origin::Elements(Arc::new(Origin::Constant(QualifiedName {
                written: Name::from("LIST"),
                resolved: ResolvedName::Exact(Name::from("LIST")),
                span: syntax::span::Span::default(),
            })))
            .context_free()
        );
    }
}
