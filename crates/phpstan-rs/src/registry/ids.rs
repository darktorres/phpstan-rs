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
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub(crate) struct $name(u32);

        impl $name {
            pub(crate) fn stub(local: u32) -> Self {
                Self::new(IdSource::Stub, local)
            }

            #[allow(dead_code)] // The context base is added in ledger row 17.5.
            pub(crate) fn context(local: u32) -> Self {
                Self::new(IdSource::Context, local)
            }

            pub(crate) fn project(local: u32) -> Self {
                Self::new(IdSource::Project, local)
            }

            fn new(source: IdSource, local: u32) -> Self {
                assert!(local < LOCAL_LIMIT, "declaration id range exhausted");
                let range = match source {
                    IdSource::Stub => 0,
                    IdSource::Context => 1,
                    IdSource::Project => 2,
                };
                Self((range << LOCAL_BITS) | local)
            }

            pub(crate) fn source(self) -> Option<IdSource> {
                match self.0 >> LOCAL_BITS {
                    0 => Some(IdSource::Stub),
                    1 => Some(IdSource::Context),
                    2 => Some(IdSource::Project),
                    _ => None,
                }
            }

            pub(crate) fn local(self) -> u32 {
                self.0 & LOCAL_MASK
            }
        }
    };
}

declaration_id!(ClassId);
declaration_id!(FnId);

#[cfg(test)]
mod tests {
    use super::{ClassId, FnId, IdSource, LOCAL_LIMIT};

    #[test]
    fn each_source_has_a_distinct_range_for_both_id_kinds() {
        for (source, class, function) in [
            (IdSource::Stub, ClassId::stub(17), FnId::stub(23)),
            (IdSource::Context, ClassId::context(17), FnId::context(23)),
            (IdSource::Project, ClassId::project(17), FnId::project(23)),
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
        let project = ClassId::project(0);

        assert_eq!(stub.source(), Some(IdSource::Stub));
        assert_eq!(context.source(), Some(IdSource::Context));
        assert_eq!(project.source(), Some(IdSource::Project));
        assert!(stub.0 < context.0);
        assert!(context.0 < project.0);
    }
}
