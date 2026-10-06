//! Per-file declaration storage. Arenas make indices and ranges specific to
//! their declaration kind, so member and parameter ranges cannot be confused.

use la_arena::{Arena, Idx, IdxRange};
use syntax::name::Name;

use super::ClassReflection;
use super::class_info::{ConstantReflection, PropertyReflection};
use super::function_sig::{FunctionReflection, ParameterReflection};

#[allow(dead_code)] // Consumed as row 4.3 moves readers to typed ranges.
#[derive(Clone, Debug)]
pub(crate) struct FileMember {
    pub(crate) name: Name,
    pub(crate) declaration: FunctionReflection,
}

#[allow(dead_code)] // Consumed as row 4.3 moves readers to typed ranges.
#[derive(Clone, Debug)]
pub(crate) struct ClassLayout {
    pub(crate) declaration: Idx<ClassReflection>,
    pub(crate) members: IdxRange<FileMember>,
    pub(crate) properties: IdxRange<PropertyReflection>,
    pub(crate) constants: IdxRange<ConstantReflection>,
    pub(crate) names: IdxRange<Name>,
}

#[allow(dead_code)] // Consumed as row 4.3 moves readers to typed ranges.
#[derive(Clone, Debug)]
pub(crate) struct FunctionLayout {
    pub(crate) declaration: Idx<FunctionReflection>,
    pub(crate) parameters: IdxRange<ParameterReflection>,
}

#[allow(dead_code)] // Populated and consumed as registry storage migrates in row 4.3.
pub(crate) struct FileDecls {
    pub(crate) classes: Arena<ClassReflection>,
    pub(crate) functions: Arena<FunctionReflection>,
    pub(crate) class_layouts: Arena<ClassLayout>,
    pub(crate) function_layouts: Arena<FunctionLayout>,
    pub(crate) members: Arena<FileMember>,
    pub(crate) parameters: Arena<ParameterReflection>,
    pub(crate) properties: Arena<PropertyReflection>,
    pub(crate) constants: Arena<ConstantReflection>,
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
        declaration: ClassReflection,
        members: impl IntoIterator<Item = FileMember>,
        properties: impl IntoIterator<Item = PropertyReflection>,
        constants: impl IntoIterator<Item = ConstantReflection>,
        names: impl IntoIterator<Item = Name>,
    ) -> Idx<ClassLayout> {
        let declaration = self.classes.alloc(declaration);
        let members = self.members.alloc_many(members);
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
        declaration: FunctionReflection,
        parameters: impl IntoIterator<Item = ParameterReflection>,
    ) -> Idx<FunctionLayout> {
        let declaration = self.functions.alloc(declaration);
        let parameters = self.parameters.alloc_many(parameters);
        self.function_layouts.alloc(FunctionLayout {
            declaration,
            parameters,
        })
    }
}
