use crate::checker::narrowing::predicates::narrowing_type_for_predicate;
use crate::checker::{Checker, Locals, PregCall, resolve_call_args};
use crate::registry::FunctionReflection;
use syntax::ast::*;
use types::{AccessoryKind, OrderingOp, Type};

const ASSERT: &str = "PHPUnit\\Framework\\Assert";
/// What every assertion method's name starts with, in any case.
const ASSERT_PREFIX: &str = "assert";
const FUNCTIONS: &str = "phpunit\\framework\\";

/// What a PHPUnit assertion or constraint checks of a value.
enum Check<'e> {
    /// The value is of this type; negated, it isn't.
    Type(Type),
    /// The value is an instance of this type; negated only when `exact`
    /// names one class, not a class-string that may name a subclass.
    Instance {
        ty: Type,
        exact: bool,
    },
    /// `===` this expression.
    Identical(&'e Expr),
    /// Equal as `sebastian/comparator` compares; `exact` without a delta,
    /// canonicalizing or ignoring case.
    Equal {
        expected: &'e Expr,
        exact: bool,
    },
    /// Empty: `empty()`, or a `Countable` counting 0.
    Empty,
    /// Counts exactly a value of this type.
    Count(Type),
    /// Has this key, as `array_key_exists` or `offsetExists`.
    HasKey(&'e Expr),
    /// `value op other` holds.
    Ordering(OrderingOp, &'e Expr),
    /// A string, non-empty when `non_empty`; through a `string` parameter
    /// (`coerced`), a scalar or `Stringable` passes too.
    Str {
        non_empty: bool,
        coerced: bool,
    },
    /// A string matching this pattern.
    Matches {
        pattern: &'e Expr,
        coerced: bool,
    },
    /// A haystack holding this needle, compared with `===` (`strict`) or
    /// `==`.
    Contains {
        needle: &'e Expr,
        strict: bool,
    },
    /// An object with this property.
    HasProperty(&'e Expr),
    Not(Box<Check<'e>>),
    And(Vec<Check<'e>>),
    Or(Vec<Check<'e>>),
}

impl<'a> Checker<'a> {
    /// Narrows by a PHPUnit assertion statement: a method of
    /// `PHPUnit\Framework\Assert` (`$this->assertX()`, `self::assertX()`,
    /// `Assert::assertX()`) or one of its global `assertX()` functions.
    /// Past the statement the assertion held, since a failing one throws.
    pub(crate) fn apply_phpunit_assertion_narrowing(&self, e: &Expr, locals: &mut Locals) {
        // Every assertion method's name starts with `assert`; a call written
        // with another name is not probed for its owner. A function call is
        // always probed, since `use function ... as` can rename an assertion.
        if let ExprKind::MethodCall(_, method, ..) | ExprKind::StaticCall(_, method, _) =
            &e.peeled().kind
            && !method
                .as_bytes()
                .get(..ASSERT_PREFIX.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(ASSERT_PREFIX.as_bytes()))
        {
            return;
        }
        let Some((name, sig, args)) = self.phpunit_method(e, locals) else {
            return;
        };
        let resolved = resolve_call_args(args, sig.params());
        let arg = |i: usize| resolved.get(i).copied().flatten();
        let Some((value, check, holds)) = self.assertion_check(&name, sig, &arg, locals) else {
            return;
        };
        self.apply_check(value, &check, holds, locals);
        // `assertSameSize` bounds both counts.
        if matches!(name.as_str(), "assertsamesize" | "assertnotsamesize")
            && let (Some(expected), Some(actual)) = (arg(0), arg(1))
            && let Some(count) = self.count_type(actual, locals)
        {
            self.apply_check(expected, &Check::Count(count), holds, locals);
        }
    }

    /// A call's PHPUnit method or function: its lowercase name, signature
    /// and arguments.
    fn phpunit_method<'e>(
        &self,
        e: &'e Expr,
        locals: &Locals,
    ) -> Option<(String, &'a FunctionReflection, &'e [Arg])> {
        match &e.peeled().kind {
            ExprKind::MethodCall(recv, method, args, _) => {
                let recv_ty = self.infer_expr(recv, locals);
                let (owner, sig) = self.find_method_owner(&recv_ty, method)?;
                owner
                    .eq_ignore_ascii_case(ASSERT)
                    .then(|| (method.to_ascii_lowercase(), sig, args.as_slice()))
            }
            ExprKind::StaticCall(target, method, args) => {
                let class = self.infer_static_target_class(target, locals)?;
                let (owner, sig) = self.lookup_method_with_owner(&class, method)?;
                owner
                    .eq_ignore_ascii_case(ASSERT)
                    .then(|| (method.to_ascii_lowercase(), sig, args.as_slice()))
            }
            ExprKind::Call(callee, args) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return None;
                };
                let (resolved, sig) = self.registry.resolve_function(name)?;
                let lower = resolved.to_ascii_lowercase();
                let bare = lower.strip_prefix(FUNCTIONS)?;
                (!bare.contains('\\')).then(|| (bare.to_string(), sig, args.as_slice()))
            }
            _ => None,
        }
    }

    /// The value an assertion checks, the check, and whether it holds.
    fn assertion_check<'e>(
        &self,
        name: &str,
        sig: &FunctionReflection,
        arg: &dyn Fn(usize) -> Option<&'e Expr>,
        locals: &Locals,
    ) -> Option<(&'e Expr, Check<'e>, bool)> {
        if let Some(kind) = name.strip_prefix("assertisnot") {
            return Some((arg(0)?, Check::Type(is_kind_type(kind)?), false));
        }
        if let Some(kind) = name.strip_prefix("assertis")
            && let Some(ty) = is_kind_type(kind)
        {
            return Some((arg(0)?, Check::Type(ty), true));
        }
        let string = |non_empty| Check::Str {
            non_empty,
            coerced: true,
        };
        let non_empty_needle =
            |e: Option<&Expr>| e.is_some_and(|e| self.is_non_empty_string(e, locals));
        let equal = |exact| Check::Equal {
            expected: arg(0).expect("checked by the caller"),
            exact,
        };
        Some(match name {
            "assertnull" => (arg(0)?, Check::Type(Type::null()), true),
            "assertnotnull" => (arg(0)?, Check::Type(Type::null()), false),
            "asserttrue" => (arg(0)?, Check::Type(Type::bool_literal(true)), true),
            "assertnottrue" => (arg(0)?, Check::Type(Type::bool_literal(true)), false),
            "assertfalse" => (arg(0)?, Check::Type(Type::bool_literal(false)), true),
            "assertnotfalse" => (arg(0)?, Check::Type(Type::bool_literal(false)), false),
            "assertinstanceof" | "assertnotinstanceof" => {
                let (ty, exact) = self.instance_target(arg(0)?, locals)?;
                (
                    arg(1)?,
                    Check::Instance { ty, exact },
                    name == "assertinstanceof",
                )
            }
            "assertsame" | "assertnotsame" => {
                (arg(1)?, Check::Identical(arg(0)?), name == "assertsame")
            }
            "assertequals" | "assertnotequals" => {
                arg(0)?;
                (arg(1)?, equal(true), name == "assertequals")
            }
            "assertequalscanonicalizing"
            | "assertequalsignoringcase"
            | "assertequalswithdelta"
            | "assertnotequalscanonicalizing"
            | "assertnotequalsignoringcase"
            | "assertnotequalswithdelta" => {
                arg(0)?;
                (arg(1)?, equal(false), !name.starts_with("assertnot"))
            }
            "assertempty" => (arg(0)?, Check::Empty, true),
            "assertnotempty" => (arg(0)?, Check::Empty, false),
            "assertarrayhaskey" => (arg(1)?, Check::HasKey(arg(0)?), true),
            "assertarraynothaskey" => (arg(1)?, Check::HasKey(arg(0)?), false),
            "assertcount" | "assertnotcount" => {
                let count = self.infer_expr(arg(0)?, locals);
                (arg(1)?, Check::Count(count), name == "assertcount")
            }
            "assertsamesize" | "assertnotsamesize" => {
                let count = self.count_type(arg(0)?, locals)?;
                (arg(1)?, Check::Count(count), name == "assertsamesize")
            }
            // Before PHPUnit 9 `assertContains` compared non-objects
            // loosely unless told otherwise.
            "assertcontains" | "assertnotcontains"
                if !sig
                    .params()
                    .iter()
                    .any(|p| p.name().as_ref() == "checkForNonObjectIdentity") =>
            {
                (
                    arg(1)?,
                    Check::Contains {
                        needle: arg(0)?,
                        strict: true,
                    },
                    name == "assertcontains",
                )
            }
            "assertcontainsequals" | "assertnotcontainsequals" => (
                arg(1)?,
                Check::Contains {
                    needle: arg(0)?,
                    strict: false,
                },
                name == "assertcontainsequals",
            ),
            "assertgreaterthan" => (arg(1)?, Check::Ordering(OrderingOp::Gt, arg(0)?), true),
            "assertlessthan" => (arg(1)?, Check::Ordering(OrderingOp::Lt, arg(0)?), true),
            // `greaterThanOrEqual` is `logicalOr(equalTo, greaterThan)`.
            "assertgreaterthanorequal" | "assertlessthanorequal" => {
                let op = if name == "assertgreaterthanorequal" {
                    OrderingOp::Gt
                } else {
                    OrderingOp::Lt
                };
                (
                    arg(1)?,
                    Check::Or(vec![equal(true), Check::Ordering(op, arg(0)?)]),
                    true,
                )
            }
            "assertobjecthasproperty" => (arg(1)?, Check::HasProperty(arg(0)?), true),
            "assertstringstartswith"
            | "assertstringendswith"
            | "assertstringcontainsstring"
            | "assertstringcontainsstringignoringcase"
            | "assertstringcontainsstringignoringlineendings" => {
                (arg(1)?, string(non_empty_needle(arg(0))), true)
            }
            "assertstringstartsnotwith"
            | "assertstringendsnotwith"
            | "assertstringnotcontainsstring"
            | "assertstringnotcontainsstringignoringcase"
            | "assertdoesnotmatchregularexpression"
            | "assertstringmatchesformat" => (arg(1)?, string(false), true),
            "assertmatchesregularexpression" => (
                arg(1)?,
                Check::Matches {
                    pattern: arg(0)?,
                    coerced: true,
                },
                true,
            ),
            "assertjson" => (arg(0)?, string(true), true),
            "assertnan" | "assertinfinite" => (arg(0)?, Check::Type(Type::float()), true),
            "assertfinite" => (
                arg(0)?,
                Check::Type(self.relation().union(Type::int(), Type::float())),
                true,
            ),
            "assertthat" => (arg(0)?, self.constraint(arg(1)?, locals)?, true),
            _ => return None,
        })
    }

    /// An `assertThat` constraint built directly by one of `Assert`'s
    /// factories; `None` for any other expression.
    fn constraint<'e>(&self, e: &'e Expr, locals: &Locals) -> Option<Check<'e>> {
        stack::guard(|| {
            let (name, sig, args) = self.phpunit_method(e, locals)?;
            let resolved = resolve_call_args(args, sig.params());
            let arg = |i: usize| resolved.get(i).copied().flatten();
            let parts = |locals: &Locals| -> Option<Vec<Check<'e>>> {
                args.iter()
                    .map(|a| (!a.spread).then(|| self.constraint(&a.value, locals))?)
                    .collect()
            };
            Some(match name.as_str() {
                "istrue" => Check::Type(Type::bool_literal(true)),
                "isfalse" => Check::Type(Type::bool_literal(false)),
                "isnull" => Check::Type(Type::null()),
                "istype" => {
                    let kind = literal_string_value(arg(0)?)?;
                    Check::Type(native_type(&kind)?)
                }
                "isinstanceof" => {
                    let (ty, exact) = self.instance_target(arg(0)?, locals)?;
                    Check::Instance { ty, exact }
                }
                "identicalto" => Check::Identical(arg(0)?),
                "equalto" => Check::Equal {
                    expected: arg(0)?,
                    exact: true,
                },
                "isempty" => Check::Empty,
                "countof" => Check::Count(self.infer_expr(arg(0)?, locals)),
                "arrayhaskey" => Check::HasKey(arg(0)?),
                "greaterthan" => Check::Ordering(OrderingOp::Gt, arg(0)?),
                "lessthan" => Check::Ordering(OrderingOp::Lt, arg(0)?),
                "stringstartswith" => Check::Str {
                    non_empty: self.is_non_empty_string(arg(0)?, locals),
                    coerced: true,
                },
                // A haystack that isn't a string fails, unless the needle is
                // empty.
                "stringcontains" => {
                    let non_empty = self.is_non_empty_string(arg(0)?, locals);
                    Check::Str {
                        non_empty,
                        coerced: !non_empty,
                    }
                }
                "isjson" => Check::Str {
                    non_empty: true,
                    coerced: false,
                },
                "matchesregularexpression" => Check::Matches {
                    pattern: arg(0)?,
                    coerced: false,
                },
                "containsidentical" => Check::Contains {
                    needle: arg(0)?,
                    strict: true,
                },
                "containsequal" => Check::Contains {
                    needle: arg(0)?,
                    strict: false,
                },
                "islist" => Check::Type(is_kind_type("list")?),
                "isnan" | "isinfinite" => Check::Type(Type::float()),
                "isfinite" => Check::Type(self.relation().union(Type::int(), Type::float())),
                "logicalnot" => Check::Not(Box::new(self.constraint(arg(0)?, locals)?)),
                "logicaland" => Check::And(parts(locals)?),
                "logicalor" => Check::Or(parts(locals)?),
                // `isInt()`, `isString()` and the other type factories.
                other => Check::Type(is_kind_type(other.strip_prefix("is")?)?),
            })
        })
    }

    /// The type an `assertInstanceOf` class argument names, and whether
    /// it names exactly one class (`Foo::class`, `'Foo'`).
    fn instance_target(&self, e: &Expr, locals: &Locals) -> Option<(Type, bool)> {
        if let ExprKind::ClassNameFetch(target) = &e.kind
            && let Some(name) = target.as_name()
        {
            return Some((self.resolve_name_expr(name), true));
        }
        let ty = self.infer_expr(e, locals);
        let mut exact = true;
        let classes: Option<Vec<Type>> = ty
            .union_members()
            .iter()
            .map(|m| {
                if let Some(class) = m.class_string_class() {
                    exact = false;
                    Some(Type::object_ref(class, &[]))
                } else {
                    let name = m.string_literal_value()?.as_str()?;
                    Some(Type::object(
                        name.trim_start_matches('\\').into(),
                        Vec::new(),
                    ))
                }
            })
            .collect();
        let classes = classes?;
        let exact = exact && classes.len() == 1;
        Some((self.relation().union_of(classes), exact))
    }

    /// Whether `e` is surely a non-empty string.
    fn is_non_empty_string(&self, e: &Expr, locals: &Locals) -> bool {
        self.infer_expr(e, locals)
            .is_refined_string(AccessoryKind::NonEmptyString)
            .yes()
    }

    /// The count type of `e`: its count range, when it's an array.
    fn count_type(&self, e: &Expr, locals: &Locals) -> Option<Type> {
        let (lo, hi) = self.infer_expr(e, locals).count_range()?;
        Some(Type::int_range(Some(lo), hi))
    }

    /// Narrows `value` by `check` having held (`holds`) or failed.
    fn apply_check(&self, value: &Expr, check: &Check<'_>, holds: bool, locals: &mut Locals) {
        stack::guard(|| {
            let key = locals.key_of(value);
            let narrow_value = |locals: &mut Locals, f: &dyn Fn(&Type) -> Option<Type>| {
                if let Some(key) = key {
                    let current = self.current_narrowed_type(value, key, locals);
                    if let Some(narrowed) = f(&current) {
                        locals.insert(key, narrowed);
                    }
                }
            };
            match check {
                Check::Type(ty) => narrow_value(locals, &|current| {
                    Some(if holds {
                        self.relation().narrow(current, ty)
                    } else {
                        self.exclude(current, ty)
                    })
                }),
                Check::Instance { ty, exact } => narrow_value(locals, &|current| {
                    if holds {
                        Some(self.relation().narrow_to_instance(current, ty))
                    } else {
                        exact.then(|| self.exclude(current, ty))
                    }
                }),
                Check::Identical(other) => {
                    self.apply_equality_narrowing(value, other, locals, holds)
                }
                Check::Equal { expected, exact } => {
                    let expected = self.infer_expr(expected, locals);
                    narrow_value(locals, &|current| {
                        Some(
                            self.relation()
                                .narrow_comparator(current, &expected, holds, *exact),
                        )
                    })
                }
                Check::Empty if holds => narrow_value(locals, &|current| {
                    // A `Countable` object counting 0 is empty too.
                    let countable = Type::object("Countable".into(), Vec::new());
                    let kept: Vec<Type> = current
                        .union_members()
                        .iter()
                        .filter_map(|m| {
                            if !m.is_object().no()
                                && !self.relation().is_super_type_of(&countable, m).no()
                            {
                                Some(m.clone())
                            } else {
                                let falsy = m.only_falsy(&self.relation());
                                (!falsy.is_never()).then_some(falsy)
                            }
                        })
                        .collect();
                    Some(self.relation().union_of(kept))
                }),
                Check::Empty => self.narrow_emptiness(value, false, true, locals),
                Check::Count(count) => {
                    self.narrow_counted(value, false, BinOp::Identical, count, holds, locals)
                }
                Check::HasKey(k) => self.narrow_key_presence(k, value, holds, locals),
                Check::Ordering(op, other) => {
                    self.apply_ordering_narrowing(value, other, *op, holds, locals)
                }
                Check::Str { non_empty, coerced } if holds => narrow_value(locals, &|current| {
                    let target = if *non_empty {
                        Type::string().with_string_accessory(AccessoryKind::NonEmptyString)
                    } else {
                        Type::string()
                    };
                    Some(self.refine_string_members(current, &target, *coerced))
                }),
                Check::Matches { pattern, coerced } if holds => {
                    let call = PregCall {
                        all: false,
                        pattern,
                        subject: Some(value),
                        matches: None,
                        flags: None,
                        offset: None,
                    };
                    self.narrow_preg_outcome(&call, &Type::int_literal(1), locals);
                    narrow_value(locals, &|current| {
                        Some(self.refine_string_members(current, &Type::string(), *coerced))
                    });
                }
                Check::Contains { needle, strict } => {
                    if *strict {
                        self.narrow_strict_membership(needle, value, holds, locals);
                    } else {
                        self.narrow_loose_membership(needle, value, holds, locals);
                    }
                }
                Check::HasProperty(name) if holds => {
                    let property = literal_string_value(name);
                    narrow_value(locals, &|current| {
                        let object = match &property {
                            Some(p) => Type::intersection_of(vec![
                                Type::object_without_class(),
                                Type::has_property(p.as_ref().into()),
                            ]),
                            None => Type::object_without_class(),
                        };
                        Some(self.relation().narrow(current, &object))
                    });
                }
                Check::Not(inner) => self.apply_check(value, inner, !holds, locals),
                Check::And(parts) if holds => {
                    for part in parts {
                        self.apply_check(value, part, true, locals);
                    }
                }
                Check::Or(parts) if !holds => {
                    for part in parts {
                        self.apply_check(value, part, false, locals);
                    }
                }
                // One part held: each part's narrowing of the value, joined.
                Check::Or(parts) => {
                    let Some(key) = key else {
                        return;
                    };
                    let mut joined: Vec<Type> = Vec::new();
                    for part in parts {
                        let mut branch = locals.clone();
                        self.apply_check(value, part, true, &mut branch);
                        joined.push(self.current_narrowed_type(value, key, &branch).into_owned());
                    }
                    locals.insert(key, self.relation().union_of(joined));
                }
                // A failed `And`, and the string and property checks failing,
                // say nothing narrowable.
                Check::And(_)
                | Check::Str { .. }
                | Check::Matches { .. }
                | Check::HasProperty(_) => {}
            }
        })
    }

    /// `current` once a `string` parameter took it, its string members
    /// met with `target`. Coerced (a user function's parameter outside
    /// strict types), a scalar and an object that may be `Stringable`
    /// pass unchanged and only `null`, arrays and resources fail;
    /// otherwise only strings pass.
    pub(super) fn refine_string_members(
        &self,
        current: &Type,
        target: &Type,
        coerced: bool,
    ) -> Type {
        let kept: Vec<Type> = current
            .union_members()
            .iter()
            .filter_map(|m| {
                if m.is_string().yes() {
                    let refined = self.relation().narrow(m, target);
                    return (!refined.is_never()).then_some(refined);
                }
                if m.is_string().maybe() {
                    return Some(if coerced {
                        m.clone()
                    } else {
                        self.relation().narrow(m, target)
                    });
                }
                let passes =
                    coerced && (m.is_scalar().yes() || (m.is_object().yes() && !m.is_enum_case()));
                passes.then(|| m.clone())
            })
            .collect();
        self.relation().union_of(kept)
    }
}

/// The type an `assertIsX` / `assertIsNotX` suffix checks.
fn is_kind_type(kind: &str) -> Option<Type> {
    match kind {
        "numeric" => Some(types::parse_type_string("int|float|numeric-string")),
        "list" => Some(types::parse_type_string("list<mixed>")),
        "closedresource" => Some(Type::resource(true)),
        "integer" => narrowing_type_for_predicate("is_int"),
        "boolean" => narrowing_type_for_predicate("is_bool"),
        other => narrowing_type_for_predicate(&format!("is_{other}")),
    }
}

/// The type `isType($type)` checks.
fn native_type(kind: &str) -> Option<Type> {
    match kind {
        "bool" | "boolean" => is_kind_type("bool"),
        "double" | "float" | "real" => is_kind_type("float"),
        "int" | "integer" => is_kind_type("int"),
        "resource (closed)" => is_kind_type("closedresource"),
        "array" | "callable" | "iterable" | "null" | "numeric" | "object" | "resource"
        | "scalar" | "string" => is_kind_type(kind),
        _ => None,
    }
}
