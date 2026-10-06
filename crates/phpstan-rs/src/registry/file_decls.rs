//! Per-file declaration storage. Arenas make indices and ranges specific to
//! their declaration kind, so member and parameter ranges cannot be confused.

use la_arena::{Arena, Idx, IdxRange, RawIdx};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;
use syntax::name::Name;

use super::ClassReflection;
use super::class_info::{ConstantReflection, PropertyReflection};
use super::function_sig::{FunctionReflection, ParameterReflection};

#[allow(dead_code)] // Used by the cached per-file arena representation in row 4.3.
fn encode_idx<T>(idx: Idx<T>) -> u32 {
    u32::from(idx.into_raw())
}

#[allow(dead_code)] // Used by the cached per-file arena representation in row 4.3.
fn decode_idx<T>(idx: u32) -> Idx<T> {
    Idx::from_raw(RawIdx::from(idx))
}

#[allow(dead_code)] // Used by the cached per-file arena representation in row 4.3.
fn encode_range<T>(range: &IdxRange<T>) -> (u32, u32) {
    (encode_idx(range.start()), encode_idx(range.end()))
}

#[allow(dead_code)] // Used by the cached per-file arena representation in row 4.3.
fn decode_range<T>(range: (u32, u32)) -> IdxRange<T> {
    IdxRange::new(decode_idx(range.0)..decode_idx(range.1))
}

#[allow(dead_code)] // Used by the cached per-file arena reader in row 4.3.
fn idx_is_valid<T>(idx: Idx<T>, arena_len: usize) -> bool {
    usize::try_from(encode_idx(idx)).is_ok_and(|index| index < arena_len)
}

#[allow(dead_code)] // Used by the cached per-file arena reader in row 4.3.
fn range_is_valid<T>(range: &IdxRange<T>, arena_len: usize) -> bool {
    let start = usize::try_from(encode_idx(range.start()));
    let end = usize::try_from(encode_idx(range.end()));
    matches!((start, end), (Ok(start), Ok(end)) if start <= end && end <= arena_len)
}

#[allow(dead_code)] // Consumed as row 4.3 moves readers to typed ranges.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FileMember {
    pub(crate) name: Name,
    pub(crate) declaration: FunctionReflection,
    parameters: IdxRange<ParameterReflection>,
}

impl FileMember {
    pub(crate) fn new(name: Name, declaration: FunctionReflection) -> Self {
        Self {
            name,
            declaration,
            parameters: decode_range((0, 0)),
        }
    }
}

#[allow(dead_code)] // Consumed as row 4.3 moves readers to typed ranges.
#[derive(Clone, Debug)]
pub(crate) struct ClassLayout {
    pub(crate) declaration: Idx<Arc<ClassReflection>>,
    pub(crate) members: IdxRange<FileMember>,
    pub(crate) properties: IdxRange<Arc<PropertyReflection>>,
    pub(crate) constants: IdxRange<Arc<ConstantReflection>>,
    pub(crate) names: IdxRange<Name>,
}

#[allow(dead_code)] // Consumed as row 4.3 moves readers to typed ranges.
#[derive(Clone, Debug)]
pub(crate) struct FunctionLayout {
    pub(crate) declaration: Idx<Arc<FunctionReflection>>,
    pub(crate) parameters: IdxRange<ParameterReflection>,
}

#[allow(dead_code)] // Populated and consumed as registry storage migrates in row 4.3.
pub(crate) struct FileDecls {
    pub(crate) classes: Arena<Arc<ClassReflection>>,
    pub(crate) functions: Arena<Arc<FunctionReflection>>,
    pub(crate) class_layouts: Arena<ClassLayout>,
    pub(crate) function_layouts: Arena<FunctionLayout>,
    pub(crate) members: Arena<FileMember>,
    pub(crate) parameters: Arena<ParameterReflection>,
    pub(crate) properties: Arena<Arc<PropertyReflection>>,
    pub(crate) constants: Arena<Arc<ConstantReflection>>,
    pub(crate) names: Arena<Name>,
}

impl FileDecls {
    #[allow(dead_code)] // Used by the per-file declaration builder during row 4.3.
    pub(crate) fn new() -> Self {
        Self {
            classes: Arena::new(),
            functions: Arena::new(),
            class_layouts: Arena::new(),
            function_layouts: Arena::new(),
            members: Arena::new(),
            parameters: Arena::new(),
            properties: Arena::new(),
            constants: Arena::new(),
            names: Arena::new(),
        }
    }

    #[allow(dead_code)] // Called by the per-file declaration builder during row 4.3.
    pub(crate) fn add_class(
        &mut self,
        declaration: Arc<ClassReflection>,
        members: impl IntoIterator<Item = FileMember>,
        properties: impl IntoIterator<Item = Arc<PropertyReflection>>,
        constants: impl IntoIterator<Item = Arc<ConstantReflection>>,
        names: impl IntoIterator<Item = Name>,
    ) -> Idx<ClassLayout> {
        let declaration = self.classes.alloc(declaration);
        let mut stored_members = Vec::new();
        for mut member in members {
            member.parameters = self
                .parameters
                .alloc_many(member.declaration.params().iter().cloned());
            stored_members.push(member);
        }
        let members = self.members.alloc_many(stored_members);
        let properties = self.properties.alloc_many(properties);
        let constants = self.constants.alloc_many(constants);
        let names = self.names.alloc_many(names);
        self.class_layouts.alloc(ClassLayout {
            declaration,
            members,
            properties,
            constants,
            names,
        })
    }

    #[allow(dead_code)] // Called by the per-file declaration builder during row 4.3.
    pub(crate) fn add_function(
        &mut self,
        declaration: Arc<FunctionReflection>,
        parameters: impl IntoIterator<Item = ParameterReflection>,
    ) -> Idx<FunctionLayout> {
        let declaration = self.functions.alloc(declaration);
        let parameters = self.parameters.alloc_many(parameters);
        self.function_layouts.alloc(FunctionLayout {
            declaration,
            parameters,
        })
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn class(&self, layout: Idx<ClassLayout>) -> &ClassReflection {
        self.classes[self.class_layouts[layout].declaration].as_ref()
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn members(&self, layout: Idx<ClassLayout>) -> &[FileMember] {
        &self.members[self.class_layouts[layout].members.clone()]
    }

    #[allow(dead_code)] // Used by registry accessors as method parameters move to FileDecls.
    pub(crate) fn member_parameters(&self, member: &FileMember) -> &[ParameterReflection] {
        &self.parameters[member.parameters.clone()]
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn properties(&self, layout: Idx<ClassLayout>) -> &[Arc<PropertyReflection>] {
        &self.properties[self.class_layouts[layout].properties.clone()]
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn constants(&self, layout: Idx<ClassLayout>) -> &[Arc<ConstantReflection>] {
        &self.constants[self.class_layouts[layout].constants.clone()]
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn names(&self, layout: Idx<ClassLayout>) -> &[Name] {
        &self.names[self.class_layouts[layout].names.clone()]
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn function(&self, layout: Idx<FunctionLayout>) -> &FunctionReflection {
        self.functions[self.function_layouts[layout].declaration].as_ref()
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn parameters(&self, layout: Idx<FunctionLayout>) -> &[ParameterReflection] {
        &self.parameters[self.function_layouts[layout].parameters.clone()]
    }

    #[allow(dead_code)] // Used by the cached per-file arena reader in row 4.3.
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        for (_, layout) in self.class_layouts.iter() {
            if !idx_is_valid(layout.declaration, self.classes.len())
                || !range_is_valid(&layout.members, self.members.len())
                || !range_is_valid(&layout.properties, self.properties.len())
                || !range_is_valid(&layout.constants, self.constants.len())
                || !range_is_valid(&layout.names, self.names.len())
            {
                return Err("class layout refers outside its file declaration arenas");
            }
        }
        for (_, member) in self.members.iter() {
            if !range_is_valid(&member.parameters, self.parameters.len()) {
                return Err("class member refers outside its parameter arena");
            }
        }
        for (_, layout) in self.function_layouts.iter() {
            if !idx_is_valid(layout.declaration, self.functions.len())
                || !range_is_valid(&layout.parameters, self.parameters.len())
            {
                return Err("function layout refers outside its file declaration arenas");
            }
        }
        Ok(())
    }
}

#[allow(dead_code)] // Used once ParsedFile stores its declaration arenas.
#[derive(Serialize)]
struct EncodedFileDecls<'a> {
    classes: Vec<&'a Arc<ClassReflection>>,
    functions: Vec<&'a Arc<FunctionReflection>>,
    class_layouts: Vec<EncodedClassLayout>,
    function_layouts: Vec<EncodedFunctionLayout>,
    members: Vec<EncodedFileMember<'a>>,
    parameters: Vec<&'a ParameterReflection>,
    properties: Vec<&'a Arc<PropertyReflection>>,
    constants: Vec<&'a Arc<ConstantReflection>>,
    names: Vec<&'a Name>,
}

#[derive(Serialize)]
struct EncodedFileMember<'a> {
    name: &'a Name,
    declaration: &'a FunctionReflection,
    parameters: (u32, u32),
}

#[derive(Deserialize)]
struct DecodedFileMember {
    name: Name,
    declaration: FunctionReflection,
    parameters: (u32, u32),
}

#[allow(dead_code)] // Used once ParsedFile stores its declaration arenas.
#[derive(Serialize, Deserialize)]
struct EncodedClassLayout {
    declaration: u32,
    members: (u32, u32),
    properties: (u32, u32),
    constants: (u32, u32),
    names: (u32, u32),
}

#[allow(dead_code)] // Used once ParsedFile stores its declaration arenas.
#[derive(Serialize, Deserialize)]
struct EncodedFunctionLayout {
    declaration: u32,
    parameters: (u32, u32),
}

impl Serialize for FileDecls {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = EncodedFileDecls {
            classes: self.classes.iter().map(|(_, value)| value).collect(),
            functions: self.functions.iter().map(|(_, value)| value).collect(),
            class_layouts: self
                .class_layouts
                .iter()
                .map(|(_, layout)| EncodedClassLayout {
                    declaration: encode_idx(layout.declaration),
                    members: encode_range(&layout.members),
                    properties: encode_range(&layout.properties),
                    constants: encode_range(&layout.constants),
                    names: encode_range(&layout.names),
                })
                .collect(),
            function_layouts: self
                .function_layouts
                .iter()
                .map(|(_, layout)| EncodedFunctionLayout {
                    declaration: encode_idx(layout.declaration),
                    parameters: encode_range(&layout.parameters),
                })
                .collect(),
            members: self
                .members
                .iter()
                .map(|(_, member)| EncodedFileMember {
                    name: &member.name,
                    declaration: &member.declaration,
                    parameters: encode_range(&member.parameters),
                })
                .collect(),
            parameters: self.parameters.iter().map(|(_, value)| value).collect(),
            properties: self.properties.iter().map(|(_, value)| value).collect(),
            constants: self.constants.iter().map(|(_, value)| value).collect(),
            names: self.names.iter().map(|(_, value)| value).collect(),
        };
        encoded.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FileDecls {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct EncodedFileDecls {
            classes: Vec<Arc<ClassReflection>>,
            functions: Vec<Arc<FunctionReflection>>,
            class_layouts: Vec<EncodedClassLayout>,
            function_layouts: Vec<EncodedFunctionLayout>,
            members: Vec<DecodedFileMember>,
            parameters: Vec<ParameterReflection>,
            properties: Vec<Arc<PropertyReflection>>,
            constants: Vec<Arc<ConstantReflection>>,
            names: Vec<Name>,
        }

        let encoded = EncodedFileDecls::deserialize(deserializer)?;
        let declarations = FileDecls {
            classes: Arena::from_iter(encoded.classes),
            functions: Arena::from_iter(encoded.functions),
            class_layouts: Arena::from_iter(encoded.class_layouts.into_iter().map(|layout| {
                ClassLayout {
                    declaration: decode_idx(layout.declaration),
                    members: decode_range(layout.members),
                    properties: decode_range(layout.properties),
                    constants: decode_range(layout.constants),
                    names: decode_range(layout.names),
                }
            })),
            function_layouts: Arena::from_iter(encoded.function_layouts.into_iter().map(
                |layout| FunctionLayout {
                    declaration: decode_idx(layout.declaration),
                    parameters: decode_range(layout.parameters),
                },
            )),
            members: Arena::from_iter(encoded.members.into_iter().map(|member| FileMember {
                name: member.name,
                declaration: member.declaration,
                parameters: decode_range(member.parameters),
            })),
            parameters: Arena::from_iter(encoded.parameters),
            properties: Arena::from_iter(encoded.properties),
            constants: Arena::from_iter(encoded.constants),
            names: Arena::from_iter(encoded.names),
        };
        declarations.validate().map_err(D::Error::custom)?;
        Ok(declarations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::lexer::Lexer;
    use syntax::parser::Parser;

    #[test]
    fn file_declarations_round_trip_with_typed_ranges() {
        let tokens = Lexer::new(
            "<?php class Demo { public int $value; public const int MARK = 1; public function run(int $n): int {} } function helper(string $text): bool {}",
        )
        .tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        let registry = crate::registry::Registry::build(&program);
        let class = registry.lookup_class("Demo").unwrap().1.clone();
        let function = registry.lookup_function("helper").unwrap().clone();

        let members = class
            .methods
            .iter()
            .map(|(name, declaration)| FileMember::new(name.clone(), declaration.clone()))
            .collect::<Vec<_>>();
        let properties = class
            .properties
            .iter()
            .map(|(_, property)| Arc::clone(property))
            .collect::<Vec<_>>();
        let constants = class
            .consts
            .iter()
            .map(|(_, constant)| Arc::clone(constant))
            .collect::<Vec<_>>();
        let names = class
            .extends
            .iter()
            .chain(&class.implements)
            .chain(&class.uses)
            .cloned()
            .collect::<Vec<_>>();

        let mut declarations = FileDecls::new();
        let class_layout = declarations.add_class(
            Arc::new(class.clone()),
            members.clone(),
            properties.clone(),
            constants.clone(),
            names.clone(),
        );
        let function_layout =
            declarations.add_function(Arc::new(function.clone()), function.params.clone());
        let encoded = postcard::to_allocvec(&declarations).unwrap();
        let decoded: FileDecls = postcard::from_bytes(&encoded).unwrap();

        assert_eq!(decoded.class(class_layout), &class);
        let decoded_members = decoded.members(class_layout);
        assert_eq!(decoded_members.len(), members.len());
        assert_eq!(decoded_members[0].name, members[0].name);
        assert_eq!(decoded_members[0].declaration, members[0].declaration);
        assert_eq!(
            decoded.member_parameters(&decoded_members[0]),
            members[0].declaration.params()
        );
        assert_eq!(decoded.properties(class_layout), properties);
        assert_eq!(decoded.constants(class_layout), constants);
        assert_eq!(decoded.names(class_layout), names);
        assert_eq!(decoded.function(function_layout), &function);
        assert_eq!(decoded.parameters(function_layout), function.params);
    }
}
