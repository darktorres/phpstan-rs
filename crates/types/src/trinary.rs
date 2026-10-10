//! PHPStan's `TrinaryLogic`: yes, no, or maybe.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TrinaryLogic {
    Yes,
    Maybe,
    No,
}

impl TrinaryLogic {
    pub fn from_bool(value: bool) -> TrinaryLogic {
        if value {
            TrinaryLogic::Yes
        } else {
            TrinaryLogic::No
        }
    }

    #[inline]
    pub fn yes(self) -> bool {
        self == TrinaryLogic::Yes
    }

    pub fn maybe(self) -> bool {
        self == TrinaryLogic::Maybe
    }

    pub fn no(self) -> bool {
        self == TrinaryLogic::No
    }

    /// `TrinaryLogic::and`: the weaker of the two.
    pub fn and(self, other: TrinaryLogic) -> TrinaryLogic {
        match (self, other) {
            (TrinaryLogic::No, _) | (_, TrinaryLogic::No) => TrinaryLogic::No,
            (TrinaryLogic::Maybe, _) | (_, TrinaryLogic::Maybe) => TrinaryLogic::Maybe,
            _ => TrinaryLogic::Yes,
        }
    }

    /// `TrinaryLogic::or`: the stronger of the two.
    pub fn or(self, other: TrinaryLogic) -> TrinaryLogic {
        match (self, other) {
            (TrinaryLogic::Yes, _) | (_, TrinaryLogic::Yes) => TrinaryLogic::Yes,
            (TrinaryLogic::Maybe, _) | (_, TrinaryLogic::Maybe) => TrinaryLogic::Maybe,
            _ => TrinaryLogic::No,
        }
    }

    pub fn negate(self) -> TrinaryLogic {
        match self {
            TrinaryLogic::Yes => TrinaryLogic::No,
            TrinaryLogic::Maybe => TrinaryLogic::Maybe,
            TrinaryLogic::No => TrinaryLogic::Yes,
        }
    }

    /// `TrinaryLogic::extremeIdentity`: every operand agreeing decides,
    /// anything mixed is `Maybe` (none at all is `Yes`) - a union on the
    /// sub-type side of `isSuperTypeOf`.
    pub fn extreme_identity(results: impl Iterator<Item = TrinaryLogic>) -> TrinaryLogic {
        let mut seen_yes = false;
        let mut seen_no = false;
        for r in results {
            match r {
                TrinaryLogic::Yes => seen_yes = true,
                TrinaryLogic::No => seen_no = true,
                TrinaryLogic::Maybe => return TrinaryLogic::Maybe,
            }
            // Yes and No together are `Maybe` whatever else follows.
            if seen_yes && seen_no {
                return TrinaryLogic::Maybe;
            }
        }
        if seen_no {
            TrinaryLogic::No
        } else {
            TrinaryLogic::Yes
        }
    }

    /// `TrinaryLogic::lazyMaxMin`: any operand being `Yes` decides, all
    /// `No` (or none at all) is `No` - a union on the super-type side.
    pub fn max_min(results: impl Iterator<Item = TrinaryLogic>) -> TrinaryLogic {
        let mut best = TrinaryLogic::No;
        for r in results {
            match r {
                TrinaryLogic::Yes => return TrinaryLogic::Yes,
                TrinaryLogic::Maybe => best = TrinaryLogic::Maybe,
                TrinaryLogic::No => {}
            }
        }
        best
    }
}
