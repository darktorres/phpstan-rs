//! The parts of clang's JSON AST dump (`-ast-dump=json`) the catalog reads,
//! read as clang writes them.
//!
//! The dump writes a location's file and line only where they differ from
//! the location written before it, so every location is read in order, in
//! the declarations skipped as in the ones kept.

use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

/// Where a node starts: the file, line, column and offset of its
/// expansion, the file as the dump names it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Loc {
    pub(crate) file: Option<Arc<str>>,
    pub(crate) line: u32,
    pub(crate) col: u32,
    pub(crate) offset: u32,
}

/// The file and line the dump wrote last, and the file names seen.
#[derive(Default)]
struct LocState {
    file: Option<Arc<str>>,
    line: u32,
    files: FxHashMap<Box<str>, Arc<str>>,
}

thread_local! {
    static STATE: RefCell<LocState> = RefCell::new(LocState::default());
}

/// Forgets the file and line written last, for a new dump.
pub(crate) fn reset_locations() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.file = None;
        state.line = 0;
    });
}

fn set_file(name: &str) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let file = match state.files.get(name) {
            Some(file) => file.clone(),
            None => {
                let file: Arc<str> = Arc::from(name);
                state.files.insert(name.into(), file.clone());
                file
            }
        };
        state.file = Some(file);
    });
}

fn set_line(line: u32) {
    STATE.with(|state| state.borrow_mut().line = line);
}

fn current() -> (Option<Arc<str>>, u32) {
    STATE.with(|state| {
        let state = state.borrow();
        (state.file.clone(), state.line)
    })
}

/// A node's kind, those the catalog reads by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `{}`: a child the node does not have, such as a `for` without a
    /// condition.
    Null,
    TranslationUnitDecl,
    FunctionDecl,
    ParmVarDecl,
    VarDecl,
    EnumDecl,
    EnumConstantDecl,
    RecordDecl,
    FieldDecl,
    TypedefDecl,
    CompoundStmt,
    DeclStmt,
    IfStmt,
    WhileStmt,
    DoStmt,
    ForStmt,
    SwitchStmt,
    CaseStmt,
    DefaultStmt,
    BreakStmt,
    ContinueStmt,
    ReturnStmt,
    GotoStmt,
    IndirectGotoStmt,
    LabelStmt,
    NullStmt,
    AttributedStmt,
    GccAsmStmt,
    BinaryOperator,
    CompoundAssignOperator,
    UnaryOperator,
    ConditionalOperator,
    BinaryConditionalOperator,
    CallExpr,
    ImplicitCastExpr,
    CStyleCastExpr,
    ParenExpr,
    DeclRefExpr,
    MemberExpr,
    ArraySubscriptExpr,
    IntegerLiteral,
    CharacterLiteral,
    FloatingLiteral,
    StringLiteral,
    UnaryExprOrTypeTraitExpr,
    InitListExpr,
    CompoundLiteralExpr,
    ImplicitValueInitExpr,
    ConstantExpr,
    PredefinedExpr,
    StmtExpr,
    VaArgExpr,
    OffsetOfExpr,
    GenericSelectionExpr,
    ChooseExpr,
    AtomicExpr,
    OpaqueValueExpr,
    AddrLabelExpr,
    ShuffleVectorExpr,
    ConvertVectorExpr,
    ExprWithCleanups,
    MaterializeTemporaryExpr,
    BuiltinBitCastExpr,
    TypeTraitExpr,
    /// An attribute, by its kind (`NoReturnAttr`).
    Attr(Box<str>),
    /// Any other kind, by name.
    Other(Box<str>),
}

impl Kind {
    fn from_name(name: &str) -> Kind {
        match name {
            "TranslationUnitDecl" => Kind::TranslationUnitDecl,
            "FunctionDecl" => Kind::FunctionDecl,
            "ParmVarDecl" => Kind::ParmVarDecl,
            "VarDecl" => Kind::VarDecl,
            "EnumDecl" => Kind::EnumDecl,
            "EnumConstantDecl" => Kind::EnumConstantDecl,
            "RecordDecl" => Kind::RecordDecl,
            "FieldDecl" => Kind::FieldDecl,
            "TypedefDecl" => Kind::TypedefDecl,
            "CompoundStmt" => Kind::CompoundStmt,
            "DeclStmt" => Kind::DeclStmt,
            "IfStmt" => Kind::IfStmt,
            "WhileStmt" => Kind::WhileStmt,
            "DoStmt" => Kind::DoStmt,
            "ForStmt" => Kind::ForStmt,
            "SwitchStmt" => Kind::SwitchStmt,
            "CaseStmt" => Kind::CaseStmt,
            "DefaultStmt" => Kind::DefaultStmt,
            "BreakStmt" => Kind::BreakStmt,
            "ContinueStmt" => Kind::ContinueStmt,
            "ReturnStmt" => Kind::ReturnStmt,
            "GotoStmt" => Kind::GotoStmt,
            "IndirectGotoStmt" => Kind::IndirectGotoStmt,
            "LabelStmt" => Kind::LabelStmt,
            "NullStmt" => Kind::NullStmt,
            "AttributedStmt" => Kind::AttributedStmt,
            "GCCAsmStmt" => Kind::GccAsmStmt,
            "BinaryOperator" => Kind::BinaryOperator,
            "CompoundAssignOperator" => Kind::CompoundAssignOperator,
            "UnaryOperator" => Kind::UnaryOperator,
            "ConditionalOperator" => Kind::ConditionalOperator,
            "BinaryConditionalOperator" => Kind::BinaryConditionalOperator,
            "CallExpr" => Kind::CallExpr,
            "ImplicitCastExpr" => Kind::ImplicitCastExpr,
            "CStyleCastExpr" => Kind::CStyleCastExpr,
            "ParenExpr" => Kind::ParenExpr,
            "DeclRefExpr" => Kind::DeclRefExpr,
            "MemberExpr" => Kind::MemberExpr,
            "ArraySubscriptExpr" => Kind::ArraySubscriptExpr,
            "IntegerLiteral" => Kind::IntegerLiteral,
            "CharacterLiteral" => Kind::CharacterLiteral,
            "FloatingLiteral" => Kind::FloatingLiteral,
            "StringLiteral" => Kind::StringLiteral,
            "UnaryExprOrTypeTraitExpr" => Kind::UnaryExprOrTypeTraitExpr,
            "InitListExpr" => Kind::InitListExpr,
            "CompoundLiteralExpr" => Kind::CompoundLiteralExpr,
            "ImplicitValueInitExpr" => Kind::ImplicitValueInitExpr,
            "ConstantExpr" => Kind::ConstantExpr,
            "PredefinedExpr" => Kind::PredefinedExpr,
            "StmtExpr" => Kind::StmtExpr,
            "VAArgExpr" => Kind::VaArgExpr,
            "OffsetOfExpr" => Kind::OffsetOfExpr,
            "GenericSelectionExpr" => Kind::GenericSelectionExpr,
            "ChooseExpr" => Kind::ChooseExpr,
            "AtomicExpr" => Kind::AtomicExpr,
            "OpaqueValueExpr" => Kind::OpaqueValueExpr,
            "AddrLabelExpr" => Kind::AddrLabelExpr,
            "ShuffleVectorExpr" => Kind::ShuffleVectorExpr,
            "ConvertVectorExpr" => Kind::ConvertVectorExpr,
            "ExprWithCleanups" => Kind::ExprWithCleanups,
            "MaterializeTemporaryExpr" => Kind::MaterializeTemporaryExpr,
            "BuiltinBitCastExpr" => Kind::BuiltinBitCastExpr,
            "TypeTraitExpr" => Kind::TypeTraitExpr,
            other if other.ends_with("Attr") => Kind::Attr(other.into()),
            other => Kind::Other(other.into()),
        }
    }
}

/// A literal's value as the dump writes it: integers and floating literals
/// as their digits, a string literal as its source spelling, a character
/// literal as its code.
#[derive(Debug, Clone)]
pub(crate) enum Literal {
    Text(Box<str>),
    Number(i64),
}

/// The declaration an expression names.
#[derive(Debug, Clone, Default)]
pub(crate) struct Referenced {
    pub(crate) id: u64,
    pub(crate) kind: Box<str>,
    pub(crate) name: Option<Box<str>>,
}

/// A node of the dump, with the fields the catalog reads.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) kind: Kind,
    pub(crate) id: u64,
    pub(crate) loc: Loc,
    pub(crate) name: Option<Box<str>>,
    /// The type as written (`qualType`).
    pub(crate) ty: Option<Box<str>>,
    /// The type with its typedefs resolved (`desugaredQualType`), where it
    /// differs.
    pub(crate) desugared: Option<Box<str>>,
    pub(crate) opcode: Option<Box<str>>,
    pub(crate) cast_kind: Option<Box<str>>,
    pub(crate) value: Option<Literal>,
    pub(crate) referenced: Option<Referenced>,
    /// The member an initializer list of a union initializes.
    pub(crate) field: Option<Referenced>,
    pub(crate) member_decl: Option<u64>,
    pub(crate) is_arrow: bool,
    pub(crate) is_postfix: bool,
    pub(crate) is_inline: bool,
    pub(crate) is_variadic: bool,
    pub(crate) has_else: bool,
    pub(crate) is_gnu_range: bool,
    pub(crate) is_implicit: bool,
    /// A `_Generic` association the selection chose.
    pub(crate) is_selected: bool,
    /// An expression that names an object.
    pub(crate) is_lvalue: bool,
    /// A variable declared with an initializer.
    pub(crate) has_init: bool,
    /// A record declaration with its members.
    pub(crate) is_definition: bool,
    pub(crate) storage_class: Option<Box<str>>,
    pub(crate) target_label: Option<u64>,
    pub(crate) decl_id: Option<u64>,
    pub(crate) arg_type: Option<Box<str>>,
    pub(crate) inner: Vec<Node>,
    /// The value an initializer list gives the array elements it does not
    /// list.
    pub(crate) array_filler: Option<Box<Node>>,
}

impl Node {
    fn empty() -> Node {
        Node {
            kind: Kind::Null,
            id: 0,
            loc: Loc::default(),
            name: None,
            ty: None,
            desugared: None,
            opcode: None,
            cast_kind: None,
            value: None,
            referenced: None,
            field: None,
            member_decl: None,
            is_arrow: false,
            is_postfix: false,
            is_inline: false,
            is_variadic: false,
            has_else: false,
            is_gnu_range: false,
            is_implicit: false,
            is_selected: false,
            is_lvalue: false,
            has_init: false,
            is_definition: false,
            storage_class: None,
            target_label: None,
            decl_id: None,
            arg_type: None,
            inner: Vec::new(),
            array_filler: None,
        }
    }

    /// The type with its typedefs resolved.
    pub(crate) fn canonical_type(&self) -> Option<&str> {
        self.desugared.as_deref().or(self.ty.as_deref())
    }
}

/// Decides, for each declaration at a dump's top level, whether its
/// children are read; the ones it declines are skipped, their locations
/// still read.
pub(crate) trait TopLevel {
    /// Whether the declaration's children are wanted, given what precedes
    /// them in the dump: its kind, name and location.
    fn wants(&mut self, node: &Node) -> bool;
    /// Takes a declaration read, its children read where wanted.
    fn take(&mut self, node: Node);
}

/// Reads a whole dump, handing each top-level declaration to `top`.
pub(crate) fn read_dump<'de, D: Deserializer<'de>>(
    deserializer: D,
    top: &mut dyn TopLevel,
) -> Result<(), D::Error> {
    reset_locations();
    deserializer.deserialize_map(UnitVisitor { top })
}

struct UnitVisitor<'t> {
    top: &'t mut dyn TopLevel,
}

impl<'de> Visitor<'de> for UnitVisitor<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a translation unit")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<Key>()? {
            match key {
                Key::Inner => map.next_value_seed(TopSeq {
                    top: &mut *self.top,
                })?,
                _ => map.next_value_seed(Track)?,
            }
        }
        Ok(())
    }
}

struct TopSeq<'t> {
    top: &'t mut dyn TopLevel,
}

impl<'de> DeserializeSeed<'de> for TopSeq<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for TopSeq<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("the declarations of a translation unit")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(node) = seq.next_element_seed(NodeSeed {
            top: Some(&mut *self.top),
        })? {
            self.top.take(node);
        }
        Ok(())
    }
}

/// A key the reader acts on; the rest are `Key::Other`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Id,
    Kind,
    Loc,
    Range,
    Begin,
    Name,
    Type,
    Opcode,
    CastKind,
    Value,
    ReferencedDecl,
    Field,
    ReferencedMemberDecl,
    IsArrow,
    IsPostfix,
    Inline,
    Variadic,
    HasElse,
    IsGnuRange,
    IsImplicit,
    Selected,
    ValueCategory,
    Init,
    CompleteDefinition,
    StorageClass,
    TargetLabelDeclId,
    DeclId,
    LabelDeclId,
    ArgType,
    Inner,
    ArrayFiller,
    Offset,
    File,
    Line,
    Col,
    SpellingLoc,
    ExpansionLoc,
    IncludedFrom,
    Other,
}

impl<'de> de::Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Key, D::Error> {
        struct KeyVisitor;
        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = Key;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a key")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Key, E> {
                Ok(match v {
                    "id" => Key::Id,
                    "kind" => Key::Kind,
                    "loc" => Key::Loc,
                    "range" => Key::Range,
                    "begin" => Key::Begin,
                    "name" => Key::Name,
                    "type" => Key::Type,
                    "opcode" => Key::Opcode,
                    "castKind" => Key::CastKind,
                    "value" => Key::Value,
                    "referencedDecl" => Key::ReferencedDecl,
                    "field" => Key::Field,
                    "referencedMemberDecl" => Key::ReferencedMemberDecl,
                    "isArrow" => Key::IsArrow,
                    "isPostfix" => Key::IsPostfix,
                    "inline" => Key::Inline,
                    "variadic" => Key::Variadic,
                    "hasElse" => Key::HasElse,
                    "isGNURange" => Key::IsGnuRange,
                    "isImplicit" => Key::IsImplicit,
                    "selected" => Key::Selected,
                    "valueCategory" => Key::ValueCategory,
                    "init" => Key::Init,
                    "completeDefinition" => Key::CompleteDefinition,
                    "storageClass" => Key::StorageClass,
                    "targetLabelDeclId" => Key::TargetLabelDeclId,
                    "declId" => Key::DeclId,
                    "labelDeclId" => Key::LabelDeclId,
                    "argType" => Key::ArgType,
                    "inner" => Key::Inner,
                    "array_filler" => Key::ArrayFiller,
                    "offset" => Key::Offset,
                    "file" => Key::File,
                    "line" => Key::Line,
                    "col" => Key::Col,
                    "spellingLoc" => Key::SpellingLoc,
                    "expansionLoc" => Key::ExpansionLoc,
                    "includedFrom" => Key::IncludedFrom,
                    _ => Key::Other,
                })
            }
        }
        deserializer.deserialize_identifier(KeyVisitor)
    }
}

/// A `file` value, taken as the file the dump is at.
struct FileSeed;

impl<'de> DeserializeSeed<'de> for FileSeed {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for FileSeed {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a file name")
    }

    fn visit_str<E>(self, v: &str) -> Result<(), E> {
        set_file(v);
        Ok(())
    }
}

/// An `id`, `0x` and hexadecimal digits.
struct IdSeed;

impl<'de> DeserializeSeed<'de> for IdSeed {
    type Value = u64;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<u64, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for IdSeed {
    type Value = u64;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an id")
    }

    fn visit_str<E>(self, v: &str) -> Result<u64, E> {
        Ok(parse_id(v))
    }
}

/// A `kind`.
struct KindSeed;

impl<'de> DeserializeSeed<'de> for KindSeed {
    type Value = Kind;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Kind, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for KindSeed {
    type Value = Kind;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a node kind")
    }

    fn visit_str<E>(self, v: &str) -> Result<Kind, E> {
        Ok(Kind::from_name(v))
    }
}

/// Reads a value only for the locations in it, keeping nothing.
struct Track;

impl<'de> DeserializeSeed<'de> for Track {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(TrackVisitor)
    }
}

struct TrackVisitor;

impl<'de> Visitor<'de> for TrackVisitor {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_none<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while seq.next_element_seed(Track)?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<Key>()? {
            match key {
                Key::File => map.next_value_seed(FileSeed)?,
                Key::Line => set_line(map.next_value::<u32>()?),
                Key::IncludedFrom => {
                    map.next_value::<IgnoredAny>()?;
                }
                _ => map.next_value_seed(Track)?,
            }
        }
        Ok(())
    }
}

/// Reads a location (`loc`, or a range's `begin`), giving where its
/// expansion is.
struct LocSeed;

impl<'de> DeserializeSeed<'de> for LocSeed {
    type Value = Loc;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Loc, D::Error> {
        deserializer.deserialize_map(LocVisitor)
    }
}

struct LocVisitor;

impl<'de> Visitor<'de> for LocVisitor {
    type Value = Loc;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a source location")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Loc, A::Error> {
        let mut offset = None;
        let mut col = 0;
        let mut expansion = None;
        let mut any = false;
        while let Some(key) = map.next_key::<Key>()? {
            any = true;
            match key {
                Key::Offset => offset = Some(map.next_value::<u32>()?),
                Key::Col => col = map.next_value::<u32>()?,
                Key::File => map.next_value_seed(FileSeed)?,
                Key::Line => set_line(map.next_value::<u32>()?),
                Key::SpellingLoc => {
                    map.next_value_seed(LocSeed)?;
                }
                Key::ExpansionLoc => expansion = Some(map.next_value_seed(LocSeed)?),
                Key::IncludedFrom => {
                    map.next_value::<IgnoredAny>()?;
                }
                _ => map.next_value_seed(Track)?,
            }
        }
        if let Some(expansion) = expansion {
            return Ok(expansion);
        }
        if !any {
            return Ok(Loc::default());
        }
        let (file, line) = current();
        Ok(Loc {
            file,
            line,
            col,
            offset: offset.unwrap_or(0),
        })
    }
}

/// Reads a range, giving where its start's expansion is.
struct RangeSeed;

impl<'de> DeserializeSeed<'de> for RangeSeed {
    type Value = Loc;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Loc, D::Error> {
        deserializer.deserialize_map(RangeVisitor)
    }
}

struct RangeVisitor;

impl<'de> Visitor<'de> for RangeVisitor {
    type Value = Loc;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a source range")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Loc, A::Error> {
        let mut begin = Loc::default();
        while let Some(key) = map.next_key::<Key>()? {
            match key {
                Key::Begin => begin = map.next_value_seed(LocSeed)?,
                _ => map.next_value_seed(Track)?,
            }
        }
        Ok(begin)
    }
}

/// Reads a node; at the top level, `top` decides whether its children are
/// read.
struct NodeSeed<'t> {
    top: Option<&'t mut dyn TopLevel>,
}

impl<'de> DeserializeSeed<'de> for NodeSeed<'_> {
    type Value = Node;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Node, D::Error> {
        deserializer.deserialize_map(self)
    }
}

/// A node's children, read in full.
struct Children;

impl<'de> DeserializeSeed<'de> for Children {
    type Value = Vec<Node>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<Node>, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for Children {
    type Value = Vec<Node>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a node's children")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<Node>, A::Error> {
        let mut nodes = Vec::new();
        while let Some(node) = seq.next_element_seed(NodeSeed { top: None })? {
            nodes.push(node);
        }
        Ok(nodes)
    }
}

/// `{"qualType": ..., "desugaredQualType": ...}`.
#[derive(serde::Deserialize)]
struct TypeField {
    #[serde(rename = "qualType")]
    qual: Option<Box<str>>,
    #[serde(rename = "desugaredQualType")]
    desugared: Option<Box<str>>,
}

#[derive(serde::Deserialize)]
struct ReferencedField {
    id: Box<str>,
    kind: Box<str>,
    name: Option<Box<str>>,
}

impl ReferencedField {
    fn into_referenced(self) -> Referenced {
        Referenced {
            id: parse_id(&self.id),
            kind: self.kind,
            name: self.name,
        }
    }
}

fn parse_id(text: &str) -> u64 {
    u64::from_str_radix(text.trim_start_matches("0x"), 16).unwrap_or(0)
}

/// A literal's `value`: a string or a number.
struct LiteralSeed;

impl<'de> DeserializeSeed<'de> for LiteralSeed {
    type Value = Literal;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Literal, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for LiteralSeed {
    type Value = Literal;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a literal value")
    }

    fn visit_str<E>(self, v: &str) -> Result<Literal, E> {
        Ok(Literal::Text(v.into()))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Literal, E> {
        Ok(Literal::Number(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Literal, E> {
        Ok(Literal::Number(i64::try_from(v).map_err(E::custom)?))
    }
    fn visit_bool<E>(self, v: bool) -> Result<Literal, E> {
        Ok(Literal::Number(i64::from(v)))
    }
}

impl<'de> Visitor<'de> for NodeSeed<'_> {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a node")
    }

    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<Node, A::Error> {
        let mut node = Node::empty();
        while let Some(key) = map.next_key::<Key>()? {
            match key {
                Key::Id => node.id = map.next_value_seed(IdSeed)?,
                Key::Kind => node.kind = map.next_value_seed(KindSeed)?,
                Key::Loc => node.loc = map.next_value_seed(LocSeed)?,
                Key::Range => {
                    let begin = map.next_value_seed(RangeSeed)?;
                    if node.loc.file.is_none() {
                        node.loc = begin;
                    }
                }
                Key::Name => node.name = Some(map.next_value()?),
                Key::Type => {
                    let ty: TypeField = map.next_value()?;
                    node.ty = ty.qual;
                    node.desugared = ty.desugared;
                }
                Key::Opcode => node.opcode = Some(map.next_value()?),
                Key::CastKind => node.cast_kind = Some(map.next_value()?),
                Key::Value => node.value = Some(map.next_value_seed(LiteralSeed)?),
                Key::ReferencedDecl => {
                    node.referenced = Some(map.next_value::<ReferencedField>()?.into_referenced());
                }
                Key::Field => {
                    node.field = Some(map.next_value::<ReferencedField>()?.into_referenced());
                }
                Key::ReferencedMemberDecl => node.member_decl = Some(map.next_value_seed(IdSeed)?),
                Key::IsArrow => node.is_arrow = map.next_value()?,
                Key::IsPostfix => node.is_postfix = map.next_value()?,
                Key::Inline => node.is_inline = map.next_value()?,
                Key::Variadic => node.is_variadic = map.next_value()?,
                Key::HasElse => node.has_else = map.next_value()?,
                Key::IsGnuRange => node.is_gnu_range = map.next_value()?,
                Key::IsImplicit => node.is_implicit = map.next_value()?,
                Key::Selected => node.is_selected = map.next_value()?,
                Key::ValueCategory => {
                    let category: Box<str> = map.next_value()?;
                    node.is_lvalue = &*category == "lvalue";
                }
                Key::Init => {
                    map.next_value::<IgnoredAny>()?;
                    node.has_init = true;
                }
                Key::CompleteDefinition => node.is_definition = map.next_value()?,
                Key::StorageClass => node.storage_class = Some(map.next_value()?),
                Key::TargetLabelDeclId => node.target_label = Some(map.next_value_seed(IdSeed)?),
                Key::DeclId | Key::LabelDeclId => {
                    node.decl_id = Some(map.next_value_seed(IdSeed)?);
                }
                Key::ArgType => {
                    let ty: TypeField = map.next_value()?;
                    node.arg_type = ty.desugared.or(ty.qual);
                }
                Key::Inner => {
                    let wanted = match self.top.as_mut() {
                        Some(top) => top.wants(&node),
                        None => true,
                    };
                    if wanted {
                        node.inner = map.next_value_seed(Children)?;
                    } else {
                        map.next_value_seed(Track)?;
                    }
                }
                // The dump puts a node's children in the array of the first
                // child's label, so an initializer list with a filler has
                // the filler, then its elements, in `array_filler`.
                Key::ArrayFiller => {
                    let mut children = map.next_value_seed(Children)?.into_iter();
                    node.array_filler = children.next().map(Box::new);
                    node.inner = children.collect();
                }
                _ => map.next_value_seed(Track)?,
            }
        }
        Ok(node)
    }
}
