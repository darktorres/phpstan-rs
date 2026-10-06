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
    class_by_ptr: FxHashMap<usize, Idx<ClassLayout>>,
    function_by_ptr: FxHashMap<usize, Idx<FunctionLayout>>,
    pub(crate) members: Arena<FileMember>,
    /// Case-folded member name to the file-local member slot. The class
    /// layout index is part of the key because each class has its own
    /// member namespace while all members share one typed arena.
    member_slots: FxHashMap<(u32, String), Idx<FileMember>>,
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
            class_by_ptr: FxHashMap::default(),
            function_by_ptr: FxHashMap::default(),
            members: Arena::new(),
            member_slots: FxHashMap::default(),
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
                .alloc_many(member.declaration.get().params().iter().cloned());
            stored_members.push(member);
        }
        let members = self.members.alloc_many(stored_members);
        let properties = self.properties.alloc_many(properties);
        let constants = self.constants.alloc_many(constants);
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
        for (_, layout) in class_layouts.iter() {
            if !idx_is_valid(layout.declaration, classes.len())
                || !range_is_valid(&layout.members, encoded.members.len())
            {
                return Err(D::Error::custom("class member range is invalid"));
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
        }
        let members = decoded_members
            .into_iter()
            .map(|member| member.ok_or_else(|| D::Error::custom("class member range is missing")))
            .collect::<Result<Vec<_>, _>>()?;
        let members = Arena::from_iter(members);
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
        let declarations = FileDecls {
            classes,
            functions: Arena::from_iter(encoded.functions),
            class_layouts,
            function_layouts,
            class_by_ptr,
            function_by_ptr,
            members,
            member_slots,
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
        assert_eq!(decoded.properties(class_layout), properties);
        assert_eq!(decoded.constants(class_layout), constants);
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
            "<?php class Demo { public function run(int $n): int {} } function helper(string $text): bool {}",
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
            class.properties().values().map(Arc::clone),
            class.consts().values().map(Arc::clone),
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
        let mut merged = registry.clone();
        merged.merge_stub_classes(vec![(Name::from("Demo"), (*class).clone())]);
        assert!(merged.file_class_by_id(class_id).is_none());
        registry.remove_declarations(&[Name::from("helper")], &[Name::from("Demo")]);
        assert!(registry.file_class_by_id(class_id).is_none());
        assert!(registry.file_function_by_id(function_id).is_none());
    }
}
