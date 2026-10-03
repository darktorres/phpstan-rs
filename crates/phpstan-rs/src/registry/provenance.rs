//! Where a declaration comes from: the file that declares it and the span
//! of its name there, recorded for every class, function and member as it
//! is indexed.

use std::path::Path;
use std::sync::Arc;
use syntax::name::Name;

use syntax::span::Span;

use crate::registry::Registry;

/// The source a declaration was indexed from.
// The equality below is the derived one with a shortcut, so it agrees with
// the derived hash.
#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Debug, Clone, Default, Eq, Hash)]
pub enum Origin {
    /// Not recorded: a registry built from bare source.
    #[default]
    Unknown,
    /// A project file, analysed or loaded for context.
    File(Arc<Path>),
    /// A phpstorm-stubs file (its path below the stubs root): a PHP
    /// built-in, of the extension named by its directory.
    Builtin {
        file: &'static str,
        extension: &'static str,
    },
    /// PHPStan's own source, embedded (its path below `src/`).
    Phpstan { file: &'static str },
}

/// Two embedded paths are the same text; they are usually the same static
/// too, which is cheaper to see than comparing the text.
fn same_static(a: &'static str, b: &'static str) -> bool {
    (std::ptr::eq(a.as_ptr(), b.as_ptr()) && a.len() == b.len()) || a == b
}

impl PartialEq for Origin {
    fn eq(&self, other: &Origin) -> bool {
        match (self, other) {
            (Origin::Unknown, Origin::Unknown) => true,
            (Origin::File(a), Origin::File(b)) => a == b,
            (
                Origin::Builtin { file, extension },
                Origin::Builtin {
                    file: other_file,
                    extension: other_extension,
                },
            ) => same_static(file, other_file) && same_static(extension, other_extension),
            (Origin::Phpstan { file }, Origin::Phpstan { file: other }) => same_static(file, other),
            _ => false,
        }
    }
}

/// `Origin` as serialized: the embedded stub paths as text, found in the
/// embedded tables again on the way back.
#[derive(serde::Serialize, serde::Deserialize)]
enum OriginData {
    Unknown,
    File(Arc<Path>),
    /// The index of the file in `PHPSTORM_STUBS_PATHS`, whose extension is
    /// the one at the same index.
    Builtin(u32),
    /// The index of the file in `PHPSTAN_SRC_PATHS`.
    Phpstan(u32),
}

impl serde::Serialize for Origin {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        OriginData::try_from(self)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Origin {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Origin, D::Error> {
        Origin::try_from(OriginData::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Where each embedded stub file's path sits in its table.
fn embedded_index(paths: &'static [&'static str]) -> rustc_hash::FxHashMap<&'static str, u32> {
    paths
        .iter()
        .enumerate()
        .map(|(i, path)| (*path, i as u32))
        .collect()
}

impl TryFrom<&Origin> for OriginData {
    type Error = String;

    fn try_from(origin: &Origin) -> Result<OriginData, String> {
        static BUILTIN: std::sync::LazyLock<rustc_hash::FxHashMap<&'static str, u32>> =
            std::sync::LazyLock::new(|| embedded_index(stub_sources::PHPSTORM_STUBS_PATHS));
        static PHPSTAN: std::sync::LazyLock<rustc_hash::FxHashMap<&'static str, u32>> =
            std::sync::LazyLock::new(|| embedded_index(stub_sources::PHPSTAN_SRC_PATHS));
        Ok(match origin {
            Origin::Unknown => OriginData::Unknown,
            Origin::File(path) => OriginData::File(Arc::clone(path)),
            Origin::Builtin { file, extension } => {
                let index = *BUILTIN
                    .get(file)
                    .ok_or_else(|| format!("{file} is not an embedded stub"))?;
                if stub_sources::PHPSTORM_STUBS_EXTENSIONS[index as usize] != *extension {
                    return Err(format!("{extension} is not the extension of {file}"));
                }
                OriginData::Builtin(index)
            }
            Origin::Phpstan { file } => OriginData::Phpstan(
                *PHPSTAN
                    .get(file)
                    .ok_or_else(|| format!("{file} is not an embedded stub"))?,
            ),
        })
    }
}

impl TryFrom<OriginData> for Origin {
    type Error = String;

    fn try_from(data: OriginData) -> Result<Origin, String> {
        Ok(match data {
            OriginData::Unknown => Origin::Unknown,
            OriginData::File(path) => Origin::File(path),
            OriginData::Builtin(index) => Origin::Builtin {
                file: stub_sources::PHPSTORM_STUBS_PATHS
                    .get(index as usize)
                    .copied()
                    .ok_or_else(|| format!("{index} is not an embedded stub"))?,
                extension: stub_sources::PHPSTORM_STUBS_EXTENSIONS[index as usize],
            },
            OriginData::Phpstan(index) => Origin::Phpstan {
                file: stub_sources::PHPSTAN_SRC_PATHS
                    .get(index as usize)
                    .copied()
                    .ok_or_else(|| format!("{index} is not an embedded stub"))?,
            },
        })
    }
}

/// An embedded stub file's extension, as the embedded table holds it.
pub(crate) fn embedded_str(text: &str) -> Result<&'static str, String> {
    static EMBEDDED: std::sync::LazyLock<rustc_hash::FxHashSet<&'static str>> =
        std::sync::LazyLock::new(|| {
            stub_sources::PHPSTORM_STUBS_PATHS
                .iter()
                .chain(stub_sources::PHPSTORM_STUBS_EXTENSIONS)
                .chain(stub_sources::PHPSTAN_SRC_PATHS)
                .copied()
                .collect()
        });
    EMBEDDED
        .get(text)
        .copied()
        .ok_or_else(|| format!("{text} is not an embedded stub"))
}

/// A declaration's source, the span of its name there, and for a member,
/// the class-like that declares it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    origin: Origin,
    span: Span,
    declaring_class: Option<Name>,
}

impl Provenance {
    pub fn new(origin: Origin, span: Span) -> Provenance {
        Provenance {
            origin,
            span,
            declaring_class: None,
        }
    }

    /// A member's, declared by `class`.
    pub fn member(origin: Origin, span: Span, class: Name) -> Provenance {
        Provenance {
            origin,
            span,
            declaring_class: Some(class),
        }
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The project file declaring it; `None` for a built-in, PHPStan's
    /// own source, or an unrecorded origin.
    pub fn file(&self) -> Option<&Path> {
        match &self.origin {
            Origin::File(path) => Some(path),
            _ => None,
        }
    }

    /// Whether PHP itself provides it.
    pub fn is_builtin(&self) -> bool {
        matches!(self.origin, Origin::Builtin { .. })
    }

    /// The PHP extension providing a built-in.
    pub fn extension_name(&self) -> Option<&'static str> {
        match self.origin {
            Origin::Builtin { extension, .. } => Some(extension),
            _ => None,
        }
    }

    /// The span of the declaration's name in its file.
    pub fn declaration_span(&self) -> Span {
        self.span
    }

    /// For a member, the class-like declaring it (a trait for one a class
    /// takes from it), as its declaration spells it.
    pub fn declaring_class(&self) -> Option<&str> {
        self.declaring_class.as_deref()
    }

    /// The project file declaring it, and its name's span there.
    fn in_file(&self) -> Option<(&Path, Span)> {
        self.file().map(|file| (file, self.span))
    }
}

impl Registry {
    /// Where a project file declares class-like `fqn`.
    pub fn class_declaration(&self, fqn: &str) -> Option<(&Path, Span)> {
        self.classes.get(fqn)?.provenance.in_file()
    }

    /// Where a project file declares function `fqn`.
    pub fn function_declaration(&self, fqn: &str) -> Option<(&Path, Span)> {
        self.functions.get(fqn)?.provenance.in_file()
    }

    /// Where a project file declares `owner`'s method `name`.
    pub fn method_declaration(&self, owner: &str, name: &str) -> Option<(&Path, Span)> {
        self.classes
            .get(owner)?
            .methods
            .get(name)?
            .provenance
            .in_file()
    }

    /// Where a project file declares `owner`'s property `name`.
    pub fn property_declaration(&self, owner: &str, name: &str) -> Option<(&Path, Span)> {
        self.classes
            .get(owner)?
            .properties
            .get(name)?
            .provenance
            .in_file()
    }

    /// Where a project file declares `owner`'s constant or enum case `name`.
    pub fn constant_declaration(&self, owner: &str, name: &str) -> Option<(&Path, Span)> {
        self.classes
            .get(owner)?
            .consts
            .get(name)?
            .provenance
            .in_file()
    }

    /// Where a project file declares `owner`'s member `name` of unknown
    /// kind: a constant or enum case, else a property, else a method.
    pub fn member_declaration(&self, owner: &str, name: &str) -> Option<(&Path, Span)> {
        self.constant_declaration(owner, name)
            .or_else(|| self.property_declaration(owner, name))
            .or_else(|| self.method_declaration(owner, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::stub_cache::{decode, encode};

    #[test]
    fn the_embedded_paths_are_unique_and_pair_with_their_extensions() {
        let unique = |paths: &[&str]| {
            paths.iter().collect::<rustc_hash::FxHashSet<_>>().len() == paths.len()
        };
        assert!(unique(stub_sources::PHPSTORM_STUBS_PATHS));
        assert!(unique(stub_sources::PHPSTAN_SRC_PATHS));
        assert_eq!(
            stub_sources::PHPSTORM_STUBS_PATHS.len(),
            stub_sources::PHPSTORM_STUBS_EXTENSIONS.len()
        );
    }

    #[test]
    fn an_embedded_origin_survives_the_codec_by_its_index() {
        let paths = stub_sources::PHPSTORM_STUBS_PATHS;
        for i in [0, paths.len() / 2, paths.len() - 1] {
            let origin = Origin::Builtin {
                file: paths[i],
                extension: stub_sources::PHPSTORM_STUBS_EXTENSIONS[i],
            };
            assert_eq!(decode::<Origin>(&encode(&origin).unwrap()).unwrap(), origin);
        }
        let paths = stub_sources::PHPSTAN_SRC_PATHS;
        for i in [0, paths.len() - 1] {
            let origin = Origin::Phpstan { file: paths[i] };
            assert_eq!(decode::<Origin>(&encode(&origin).unwrap()).unwrap(), origin);
        }
        assert_eq!(
            decode::<Origin>(&encode(&Origin::Unknown).unwrap()).unwrap(),
            Origin::Unknown
        );
    }

    #[test]
    fn an_origin_that_is_not_embedded_does_not_encode_and_a_stray_index_does_not_decode() {
        let stray = Origin::Builtin {
            file: "not/a/stub.php",
            extension: "none",
        };
        assert!(encode(&stray).is_err());
        let wrong_extension = Origin::Builtin {
            file: stub_sources::PHPSTORM_STUBS_PATHS[0],
            extension: "no-such-extension",
        };
        assert!(encode(&wrong_extension).is_err());
        let past_the_end = postcard::to_allocvec(&OriginData::Builtin(u32::MAX)).unwrap();
        assert!(decode::<Origin>(&past_the_end).is_err());
    }
}
