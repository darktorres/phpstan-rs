//! Per-file declaration storage. Arenas make indices and ranges specific to
//! their declaration kind, so member and parameter ranges cannot be confused.

use la_arena::{Arena, Idx, IdxRange, RawIdx};
use rustc_hash::FxHashMap;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;
use syntax::name::Name;

use super::ClassReflection;
use super::class_info::{ConstantReflection, PropertyReflection};
use super::function_sig::{FunctionReflection, ParameterReflection};
use super::name_table::Lazy;

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
    pub(crate) declaration: Arc<Lazy<FunctionReflection>>,
    parameters: IdxRange<ParameterReflection>,
}

impl FileMember {
    #[cfg(test)]
    pub(crate) fn new(name: Name, declaration: FunctionReflection) -> Self {
        Self::from_shared(name, Arc::new(Lazy::decoded(declaration)))
    }

    pub(crate) fn from_shared(name: Name, declaration: Arc<Lazy<FunctionReflection>>) -> Self {
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
    pub(crate) properties: IdxRange<FileProperty>,
    pub(crate) constants: IdxRange<FileConstant>,
    pub(crate) names: IdxRange<Name>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FileProperty {
    pub(crate) name: Name,
    pub(crate) declaration: Arc<PropertyReflection>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FileConstant {
    pub(crate) name: Name,
    pub(crate) declaration: Arc<ConstantReflection>,
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
    class_by_ptr: FxHashMap<usize, Idx<ClassLayout>>,
    function_by_ptr: FxHashMap<usize, Idx<FunctionLayout>>,
    pub(crate) members: Arena<FileMember>,
    /// Case-folded member name to the file-local member slot. The class
    /// layout index is part of the key because each class has its own
    /// member namespace while all members share one typed arena.
    member_slots: FxHashMap<(u32, String), Idx<FileMember>>,
    property_slots: FxHashMap<(u32, String), Idx<FileProperty>>,
    constant_slots: FxHashMap<(u32, String), Idx<FileConstant>>,
    pub(crate) parameters: Arena<ParameterReflection>,
    pub(crate) properties: Arena<FileProperty>,
    pub(crate) constants: Arena<FileConstant>,
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
            class_by_ptr: FxHashMap::default(),
            function_by_ptr: FxHashMap::default(),
            members: Arena::new(),
            member_slots: FxHashMap::default(),
            property_slots: FxHashMap::default(),
            constant_slots: FxHashMap::default(),
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
        properties: impl IntoIterator<Item = (Name, Arc<PropertyReflection>)>,
        constants: impl IntoIterator<Item = (Name, Arc<ConstantReflection>)>,
        names: impl IntoIterator<Item = Name>,
    ) -> Idx<ClassLayout> {
        let declaration = self.classes.alloc(declaration);
        let mut stored_members = Vec::new();
        for mut member in members {
            member.parameters = self
                .parameters
                .alloc_many(member.declaration.get().params().iter().cloned());
            stored_members.push(member);
        }
        let members = self.members.alloc_many(stored_members);
        let properties = self.properties.alloc_many(
            properties
                .into_iter()
                .map(|(name, declaration)| FileProperty { name, declaration }),
        );
        let constants = self.constants.alloc_many(
            constants
                .into_iter()
                .map(|(name, declaration)| FileConstant { name, declaration }),
        );
        let names = self.names.alloc_many(names);
        let layout = self.class_layouts.alloc(ClassLayout {
            declaration,
            members,
            properties,
            constants,
            names,
        });
        let declaration_ptr = Arc::as_ptr(&self.classes[declaration]) as usize;
        assert!(self.class_by_ptr.insert(declaration_ptr, layout).is_none());
        self.index_members(layout);
        self.index_properties(layout);
        self.index_constants(layout);
        layout
    }

    #[allow(dead_code)] // Called by the per-file declaration builder during row 4.3.
    pub(crate) fn add_function(
        &mut self,
        declaration: Arc<FunctionReflection>,
        parameters: impl IntoIterator<Item = ParameterReflection>,
    ) -> Idx<FunctionLayout> {
        let declaration = self.functions.alloc(declaration);
        let parameters = self.parameters.alloc_many(parameters);
        let layout = self.function_layouts.alloc(FunctionLayout {
            declaration,
            parameters,
        });
        let declaration_ptr = Arc::as_ptr(&self.functions[declaration]) as usize;
        assert!(
            self.function_by_ptr
                .insert(declaration_ptr, layout)
                .is_none()
        );
        layout
    }

    pub(crate) fn class_layout_for(
        &self,
        declaration: &Arc<ClassReflection>,
    ) -> Option<Idx<ClassLayout>> {
        self.class_by_ptr
            .get(&(Arc::as_ptr(declaration) as usize))
            .copied()
    }

    pub(crate) fn function_layout_for(
        &self,
        declaration: &Arc<FunctionReflection>,
    ) -> Option<Idx<FunctionLayout>> {
        self.function_by_ptr
            .get(&(Arc::as_ptr(declaration) as usize))
            .copied()
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn class(&self, layout: Idx<ClassLayout>) -> &ClassReflection {
        self.classes[self.class_layouts[layout].declaration].as_ref()
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn members(&self, layout: Idx<ClassLayout>) -> &[FileMember] {
        &self.members[self.class_layouts[layout].members.clone()]
    }

    pub(crate) fn property(
        &self,
        layout: Idx<ClassLayout>,
        name: &str,
    ) -> Option<&PropertyReflection> {
        let slot = self
            .property_slots
            .get(&(encode_idx(layout), name.to_owned()))?;
        Some(self.properties[*slot].declaration.as_ref())
    }

    pub(crate) fn constant(
        &self,
        layout: Idx<ClassLayout>,
        name: &str,
    ) -> Option<&ConstantReflection> {
        let slot = self
            .constant_slots
            .get(&(encode_idx(layout), name.to_owned()))?;
        Some(self.constants[*slot].declaration.as_ref())
    }

    /// A member's file-local slot by PHP's case-insensitive method-name
    /// rules.
    #[allow(dead_code)] // Used as Registry member lookups migrate to FileDecls.
    pub(crate) fn member(&self, layout: Idx<ClassLayout>, name: &str) -> Option<&FileMember> {
        let slot = self
            .member_slots
            .get(&(encode_idx(layout), name.to_ascii_lowercase()))?;
        Some(&self.members[*slot])
    }

    fn index_members(&mut self, layout: Idx<ClassLayout>) {
        let class = encode_idx(layout);
        let (start, end) = encode_range(&self.class_layouts[layout].members);
        for raw in start..end {
            let slot = decode_idx::<FileMember>(raw);
            let name = self.members[slot].name.lower().to_owned();
            assert!(
                self.member_slots.insert((class, name), slot).is_none(),
                "duplicate folded method in class layout"
            );
        }
    }

    fn index_properties(&mut self, layout: Idx<ClassLayout>) {
        let class = encode_idx(layout);
        let (start, end) = encode_range(&self.class_layouts[layout].properties);
        for raw in start..end {
            let slot = decode_idx::<FileProperty>(raw);
            let name = self.properties[slot].name.as_str().to_owned();
            assert!(
                self.property_slots.insert((class, name), slot).is_none(),
                "duplicate property name in class layout"
            );
        }
    }

    fn index_constants(&mut self, layout: Idx<ClassLayout>) {
        let class = encode_idx(layout);
        let (start, end) = encode_range(&self.class_layouts[layout].constants);
        for raw in start..end {
            let slot = decode_idx::<FileConstant>(raw);
            let name = self.constants[slot].name.as_str().to_owned();
            assert!(
                self.constant_slots.insert((class, name), slot).is_none(),
                "duplicate constant name in class layout"
            );
        }
    }

    #[allow(dead_code)] // Used by registry accessors as method parameters move to FileDecls.
    pub(crate) fn member_parameters(&self, member: &FileMember) -> &[ParameterReflection] {
        &self.parameters[member.parameters.clone()]
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn properties(&self, layout: Idx<ClassLayout>) -> &[FileProperty] {
        &self.properties[self.class_layouts[layout].properties.clone()]
    }

    #[allow(dead_code)] // Used by registry accessors as declarations move to FileDecls.
    pub(crate) fn constants(&self, layout: Idx<ClassLayout>) -> &[FileConstant] {
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
        for ((class, name), member) in &self.member_slots {
            let layout = decode_idx::<ClassLayout>(*class);
            if !idx_is_valid(layout, self.class_layouts.len()) {
                return Err("member index refers outside class layouts");
            }
            let layout = &self.class_layouts[layout];
            let (start, end) = encode_range(&layout.members);
            let member_index = encode_idx(*member);
            if !range_is_valid(&layout.members, self.members.len())
                || !(start..end).contains(&member_index)
                || self.members[*member].name.lower() != name
            {
                return Err("member index points outside its class member range");
            }
        }
        for ((class, name), property) in &self.property_slots {
            let layout = decode_idx::<ClassLayout>(*class);
            if !idx_is_valid(layout, self.class_layouts.len()) {
                return Err("property index refers outside class layouts");
            }
            let range = &self.class_layouts[layout].properties;
            let (start, end) = encode_range(range);
            let property_index = encode_idx(*property);
            if !range_is_valid(range, self.properties.len())
                || !(start..end).contains(&property_index)
                || self.properties[*property].name.as_str() != name
            {
                return Err("property index points outside its class property range");
            }
        }
        for ((class, name), constant) in &self.constant_slots {
            let layout = decode_idx::<ClassLayout>(*class);
            if !idx_is_valid(layout, self.class_layouts.len()) {
                return Err("constant index refers outside class layouts");
            }
            let range = &self.class_layouts[layout].constants;
            let (start, end) = encode_range(range);
            let constant_index = encode_idx(*constant);
            if !range_is_valid(range, self.constants.len())
                || !(start..end).contains(&constant_index)
                || self.constants[*constant].name.as_str() != name
            {
                return Err("constant index points outside its class constant range");
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
    properties: Vec<&'a Name>,
    constants: Vec<&'a Name>,
    names: Vec<&'a Name>,
}

#[derive(Serialize)]
struct EncodedFileMember<'a> {
    name: &'a Name,
    parameters: (u32, u32),
}

#[derive(Deserialize)]
struct DecodedFileMember {
    name: Name,
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
                    parameters: encode_range(&member.parameters),
                })
                .collect(),
            parameters: self.parameters.iter().map(|(_, value)| value).collect(),
            properties: self
                .properties
                .iter()
                .map(|(_, property)| &property.name)
                .collect(),
            constants: self
                .constants
                .iter()
                .map(|(_, constant)| &constant.name)
                .collect(),
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
            properties: Vec<Name>,
            constants: Vec<Name>,
            names: Vec<Name>,
        }

        let encoded = EncodedFileDecls::deserialize(deserializer)?;
        let classes = Arena::from_iter(encoded.classes);
        let class_layouts =
            Arena::from_iter(encoded.class_layouts.into_iter().map(|layout| ClassLayout {
                declaration: decode_idx(layout.declaration),
                members: decode_range(layout.members),
                properties: decode_range(layout.properties),
                constants: decode_range(layout.constants),
                names: decode_range(layout.names),
            }));
        let mut decoded_members: Vec<Option<FileMember>> = std::iter::repeat_with(|| None)
            .take(encoded.members.len())
            .collect();
        let mut decoded_properties: Vec<Option<FileProperty>> = std::iter::repeat_with(|| None)
            .take(encoded.properties.len())
            .collect();
        let mut decoded_constants: Vec<Option<FileConstant>> = std::iter::repeat_with(|| None)
            .take(encoded.constants.len())
            .collect();
        for (_, layout) in class_layouts.iter() {
            if !idx_is_valid(layout.declaration, classes.len())
                || !range_is_valid(&layout.members, encoded.members.len())
                || !range_is_valid(&layout.properties, encoded.properties.len())
                || !range_is_valid(&layout.constants, encoded.constants.len())
            {
                return Err(D::Error::custom("class declaration range is invalid"));
            }
            let class = &classes[layout.declaration];
            let (start, end) = encode_range(&layout.members);
            for index in start..end {
                let Some(member) = encoded.members.get(index as usize) else {
                    return Err(D::Error::custom("class member range is invalid"));
                };
                let Some(declaration) = class.methods.shared_get(&member.name) else {
                    return Err(D::Error::custom(
                        "class member is missing from its declaration",
                    ));
                };
                let Some(slot) = decoded_members.get_mut(index as usize) else {
                    return Err(D::Error::custom("class member range is invalid"));
                };
                if slot
                    .replace(FileMember {
                        name: member.name.clone(),
                        declaration,
                        parameters: decode_range(member.parameters),
                    })
                    .is_some()
                {
                    return Err(D::Error::custom("class member ranges overlap"));
                }
            }
            let (start, end) = encode_range(&layout.properties);
            for index in start..end {
                let Some(name) = encoded.properties.get(index as usize) else {
                    return Err(D::Error::custom("class property range is invalid"));
                };
                let Some(declaration) = class.properties.get(name) else {
                    return Err(D::Error::custom(
                        "class property is missing from its declaration",
                    ));
                };
                let Some(slot) = decoded_properties.get_mut(index as usize) else {
                    return Err(D::Error::custom("class property range is invalid"));
                };
                if slot
                    .replace(FileProperty {
                        name: name.clone(),
                        declaration: Arc::clone(declaration),
                    })
                    .is_some()
                {
                    return Err(D::Error::custom("class property ranges overlap"));
                }
            }
            let (start, end) = encode_range(&layout.constants);
            for index in start..end {
                let Some(name) = encoded.constants.get(index as usize) else {
                    return Err(D::Error::custom("class constant range is invalid"));
                };
                let Some(declaration) = class.consts.get(name) else {
                    return Err(D::Error::custom(
                        "class constant is missing from its declaration",
                    ));
                };
                let Some(slot) = decoded_constants.get_mut(index as usize) else {
                    return Err(D::Error::custom("class constant range is invalid"));
                };
                if slot
                    .replace(FileConstant {
                        name: name.clone(),
                        declaration: Arc::clone(declaration),
                    })
                    .is_some()
                {
                    return Err(D::Error::custom("class constant ranges overlap"));
                }
            }
        }
        let members = decoded_members
            .into_iter()
            .map(|member| member.ok_or_else(|| D::Error::custom("class member range is missing")))
            .collect::<Result<Vec<_>, _>>()?;
        let members = Arena::from_iter(members);
        let properties = decoded_properties
            .into_iter()
            .map(|property| {
                property.ok_or_else(|| D::Error::custom("class property range is missing"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let properties = Arena::from_iter(properties);
        let constants = decoded_constants
            .into_iter()
            .map(|constant| {
                constant.ok_or_else(|| D::Error::custom("class constant range is missing"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let constants = Arena::from_iter(constants);
        let mut class_by_ptr = FxHashMap::default();
        for (layout, class_layout) in class_layouts.iter() {
            let declaration = &classes[class_layout.declaration];
            if class_by_ptr
                .insert(Arc::as_ptr(declaration) as usize, layout)
                .is_some()
            {
                return Err(D::Error::custom("class declaration has multiple layouts"));
            }
        }
        let function_layouts = Arena::from_iter(encoded.function_layouts.into_iter().map(
            |layout| FunctionLayout {
                declaration: decode_idx(layout.declaration),
                parameters: decode_range(layout.parameters),
            },
        ));
        let mut function_by_ptr = FxHashMap::default();
        for (layout, function_layout) in function_layouts.iter() {
            if !idx_is_valid(function_layout.declaration, encoded.functions.len()) {
                return Err(D::Error::custom("function declaration index is invalid"));
            }
            let declaration = &encoded.functions[encode_idx(function_layout.declaration) as usize];
            if function_by_ptr
                .insert(Arc::as_ptr(declaration) as usize, layout)
                .is_some()
            {
                return Err(D::Error::custom(
                    "function declaration has multiple layouts",
                ));
            }
        }
        let mut member_slots = FxHashMap::default();
        for (layout, class_layout) in class_layouts.iter() {
            let (start, end) = encode_range(&class_layout.members);
            for raw in start..end {
                let slot = decode_idx::<FileMember>(raw);
                let name = members[slot].name.lower().to_owned();
                if member_slots
                    .insert((encode_idx(layout), name), slot)
                    .is_some()
                {
                    return Err(D::Error::custom("duplicate folded class member"));
                }
            }
        }
        let mut property_slots = FxHashMap::default();
        let mut constant_slots = FxHashMap::default();
        for (layout, class_layout) in class_layouts.iter() {
            for raw in
                encode_range(&class_layout.properties).0..encode_range(&class_layout.properties).1
            {
                let slot = decode_idx::<FileProperty>(raw);
                let name = properties[slot].name.as_str().to_owned();
                if property_slots
                    .insert((encode_idx(layout), name), slot)
                    .is_some()
                {
                    return Err(D::Error::custom("duplicate property in class layout"));
                }
            }
            for raw in
                encode_range(&class_layout.constants).0..encode_range(&class_layout.constants).1
            {
                let slot = decode_idx::<FileConstant>(raw);
                let name = constants[slot].name.as_str().to_owned();
                if constant_slots
                    .insert((encode_idx(layout), name), slot)
                    .is_some()
                {
                    return Err(D::Error::custom("duplicate constant in class layout"));
                }
            }
        }
        let declarations = FileDecls {
            classes,
            functions: Arena::from_iter(encoded.functions),
            class_layouts,
            function_layouts,
            class_by_ptr,
            function_by_ptr,
            members,
            member_slots,
            properties,
            constants,
            property_slots,
            constant_slots,
            parameters: Arena::from_iter(encoded.parameters),
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
            "<?php class Demo { public int $value; public const int MARK = 1; public function zebra(): void {} public function run(int $n): int {} public function alpha(): void {} } function helper(string $text): bool {}",
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
            .map(|(name, property)| (name.clone(), Arc::clone(property)))
            .collect::<Vec<_>>();
        let constants = class
            .consts
            .iter()
            .map(|(name, constant)| (name.clone(), Arc::clone(constant)))
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
        assert_eq!(
            decoded.class_layout_for(decoded.classes.iter().next().unwrap().1),
            Some(class_layout)
        );
        let decoded_members = decoded.members(class_layout);
        assert_eq!(decoded_members.len(), members.len());
        assert_eq!(decoded.member(class_layout, "RUN").unwrap().name, "run");
        assert_eq!(decoded_members[0].name, "zebra");
        assert_eq!(
            decoded_members[0].declaration.get(),
            members[0].declaration.get()
        );
        let shared = decoded
            .class(class_layout)
            .methods
            .shared_get(&decoded_members[0].name)
            .unwrap();
        assert!(Arc::ptr_eq(&shared, &decoded_members[0].declaration));
        assert_eq!(
            decoded.member_parameters(&decoded_members[0]),
            members[0].declaration.get().params()
        );
        assert_eq!(
            decoded
                .properties(class_layout)
                .iter()
                .map(|property| property.name.clone())
                .collect::<Vec<_>>(),
            properties
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            decoded
                .constants(class_layout)
                .iter()
                .map(|constant| constant.name.clone())
                .collect::<Vec<_>>(),
            constants
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        );
        for property in decoded.properties(class_layout) {
            let class_property = decoded
                .class(class_layout)
                .properties
                .get(&property.name)
                .unwrap();
            assert!(Arc::ptr_eq(&property.declaration, class_property));
        }
        for constant in decoded.constants(class_layout) {
            let class_constant = decoded
                .class(class_layout)
                .consts
                .get(&constant.name)
                .unwrap();
            assert!(Arc::ptr_eq(&constant.declaration, class_constant));
        }
        assert_eq!(decoded.names(class_layout), names);
        assert_eq!(decoded.function(function_layout), &function);
        assert_eq!(
            decoded.function_layout_for(decoded.functions.iter().next().unwrap().1),
            Some(function_layout)
        );
        assert_eq!(decoded.parameters(function_layout), function.params);
    }

    #[test]
    fn project_ids_retain_winning_file_declaration_locations() {
        let tokens = Lexer::new(
            "<?php class Demo { public int $value; public const int MARK = 1; public function run(int $n): int {} } function helper(string $text): bool {}",
        )
        .tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        let built = crate::registry::Registry::build(&program);
        let class = Arc::new(built.lookup_class("Demo").unwrap().1.clone());
        let function = Arc::new(built.lookup_function("helper").unwrap().clone());
        let class_members = class
            .methods()
            .shared_iter()
            .map(|(name, declaration)| FileMember::from_shared(name.clone(), declaration));
        let mut declarations = FileDecls::new();
        declarations.add_class(
            Arc::clone(&class),
            class_members,
            class
                .properties()
                .iter()
                .map(|(name, property)| (name.clone(), Arc::clone(property))),
            class
                .consts()
                .iter()
                .map(|(name, constant)| (name.clone(), Arc::clone(constant))),
            class
                .extends()
                .iter()
                .chain(class.implements())
                .chain(class.uses())
                .cloned(),
        );
        declarations.add_function(Arc::clone(&function), function.params().iter().cloned());
        let declarations = Arc::new(declarations);

        let mut registry = crate::registry::Registry::default();
        registry.insert_built_decl_from_file(
            crate::registry::BuiltDecl::Class {
                name: Name::from("Demo"),
                conditional: false,
                guard: None,
                info: Arc::clone(&class),
                namespace: Name::from(""),
            },
            true,
            Some(&declarations),
        );
        registry.insert_built_decl_from_file(
            crate::registry::BuiltDecl::Function {
                name: Name::from("helper"),
                conditional: false,
                guard: None,
                sig: Arc::clone(&function),
                site: 0,
            },
            true,
            Some(&declarations),
        );
        registry.precompute_member_owners();
        registry.end_declaration_changes(&crate::registry::scope::Scope::all(), None, true);
        registry.begin_declaration_changes();

        let class_id = registry.class("demo").unwrap().0;
        let function_id = registry.function("HELPER").unwrap().0;
        // Raw reads for project declarations come from their immutable file
        // locators. The mutable tables are no longer needed to find them by
        // name once the ID index has selected the winner.
        assert!(registry.classes.remove("Demo"));
        assert!(registry.functions.remove("helper"));
        assert_eq!(registry.class("DEMO").unwrap().0, class_id);
        assert_eq!(registry.function("helper").unwrap().0, function_id);
        assert!(std::ptr::eq(
            registry.file_class_by_id(class_id).unwrap(),
            registry.class("Demo").unwrap().2
        ));
        assert!(std::ptr::eq(
            registry.file_function_by_id(function_id).unwrap(),
            registry.function("helper").unwrap().2
        ));
        assert!(std::ptr::eq(
            registry.lookup_method("Demo", "RUN").unwrap(),
            registry.file_method_by_class_id(class_id, "run").unwrap()
        ));
        let method_ref = registry
            .file_method_ref_by_class_id(class_id, "RUN")
            .unwrap();
        assert_eq!(method_ref.class(), class_id);
        assert!(std::ptr::eq(
            registry.file_method_by_ref(method_ref).unwrap(),
            registry.file_method_by_class_id(class_id, "run").unwrap()
        ));
        assert!(std::ptr::eq(
            registry.lookup_property("Demo", "value").unwrap(),
            registry
                .file_property_by_class_id(class_id, "value")
                .unwrap()
        ));
        assert!(std::ptr::eq(
            registry.lookup_const("Demo", "MARK").unwrap(),
            registry
                .file_constant_by_class_id(class_id, "MARK")
                .unwrap()
        ));
        let mut merged = registry.clone();
        merged.merge_stub_classes(vec![(Name::from("Demo"), (*class).clone())]);
        assert!(merged.file_class_by_id(class_id).is_none());
        registry.remove_declarations(&[Name::from("helper")], &[Name::from("Demo")]);
        assert!(registry.file_class_by_id(class_id).is_none());
        assert!(registry.file_function_by_id(function_id).is_none());
    }

    #[test]
    fn function_declarations_stay_as_built_while_class_state_resets() {
        let tokens = Lexer::new("<?php function helper(): int {}").tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        let mut registry = crate::registry::Registry::build(&program);
        let all = crate::registry::scope::Scope::all();
        registry.end_declaration_changes(&all, None, true);

        let mut edited = registry.functions.get("helper").unwrap().clone();
        edited.return_type = Type::string();
        registry.begin_declaration_changes();
        registry
            .functions
            .insert(Name::from("helper"), edited.clone());

        let scope = crate::registry::scope::Scope::of(std::iter::empty::<&str>(), ["helper"]);
        let before = registry
            .end_declaration_changes(&scope, Some(&scope), true)
            .unwrap();
        assert_eq!(registry.functions.get("helper"), Some(&edited));
        assert_eq!(before.functions.len(), 1);
        assert_eq!(
            before.functions[0].as_ref().unwrap().get().return_type,
            Type::int()
        );
        assert!(
            !registry
                .changed_since(&before, &crate::registry::ChangedNames::default())
                .is_empty()
        );

        registry.begin_declaration_changes();
        let unchanged = registry
            .end_declaration_changes(&scope, Some(&scope), true)
            .unwrap();
        assert!(
            registry
                .changed_since(&unchanged, &crate::registry::ChangedNames::default())
                .is_empty()
        );
    }

    #[test]
    fn switching_registry_bases_invalidates_the_previous_id_generation() {
        let tokens = Lexer::new(
            "<?php class ParentDemo { public function work(): void {} } class Demo extends ParentDemo implements \\PHPStan\\Rules\\Rule {}",
        )
        .tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        let mut registry = crate::registry::Registry::build(&program);
        let old_id = registry.class("Demo").unwrap().0;
        assert!(registry.class_has_phpstan_ancestry("Demo"));
        assert!(registry.is_subclass_of("Demo", "PHPStan\\Rules\\Rule"));
        assert_eq!(
            registry
                .lookup_method_with_owner("Demo", "work")
                .map(|(owner, _)| owner),
            Some(syntax::name::Name::from("ParentDemo"))
        );
        let mut replacement = crate::registry::Registry::build(&program);
        replacement.freeze();

        registry.switch_base(replacement);

        let current_id = registry.class("Demo").unwrap().0;
        assert_ne!(current_id, old_id);
        assert_eq!(current_id.generation(), old_id.generation() + 1);
        assert!(registry.class_has_phpstan_ancestry("Demo"));
        assert!(registry.is_subclass_of("Demo", "PHPStan\\Rules\\Rule"));
        assert_eq!(
            registry
                .lookup_method_with_owner("Demo", "work")
                .map(|(owner, _)| owner),
            Some(syntax::name::Name::from("ParentDemo"))
        );
        let stale_read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.class_by_id(old_id)
        }));
        assert!(stale_read.is_err());
    }

    #[test]
    fn trait_members_and_aliases_receive_ordered_class_local_slots() {
        let tokens = Lexer::new(
            "<?php trait Extra { public function zebra(): void {} public function alpha(): void {} } class Host { public function ownBefore(): void {} use Extra { zebra as zebraAlias; } public function ownAfter(): void {} }",
        )
        .tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        let registry = crate::registry::Registry::build(&program);
        let (class_id, _, class) = registry.class("Host").unwrap();

        assert_eq!(
            class
                .member_order
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["ownBefore", "ownAfter", "zebraAlias", "zebra", "alpha"]
        );
        for name in ["ownBefore", "ownAfter", "zebraAlias", "zebra", "alpha"] {
            let method_ref = registry
                .file_method_ref_by_class_id(class_id, name)
                .unwrap();
            assert!(std::ptr::eq(
                registry.method_by_ref(method_ref).unwrap(),
                registry.lookup_method("Host", name).unwrap()
            ));
        }
    }
}
