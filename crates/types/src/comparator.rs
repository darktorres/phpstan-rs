//! PHPUnit's `assertEquals`: `sebastian/comparator`'s dispatch
//! (`Factory::registerDefaultComparators`), lifted to types. It isn't PHP
//! `==`: `null` doesn't equal `[]`, two strings compare byte for byte, and
//! objects of different classes never compare equal.

use crate::union::shape_parts;
use crate::{PhpVersion, Relation, TrinaryLogic, Type};

/// Classes whose comparator compares content across classes: DateTime
/// and DateTimeImmutable by instant, DOM nodes as canonical XML, object
/// storages by their contents.
const CROSS_CLASS: [&str; 4] = [
    "DateTime",
    "DateTimeImmutable",
    "DOMNode",
    "SplObjectStorage",
];

impl Relation<'_> {
    /// Whether the comparator finds `expected` and `actual` equal. With
    /// `exact` false (a delta, canonicalizing or ignoring case), only what
    /// no such option changes is decided.
    pub fn comparator_equal(&self, expected: &Type, actual: &Type, exact: bool) -> TrinaryLogic {
        let mut any_yes = false;
        let mut any_no = false;
        for e in expected.union_members().iter() {
            for a in actual.union_members().iter() {
                match self.comparator_equal_member(e, a, exact) {
                    TrinaryLogic::Yes => any_yes = true,
                    TrinaryLogic::No => any_no = true,
                    TrinaryLogic::Maybe => return TrinaryLogic::Maybe,
                }
            }
        }
        match (any_yes, any_no) {
            (true, false) => TrinaryLogic::Yes,
            (false, true) => TrinaryLogic::No,
            _ => TrinaryLogic::Maybe,
        }
    }

    /// `current` where the comparator finds it `equal` to `expected`: a
    /// member it can't (or, for the false branch, surely does) equal is
    /// dropped, and against one scalar value the true branch keeps only a
    /// member's equal values.
    pub fn narrow_comparator(
        &self,
        current: &Type,
        expected: &Type,
        equal: bool,
        exact: bool,
    ) -> Type {
        let single = exact && expected.is_single_value() && is_scalar_or_null(expected);
        let kept: Vec<Type> = current
            .union_members()
            .iter()
            .filter_map(|member| {
                let verdict = self.comparator_equal(expected, member, exact);
                if equal {
                    if verdict.no() {
                        return None;
                    }
                    if single && is_scalar_or_null(member) {
                        let part = if member.is_string().yes() && expected.is_string().yes() {
                            self.narrow(member, expected)
                        } else {
                            self.narrow_loose(member, expected, true, Some(PhpVersion::new(8, 0)))
                        };
                        return (!part.is_never()).then_some(part);
                    }
                    Some(member.clone())
                } else {
                    (!verdict.yes()).then(|| member.clone())
                }
            })
            .collect();
        self.union_of(kept)
    }

    fn comparator_equal_member(&self, e: &Type, a: &Type, exact: bool) -> TrinaryLogic {
        // `BcMath\Number` compares with ints and numeric strings.
        if self.may_be_class(e, "BcMath\\Number") || self.may_be_class(a, "BcMath\\Number") {
            return TrinaryLogic::Maybe;
        }
        let (eo, ao) = (e.is_object(), a.is_object());
        if eo.yes() && ao.yes() {
            return self.objects_equal(e, a);
        }
        if eo.yes() || ao.yes() {
            let (object, other) = if eo.yes() { (e, a) } else { (a, e) };
            // A string and an object with `__toString` compare as strings.
            if !other.is_string().no() && self.may_have_to_string(object) {
                return TrinaryLogic::Maybe;
            }
            return if other.is_object().no() {
                TrinaryLogic::No
            } else {
                TrinaryLogic::Maybe
            };
        }
        if !eo.no() || !ao.no() {
            return TrinaryLogic::Maybe;
        }
        let (ea, aa) = (e.is_array(), a.is_array());
        if ea.yes() && aa.yes() {
            return self.arrays_equal(e, a, exact);
        }
        if ea.yes() || aa.yes() {
            let other = if ea.yes() { a } else { e };
            return if other.is_array().no() {
                TrinaryLogic::No
            } else {
                TrinaryLogic::Maybe
            };
        }
        if !is_scalar_or_null(e) || !is_scalar_or_null(a) {
            return TrinaryLogic::Maybe;
        }
        if !exact {
            return TrinaryLogic::Maybe;
        }
        // Two strings compare byte for byte; other scalars as PHP 8's `==`
        // does, a string against a non-bool as strings.
        if e.is_string().yes() && a.is_string().yes() {
            return if e.is_single_value() && a.is_single_value() {
                TrinaryLogic::from_bool(e == a)
            } else if self.narrow(e, a).is_never() {
                TrinaryLogic::No
            } else {
                TrinaryLogic::Maybe
            };
        }
        self.loose_equal(e, a, Some(PhpVersion::new(8, 0)))
    }

    fn objects_equal(&self, e: &Type, a: &Type) -> TrinaryLogic {
        if let (Some((ec, ecase)), Some((ac, acase))) = (e.as_enum_case(), a.as_enum_case()) {
            return TrinaryLogic::from_bool(ec.eq_ignore_case(ac) && ecase == acase);
        }
        let cross_class = |t: &Type| CROSS_CLASS.iter().any(|class| self.may_be_class(t, class));
        if cross_class(e) && cross_class(a) {
            return TrinaryLogic::Maybe;
        }
        // Otherwise the classes must be the same.
        if self.disjoint(e, a) {
            TrinaryLogic::No
        } else {
            TrinaryLogic::Maybe
        }
    }

    fn arrays_equal(&self, e: &Type, a: &Type, exact: bool) -> TrinaryLogic {
        if let (Some((el, eh)), Some((al, ah))) = (e.count_range(), a.count_range())
            && (eh.is_some_and(|h| h < al) || ah.is_some_and(|h| h < el))
        {
            return TrinaryLogic::No;
        }
        let (Some((es, _)), Some((as_, _))) = (shape_parts(e), shape_parts(a)) else {
            return TrinaryLogic::Maybe;
        };
        let constant = |s: &crate::Shape| s.is_sealed() && s.fields.iter().all(|f| !f.optional);
        if !constant(es) || !constant(as_) {
            return TrinaryLogic::Maybe;
        }
        // Every key on both sides, each value compared as a pair.
        if es.fields.len() != as_.fields.len()
            || es
                .fields
                .iter()
                .any(|f| !as_.fields.iter().any(|g| g.key == f.key))
        {
            return TrinaryLogic::No;
        }
        let mut out = TrinaryLogic::Yes;
        for f in &es.fields {
            let g = as_
                .fields
                .iter()
                .find(|g| g.key == f.key)
                .expect("key sets are equal");
            match self.comparator_equal(&f.ty, &g.ty, exact) {
                TrinaryLogic::No => return TrinaryLogic::No,
                TrinaryLogic::Maybe => out = TrinaryLogic::Maybe,
                TrinaryLogic::Yes => {}
            }
        }
        out
    }

    /// Whether a value of `ty` may be an instance of `class`; never when
    /// the class doesn't exist.
    fn may_be_class(&self, ty: &Type, class: &str) -> bool {
        !ty.is_object().no()
            && self.hierarchy.find_class(class).is_some()
            && !self.disjoint(ty, &Type::object(class.into(), Vec::new()))
    }

    fn may_have_to_string(&self, object: &Type) -> bool {
        let names = object.object_class_names();
        names.is_empty()
            || names.iter().any(|class| {
                self.hierarchy.find_class(class).is_none()
                    || self.hierarchy.has_method(class, "__toString")
                    || !self.hierarchy.find_class(class).is_some_and(|c| c.is_final)
            })
    }
}

fn is_scalar_or_null(ty: &Type) -> bool {
    ty.union_members()
        .iter()
        .all(|m| m.is_scalar().yes() || m.is_null().yes())
}
