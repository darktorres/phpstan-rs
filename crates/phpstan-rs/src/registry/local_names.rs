//! The names of the declarations a registry placed itself, by id local.

use syntax::name::Name;

/// Names by id local. The table starts at the lowest local set, so a
/// registry holding one declaration with a high local holds one slot.
#[derive(Debug, Clone, Default)]
pub(crate) struct LocalNames {
    base: u32,
    names: Vec<Option<Name>>,
}

impl LocalNames {
    pub(crate) fn get(&self, local: u32) -> Option<&Name> {
        let at = usize::try_from(local.checked_sub(self.base)?).ok()?;
        self.names.get(at)?.as_ref()
    }

    pub(crate) fn set(&mut self, local: u32, name: Name) {
        if self.names.is_empty() {
            self.base = local;
        } else if local < self.base {
            let gap = (self.base - local) as usize;
            self.names.splice(0..0, std::iter::repeat_n(None, gap));
            self.base = local;
        }
        let at = (local - self.base) as usize;
        if self.names.len() <= at {
            self.names.resize_with(at + 1, || None);
        }
        self.names[at] = Some(name);
    }

    /// One above the highest local the table has room for; zero when empty.
    pub(crate) fn len(&self) -> usize {
        if self.names.is_empty() {
            0
        } else {
            self.base as usize + self.names.len()
        }
    }

    pub(crate) fn clear(&mut self) {
        self.base = 0;
        self.names.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_high_local_takes_one_slot_and_a_lower_one_is_added_below() {
        let mut names = LocalNames::default();
        assert_eq!(names.len(), 0);
        names.set(5000, Name::from("A"));
        assert_eq!(names.names.len(), 1);
        assert_eq!(names.len(), 5001);
        names.set(4998, Name::from("B"));
        names.set(5002, Name::from("C"));
        assert_eq!(names.get(5000).map(Name::as_str), Some("A"));
        assert_eq!(names.get(4998).map(Name::as_str), Some("B"));
        assert_eq!(names.get(5002).map(Name::as_str), Some("C"));
        assert!(names.get(4999).is_none());
        assert!(names.get(7).is_none());
        assert!(names.get(6000).is_none());
        assert_eq!(names.len(), 5003);
        names.clear();
        assert_eq!(names.len(), 0);
        assert!(names.get(5000).is_none());
    }
}
