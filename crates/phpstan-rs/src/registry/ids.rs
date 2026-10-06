//! Declaration identifiers split into fixed ranges by declaration source.

const LOCAL_BITS: u32 = 30;
const LOCAL_LIMIT: u32 = 1 << LOCAL_BITS;
const LOCAL_MASK: u32 = LOCAL_LIMIT - 1;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum IdSource {
    Stub,
    Context,
    Project,
}

macro_rules! declaration_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name {
            raw: u32,
            generation: u32,
        }

        impl $name {
            pub(crate) fn stub(local: u32) -> Self {
                Self::new(IdSource::Stub, local, 0)
            }

            #[allow(dead_code)] // The context base is added in ledger row 17.5.
            pub(crate) fn context(local: u32) -> Self {
                Self::new(IdSource::Context, local, 0)
            }

            pub(crate) fn project_in_generation(local: u32, generation: u32) -> Self {
                Self::new(IdSource::Project, local, generation)
            }

            fn new(source: IdSource, local: u32, generation: u32) -> Self {
                assert!(local < LOCAL_LIMIT, "declaration id range exhausted");
                let range = match source {
                    IdSource::Stub => 0,
                    IdSource::Context => 1,
                    IdSource::Project => 2,
                };
                Self {
                    raw: (range << LOCAL_BITS) | local,
                    generation,
                }
            }

            pub(crate) fn source(self) -> Option<IdSource> {
                match self.raw >> LOCAL_BITS {
                    0 => Some(IdSource::Stub),
                    1 => Some(IdSource::Context),
                    2 => Some(IdSource::Project),
                    _ => None,
                }
            }

            pub(crate) fn local(self) -> u32 {
                self.raw & LOCAL_MASK
            }

            pub(crate) fn generation(self) -> u32 {
                self.generation
            }

            pub(crate) fn set_generation(&mut self, generation: u32) {
                self.generation = generation;
            }

            pub(crate) fn with_generation(mut self, generation: u32) -> Self {
                self.set_generation(generation);
                self
            }
        }
    };
}

declaration_id!(ClassId);
declaration_id!(FnId);

/// A method's slot in one class's declaration-ordered member list.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct MethodRef {
    class: ClassId,
    slot: u16,
}

impl MethodRef {
    pub(crate) fn new(class: ClassId, slot: u16) -> Self {
        Self { class, slot }
    }

    pub(crate) fn class(self) -> ClassId {
        self.class
    }

    pub(crate) fn slot(self) -> u16 {
        self.slot
    }
}

/// A property's stable slot in one class's declaration-ordered member list.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct PropertyRef {
    class: ClassId,
    slot: u32,
}

impl PropertyRef {
    pub(crate) fn new(class: ClassId, slot: u32) -> Self {
        Self { class, slot }
    }

    pub(crate) fn class(self) -> ClassId {
        self.class
    }

    pub(crate) fn slot(self) -> u32 {
        self.slot
    }

    pub(crate) fn with_generation(mut self, generation: u32) -> Self {
        self.class = self.class.with_generation(generation);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ClassId, FnId, IdSource, LOCAL_LIMIT, PropertyRef};

    #[test]
    fn each_source_has_a_distinct_range_for_both_id_kinds() {
        for (source, class, function) in [
            (IdSource::Stub, ClassId::stub(17), FnId::stub(23)),
            (IdSource::Context, ClassId::context(17), FnId::context(23)),
            (
                IdSource::Project,
                ClassId::project_in_generation(17, 0),
                FnId::project_in_generation(23, 0),
            ),
        ] {
            assert_eq!(class.source(), Some(source));
            assert_eq!(class.local(), 17);
            assert_eq!(function.source(), Some(source));
            assert_eq!(function.local(), 23);
        }
    }

    #[test]
    fn each_source_range_ends_before_the_next_one_starts() {
        let stub = ClassId::stub(LOCAL_LIMIT - 1);
        let context = ClassId::context(0);
        let project = ClassId::project_in_generation(0, 0);

        assert_eq!(stub.source(), Some(IdSource::Stub));
        assert_eq!(context.source(), Some(IdSource::Context));
        assert_eq!(project.source(), Some(IdSource::Project));
        assert!(stub.raw < context.raw);
        assert!(context.raw < project.raw);
    }

    #[test]
    fn a_new_generation_makes_old_ids_distinct() {
        assert_ne!(
            ClassId::project_in_generation(7, 0),
            ClassId::project_in_generation(7, 1)
        );
        assert_ne!(
            FnId::project_in_generation(3, 0),
            FnId::project_in_generation(3, 1)
        );
    }

    #[test]
    fn property_ref_preserves_its_class_and_slot() {
        let class = ClassId::project_in_generation(41, 3);
        let property = PropertyRef::new(class, u32::MAX);

        assert_eq!(property.class(), class);
        assert_eq!(property.slot(), u32::MAX);
    }

    #[test]
    fn retagging_a_property_ref_changes_only_its_class_generation() {
        let original = PropertyRef::new(ClassId::project_in_generation(41, 3), 7);
        let retagged = original.with_generation(9);

        assert_eq!(retagged.class().source(), Some(IdSource::Project));
        assert_eq!(retagged.class().local(), 41);
        assert_eq!(retagged.class().generation(), 9);
        assert_eq!(retagged.slot(), 7);
        assert_ne!(retagged, original);
    }
}
