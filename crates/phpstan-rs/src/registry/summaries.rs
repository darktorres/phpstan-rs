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
use syntax::ast::*;
use syntax::name::Name;
use syntax::visit::arena::{self, Visitor};

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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Place {
    pub(crate) root: Root,
    pub(crate) steps: Box<[Step]>,
}

/// What a place starts from.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Receiver {
    /// `$this`.
    This,
    /// A property of `$this`.
    ThisProperty(Name),
    /// Another variable.
    Variable(Name),
    /// A static property, the class as written.
    Static(QualifiedName, Name),
    /// Any other value: a call's result, an element, a literal.
    Unknown,
}

/// What a call is to.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
        on: Option<Box<[Origin]>>,
        operation: Operation,
    },
    /// `parent::$property::get()`, or `::set()` when `set`: the parent
    /// class's hook of the property, on `$this`.
    ParentHook {
        property: Name,
        set: bool,
    },
}

/// An operation on a value through which PHP may run the value's code.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
}

/// What an access does with a property or an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    Returned(Callee),
    /// An object `new` made of the class named (`self`, `static` and
    /// `parent` as written); `None` for a class computed at run time.
    New(Option<QualifiedName>),
    /// An object of the anonymous class of this name.
    Anonymous(Option<Name>),
    /// A closure, an arrow function or a first-class callable.
    Closure,
    /// An exception a `catch` caught, of one of the classes.
    Caught(Box<[QualifiedName]>),
}

impl Origin {
    /// Whether it is the same from any scope: no variable of the scope it is
    /// written in decides it.
    fn context_free(&self) -> bool {
        !matches!(self, Origin::Place(_) | Origin::Returned(_))
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
            ExprKind::Call(callee, _) => match arenas
                .peeled(*callee)
                .and_then(|id| arenas.expr(id))
                .map(|callee| &callee.kind)
            {
                Some(ExprKind::Name(name)) => {
                    vec![Origin::Returned(Callee::Function(
                        arenas.qualified_name(name)?.clone(),
                    ))]
                }
                _ => return None,
            },
            ExprKind::MethodCall(recv, method, _, _) => vec![Origin::Returned(Callee::Method {
                receiver: receiver(arenas, *recv),
                method: Some(method.name.clone()),
            })],
            ExprKind::StaticCall(target, method, _) => {
                vec![Origin::Returned(static_callee(arenas, target, method))]
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
            ExprKind::Grouping(_)
            | ExprKind::ErrorSuppress(_)
            | ExprKind::Destructure(..)
            | ExprKind::DynamicMethodCall(..)
            | ExprKind::DynamicStaticCall(..)
            | ExprKind::ClassConstFetch(..)
            | ExprKind::DynamicClassConstFetch(..)
            | ExprKind::Name(_)
            | ExprKind::Eval(_)
            | ExprKind::Include(..)
            | ExprKind::Yield(..)
            | ExprKind::YieldFrom(_) => return None,
        })
    })
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

impl Summarizer {
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

/// A call in a body, with its arguments that are places, where a
/// by-reference parameter writes them, and those that name a callable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CallWrites {
    pub(crate) callee: Callee,
    /// Where each argument is passed.
    pub(crate) arguments: Box<[ArgAt]>,
    pub(crate) places: Box<[(ArgAt, Place)]>,
    pub(crate) callables: Box<[(ArgAt, Callable)]>,
}

/// What a body or a closure in it writes, before any declaration is looked
/// up. A closure, an arrow function or a first-class callable it writes is
/// not run by being written: what it writes is its own.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WriteFacts {
    /// The variables the body assigns, which a declared type of a parameter
    /// then no longer describes.
    pub(crate) reassigned: Box<[Name]>,
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

    /// The summary of the body of a function with the parameters `params`:
    /// a constructor's assigns each property it promotes, first.
    pub fn of_function(arenas: &NodeArenas, params: &[Param], body: &[StmtId]) -> BodySummary {
        let mut walk = Summarizer::default();
        for param in params.iter().filter(|param| param.promotion.is_some()) {
            walk.implicit_on(
                Some(vec![Origin::Place(Place::root(Root::This))]),
                Operation::Property(Some(param.name.name.clone()), Access::Write),
            );
            walk.write_this_property(&param.name.name);
        }
        arena::walk_stmts(&mut walk, arenas, body);
        let mut summary = walk.finish();
        summary.empty = body.is_empty();
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

/// What one scope writes, as the walk finds it: the body's own, or a
/// closure's.
#[derive(Default)]
struct Scope {
    reassigned: FxHashSet<Name>,
    writes: Vec<Place>,
    bindings: Vec<(Option<Name>, Binding)>,
    // What each variable written holds: what the body made, or `None` once
    // it is written anything else.
    made: FxHashMap<Name, Option<Made>>,
    // Where what each variable written holds comes from, or `None` once it
    // is written a value not known.
    origins: FxHashMap<Name, Option<Vec<Origin>>>,
    computed: bool,
    anything: bool,
    calls: Vec<CallWrites>,
    // The operations listed in `calls`, as printed.
    implicit: FxHashSet<String>,
}

impl Scope {
    /// The scope's facts; `all_written` keeps every variable written in
    /// `made`, not only those holding what the body made.
    fn finish(self, all_written: bool) -> WriteFacts {
        let mut reassigned: Vec<Name> = self.reassigned.into_iter().collect();
        reassigned.sort();
        let mut made: Vec<(Name, Option<Made>)> = self
            .made
            .into_iter()
            .filter(|(_, made)| all_written || made.is_some())
            .collect();
        made.sort_by(|a, b| a.0.cmp(&b.0));
        let mut origins: Vec<(Name, Box<[Origin]>)> = self
            .origins
            .into_iter()
            .filter_map(|(name, origins)| {
                let mut seen = FxHashSet::default();
                let origins: Box<[Origin]> = origins?
                    .into_iter()
                    .filter(|origin| seen.insert(syntax::canonical::print(origin)))
                    .collect();
                Some((name, origins))
            })
            .collect();
        origins.sort_by(|a, b| a.0.cmp(&b.0));
        WriteFacts {
            reassigned: reassigned.into_boxed_slice(),
            writes: self.writes.into_boxed_slice(),
            bindings: self.bindings.into_boxed_slice(),
            made: made.into_boxed_slice(),
            origins: origins.into_boxed_slice(),
            computed: self.computed,
            anything: self.anything,
            calls: self.calls.into_boxed_slice(),
        }
    }
}

/// A scope the walk is inside of, while it walks a closure written in it.
struct Enclosing {
    scope: Scope,
    // The variables the closure written in it takes from it by reference.
    shared: FxHashSet<Name>,
}

#[derive(Default)]
struct Summarizer {
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
    // Each closure's index, by its file-local expression id.
    ids: FxHashMap<ExprId, u32>,
    written: Vec<ClosureFacts>,
    // What an access does with a property or an element, by its expression
    // id, when not only a read (`READ`, `WRITE`, `ISSET`,
    // `UNSET`).
    accesses: FxHashMap<ExprId, u8>,
}

const READ: u8 = 1;
const WRITE: u8 = 2;
const ISSET: u8 = 4;
const UNSET: u8 = 8;

impl Summarizer {
    fn finish(self) -> BodySummary {
        BodySummary {
            purity: (!self.impure).then(|| self.purity_calls.into_boxed_slice()),
            writes: self.scope.finish(false),
            closures: self.written.into_boxed_slice(),
            empty: false,
            assigned: self.assigned.finish(),
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
        walk(self);
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
        }
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

    fn bind(&mut self, variable: Option<Name>, binding: Binding) {
        match &variable {
            Some(name) => {
                self.scope.made.insert(name.clone(), None);
                self.scope.origins.insert(name.clone(), None);
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

    /// An operation PHP may run code of the value `operand` for.
    fn implicit(&mut self, arenas: &NodeArenas, operand: ExprId, operation: Operation) {
        self.implicit_on(origins_of(arenas, operand), operation);
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
        let callee = Callee::Implicit {
            on: on.map(Vec::into_boxed_slice),
            operation,
        };
        if !self
            .scope
            .implicit
            .insert(syntax::canonical::print(&callee))
        {
            return;
        }
        self.scope.calls.push(CallWrites {
            callee,
            arguments: Box::new([]),
            places: Box::new([]),
            callables: Box::new([]),
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
        let on = origins_of(arenas, base);
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
        let args = arenas.args(args);
        let mut spread = false;
        let mut arguments = Vec::new();
        let mut places = Vec::new();
        let mut callables = Vec::new();
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
            if let Some(place) = Place::of(arenas, arg.value) {
                places.push((at, place));
            }
        }
        self.scope.calls.push(CallWrites {
            callee,
            arguments: arguments.into_boxed_slice(),
            places: places.into_boxed_slice(),
            callables: callables.into_boxed_slice(),
        });
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

impl Summarizer {
    /// Records `e` when it is a call.
    fn record_call(&mut self, arenas: &NodeArenas, id: ExprId) {
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
                        receiver: receiver(arenas, *recv),
                        method: Some(method.name.clone()),
                    },
                    args.clone(),
                );
            }
            ExprKind::DynamicMethodCall(recv, _, args, _) => self.call(
                arenas,
                Callee::Method {
                    receiver: receiver(arenas, *recv),
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
        _ => Receiver::Unknown,
    }
}

impl Visitor for Summarizer {
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
            match &stmt.kind {
                StmtKind::Unset(targets) => {
                    for &target in arenas.expr_ids(targets) {
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
                    let source = origins_of(arenas, *subject);
                    self.implicit_on(
                        source.clone(),
                        Operation::Iterate {
                            keys: key.is_some(),
                        },
                    );
                    if let Some(key) = key {
                        self.write(arenas, *key, None, None);
                        self.mark(arenas, *key, WRITE);
                    }
                    let element = source.and_then(element_origins);
                    let value_node = arenas.peeled(*value).and_then(|id| arenas.expr(id));
                    match value_node.map(|node| &node.kind) {
                        Some(ExprKind::Variable(name)) if *by_ref => {
                            let element = Place::of(arenas, *subject)
                                .unwrap_or_else(|| Place::root(Root::Value))
                                .then(Step::Element);
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
                        self.write(arenas, *subject, None, None);
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
                        self.bind(Some(variable.name.name.clone()), Binding::Static);
                    }
                }
                _ => {}
            }
            arena::walk_stmt(self, arenas, id);
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
            match &e.kind {
                ExprKind::Assign(lhs, rhs) => {
                    let made = self.made_by(arenas, *rhs);
                    let origins = origins_of(arenas, *rhs);
                    self.write(arenas, *lhs, made, origins.clone());
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
                        BinOp::Coalesce => origins_of(arenas, *rhs),
                        _ => Some(vec![Origin::Arithmetic]),
                    };
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
                            self.bind(Some(name.clone()), Binding::Reference(place));
                        }
                        _ => self.write(arenas, *lhs, None, None),
                    }
                    self.write(arenas, *rhs, None, None);
                    self.reassign(arenas, *lhs);
                }
                ExprKind::Unary(
                    UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                    target,
                ) => {
                    self.write(arenas, *target, None, Some(vec![Origin::Arithmetic]));
                    self.mark(arenas, *target, READ | WRITE);
                }
                ExprKind::Eval(_) | ExprKind::Include(..) => self.scope.anything = true,
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
                    self.closure(arenas, id, &[], None, |walk| {
                        walk.record_call(arenas, *call);
                        if let Some(own) = walk.scope.calls.last_mut() {
                            own.arguments = Box::new([ArgAt::Spread]);
                        }
                    });
                    arena::walk_expr(self, arenas, *call);
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
        if let ExprKind::Assign(lhs, _)
        | ExprKind::AssignRef(lhs, _)
        | ExprKind::CompoundAssign(_, lhs, _) = &e.kind
        {
            collect_assigned(arenas, *lhs, self.0);
        }
        arena::walk_expr(self, arenas, id);
    }

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        if let StmtKind::Foreach { key, value, .. } = &stmt.kind {
            for target in key.iter().copied().chain([*value]) {
                collect_assigned(arenas, target, self.0);
            }
        }
        arena::walk_stmt(self, arenas, id);
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

syntax::canonical_struct!(Place { root, steps });

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
});

syntax::canonical_enum!(Operation {
    Property(name, access),
    Element(access),
    String,
    Clone { with },
    Iterate { keys },
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
    callables,
});

syntax::canonical_struct!(WriteFacts {
    reassigned,
    writes,
    bindings,
    made,
    origins,
    computed,
    anything,
    calls,
});

syntax::canonical_struct!(ClosureFacts {
    within,
    params,
    captures,
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
});

syntax::canonical_struct!(AssignedProperties {
    on_this,
    on_variables,
    new_instances,
});
