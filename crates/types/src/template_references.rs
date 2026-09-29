//! Where a type uses templates, and the variance of each position
//! (PHPStan's `getReferencedTemplateTypes()`).

use crate::{CallSiteVariance, ClassHierarchy, Repr, TemplateParam, Type, Variance};

/// The variance of a position a type occurs in (PHPStan's
/// `TemplateTypeVariance` as a position).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionVariance {
    Invariant,
    Covariant,
    Contravariant,
    Bivariant,
    /// Takes the variance of whatever it is composed with: an ancestor
    /// type's own position.
    Static,
}

impl PositionVariance {
    /// A position of variance `inner` within this one: covariance keeps
    /// the direction, contravariance flips it, invariance fixes it.
    pub fn compose(self, inner: PositionVariance) -> PositionVariance {
        use PositionVariance::*;
        match (self, inner) {
            (Static, inner) => inner,
            (Invariant, _) => Invariant,
            (Bivariant, _) | (_, Bivariant) => Bivariant,
            (Covariant, Covariant) | (Contravariant, Contravariant) => Covariant,
            (Covariant, Contravariant) | (Contravariant, Covariant) => Contravariant,
            (_, Invariant | Static) => Invariant,
        }
    }

    /// Whether a template declared `declared` may occur in this position.
    pub fn is_valid_for(self, declared: Variance) -> bool {
        declared == Variance::Invariant
            || matches!(self, PositionVariance::Bivariant | PositionVariance::Static)
            || PositionVariance::from(declared) == self
    }

    pub fn describe(self) -> &'static str {
        match self {
            PositionVariance::Invariant => "invariant",
            PositionVariance::Covariant => "covariant",
            PositionVariance::Contravariant => "contravariant",
            PositionVariance::Bivariant => "bivariant",
            PositionVariance::Static => "static",
        }
    }
}

impl From<Variance> for PositionVariance {
    fn from(variance: Variance) -> PositionVariance {
        match variance {
            Variance::Invariant => PositionVariance::Invariant,
            Variance::Covariant => PositionVariance::Covariant,
            Variance::Contravariant => PositionVariance::Contravariant,
        }
    }
}

impl From<CallSiteVariance> for PositionVariance {
    fn from(variance: CallSiteVariance) -> PositionVariance {
        match variance {
            CallSiteVariance::Covariant => PositionVariance::Covariant,
            CallSiteVariance::Contravariant => PositionVariance::Contravariant,
            CallSiteVariance::Bivariant => PositionVariance::Bivariant,
        }
    }
}

impl Type {
    /// Each template this type uses, with the variance of the position it
    /// occurs in when this type occurs at `position`. A generic argument
    /// sits at its slot's variance, `hierarchy`'s declared one unless a
    /// call-site annotation overrides it; an argument of a class
    /// `hierarchy` doesn't know is skipped.
    pub fn referenced_templates(
        &self,
        position: PositionVariance,
        hierarchy: &dyn ClassHierarchy,
    ) -> Vec<(&TemplateParam, PositionVariance)> {
        let mut out = Vec::new();
        self.collect_templates(position, hierarchy, &mut out);
        out
    }

    fn collect_templates<'t>(
        &'t self,
        position: PositionVariance,
        hierarchy: &dyn ClassHierarchy,
        out: &mut Vec<(&'t TemplateParam, PositionVariance)>,
    ) {
        match self.repr() {
            Repr::TemplateParam(param) => out.push((param, position)),
            Repr::Object(class, args) => {
                for (i, arg) in args.iter().enumerate() {
                    let (slot, arg) = match arg.repr() {
                        Repr::VarianceArg(variance, inner) => ((*variance).into(), inner),
                        _ => match hierarchy.slot_variance(class, i) {
                            Some(variance) => (variance.into(), arg),
                            None if hierarchy.find_class(class).is_some() => {
                                (PositionVariance::Invariant, arg)
                            }
                            None => continue,
                        },
                    };
                    arg.collect_templates(position.compose(slot), hierarchy, out);
                }
            }
            Repr::VarianceArg(variance, inner) => {
                inner.collect_templates(position.compose((*variance).into()), hierarchy, out)
            }
            Repr::Array(..) | Repr::ArrayShape(_) | Repr::ObjectShape(_) | Repr::ClassString(_) => {
                let inner = position.compose(PositionVariance::Covariant);
                self.for_each_child(&mut |t| t.collect_templates(inner, hierarchy, out));
            }
            Repr::Callable(Some(sig)) => {
                let params = position.compose(PositionVariance::Contravariant);
                for param in &sig.params {
                    param.ty.collect_templates(params, hierarchy, out);
                }
                let ret = position.compose(PositionVariance::Covariant);
                sig.return_type.collect_templates(ret, hierarchy, out);
            }
            Repr::Union(_)
            | Repr::BenevolentUnion(_)
            | Repr::Intersection(_)
            | Repr::ValueOf(_)
            | Repr::KeyOf(_)
            | Repr::OffsetAccess(..)
            | Repr::ClassConstant { .. }
            | Repr::Conditional { .. } => {
                self.for_each_child(&mut |t| t.collect_templates(position, hierarchy, out));
            }
            _ => {}
        }
    }
}
