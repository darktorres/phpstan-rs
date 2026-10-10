use std::borrow::Cow;
use std::sync::Arc;

use crate::name::Name;
use crate::php_string::PhpString;
use crate::span::Span;

/// One `Name(args)` entry inside an attribute group
/// (`#[Name(args), ...]`). `name` is the raw class name as written,
/// resolved by the checker (not here). `args` reuses `Arg`'s call-argument
/// shape, since PHP's grammar doesn't distinguish the two.
#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: QualifiedName,
    pub args: ArgRange,
    pub span: Span,
    // What deleting this attribute removes: its whole `#[...]` group up
    // to the next token when it's alone in the group, else its own entry
    // with the comma separating it from a neighbour.
    pub removal_span: Span,
}

/// A statement. Its real source span is stored in `NodeArenas::stmt_spans`
/// at the matching statement ID.
#[derive(Debug, Clone)]
pub struct StmtNode {
    pub id: StmtId,
    pub kind: StmtKind,
}

/// A typed index into a parsed file's expression arena.
pub type ExprId = la_arena::Idx<ExprNode>;

/// A typed index into a parsed file's statement arena.
pub type StmtId = la_arena::Idx<StmtNode>;

/// The file-owned storage addressed by [`ExprId`].
pub type ExprArena = la_arena::Arena<ExprNode>;

/// The file-owned storage addressed by [`StmtId`].
pub type StmtArena = la_arena::Arena<StmtNode>;

/// Storage for the less common declaration payloads kept out of expression
/// and statement nodes in the file-owned arena tree.
pub type FunctionArena = la_arena::Arena<FunctionDecl>;
pub type MethodArena = la_arena::Arena<MethodDecl>;
pub type ClassArena = la_arena::Arena<ClassDef>;
pub type ClosureArena = la_arena::Arena<ClosurePayload>;
pub type AnonymousClassArena = la_arena::Arena<AnonymousClass>;
pub type FunctionId = la_arena::Idx<FunctionDecl>;
pub type ClassId = la_arena::Idx<ClassDef>;
pub type ClosureId = la_arena::Idx<ClosurePayload>;
pub type AnonymousClassId = la_arena::Idx<AnonymousClass>;

/// Source locations for one doc comment or a group of consecutive comments.
/// A single comment is stored inline; only grouped comments need shared
/// range storage.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DocCommentRanges {
    One(Span),
    Group(Arc<[Span]>),
    /// A doc comment whose source text was extended by a parser-time
    /// transformation. Keep the original spans for navigation while the
    /// owner text carries the final annotation payload.
    Appended {
        spans: Arc<[Span]>,
        text: Arc<str>,
    },
}

impl DocCommentRanges {
    pub fn one(span: Span) -> Self {
        Self::One(span)
    }

    pub fn group(spans: impl Into<Arc<[Span]>>) -> Self {
        let spans = spans.into();
        assert!(spans.len() > 1, "a doc group has multiple comments");
        Self::Group(spans)
    }

    pub fn spans(&self) -> &[Span] {
        match self {
            Self::One(span) => std::slice::from_ref(span),
            Self::Group(spans) => spans,
            Self::Appended { spans, .. } => spans,
        }
    }

    /// Resolves these source ranges to the text consumed by docblock readers.
    pub fn text<'a>(&'a self, source: &'a [u8]) -> Option<Cow<'a, str>> {
        let text = |span: &Span| {
            let bytes = source.get(span.start as usize..span.end as usize)?;
            Some(crate::lexer::lossy(bytes))
        };
        match self {
            Self::One(span) => text(span),
            Self::Group(spans) => {
                let bodies = spans
                    .iter()
                    .map(|span| {
                        text(span).map(|text| {
                            text.trim()
                                .trim_start_matches("/**")
                                .trim_end_matches("*/")
                                .to_owned()
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(Cow::Owned(format!("/**{}\n*/", bodies.join("\n"))))
            }
            Self::Appended { text, .. } => Some(Cow::Borrowed(text)),
        }
    }

    /// Replaces the reader text with a version that includes a synthesized
    /// PHPDoc tag while preserving the source locations of the original doc.
    pub fn with_appended_text(&self, text: impl Into<Arc<str>>) -> Self {
        Self::Appended {
            spans: Arc::from(self.spans()),
            text: text.into(),
        }
    }
}

/// Storage for list elements addressed by ranges in the file-owned tree.
pub type ArgArena = la_arena::Arena<Arg>;
pub type StmtIdArena = la_arena::Arena<StmtId>;
pub type ExprIdArena = la_arena::Arena<ExprId>;
pub type DestructureItemArena = la_arena::Arena<DestructureItem>;
pub type ArrayItemArena = la_arena::Arena<ArrayItem>;
pub type ParamArena = la_arena::Arena<Param>;
pub type MatchArmArena = la_arena::Arena<MatchArm>;
pub type AttributeArena = la_arena::Arena<Attribute>;
pub type ArgRange = la_arena::IdxRange<Arg>;
pub type StmtIdRange = la_arena::IdxRange<StmtId>;
pub type ExprIdRange = la_arena::IdxRange<ExprId>;
pub type DestructureItemRange = la_arena::IdxRange<DestructureItem>;
pub type ArrayItemRange = la_arena::IdxRange<ArrayItem>;
pub type MatchArmRange = la_arena::IdxRange<MatchArm>;
pub type ElseIfRange = la_arena::IdxRange<ElseIf>;
pub type SwitchCaseRange = la_arena::IdxRange<SwitchCase>;
pub type CatchClauseRange = la_arena::IdxRange<CatchClause>;
pub type DeclareDirectiveRange = la_arena::IdxRange<DeclareDirective>;
pub type GlobalVarRange = la_arena::IdxRange<GlobalVar>;
pub type StaticVarRange = la_arena::IdxRange<StaticVarDecl>;
pub type ClassDeclRange = la_arena::IdxRange<ClassDecl>;
pub type ParamRange = la_arena::IdxRange<Param>;
pub type AttributeRange = la_arena::IdxRange<Attribute>;
pub type MethodRange = la_arena::IdxRange<MethodDecl>;
pub type QualifiedNameRange = la_arena::IdxRange<QualifiedName>;
pub type PropertyRange = la_arena::IdxRange<PropertyDecl>;
pub type ClassConstRange = la_arena::IdxRange<ClassConst>;
pub type TraitUseRange = la_arena::IdxRange<TraitUse>;
pub type EnumCaseRange = la_arena::IdxRange<EnumCase>;
pub type TraitAliasRange = la_arena::IdxRange<TraitAlias>;
pub type TraitPrecedenceRange = la_arena::IdxRange<TraitPrecedence>;
pub type UseImportRange = la_arena::IdxRange<UseImport>;
pub type ClosureUseRange = la_arena::IdxRange<ClosureUse>;
pub type TypeHintRange = la_arena::IdxRange<TypeHint>;

/// A range in the file-owned decoded byte vector. Source-backed payloads
/// continue to use [`Span`], so callers cannot confuse the two byte stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DecodedBytesRange(Span);

impl DecodedBytesRange {
    pub fn new(start: u32, end: u32) -> Self {
        Self(Span { start, end })
    }

    pub fn start(self) -> u32 {
        self.0.start
    }

    pub fn end(self) -> u32 {
        self.0.end
    }
}

/// A string literal payload: either a source slice, bytes decoded once into
/// the owning file's decoded-byte vector, or an owned value for synthetic
/// syntax built outside a parsed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringPayload {
    Source(Span),
    Decoded(DecodedBytesRange),
    Owned(PhpString),
}

impl From<PhpString> for StringPayload {
    fn from(value: PhpString) -> Self {
        Self::Owned(value)
    }
}

impl From<&str> for StringPayload {
    fn from(value: &str) -> Self {
        Self::Owned(PhpString::from(value))
    }
}

#[cfg(feature = "fuzzing")]
impl<'a> arbitrary::Arbitrary<'a> for StringPayload {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self::Owned(PhpString::arbitrary(u)?))
    }
}

/// The typed vector storage for one file's syntax tree. A worker can clear
/// this bundle between context files to retain its warmed capacities; a
/// kept tree can shrink each vector before it leaves the worker.
#[derive(Debug, Default, Clone)]
pub struct NodeArenas {
    /// Source bytes shared with the retained `ParsedFile` owner. Transient
    /// worker arenas leave this empty.
    pub source: Arc<[u8]>,
    pub exprs: ExprArena,
    pub stmts: StmtArena,
    pub functions: FunctionArena,
    pub methods: MethodArena,
    pub classes: ClassArena,
    pub closures: ClosureArena,
    pub anonymous_classes: AnonymousClassArena,
    /// File-local names assigned to anonymous classes after parsing,
    /// indexed by the same ID as `anonymous_classes`.
    anonymous_class_names: Vec<Option<Name>>,
    pub args: ArgArena,
    /// Statement IDs grouped into immutable body and statement lists.
    pub stmt_ids: StmtIdArena,
    /// Expression IDs grouped into immutable operand and part lists.
    pub expr_ids: ExprIdArena,
    /// Destructuring pattern elements grouped into contiguous lists.
    pub destructure_items: DestructureItemArena,
    pub array_items: ArrayItemArena,
    pub params: ParamArena,
    pub match_arms: MatchArmArena,
    pub elseifs: ListArena<ElseIf>,
    pub switch_cases: ListArena<SwitchCase>,
    pub catch_clauses: ListArena<CatchClause>,
    pub declare_directives: ListArena<DeclareDirective>,
    pub global_vars: ListArena<GlobalVar>,
    pub static_vars: ListArena<StaticVarDecl>,
    pub class_decls: ListArena<ClassDecl>,
    pub attributes: AttributeArena,
    pub qualified_names: ListArena<QualifiedName>,
    pub properties: ListArena<PropertyDecl>,
    pub class_consts: ListArena<ClassConst>,
    pub trait_uses: ListArena<TraitUse>,
    pub enum_cases: ListArena<EnumCase>,
    pub trait_aliases: ListArena<TraitAlias>,
    pub trait_precedences: ListArena<TraitPrecedence>,
    pub use_imports: ListArena<UseImport>,
    pub closure_uses: ListArena<ClosureUse>,
    pub type_hints: ListArena<TypeHint>,
    pub expr_spans: Vec<Span>,
    pub stmt_spans: Vec<Span>,
    /// Doc comments keyed by the source offset of the node that owns them.
    /// Unlike declaration doc ranges, these sites serve statements and
    /// parameters whose common-case nodes should not carry doc payloads.
    pub doc_sites: Vec<NodeDocSite>,
    /// Decoded literal bytes retained by this file. Unescaped payloads
    /// refer to `ParsedFile::source` instead of being copied here.
    pub decoded_bytes: Vec<u8>,
    /// File-local ID offset for parser fragments before they are appended.
    pub expr_base: u32,
    pub stmt_base: u32,
    /// File-local offsets for list arenas in interpolation parser fragments.
    pub args_base: u32,
    /// File-local offset for statement-list elements in parser fragments.
    pub stmt_ids_base: u32,
    /// File-local offset for expression-list elements in parser fragments.
    pub expr_ids_base: u32,
    /// File-local offset for destructuring element ranges in fragments.
    pub destructure_items_base: u32,
    pub array_items_base: u32,
    pub params_base: u32,
    pub match_arms_base: u32,
    pub attributes_base: u32,
    /// File-local offsets for large payload arenas in interpolation fragments.
    pub functions_base: u32,
    pub methods_base: u32,
    pub classes_base: u32,
    pub closures_base: u32,
    pub anonymous_classes_base: u32,
    /// File-local offset for decoded-byte ranges in parser fragments.
    pub decoded_bytes_base: u32,
}

/// A doc comment recorded for lookup by its owning node's start offset.
#[derive(Debug, Clone)]
pub struct NodeDocSite {
    pub pos: u32,
    pub line: u32,
    pub text: Arc<str>,
}

fn idx_with_base<T>(index: la_arena::Idx<T>, base: u32) -> la_arena::Idx<T> {
    let raw = u32::from(index.into_raw())
        .checked_add(base)
        .expect("payload arena exhausted");
    la_arena::Idx::from_raw(la_arena::RawIdx::from_u32(raw))
}

fn local_idx<T>(index: la_arena::Idx<T>, base: u32) -> Option<la_arena::Idx<T>> {
    let raw = u32::from(index.into_raw()).checked_sub(base)?;
    Some(la_arena::Idx::from_raw(la_arena::RawIdx::from_u32(raw)))
}

fn arena_get<T>(arena: &la_arena::Arena<T>, index: la_arena::Idx<T>, base: u32) -> Option<&T> {
    let local = local_idx(index, base)?;
    ((u32::from(local.into_raw()) as usize) < arena.len()).then(|| &arena[local])
}

fn arena_get_mut<T>(
    arena: &mut la_arena::Arena<T>,
    index: la_arena::Idx<T>,
    base: u32,
) -> Option<&mut T> {
    let local = local_idx(index, base)?;
    ((u32::from(local.into_raw()) as usize) < arena.len()).then(|| &mut arena[local])
}

fn range_with_base<T>(range: la_arena::IdxRange<T>, base: u32) -> la_arena::IdxRange<T> {
    let start = u32::from(range.start().into_raw())
        .checked_add(base)
        .expect("list arena exhausted");
    let end = u32::from(range.end().into_raw())
        .checked_add(base)
        .expect("list arena exhausted");
    la_arena::IdxRange::new(
        la_arena::Idx::<T>::from_raw(la_arena::RawIdx::from_u32(start))
            ..la_arena::Idx::<T>::from_raw(la_arena::RawIdx::from_u32(end)),
    )
}

fn local_range<T>(range: la_arena::IdxRange<T>, base: u32) -> la_arena::IdxRange<T> {
    let start = u32::from(range.start().into_raw())
        .checked_sub(base)
        .expect("list range belongs to an earlier arena fragment");
    let end = u32::from(range.end().into_raw())
        .checked_sub(base)
        .expect("list range belongs to an earlier arena fragment");
    la_arena::IdxRange::new(
        la_arena::Idx::<T>::from_raw(la_arena::RawIdx::from_u32(start))
            ..la_arena::Idx::<T>::from_raw(la_arena::RawIdx::from_u32(end)),
    )
}

/// A typed arena for one family of child-list elements. Fragment parsers
/// receive a fresh arena whose base follows the owning file's current
/// elements, then append their completed ranges without changing the IDs.
#[derive(Debug, Clone)]
pub struct ListArena<T> {
    values: la_arena::Arena<T>,
    base: u32,
}

impl<T> Default for ListArena<T> {
    fn default() -> Self {
        Self {
            values: la_arena::Arena::default(),
            base: 0,
        }
    }
}

impl<T> ListArena<T> {
    fn fragment(&self) -> Self {
        Self {
            values: la_arena::Arena::default(),
            base: self
                .base
                .checked_add(u32::try_from(self.values.len()).expect("list arena exhausted"))
                .expect("list arena exhausted"),
        }
    }

    fn append_fragment(&mut self, fragment: Self) {
        assert_eq!(
            self.base + self.values.len() as u32,
            fragment.base,
            "list fragment must immediately follow its owner"
        );
        self.values
            .extend(fragment.values.into_iter().map(|(_, value)| value));
    }

    pub fn alloc_many(&mut self, values: impl IntoIterator<Item = T>) -> la_arena::IdxRange<T> {
        range_with_base(self.values.alloc_many(values), self.base)
    }

    pub fn get(&self, range: &la_arena::IdxRange<T>) -> &[T] {
        &self.values[local_range(range.clone(), self.base)]
    }

    pub fn get_mut(&mut self, range: &la_arena::IdxRange<T>) -> &mut [T] {
        let local = local_range(range.clone(), self.base);
        let start = u32::from(local.start().into_raw()) as usize;
        let end = u32::from(local.end().into_raw()) as usize;
        &mut self.values.as_mut()[start..end]
    }

    pub fn clear(&mut self) {
        self.values.clear();
        self.base = 0;
    }

    fn base(&self) -> u32 {
        self.base
    }

    fn set_base(&mut self, base: u32) {
        self.base = base;
    }

    pub fn shrink_to_fit(&mut self) {
        self.values.shrink_to_fit();
    }
}

impl NodeArenas {
    /// Sizes the expression and statement storage of empty arenas for a file
    /// of `tokens` tokens, so parsing it does not regrow them.
    pub fn reserve_for_tokens(&mut self, tokens: usize) {
        if self.exprs.is_empty() && self.stmts.is_empty() {
            let exprs = tokens.div_ceil(3);
            self.exprs = ExprArena::with_capacity(exprs);
            self.expr_spans.reserve(exprs);
            let stmts = tokens.div_ceil(8);
            self.stmts = StmtArena::with_capacity(stmts);
            self.stmt_spans.reserve(stmts);
        }
    }

    /// Creates an empty append fragment whose IDs begin immediately after
    /// this owner's current nodes and payloads.
    pub fn new_fragment(&self) -> Self {
        let mut fragment = Self::default();
        fragment.expr_base = self.expr_base + self.expr_spans.len() as u32;
        fragment.stmt_base = self.stmt_base + self.stmt_spans.len() as u32;
        fragment.args_base = self.args_base + self.args.len() as u32;
        fragment.stmt_ids_base = self.stmt_ids_base + self.stmt_ids.len() as u32;
        fragment.expr_ids_base = self.expr_ids_base + self.expr_ids.len() as u32;
        fragment.destructure_items_base =
            self.destructure_items_base + self.destructure_items.len() as u32;
        fragment.array_items_base = self.array_items_base + self.array_items.len() as u32;
        fragment.params_base = self.params_base + self.params.len() as u32;
        fragment.match_arms_base = self.match_arms_base + self.match_arms.len() as u32;
        fragment.attributes_base = self.attributes_base + self.attributes.len() as u32;
        fragment.elseifs = self.elseifs.fragment();
        fragment.switch_cases = self.switch_cases.fragment();
        fragment.catch_clauses = self.catch_clauses.fragment();
        fragment.declare_directives = self.declare_directives.fragment();
        fragment.global_vars = self.global_vars.fragment();
        fragment.static_vars = self.static_vars.fragment();
        fragment.class_decls = self.class_decls.fragment();
        fragment.qualified_names = self.qualified_names.fragment();
        fragment.properties = self.properties.fragment();
        fragment.class_consts = self.class_consts.fragment();
        fragment.trait_uses = self.trait_uses.fragment();
        fragment.enum_cases = self.enum_cases.fragment();
        fragment.trait_aliases = self.trait_aliases.fragment();
        fragment.trait_precedences = self.trait_precedences.fragment();
        fragment.use_imports = self.use_imports.fragment();
        fragment.closure_uses = self.closure_uses.fragment();
        fragment.type_hints = self.type_hints.fragment();
        fragment.functions_base = self.functions_base + self.functions.len() as u32;
        fragment.methods_base = self.methods_base + self.methods.len() as u32;
        fragment.classes_base = self.classes_base + self.classes.len() as u32;
        fragment.closures_base = self.closures_base + self.closures.len() as u32;
        fragment.anonymous_classes_base =
            self.anonymous_classes_base + self.anonymous_classes.len() as u32;
        fragment.decoded_bytes_base = self.decoded_bytes_base + self.decoded_bytes.len() as u32;
        fragment
    }

    /// The next expression ID this file owner will allocate.
    pub fn next_expr_id(&self) -> ExprId {
        ExprId::from_raw(la_arena::RawIdx::from_u32(
            self.expr_base + self.expr_spans.len() as u32,
        ))
    }

    /// The next statement ID this file owner will allocate.
    pub fn next_stmt_id(&self) -> StmtId {
        StmtId::from_raw(la_arena::RawIdx::from_u32(
            self.stmt_base + self.stmt_spans.len() as u32,
        ))
    }

    /// Reads a source-backed payload range.
    pub fn source_slice(&self, span: Span) -> Option<&[u8]> {
        self.source.get(span.start as usize..span.end as usize)
    }

    /// Resolves source-backed doc-comment ranges to their reader text.
    pub fn doc_comment_text<'a>(&'a self, ranges: &'a DocCommentRanges) -> Option<Cow<'a, str>> {
        ranges.text(&self.source)
    }

    /// Appends decoded bytes and returns their file-local range.
    pub fn alloc_decoded_bytes(&mut self, bytes: &[u8]) -> DecodedBytesRange {
        let start = self
            .decoded_bytes_base
            .checked_add(
                u32::try_from(self.decoded_bytes.len()).expect("decoded byte vector exhausted"),
            )
            .expect("decoded byte vector exhausted");
        let end = start
            .checked_add(u32::try_from(bytes.len()).expect("decoded payload too large"))
            .expect("decoded byte vector exhausted");
        self.decoded_bytes.extend_from_slice(bytes);
        DecodedBytesRange(Span { start, end })
    }

    /// Reads decoded bytes by their file-local range.
    pub fn decoded_bytes(&self, range: DecodedBytesRange) -> Option<&[u8]> {
        let start = range.0.start.checked_sub(self.decoded_bytes_base)? as usize;
        let end = range.0.end.checked_sub(self.decoded_bytes_base)? as usize;
        self.decoded_bytes.get(start..end)
    }

    /// Resolves literal bytes through this file's source and decoded-byte
    /// storage. Owned payloads are reserved for synthetic nodes.
    pub fn string_bytes<'a>(&'a self, payload: &'a StringPayload) -> Option<&'a [u8]> {
        match payload {
            StringPayload::Source(span) => self.source_slice(*span),
            StringPayload::Decoded(range) => self.decoded_bytes(*range),
            StringPayload::Owned(value) => Some(value.as_bytes()),
        }
    }

    /// Makes a durable PHP string only for products that outlive this file.
    pub fn owned_php_string(&self, payload: &StringPayload) -> Option<PhpString> {
        Some(PhpString::from_bytes(Arc::<[u8]>::from(
            self.string_bytes(payload)?,
        )))
    }

    pub fn alloc_function(&mut self, value: FunctionDecl) -> FunctionId {
        idx_with_base(self.functions.alloc(value), self.functions_base)
    }

    pub fn function(&self, id: FunctionId) -> Option<&FunctionDecl> {
        arena_get(&self.functions, id, self.functions_base)
    }

    pub fn function_mut(&mut self, id: FunctionId) -> Option<&mut FunctionDecl> {
        arena_get_mut(&mut self.functions, id, self.functions_base)
    }

    pub fn alloc_class(&mut self, value: ClassDef) -> ClassId {
        idx_with_base(self.classes.alloc(value), self.classes_base)
    }

    pub fn class(&self, id: ClassId) -> Option<&ClassDef> {
        arena_get(&self.classes, id, self.classes_base)
    }

    pub fn class_mut(&mut self, id: ClassId) -> Option<&mut ClassDef> {
        arena_get_mut(&mut self.classes, id, self.classes_base)
    }

    pub fn alloc_methods(&mut self, values: impl IntoIterator<Item = MethodDecl>) -> MethodRange {
        range_with_base(self.methods.alloc_many(values), self.methods_base)
    }

    pub fn methods(&self, range: MethodRange) -> &[MethodDecl] {
        &self.methods[local_range(range, self.methods_base)]
    }

    pub fn alloc_closure(&mut self, value: ClosurePayload) -> ClosureId {
        idx_with_base(self.closures.alloc(value), self.closures_base)
    }

    pub fn closure(&self, id: ClosureId) -> Option<&ClosurePayload> {
        arena_get(&self.closures, id, self.closures_base)
    }

    pub fn closure_mut(&mut self, id: ClosureId) -> Option<&mut ClosurePayload> {
        arena_get_mut(&mut self.closures, id, self.closures_base)
    }

    pub fn alloc_anonymous_class(&mut self, value: AnonymousClass) -> AnonymousClassId {
        let id = idx_with_base(
            self.anonymous_classes.alloc(value),
            self.anonymous_classes_base,
        );
        self.anonymous_class_names.push(None);
        id
    }

    pub fn anonymous_class(&self, id: AnonymousClassId) -> Option<&AnonymousClass> {
        arena_get(&self.anonymous_classes, id, self.anonymous_classes_base)
    }

    pub fn anonymous_class_name(&self, id: AnonymousClassId) -> Option<&Name> {
        let local = local_idx(id, self.anonymous_classes_base)?;
        self.anonymous_class_names
            .get(u32::from(local.into_raw()) as usize)?
            .as_ref()
    }

    /// Assigns the post-parse synthetic name for one anonymous-class node.
    pub fn set_anonymous_class_name(&mut self, id: AnonymousClassId, name: Name) -> bool {
        let Some(local) = local_idx(id, self.anonymous_classes_base) else {
            return false;
        };
        let Some(slot) = self
            .anonymous_class_names
            .get_mut(u32::from(local.into_raw()) as usize)
        else {
            return false;
        };
        *slot = Some(name);
        true
    }

    /// Commits a complete argument list contiguously and returns its range.
    /// Parser callers build the temporary list before this call so a kept
    /// tree never grows an argument list one element at a time.
    pub fn alloc_args(&mut self, values: impl IntoIterator<Item = Arg>) -> ArgRange {
        range_with_base(self.args.alloc_many(values), self.args_base)
    }

    /// Reads a committed argument list from this file's arena.
    #[inline]
    pub fn args(&self, range: ArgRange) -> &[Arg] {
        &self.args[local_range(range, self.args_base)]
    }

    /// Commits one complete statement-ID list and returns its owner-local range.
    pub fn alloc_stmt_ids(&mut self, values: impl IntoIterator<Item = StmtId>) -> StmtIdRange {
        range_with_base(self.stmt_ids.alloc_many(values), self.stmt_ids_base)
    }

    /// Reads a statement-ID list committed to this file's arena.
    pub fn stmt_ids(&self, range: &StmtIdRange) -> &[StmtId] {
        &self.stmt_ids[local_range(range.clone(), self.stmt_ids_base)]
    }

    /// Commits a complete expression-ID list contiguously and returns its range.
    pub fn alloc_expr_ids(&mut self, values: impl IntoIterator<Item = ExprId>) -> ExprIdRange {
        range_with_base(self.expr_ids.alloc_many(values), self.expr_ids_base)
    }

    /// Reads an expression-ID list committed to this file's arena.
    pub fn expr_ids(&self, range: &ExprIdRange) -> &[ExprId] {
        &self.expr_ids[local_range(range.clone(), self.expr_ids_base)]
    }

    /// Commits a complete destructuring pattern contiguously and returns its range.
    pub fn alloc_destructure_items(
        &mut self,
        values: impl IntoIterator<Item = DestructureItem>,
    ) -> DestructureItemRange {
        range_with_base(
            self.destructure_items.alloc_many(values),
            self.destructure_items_base,
        )
    }

    /// Reads a destructuring pattern committed to this file's arena.
    pub fn destructure_items(&self, range: &DestructureItemRange) -> &[DestructureItem] {
        &self.destructure_items[local_range(range.clone(), self.destructure_items_base)]
    }

    /// Commits a complete array-item list contiguously and returns its range.
    pub fn alloc_array_items(
        &mut self,
        values: impl IntoIterator<Item = ArrayItem>,
    ) -> la_arena::IdxRange<ArrayItem> {
        range_with_base(self.array_items.alloc_many(values), self.array_items_base)
    }

    /// Reads a committed array-item list from this file's arena.
    pub fn array_items(&self, range: la_arena::IdxRange<ArrayItem>) -> &[ArrayItem] {
        &self.array_items[local_range(range, self.array_items_base)]
    }

    /// Commits a complete parameter list contiguously and returns its range.
    pub fn alloc_params(
        &mut self,
        values: impl IntoIterator<Item = Param>,
    ) -> la_arena::IdxRange<Param> {
        range_with_base(self.params.alloc_many(values), self.params_base)
    }

    /// Reads a committed parameter list from this file's arena.
    pub fn params(&self, range: la_arena::IdxRange<Param>) -> &[Param] {
        &self.params[local_range(range, self.params_base)]
    }

    /// Mutably reads a committed parameter list during parse-time
    /// declaration normalization.
    pub fn params_mut(&mut self, range: la_arena::IdxRange<Param>) -> &mut [Param] {
        let local = local_range(range, self.params_base);
        let start = u32::from(local.start().into_raw()) as usize;
        let end = u32::from(local.end().into_raw()) as usize;
        &mut self.params.as_mut()[start..end]
    }

    /// Commits a complete match-arm list contiguously and returns its range.
    pub fn alloc_match_arms(
        &mut self,
        values: impl IntoIterator<Item = MatchArm>,
    ) -> la_arena::IdxRange<MatchArm> {
        range_with_base(self.match_arms.alloc_many(values), self.match_arms_base)
    }

    /// Reads a committed match-arm list from this file's arena.
    pub fn match_arms(&self, range: la_arena::IdxRange<MatchArm>) -> &[MatchArm] {
        &self.match_arms[local_range(range, self.match_arms_base)]
    }

    /// Commits a complete `elseif` chain contiguously and returns its range.
    pub fn alloc_elseifs(&mut self, values: impl IntoIterator<Item = ElseIf>) -> ElseIfRange {
        self.elseifs.alloc_many(values)
    }

    /// Reads a committed `elseif` chain from this file's arena.
    pub fn elseifs(&self, range: ElseIfRange) -> &[ElseIf] {
        self.elseifs.get(&range)
    }

    /// Commits a complete switch case list contiguously and returns its range.
    pub fn alloc_switch_cases(
        &mut self,
        values: impl IntoIterator<Item = SwitchCase>,
    ) -> SwitchCaseRange {
        self.switch_cases.alloc_many(values)
    }

    /// Reads a committed switch case list from this file's arena.
    pub fn switch_cases(&self, range: SwitchCaseRange) -> &[SwitchCase] {
        self.switch_cases.get(&range)
    }

    /// Commits a complete catch clause list contiguously and returns its range.
    pub fn alloc_catch_clauses(
        &mut self,
        values: impl IntoIterator<Item = CatchClause>,
    ) -> CatchClauseRange {
        self.catch_clauses.alloc_many(values)
    }

    /// Reads a committed catch clause list from this file's arena.
    pub fn catch_clauses(&self, range: CatchClauseRange) -> &[CatchClause] {
        self.catch_clauses.get(&range)
    }

    /// Commits a complete declare directive list and returns its owner range.
    pub fn alloc_declare_directives(
        &mut self,
        values: impl IntoIterator<Item = DeclareDirective>,
    ) -> DeclareDirectiveRange {
        self.declare_directives.alloc_many(values)
    }

    /// Reads a committed declare directive list from this file's arena.
    pub fn declare_directives(&self, range: DeclareDirectiveRange) -> &[DeclareDirective] {
        self.declare_directives.get(&range)
    }

    /// Commits a complete global-variable list and returns its owner range.
    pub fn alloc_global_vars(
        &mut self,
        values: impl IntoIterator<Item = GlobalVar>,
    ) -> GlobalVarRange {
        self.global_vars.alloc_many(values)
    }

    /// Reads a committed global-variable list from this file's arena.
    pub fn global_vars(&self, range: GlobalVarRange) -> &[GlobalVar] {
        self.global_vars.get(&range)
    }

    /// Commits a complete static-variable declaration list and returns its range.
    pub fn alloc_static_vars(
        &mut self,
        values: impl IntoIterator<Item = StaticVarDecl>,
    ) -> StaticVarRange {
        self.static_vars.alloc_many(values)
    }

    /// Reads a committed static-variable declaration list from this file's arena.
    pub fn static_vars(&self, range: StaticVarRange) -> &[StaticVarDecl] {
        self.static_vars.get(&range)
    }

    /// Stores one class declaration payload outside its statement node.
    pub fn alloc_class_decl(&mut self, decl: ClassDecl) -> ClassDeclRange {
        self.class_decls.alloc_many([decl])
    }

    /// Resolves a statement's one class declaration payload.
    pub fn class_decl(&self, range: ClassDeclRange) -> &ClassDecl {
        self.class_decls
            .get(&range)
            .first()
            .expect("class declaration range contains its payload")
    }

    pub(crate) fn class_decl_mut(&mut self, range: ClassDeclRange) -> &mut ClassDecl {
        self.class_decls
            .get_mut(&range)
            .first_mut()
            .expect("class declaration range contains its payload")
    }

    /// Commits a complete attribute list contiguously and returns its range.
    pub fn alloc_attributes(
        &mut self,
        values: impl IntoIterator<Item = Attribute>,
    ) -> la_arena::IdxRange<Attribute> {
        range_with_base(self.attributes.alloc_many(values), self.attributes_base)
    }

    /// Reads a committed attribute list from this file's arena.
    pub fn attributes(&self, range: la_arena::IdxRange<Attribute>) -> &[Attribute] {
        &self.attributes[local_range(range, self.attributes_base)]
    }

    pub fn alloc_qualified_names(
        &mut self,
        values: impl IntoIterator<Item = QualifiedName>,
    ) -> QualifiedNameRange {
        self.qualified_names.alloc_many(values)
    }

    pub fn qualified_names(&self, range: &QualifiedNameRange) -> &[QualifiedName] {
        self.qualified_names.get(range)
    }

    /// Canonical and visitor readers that own a copied range use this
    /// by-value form to resolve the qualified-name list.
    pub fn qualified_names_range(&self, range: QualifiedNameRange) -> &[QualifiedName] {
        self.qualified_names(&range)
    }

    /// Commits one name payload for an expression variant that must remain
    /// small in the common expression arena.
    pub fn alloc_qualified_name(&mut self, name: QualifiedName) -> QualifiedNameRange {
        self.alloc_qualified_names([name])
    }

    /// Resolves a single-name range used by compact expression variants.
    #[inline]
    pub fn qualified_name(&self, range: &QualifiedNameRange) -> Option<&QualifiedName> {
        self.qualified_names(range).first()
    }

    pub fn alloc_properties(
        &mut self,
        values: impl IntoIterator<Item = PropertyDecl>,
    ) -> PropertyRange {
        self.properties.alloc_many(values)
    }

    pub fn properties(&self, range: &PropertyRange) -> &[PropertyDecl] {
        self.properties.get(range)
    }

    pub fn properties_mut(&mut self, range: &PropertyRange) -> &mut [PropertyDecl] {
        self.properties.get_mut(range)
    }

    pub fn alloc_class_consts(
        &mut self,
        values: impl IntoIterator<Item = ClassConst>,
    ) -> ClassConstRange {
        self.class_consts.alloc_many(values)
    }

    pub fn class_consts(&self, range: &ClassConstRange) -> &[ClassConst] {
        self.class_consts.get(range)
    }

    pub fn class_consts_mut(&mut self, range: &ClassConstRange) -> &mut [ClassConst] {
        self.class_consts.get_mut(range)
    }

    pub fn alloc_trait_uses(
        &mut self,
        values: impl IntoIterator<Item = TraitUse>,
    ) -> TraitUseRange {
        self.trait_uses.alloc_many(values)
    }

    pub fn trait_uses(&self, range: &TraitUseRange) -> &[TraitUse] {
        self.trait_uses.get(range)
    }

    pub fn trait_uses_mut(&mut self, range: &TraitUseRange) -> &mut [TraitUse] {
        self.trait_uses.get_mut(range)
    }

    pub fn alloc_enum_cases(
        &mut self,
        values: impl IntoIterator<Item = EnumCase>,
    ) -> EnumCaseRange {
        self.enum_cases.alloc_many(values)
    }

    pub fn enum_cases(&self, range: &EnumCaseRange) -> &[EnumCase] {
        self.enum_cases.get(range)
    }

    pub fn alloc_trait_aliases(
        &mut self,
        values: impl IntoIterator<Item = TraitAlias>,
    ) -> TraitAliasRange {
        self.trait_aliases.alloc_many(values)
    }

    pub fn trait_aliases(&self, range: &TraitAliasRange) -> &[TraitAlias] {
        self.trait_aliases.get(range)
    }

    pub fn alloc_trait_precedences(
        &mut self,
        values: impl IntoIterator<Item = TraitPrecedence>,
    ) -> TraitPrecedenceRange {
        self.trait_precedences.alloc_many(values)
    }

    pub fn trait_precedences(&self, range: &TraitPrecedenceRange) -> &[TraitPrecedence] {
        self.trait_precedences.get(range)
    }

    pub fn alloc_use_imports(
        &mut self,
        values: impl IntoIterator<Item = UseImport>,
    ) -> UseImportRange {
        self.use_imports.alloc_many(values)
    }

    pub fn use_imports(&self, range: &UseImportRange) -> &[UseImport] {
        self.use_imports.get(range)
    }

    pub fn alloc_closure_uses(
        &mut self,
        values: impl IntoIterator<Item = ClosureUse>,
    ) -> ClosureUseRange {
        self.closure_uses.alloc_many(values)
    }

    pub fn closure_uses(&self, range: &ClosureUseRange) -> &[ClosureUse] {
        self.closure_uses.get(range)
    }

    pub fn alloc_type_hints(
        &mut self,
        values: impl IntoIterator<Item = TypeHint>,
    ) -> TypeHintRange {
        self.type_hints.alloc_many(values)
    }

    pub fn type_hints(&self, range: &TypeHintRange) -> &[TypeHint] {
        self.type_hints.get(range)
    }

    pub fn type_hints_mut(&mut self, range: &TypeHintRange) -> &mut [TypeHint] {
        self.type_hints.get_mut(range)
    }

    /// Commits an expression node and its span at the matching dense index.
    pub fn alloc_expr(&mut self, node: ExprNode, span: Span) -> ExprId {
        let id = node.id;
        assert_eq!(
            u32::from(id.into_raw()),
            self.expr_base
                .checked_add(
                    u32::try_from(self.expr_spans.len()).expect("expression arena exhausted")
                )
                .expect("expression arena exhausted")
        );
        let local_id = self.exprs.alloc(node);
        assert_eq!(u32::from(local_id.into_raw()), self.expr_spans.len() as u32);
        self.expr_spans.push(span);
        id
    }

    /// Commits a statement node and its span at the matching dense index.
    pub fn alloc_stmt(&mut self, node: StmtNode, span: Span) -> StmtId {
        let id = node.id;
        assert_eq!(
            u32::from(id.into_raw()),
            self.stmt_base
                .checked_add(
                    u32::try_from(self.stmt_spans.len()).expect("statement arena exhausted")
                )
                .expect("statement arena exhausted")
        );
        let local_id = self.stmts.alloc(node);
        assert_eq!(u32::from(local_id.into_raw()), self.stmt_spans.len() as u32);
        self.stmt_spans.push(span);
        id
    }

    /// Appends a parser fragment's nodes and side tables after its parent.
    pub fn append_fragment(&mut self, fragment: Self) {
        assert_eq!(
            self.expr_base + self.expr_spans.len() as u32,
            fragment.expr_base
        );
        assert_eq!(
            self.stmt_base + self.stmt_spans.len() as u32,
            fragment.stmt_base
        );
        assert_eq!(self.args_base + self.args.len() as u32, fragment.args_base);
        assert_eq!(
            self.stmt_ids_base + self.stmt_ids.len() as u32,
            fragment.stmt_ids_base
        );
        assert_eq!(
            self.expr_ids_base + self.expr_ids.len() as u32,
            fragment.expr_ids_base
        );
        assert_eq!(
            self.destructure_items_base + self.destructure_items.len() as u32,
            fragment.destructure_items_base
        );
        assert_eq!(
            self.array_items_base + self.array_items.len() as u32,
            fragment.array_items_base
        );
        assert_eq!(
            self.params_base + self.params.len() as u32,
            fragment.params_base
        );
        assert_eq!(
            self.match_arms_base + self.match_arms.len() as u32,
            fragment.match_arms_base
        );
        assert_eq!(
            self.attributes_base + self.attributes.len() as u32,
            fragment.attributes_base
        );
        assert_eq!(
            self.functions_base + self.functions.len() as u32,
            fragment.functions_base
        );
        assert_eq!(
            self.methods_base + self.methods.len() as u32,
            fragment.methods_base
        );
        assert_eq!(
            self.classes_base + self.classes.len() as u32,
            fragment.classes_base
        );
        assert_eq!(
            self.closures_base + self.closures.len() as u32,
            fragment.closures_base
        );
        assert_eq!(
            self.anonymous_classes_base + self.anonymous_classes.len() as u32,
            fragment.anonymous_classes_base
        );
        assert_eq!(
            self.anonymous_class_names.len(),
            self.anonymous_classes.len()
        );
        assert_eq!(
            fragment.anonymous_class_names.len(),
            fragment.anonymous_classes.len()
        );
        assert_eq!(
            self.decoded_bytes_base + self.decoded_bytes.len() as u32,
            fragment.decoded_bytes_base
        );
        self.exprs
            .extend(fragment.exprs.into_iter().map(|(_, node)| node));
        self.stmts
            .extend(fragment.stmts.into_iter().map(|(_, node)| node));
        self.functions
            .extend(fragment.functions.into_iter().map(|(_, node)| node));
        self.methods
            .extend(fragment.methods.into_iter().map(|(_, node)| node));
        self.classes
            .extend(fragment.classes.into_iter().map(|(_, node)| node));
        self.closures
            .extend(fragment.closures.into_iter().map(|(_, node)| node));
        self.anonymous_classes
            .extend(fragment.anonymous_classes.into_iter().map(|(_, node)| node));
        self.anonymous_class_names
            .extend(fragment.anonymous_class_names);
        self.args
            .extend(fragment.args.into_iter().map(|(_, node)| node));
        self.stmt_ids
            .extend(fragment.stmt_ids.into_iter().map(|(_, id)| id));
        self.expr_ids
            .extend(fragment.expr_ids.into_iter().map(|(_, id)| id));
        self.destructure_items
            .extend(fragment.destructure_items.into_iter().map(|(_, item)| item));
        self.array_items
            .extend(fragment.array_items.into_iter().map(|(_, node)| node));
        self.params
            .extend(fragment.params.into_iter().map(|(_, node)| node));
        self.match_arms
            .extend(fragment.match_arms.into_iter().map(|(_, node)| node));
        self.attributes
            .extend(fragment.attributes.into_iter().map(|(_, node)| node));
        self.qualified_names
            .append_fragment(fragment.qualified_names);
        self.elseifs.append_fragment(fragment.elseifs);
        self.switch_cases.append_fragment(fragment.switch_cases);
        self.catch_clauses.append_fragment(fragment.catch_clauses);
        self.declare_directives
            .append_fragment(fragment.declare_directives);
        self.global_vars.append_fragment(fragment.global_vars);
        self.static_vars.append_fragment(fragment.static_vars);
        self.class_decls.append_fragment(fragment.class_decls);
        self.properties.append_fragment(fragment.properties);
        self.class_consts.append_fragment(fragment.class_consts);
        self.trait_uses.append_fragment(fragment.trait_uses);
        self.enum_cases.append_fragment(fragment.enum_cases);
        self.trait_aliases.append_fragment(fragment.trait_aliases);
        self.trait_precedences
            .append_fragment(fragment.trait_precedences);
        self.use_imports.append_fragment(fragment.use_imports);
        self.closure_uses.append_fragment(fragment.closure_uses);
        self.type_hints.append_fragment(fragment.type_hints);
        self.expr_spans.extend(fragment.expr_spans);
        self.stmt_spans.extend(fragment.stmt_spans);
        self.doc_sites.extend(fragment.doc_sites);
        self.doc_sites.sort_by_key(|site| site.pos);
        self.decoded_bytes.extend(fragment.decoded_bytes);
    }

    /// Looks up an expression in this file's expression arena.
    #[inline]
    pub fn expr(&self, id: ExprId) -> Option<&ExprNode> {
        arena_get(&self.exprs, id, self.expr_base)
    }

    pub fn expr_mut(&mut self, id: ExprId) -> Option<&mut ExprNode> {
        let local = u32::from(id.into_raw()).checked_sub(self.expr_base)?;
        ((local as usize) < self.exprs.len())
            .then(|| &mut self.exprs[ExprId::from_raw(la_arena::RawIdx::from_u32(local))])
    }

    /// Looks up a statement in this file's statement arena.
    #[inline]
    pub fn stmt(&self, id: StmtId) -> Option<&StmtNode> {
        arena_get(&self.stmts, id, self.stmt_base)
    }

    pub fn stmt_mut(&mut self, id: StmtId) -> Option<&mut StmtNode> {
        let local = u32::from(id.into_raw()).checked_sub(self.stmt_base)?;
        ((local as usize) < self.stmts.len())
            .then(|| &mut self.stmts[StmtId::from_raw(la_arena::RawIdx::from_u32(local))])
    }

    /// Follows grouping and error-suppression wrappers to the expression
    /// they contain.
    #[inline]
    pub fn peeled(&self, mut id: ExprId) -> Option<ExprId> {
        loop {
            match &self.expr(id)?.kind {
                ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => id = *inner,
                _ => return Some(id),
            }
        }
    }

    /// Looks up an expression's source span by its file-local arena id.
    #[inline]
    pub fn expr_span(&self, id: ExprId) -> Option<Span> {
        let local = u32::from(id.into_raw()).checked_sub(self.expr_base)?;
        self.expr_spans.get(local as usize).copied()
    }

    /// Looks up a statement's source span by its file-local arena id.
    pub fn stmt_span(&self, id: StmtId) -> Option<Span> {
        let local = u32::from(id.into_raw()).checked_sub(self.stmt_base)?;
        self.stmt_spans.get(local as usize).copied()
    }

    /// Updates a statement's source span after a parser fixup.
    pub fn set_stmt_span(&mut self, id: StmtId, span: Span) {
        let local = u32::from(id.into_raw())
            .checked_sub(self.stmt_base)
            .expect("statement ID belongs to this owner arena");
        *self
            .stmt_spans
            .get_mut(local as usize)
            .expect("statement span belongs to this owner arena") = span;
    }

    /// Finds a doc comment recorded for a node that starts at `pos`.
    pub fn doc_site(&self, pos: u32) -> Option<&NodeDocSite> {
        let end = self.doc_sites.partition_point(|site| site.pos <= pos);
        end.checked_sub(1)
            .map(|index| &self.doc_sites[index])
            .filter(|site| site.pos == pos)
    }

    /// Drops the current file's nodes but keeps the worker's allocations.
    pub fn clear(&mut self) {
        self.source = Arc::from([]);
        self.exprs.clear();
        self.stmts.clear();
        self.functions.clear();
        self.methods.clear();
        self.classes.clear();
        self.closures.clear();
        self.anonymous_classes.clear();
        self.anonymous_class_names.clear();
        self.args.clear();
        self.stmt_ids.clear();
        self.expr_ids.clear();
        self.destructure_items.clear();
        self.array_items.clear();
        self.params.clear();
        self.match_arms.clear();
        self.elseifs.clear();
        self.switch_cases.clear();
        self.catch_clauses.clear();
        self.declare_directives.clear();
        self.global_vars.clear();
        self.static_vars.clear();
        self.class_decls.clear();
        self.attributes.clear();
        self.qualified_names.clear();
        self.properties.clear();
        self.class_consts.clear();
        self.trait_uses.clear();
        self.enum_cases.clear();
        self.trait_aliases.clear();
        self.trait_precedences.clear();
        self.use_imports.clear();
        self.closure_uses.clear();
        self.type_hints.clear();
        self.expr_spans.clear();
        self.stmt_spans.clear();
        self.doc_sites.clear();
        self.decoded_bytes.clear();
        self.expr_base = 0;
        self.stmt_base = 0;
        self.args_base = 0;
        self.stmt_ids_base = 0;
        self.expr_ids_base = 0;
        self.destructure_items_base = 0;
        self.array_items_base = 0;
        self.params_base = 0;
        self.match_arms_base = 0;
        self.attributes_base = 0;
        self.functions_base = 0;
        self.methods_base = 0;
        self.classes_base = 0;
        self.closures_base = 0;
        self.anonymous_classes_base = 0;
        self.decoded_bytes_base = 0;
    }

    /// Clears all node payloads while retaining the global offsets of an
    /// append fragment.
    pub(crate) fn clear_preserving_bases(&mut self) {
        let bases = (
            self.expr_base,
            self.stmt_base,
            self.args_base,
            self.stmt_ids_base,
            self.expr_ids_base,
            self.destructure_items_base,
            self.array_items_base,
            self.params_base,
            self.match_arms_base,
            self.attributes_base,
            self.functions_base,
            self.methods_base,
            self.classes_base,
            self.closures_base,
            self.anonymous_classes_base,
            self.decoded_bytes_base,
            self.qualified_names.base(),
            self.properties.base(),
            self.class_consts.base(),
            self.trait_uses.base(),
            self.enum_cases.base(),
            self.trait_aliases.base(),
            self.trait_precedences.base(),
            self.use_imports.base(),
            self.closure_uses.base(),
            self.type_hints.base(),
            self.elseifs.base(),
            self.switch_cases.base(),
            self.catch_clauses.base(),
            self.declare_directives.base(),
            self.global_vars.base(),
            self.static_vars.base(),
            self.class_decls.base(),
        );
        self.clear();
        let (
            expr_base,
            stmt_base,
            args_base,
            stmt_ids_base,
            expr_ids_base,
            destructure_items_base,
            array_items_base,
            params_base,
            match_arms_base,
            attributes_base,
            functions_base,
            methods_base,
            classes_base,
            closures_base,
            anonymous_classes_base,
            decoded_bytes_base,
            qualified_names_base,
            properties_base,
            class_consts_base,
            trait_uses_base,
            enum_cases_base,
            trait_aliases_base,
            trait_precedences_base,
            use_imports_base,
            closure_uses_base,
            type_hints_base,
            elseifs_base,
            switch_cases_base,
            catch_clauses_base,
            declare_directives_base,
            global_vars_base,
            static_vars_base,
            class_decls_base,
        ) = bases;
        self.expr_base = expr_base;
        self.stmt_base = stmt_base;
        self.args_base = args_base;
        self.stmt_ids_base = stmt_ids_base;
        self.expr_ids_base = expr_ids_base;
        self.destructure_items_base = destructure_items_base;
        self.array_items_base = array_items_base;
        self.params_base = params_base;
        self.match_arms_base = match_arms_base;
        self.attributes_base = attributes_base;
        self.functions_base = functions_base;
        self.methods_base = methods_base;
        self.classes_base = classes_base;
        self.closures_base = closures_base;
        self.anonymous_classes_base = anonymous_classes_base;
        self.decoded_bytes_base = decoded_bytes_base;
        self.qualified_names.set_base(qualified_names_base);
        self.properties.set_base(properties_base);
        self.class_consts.set_base(class_consts_base);
        self.trait_uses.set_base(trait_uses_base);
        self.enum_cases.set_base(enum_cases_base);
        self.trait_aliases.set_base(trait_aliases_base);
        self.trait_precedences.set_base(trait_precedences_base);
        self.use_imports.set_base(use_imports_base);
        self.closure_uses.set_base(closure_uses_base);
        self.type_hints.set_base(type_hints_base);
        self.elseifs.set_base(elseifs_base);
        self.switch_cases.set_base(switch_cases_base);
        self.catch_clauses.set_base(catch_clauses_base);
        self.declare_directives.set_base(declare_directives_base);
        self.global_vars.set_base(global_vars_base);
        self.static_vars.set_base(static_vars_base);
        self.class_decls.set_base(class_decls_base);
    }

    /// Compacts a completed tree before its arenas leave the worker.
    pub fn shrink_to_fit(&mut self) {
        self.exprs.shrink_to_fit();
        self.stmts.shrink_to_fit();
        self.functions.shrink_to_fit();
        self.methods.shrink_to_fit();
        self.classes.shrink_to_fit();
        self.closures.shrink_to_fit();
        self.anonymous_classes.shrink_to_fit();
        self.anonymous_class_names.shrink_to_fit();
        self.args.shrink_to_fit();
        self.stmt_ids.shrink_to_fit();
        self.expr_ids.shrink_to_fit();
        self.destructure_items.shrink_to_fit();
        self.array_items.shrink_to_fit();
        self.params.shrink_to_fit();
        self.match_arms.shrink_to_fit();
        self.elseifs.shrink_to_fit();
        self.switch_cases.shrink_to_fit();
        self.catch_clauses.shrink_to_fit();
        self.declare_directives.shrink_to_fit();
        self.global_vars.shrink_to_fit();
        self.static_vars.shrink_to_fit();
        self.class_decls.shrink_to_fit();
        self.attributes.shrink_to_fit();
        self.qualified_names.shrink_to_fit();
        self.properties.shrink_to_fit();
        self.class_consts.shrink_to_fit();
        self.trait_uses.shrink_to_fit();
        self.enum_cases.shrink_to_fit();
        self.trait_aliases.shrink_to_fit();
        self.trait_precedences.shrink_to_fit();
        self.use_imports.shrink_to_fit();
        self.closure_uses.shrink_to_fit();
        self.type_hints.shrink_to_fit();
        self.expr_spans.shrink_to_fit();
        self.stmt_spans.shrink_to_fit();
        self.doc_sites.sort_by_key(|site| site.pos);
        self.doc_sites.shrink_to_fit();
        self.decoded_bytes.shrink_to_fit();
    }
}

/// The identity used for an expression synthesized outside a parsed file.
pub const SYNTHETIC_EXPR_ID: ExprId = ExprId::from_raw(la_arena::RawIdx::from_u32(u32::MAX));

/// The identity used for a statement synthesized outside a parsed file.
pub const SYNTHETIC_STMT_ID: StmtId = StmtId::from_raw(la_arena::RawIdx::from_u32(u32::MAX));

#[derive(Debug, Clone)]
pub enum StmtKind {
    InlineHtml(Span),
    Expression(ExprId),
    Echo(ExprIdRange),
    Return(Option<ExprId>),
    Block(StmtIdRange),
    If(IfStmt),
    While {
        cond: ExprId,
        body: StmtId,
    },
    DoWhile {
        body: StmtId,
        cond: ExprId,
    },
    For {
        init: ExprIdRange,
        cond: ExprIdRange,
        step: ExprIdRange,
        body: StmtId,
    },
    Foreach {
        subject: ExprId,
        key: Option<ExprId>,
        value: ExprId,
        by_ref: bool,
        body: StmtId,
    },
    // `declare(name=value, ...)`: each directive's name and value, and
    // the statements of a `declare(...) { ... }` or `declare(...): ...
    // enddeclare;` block.
    Declare {
        directives: DeclareDirectiveRange,
        body: Option<StmtIdRange>,
    },
    Break(Option<i64>),
    Continue(Option<i64>),
    Global(GlobalVarRange),
    // `unset($a, $b[1], ...)`.
    Unset(ExprIdRange),
    FunctionDecl(FunctionId),
    ClassDecl(ClassDeclRange),
    Namespace(Name),
    // Every import a `use` statement brings in - a plain `use A, B;`
    // list and a group `use Foo\{A, B as C};` both expand to one
    // `UseImport` per name here.
    Use(UseImportRange),
    // Top-level `const NAME = value, ...;`, distinct from a class's own
    // consts (ClassConst is reused for the shape, since it's identical).
    GlobalConst(ClassConstRange),
    // `static $x = 1, $y;` - a function-local static variable declaration
    // (name, optional initializer). Persistence across calls isn't
    // modeled - only its scoping (the variable is defined from here on)
    // matters for a single-call static analysis pass. The name is
    // without its `$`.
    StaticVar(StaticVarRange),
    Switch {
        subject: ExprId,
        cases: SwitchCaseRange,
    },
    Try {
        body: StmtIdRange,
        catches: CatchClauseRange,
        finally: Option<StmtIdRange>,
    },
    Throw(ExprId),
    Goto(Ident),
    // `label:` - a goto target. Control flow through goto/labels isn't
    // modeled (no effect on narrowing, terminal-statement detection,
    // etc.) - only enough structure to parse past them.
    Label(Ident),
}

/// One `name=value` pair inside a `declare(...)` statement.
#[derive(Debug, Clone)]
pub struct DeclareDirective {
    pub name: Ident,
    pub value: ExprId,
}

/// `if (cond) then elseif (…) … else …`. An `elseif` is part of the chain;
/// `else if` is an `else` whose statement is another `if`.
#[derive(Debug, Clone)]
pub struct IfStmt {
    pub cond: ExprId,
    pub then_branch: StmtId,
    pub elseifs: ElseIfRange,
    pub else_branch: Option<StmtId>,
}

/// One `elseif (cond) body` of an `if` chain.
#[derive(Debug, Clone)]
pub struct ElseIf {
    pub cond: ExprId,
    pub body: StmtId,
    pub span: Span,
}

impl IfStmt {
    /// Every condition of the chain in order: the `if`'s, then each
    /// `elseif`'s.
    pub fn conditions<'a>(&'a self, arenas: &'a NodeArenas) -> impl Iterator<Item = ExprId> + 'a {
        std::iter::once(self.cond)
            .chain(arenas.elseifs(self.elseifs.clone()).iter().map(|e| e.cond))
    }

    /// Every branch body in order: the `if`'s, each `elseif`'s, then the
    /// `else`.
    pub fn bodies<'a>(&'a self, arenas: &'a NodeArenas) -> impl Iterator<Item = StmtId> + 'a {
        std::iter::once(self.then_branch)
            .chain(arenas.elseifs(self.elseifs.clone()).iter().map(|e| e.body))
            .chain(self.else_branch)
    }
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub test: Option<ExprId>, // None = `default:`
    pub body: StmtIdRange,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CatchClause {
    pub types: QualifiedNameRange,
    // The bound variable without its `$`; `None` for `catch (Foo) {}`.
    pub var: Option<Ident>,
    pub body: StmtIdRange,
    pub span: Span,
}

/// A constructor parameter promoted to a property (`private int $id`,
/// `readonly int $id`): the property's modifiers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Promotion {
    // Public when only `readonly`, `final` or a set visibility promotes.
    pub visibility: Visibility,
    // `private(set)`: who may write the property.
    pub set_visibility: Option<Visibility>,
    pub readonly: bool,
    // `final` - valid PHP, unlike `abstract`.
    pub is_final: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    // Moved out of the lexer's interned `Token::Variable`, without its `$`.
    pub name: Ident,
    pub type_hint: Option<TypeHint>,
    /// The selected `LanguageLevelTypeAware` type, retained independently
    /// of the synthesized native hint so declaration readers can honor the
    /// override even when reconciling pre-existing PHPDoc.
    pub type_aware_override: Option<String>,
    pub default: Option<ExprId>,
    pub by_ref: bool,
    pub variadic: bool,
    // The property a constructor parameter promotes to, e.g.
    // `public function __construct(private int $id) {}`.
    pub promotion: Option<Promotion>,
    // A promoted property's own hooks, defined inline in the constructor's
    // parameter list rather than on an ordinary property declaration.
    // Boxed because PropertyHooks::set itself embeds a Param - unboxed,
    // Param would have infinite size.
    pub hooks: Option<Box<PropertyHooks>>,
    // `#[Attr]` groups written directly on this parameter
    // (`function f(#[SensitiveParameter] string $password) {}`) - empty
    // for the overwhelming majority of parameters, which have none.
    pub attrs: AttributeRange,
    pub span: Span,
}

/// A label some `goto` of a function body names. `backward` is set when a
/// `goto` that comes after the label in the source jumps to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GotoLabel {
    pub name: crate::name::Name,
    pub backward: bool,
}

/// A function's `{ ... }` body.
#[derive(Debug, Clone)]
pub struct FunctionBody {
    // The opening `{`.
    pub open: Span,
    pub stmts: StmtIdRange,
    // The labels its `goto`s name, outside nested functions and classes;
    // empty for nearly every body.
    pub goto_labels: Box<[GotoLabel]>,
    // Whether it has a `yield` or `yield from` of its own, outside nested
    // functions and classes: the function is a generator.
    pub is_generator: bool,
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: Ident,
    // The name with the file's namespace, for a function declared as a
    // statement; `None` for a method. Filled in by name resolution.
    pub namespaced_name: Option<Arc<str>>,
    pub params: ParamRange,
    pub return_type: Option<TypeHint>,
    // `None` for a bodiless `function f();` declaration (abstract and
    // interface methods), distinct from an empty `{}` body.
    pub body: Option<FunctionBody>,
    // `function &f()` - returns by reference.
    pub by_ref: bool,
    // Raw `/** ... */` text immediately preceding this declaration, if any.
    pub doc_comment: Option<DocCommentRanges>,
    // `#[Attr]` groups preceding this declaration - shared by a
    // top-level function and (via `MethodDecl.function`) a class method,
    // since real PHP attaches attributes to the same source position
    // either way (before any `public`/`static`/... modifiers).
    pub attrs: AttributeRange,
    pub span: Span,
}

impl FunctionDecl {
    /// The body's statements; none for a bodiless declaration.
    pub fn body_stmts<'a>(&self, arenas: &'a NodeArenas) -> &'a [StmtId] {
        match self.body.as_ref() {
            Some(body) => arenas.stmt_ids(&body.stmts),
            None => &[],
        }
    }

    /// The fully-qualified name a function statement declares; a method's
    /// name.
    pub fn fq_name(&self) -> &str {
        self.namespaced_name.as_deref().unwrap_or(&self.name)
    }

    /// Whether the body has a `yield` of its own: the function is a
    /// generator.
    pub fn is_generator(&self) -> bool {
        self.body.as_ref().is_some_and(|body| body.is_generator)
    }

    /// Whether this is a `__construct` method (PHP names are
    /// case-insensitive).
    pub fn is_constructor(&self) -> bool {
        is_constructor_name(&self.name)
    }
}

/// Whether a method name is `__construct`, compared as PHP does.
pub fn is_constructor_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("__construct")
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

#[derive(Debug, Clone)]
pub struct PropertyDecl {
    // Without its `$`.
    pub name: Ident,
    pub visibility: Visibility,
    // PHP 8.4 asymmetric visibility (`public private(set) string $x;`) -
    // the narrower write-side visibility, when one was declared
    // separately from `visibility` above (which always stays the
    // *read*-side visibility). `None` for an ordinary, symmetric
    // property, in which case a write is checked against `visibility`
    // itself.
    pub write_visibility: Option<Visibility>,
    pub is_static: bool,
    pub is_readonly: bool,
    // PHP 8.4 `final`/`abstract` property modifiers -
    // `is_abstract_member`/`is_final_member` are also parsed for
    // `MethodDecl`/`ClassConst`, threaded through here for a plain
    // property declaration too.
    pub is_abstract: bool,
    pub is_final: bool,
    pub type_hint: Option<TypeHint>,
    pub default: Option<ExprId>,
    pub doc_comment: Option<DocCommentRanges>,
    // PHP 8.4 property hooks (`{ get => ...; set(Type $value) { ... } }`).
    // Parsed so the file doesn't fail to parse at all, but the hook bodies
    // aren't type-checked (they're arbitrary code referencing $this/$value
    // with no function-like registry entry to check them against) - out of
    // scope for now, same as FirstClassCallable's args.
    pub hooks: Option<PropertyHooks>,
    // `#[Attr]` groups preceding this property declaration.
    pub attrs: AttributeRange,
    pub span: Span,
    // What deleting this member removes (its doc comment and attributes
    // included, and its whole lines when it has them to itself); `None`
    // when it shares a declaration with other members.
    pub removal_span: Option<Span>,
}

#[derive(Debug, Clone)]
pub enum PropertyHookBody {
    // `get => expr;` / `set(...) => expr;`
    Arrow(ExprId),
    // `get { ... }` / `set(...) { ... }`
    Block(StmtIdRange),
    // `get;` / `set(...);` - a hook declared, with no
    // body at all (only legal inside an abstract class/interface). Real
    // PHP distinguishes this from "no hook declared" (`PropertyHooks::
    // get`/`set` staying `None`) - `hasHook()` needs to answer "is a
    // hook declared at all," independent of whether it has a body.
    Abstract,
}

#[derive(Debug, Clone)]
pub struct PropertyHooks {
    pub get: Option<PropertyHook>,
    pub set: Option<PropertyHook>,
}

impl PropertyHooks {
    /// The hooks declared, `get` first.
    pub fn iter(&self) -> impl Iterator<Item = &PropertyHook> {
        self.get.iter().chain(self.set.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut PropertyHook> {
        self.get.iter_mut().chain(self.set.iter_mut())
    }
}

/// One `get` or `set` hook of a property.
#[derive(Debug, Clone)]
pub struct PropertyHook {
    pub body: PropertyHookBody,
    // The labels the body's `goto`s name; see `FunctionBody`.
    pub goto_labels: Box<[GotoLabel]>,
    // Whether the body is a generator; see `FunctionBody`.
    pub is_generator: bool,
    // `&get`: the hook returns its value by reference.
    pub by_ref: bool,
    // `set(Type $value)`: the one parameter PHP allows there; `None` when
    // the hook omits it (`set { ... }` uses an implicit `$value`), and
    // always for `get`.
    pub param: Option<Param>,
    // The `#[Attr]` group(s) immediately preceding the hook's own
    // `get`/`set` keyword - see `PropertyHookAttributesRule`.
    pub attrs: AttributeRange,
    // The `/** ... */` doc comment immediately preceding the hook's own
    // keyword (after any `#[Attr]` group(s)), so a hook can carry its own
    // `@throws` tag the way a method does.
    pub doc: Option<DocCommentRanges>,
    // The hook's own `final` modifier (independent of the property's or
    // its class's finality).
    pub is_final: bool,
}

#[derive(Debug, Clone)]
pub struct MethodDecl {
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub function: FunctionDecl,
    pub span: Span,
    // What deleting this member removes (its doc comment and attributes
    // included, and its whole lines when it has them to itself); `None`
    // when it shares a declaration with other members.
    pub removal_span: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct UseImport {
    // The fully-qualified name imported, without a leading `\`.
    pub path: Name,
    // The name as written in source (a group member's own part only).
    pub written: Ident,
    // `as Alias`.
    pub alias: Option<Ident>,
    // Which of PHP's three `use`-import namespaces this member belongs
    // to (`use Foo;` vs `use function foo;` vs `use const FOO;`) - a
    // plain `use Foo;` isn't checked for existence at all, matching real
    // PHPStan's own `ExistingNamesInUseRule` (a class reference is only
    // existence-checked where it's actually *used*, not merely
    // imported). `use const` existence is also left unchecked - this
    // project has no project-wide global-constant registry to check it
    // against.
    pub kind: UseKind,
}

/// See `UseImport::kind`'s own doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UseKind {
    Class,
    Function,
    Const,
}

#[derive(Debug, Clone)]
pub struct ClassConst {
    pub name: Ident,
    // The name with the file's namespace, for a global `const`; `None` in
    // a class. Filled in by name resolution.
    pub namespaced_name: Option<Arc<str>>,
    pub value: ExprId,
    // The `public`/`private`/`protected` modifier a real class's own
    // `const` declaration can carry (PHP 7.1+), needed by
    // `UnusedPrivateConstantRule` to tell a private constant candidate
    // apart from a public/protected one.
    // `Visibility::Public` for the top-level (non-class) `const NAME =
    // value;` statement, which reuses this same struct shape (see
    // `StmtKind::GlobalConst`'s own doc comment) but has no visibility
    // concept of its own - a top-level const is never a legal "unused
    // private constant" candidate, so this default is never actually
    // read by that check.
    pub visibility: Visibility,
    // A real, common PHPDoc convention for pinning a `const array`'s own
    // element type more precisely than its native declaration ever can
    // (e.g. `@var list<class-string<MiddlewareInterface>>` on a `public
    // const array X = [...]`) - `None` for a const with no doc comment
    // directly above it (the common case) or one parsed via the rarer
    // top-level (non-class) `const` statement path, which doesn't thread
    // a doc comment through at all.
    pub doc_comment: Option<DocCommentRanges>,
    // `#[Attr]` groups preceding this const declaration - the
    // same group applies to every name in a comma-separated
    // `const A = 1, B = 2;` list, mirroring how `visibility`/`doc_comment`
    // are already cloned onto each one.
    pub attrs: AttributeRange,
    // PHP 8.3 typed class constants (`const TYPE NAME = value;`) - same
    // "one type applies to every name in a comma-separated list" cloning
    // as `visibility`/`doc_comment`/`attrs` above.
    // Cloned onto every name in a comma-separated list.
    pub type_hint: Option<TypeHint>,
    // PHP 8.1+ `final const`, needed by
    // `FinalConstantRule`/`FinalPrivateConstantRule` to tell a `final`
    // constant apart from an ordinary one. Always `false` for the
    // top-level (non-class) `const NAME = value;` statement - real PHP
    // has no `final` syntax there at all.
    pub is_final: bool,
    pub span: Span,
    // What deleting this member removes (its doc comment and attributes
    // included, and its whole lines when it has them to itself); `None`
    // when it shares a declaration with other members.
    pub removal_span: Option<Span>,
}

impl ClassConst {
    /// The fully-qualified name a global `const` declares; a class
    /// constant's name.
    pub fn fq_name(&self) -> &str {
        self.namespaced_name.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ClassKind {
    Class,
    Interface,
    Trait,
    Enum,
}

/// A class, interface, trait or enum declaration statement: a name bound
/// to a class definition.
#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: Ident,
    // The name with the file's namespace. The name as written until name
    // resolution fills it in.
    pub namespaced_name: Arc<str>,
    pub def: ClassId,
}

impl ClassDecl {
    /// The fully-qualified name it declares.
    pub fn fq_name(&self) -> &str {
        &self.namespaced_name
    }
}

/// `new class(...) [extends X] [implements Y] { ... }`: a class
/// definition with no name in the source, and its constructor arguments.
#[derive(Debug, Clone)]
pub struct AnonymousClass {
    pub def: ClassId,
    pub args: ArgRange,
}

/// A class-like's definition: everything but its name, shared by a
/// declaration statement and an anonymous class (`new class { ... }`).
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub kind: ClassKind,
    // An enum's backing type (`int` or `string`); `None` for a pure enum
    // and every other class-like.
    pub backing_type: Option<TypeHint>,
    pub extends: QualifiedNameRange,
    pub implements: QualifiedNameRange,
    pub is_abstract: bool,
    pub is_final: bool,
    // `readonly class` (PHP 8.2), surfaced at the class level (in
    // addition to being spread onto each own property in
    // `parse_class_body`) for `check_class_like_marker_attributes`'s own
    // `#[\AllowDynamicProperties]`-on-a-readonly-class check (real
    // PHPStan's own `ClassAttributesRule`).
    pub is_readonly: bool,
    pub properties: PropertyRange,
    pub methods: MethodRange,
    pub consts: ClassConstRange,
    // Each `use TraitA, TraitB { ... }` statement in the body.
    pub trait_uses: TraitUseRange,
    // `case` members, which PHP parses in any class-like body but only
    // allows in an enum (`enum.caseOutsideOfEnum`).
    pub enum_cases: EnumCaseRange,
    // The class's own `/** ... */` doc comment, if any - needed for its
    // own `@template T` declarations (class-level generics).
    pub doc_comment: Option<DocCommentRanges>,
    // `#[Attr]` groups preceding this class/interface/trait
    // declaration (e.g. `#[Attribute(Attribute::TARGET_CLASS)]`, marking
    // the class itself usable as an attribute).
    pub attrs: AttributeRange,
    pub span: Span,
}

impl ClassDef {
    pub fn extends<'a>(&self, arenas: &'a NodeArenas) -> &'a [QualifiedName] {
        arenas.qualified_names(&self.extends)
    }

    pub fn implements<'a>(&self, arenas: &'a NodeArenas) -> &'a [QualifiedName] {
        arenas.qualified_names(&self.implements)
    }

    pub fn properties<'a>(&self, arenas: &'a NodeArenas) -> &'a [PropertyDecl] {
        arenas.properties(&self.properties)
    }

    pub fn consts<'a>(&self, arenas: &'a NodeArenas) -> &'a [ClassConst] {
        arenas.class_consts(&self.consts)
    }

    pub fn trait_uses<'a>(&self, arenas: &'a NodeArenas) -> &'a [TraitUse] {
        arenas.trait_uses(&self.trait_uses)
    }

    pub fn enum_cases<'a>(&self, arenas: &'a NodeArenas) -> &'a [EnumCase] {
        arenas.enum_cases(&self.enum_cases)
    }

    /// Its `__construct` method, matched case-insensitively.
    pub fn constructor<'a>(&self, arenas: &'a NodeArenas) -> Option<&'a MethodDecl> {
        arenas
            .methods(self.methods.clone())
            .iter()
            .find(|m| m.function.is_constructor())
    }

    /// Every trait its `use` statements name, in source order.
    pub fn used_traits<'a>(
        &'a self,
        arenas: &'a NodeArenas,
    ) -> impl Iterator<Item = &'a QualifiedName> + 'a {
        arenas
            .trait_uses(&self.trait_uses)
            .iter()
            .flat_map(|u| arenas.qualified_names(&u.traits))
    }

    /// Every `as` clause of its `use` statements.
    pub fn trait_aliases<'a>(
        &'a self,
        arenas: &'a NodeArenas,
    ) -> impl Iterator<Item = &'a TraitAlias> + 'a {
        arenas
            .trait_uses(&self.trait_uses)
            .iter()
            .flat_map(|u| arenas.trait_aliases(&u.adaptations.aliases))
    }

    /// Every `insteadof` clause of its `use` statements.
    pub fn trait_precedences<'a>(
        &'a self,
        arenas: &'a NodeArenas,
    ) -> impl Iterator<Item = &'a TraitPrecedence> + 'a {
        arenas
            .trait_uses(&self.trait_uses)
            .iter()
            .flat_map(|u| arenas.trait_precedences(&u.adaptations.precedences))
    }

    /// Every method name an adaptation clause of its `use` statements
    /// mentions.
    pub fn resolved_trait_conflicts<'a>(
        &'a self,
        arenas: &'a NodeArenas,
    ) -> impl Iterator<Item = &'a Name> + 'a {
        arenas
            .trait_uses(&self.trait_uses)
            .iter()
            .flat_map(|u| u.adaptations.mentioned_methods(arenas))
    }
}

impl TraitUse {
    pub fn traits<'a>(&self, arenas: &'a NodeArenas) -> &'a [QualifiedName] {
        arenas.qualified_names(&self.traits)
    }

    pub fn aliases<'a>(&self, arenas: &'a NodeArenas) -> &'a [TraitAlias] {
        arenas.trait_aliases(&self.adaptations.aliases)
    }

    pub fn precedences<'a>(&self, arenas: &'a NodeArenas) -> &'a [TraitPrecedence] {
        arenas.trait_precedences(&self.adaptations.precedences)
    }
}

impl TraitPrecedence {
    pub fn instead_of<'a>(&self, arenas: &'a NodeArenas) -> &'a [QualifiedName] {
        arenas.qualified_names(&self.instead_of)
    }
}

/// `use A, B;` or `use A, B { ... }` inside a class-like body.
#[derive(Debug, Clone)]
pub struct TraitUse {
    pub traits: QualifiedNameRange,
    // The statement's own doc comment, which carries `@use Trait<Args>`
    // (not the class's).
    pub doc_comment: Option<DocCommentRanges>,
    pub adaptations: TraitAdaptations,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumCase {
    pub name: Ident,
    // Some(...) for a backed enum (`case Hearts = 'H';`), None for a pure
    // one (`case Hearts;`).
    pub value: Option<ExprId>,
    // `#[Attr]` groups preceding this case.
    pub attrs: AttributeRange,
    pub span: Span,
}

/// `[Trait::]method as [visibility] [alias];` in a trait-use block.
#[derive(Debug, Clone)]
pub struct TraitAlias {
    pub trait_name: Option<QualifiedName>,
    pub method: Name,
    pub visibility: Option<Visibility>,
    pub alias: Option<Name>,
}

/// `Trait::method insteadof Other, ...;` in a trait-use block: `trait_name`'s
/// `method` wins over the one each trait in `instead_of` has.
#[derive(Debug, Clone)]
pub struct TraitPrecedence {
    pub trait_name: QualifiedName,
    pub method: Name,
    pub instead_of: QualifiedNameRange,
}

/// A trait-use block's adaptations: `B::hello as protected helloB;` and
/// `B::hello insteadof A;` clauses.
#[derive(Debug, Clone)]
pub struct TraitAdaptations {
    pub aliases: TraitAliasRange,
    pub precedences: TraitPrecedenceRange,
}

impl TraitAdaptations {
    /// Every method name a clause mentions.
    pub fn mentioned_methods<'a>(
        &'a self,
        arenas: &'a NodeArenas,
    ) -> impl Iterator<Item = &'a Name> + 'a {
        arenas
            .trait_aliases(&self.aliases)
            .iter()
            .map(|a| &a.method)
            .chain(
                arenas
                    .trait_precedences(&self.precedences)
                    .iter()
                    .map(|p| &p.method),
            )
    }
}

/// One argument in a call's own argument list - `name` is
/// `Some("matches")` for a named argument (`matches: $m`), which real PHP
/// allows out of declaration order; `spread` is `true` for `...$args`,
/// after which no later argument can be resolved to a specific declared
/// parameter (the unpacked array's contents aren't known statically).
#[derive(Debug, Clone)]
pub struct Arg {
    // `name:` for a named argument.
    pub name: Option<Ident>,
    pub spread: bool,
    pub value: ExprId,
}

/// One element of an `ExprKind::ArrayLiteral` (`[...]`) - `key` is
/// `Some(...)` for a `key => value` pair, `None` for a plain positional
/// element. `spread` is `true` for `...$other` (PHP's own array-unpacking
/// syntax) - a spread inserts an unknown number of elements, shifting
/// every subsequent positional key by an amount this checker can't know
/// statically.
#[derive(Debug, Clone)]
pub struct ArrayItem {
    pub key: Option<ExprId>,
    pub value: ExprId,
    pub spread: bool,
    /// `&$x`: the element is a reference to the variable (created as
    /// `null` if it doesn't exist).
    pub by_ref: bool,
}

/// How a destructuring pattern is spelled. PHP doesn't allow the two
/// spellings to nest in each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestructureSyntax {
    /// `[$a, $b]`
    Short,
    /// `list($a, $b)`
    List,
}

/// One element of an `ExprKind::Destructure`: where the value at `key`
/// (or at the element's position, when there is no key) goes.
#[derive(Debug, Clone)]
pub struct DestructureItem {
    pub key: Option<ExprId>,
    /// Where the value is written: a variable, property, element, or a
    /// nested pattern. `None` for a skipped slot (`[, $b]`).
    pub target: Option<ExprId>,
    /// `&$x`: the target becomes a reference to the element.
    pub by_ref: bool,
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int(i64),
    Float(f64),
    // The string's bytes: PHP strings aren't necessarily UTF-8.
    String(StringPayload),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Eq,
    Identical,
    NotEq,
    NotIdentical,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Spaceship,
    And,
    Or,
    // The word forms of &&/|| - runtime-identical to And/Or (only their
    // *precedence* differs, a parse-time concern already resolved by the
    // time this tree exists), kept as their own variants (not folded into
    // And/Or the way this comment used to argue for) because real
    // PHPStan's own identifier/message text genuinely differs by spelling
    // (`booleanAnd.*`/`logicalAnd.*`, `"&&"` vs `"and"` in the rendered
    // message) - every consumer that only cares about runtime semantics
    // matches `BinOp::And | BinOp::LogicalAnd` (`BinOp::Or | BinOp::
    // LogicalOr`) together; only the diagnostic-identifier/message sites
    // distinguish them.
    LogicalAnd,
    LogicalOr,
    // The `xor` keyword operator - logical, not `^`'s bitwise BitXor. Has
    // no symbol counterpart at all, so it never needed an And/Or-style
    // split.
    LogicalXor,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Coalesce,
    // PHP 8.5's `|>` pipe operator: `$value |> callable` is sugar for
    // `callable($value)`. Left-associative, and sits (empirically checked
    // against a real PHP 8.5 build) between concat/shift and the
    // relational operators - tighter than `<`, looser than `.`/`<<`.
    Pipe,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Plus,
    Not,
    BitNot,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Debug, Clone)]
pub struct ClosureUse {
    // The captured variable without its `$`, and its span: a `use (&$x)`
    // that auto-vivifies `$x` in the outer scope declares it there.
    pub name: Ident,
    pub by_ref: bool,
}

// The right-hand side of `instanceof`: almost always a static class name,
// but PHP also allows an arbitrary expression there (`$x instanceof
// $className`, `$x instanceof $obj`) for a runtime-determined class.
#[derive(Debug, Clone)]
pub enum InstanceofTarget {
    Name(QualifiedNameRange),
    Expr(ExprId),
}

/// `ExprKind::StaticCall`'s own receiver - `InstanceofTarget`'s identical
/// shape, reused rather than duplicated with different field names: a
/// literal class name (`Foo::method()`, still the overwhelming common
/// case) plus its own name span, or a genuinely dynamic receiver
/// expression (`$obj::method()`, `(new Foo())::method()`, `self::$
/// method()` when `$method` isn't itself the receiver but the member
/// name - only the *receiver* side is ever `ExprNode` here).
#[derive(Debug, Clone)]
pub enum StaticTarget {
    Name(Box<QualifiedName>),
    Expr(ExprId),
}

impl StaticTarget {
    /// The literal class name, or `None` for a dynamic receiver.
    pub fn as_name(&self) -> Option<&QualifiedName> {
        match self {
            StaticTarget::Name(n) => Some(n),
            StaticTarget::Expr(_) => None,
        }
    }

    /// The receiver expression, or `None` for a literal class name.
    pub fn expr(&self) -> Option<ExprId> {
        match self {
            StaticTarget::Name(..) => None,
            StaticTarget::Expr(id) => Some(*id),
        }
    }

    /// The property of a parent hook call's target, `parent::$property`
    /// in `parent::$property::get()`.
    pub fn parent_hook_property<'a>(&self, arenas: &'a NodeArenas) -> Option<&'a str> {
        match &arenas.expr(self.expr()?)?.kind {
            ExprKind::StaticPropertyFetch(StaticTarget::Name(class), property)
                if class.eq_ignore_ascii_case("parent") =>
            {
                Some(property.as_ref())
            }
            _ => None,
        }
    }
}

/// A reference to a class, function or constant by name: as written, and
/// what it refers to given the file's `namespace` and `use` statements
/// (resolved by the parser as it reads the name). Derefs to the name as
/// written.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct QualifiedName {
    pub written: Name,
    pub resolved: ResolvedName,
    pub span: Span,
}

/// What a [`QualifiedName`] refers to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ResolvedName {
    /// `self`, `parent` or `static`, a keyword type, or a name the
    /// resolution pass has not seen.
    Unresolved,
    /// The one fully-qualified name it refers to (no leading `\\`).
    Exact(Name),
    /// An unqualified function or constant name in a namespace: this
    /// namespaced name if one is declared, else the global one, which is
    /// the name as written.
    WithGlobalFallback(Name),
}

impl QualifiedName {
    pub fn new(written: impl Into<Name>, span: Span) -> QualifiedName {
        QualifiedName {
            written: written.into(),
            resolved: ResolvedName::Unresolved,
            span,
        }
    }

    /// The fully-qualified class this names; `None` for `self`, `parent`
    /// and `static`, and for a name resolution has not seen.
    pub fn resolved_class(&self) -> Option<&str> {
        match &self.resolved {
            ResolvedName::Exact(name) => Some(name.as_str()),
            _ => None,
        }
    }

    /// The class this names, as `resolved_class`, as the name it holds.
    pub fn resolved_class_name(&self) -> Option<&Name> {
        match &self.resolved {
            ResolvedName::Exact(name) => Some(name),
            _ => None,
        }
    }

    /// Whether this is `keyword` (`self`, `parent` or `static`), which
    /// PHP reads in any case.
    pub fn is_keyword(&self, keyword: &str) -> bool {
        self.written.eq_ignore_ascii_case(keyword)
    }

    /// The function or constant this names: the fully-qualified name to
    /// look up, and the global name PHP falls back to when that one isn't
    /// declared.
    pub fn candidates(&self) -> (&str, Option<&str>) {
        match &self.resolved {
            ResolvedName::Exact(name) => (name.as_str(), None),
            ResolvedName::WithGlobalFallback(name) => (name.as_str(), Some(self.written.as_str())),
            ResolvedName::Unresolved => (self.written.trim_start_matches('\\'), None),
        }
    }
}

impl std::ops::Deref for QualifiedName {
    type Target = str;

    fn deref(&self) -> &str {
        &self.written
    }
}

impl std::fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.written)
    }
}

impl PartialEq<str> for QualifiedName {
    fn eq(&self, other: &str) -> bool {
        self.written == *other
    }
}

impl PartialEq<&str> for QualifiedName {
    fn eq(&self, other: &&str) -> bool {
        self.written == **other
    }
}

/// A name written in the source - a member, method or constant name -
/// with its span. Derefs to the name.
#[derive(Debug, Clone)]
pub struct Ident {
    pub name: Name,
    /// The owning file's compact name-table index, when parsed from a file.
    pub name_id: Option<crate::name::NameId>,
    pub span: Span,
}

impl std::ops::Deref for Ident {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        &self.name
    }
}

impl PartialEq<str> for Ident {
    fn eq(&self, other: &str) -> bool {
        self.name == *other
    }
}

impl PartialEq<&str> for Ident {
    fn eq(&self, other: &&str) -> bool {
        self.name == **other
    }
}

impl AsRef<str> for Ident {
    fn as_ref(&self) -> &str {
        &self.name
    }
}

impl PartialEq<Name> for Ident {
    fn eq(&self, other: &Name) -> bool {
        self.name == *other
    }
}

impl PartialEq<Ident> for Name {
    fn eq(&self, other: &Ident) -> bool {
        *self == other.name
    }
}

impl From<&Ident> for Name {
    fn from(ident: &Ident) -> Name {
        ident.name.clone()
    }
}

impl From<Ident> for Name {
    fn from(ident: Ident) -> Name {
        ident.name
    }
}

impl std::fmt::Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// A native type declaration (`int`, `?Foo`, `A|B`, `(A&B)|null`) as
/// written, with the span of each part.
#[derive(Debug, Clone)]
pub struct TypeHint {
    pub kind: TypeHintKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeHintKind {
    /// A native type keyword (`int`, `static`, `null`), with its spelling
    /// as written (`INT`).
    Keyword(NativeType, Arc<str>),
    /// A class name, as written (`\App\Foo`, `Foo`) - including a word
    /// PHP reads as a class in a native hint (`integer`, `resource`).
    Name(QualifiedNameRange),
    /// `?T`.
    Nullable(Box<TypeHint>),
    /// `A|B`.
    Union(TypeHintRange),
    /// `A&B`, or a parenthesized `(A&B)` term of a union.
    Intersection(TypeHintRange),
}

/// A type keyword PHP recognizes in a native declaration, whatever its
/// case. Any other name in a type hint is a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeType {
    Int,
    Float,
    String,
    Bool,
    Array,
    Object,
    Mixed,
    Void,
    Never,
    Null,
    True,
    False,
    Iterable,
    Callable,
    SelfType,
    Static,
    Parent,
}

impl NativeType {
    /// The keyword `name` spells, compared case-insensitively.
    pub fn from_name(name: &str) -> Option<NativeType> {
        Some(match name.to_ascii_lowercase().as_str() {
            "int" => NativeType::Int,
            "float" => NativeType::Float,
            "string" => NativeType::String,
            "bool" => NativeType::Bool,
            "array" => NativeType::Array,
            "object" => NativeType::Object,
            "mixed" => NativeType::Mixed,
            "void" => NativeType::Void,
            "never" => NativeType::Never,
            "null" => NativeType::Null,
            "true" => NativeType::True,
            "false" => NativeType::False,
            "iterable" => NativeType::Iterable,
            "callable" => NativeType::Callable,
            "self" => NativeType::SelfType,
            "static" => NativeType::Static,
            "parent" => NativeType::Parent,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            NativeType::Int => "int",
            NativeType::Float => "float",
            NativeType::String => "string",
            NativeType::Bool => "bool",
            NativeType::Array => "array",
            NativeType::Object => "object",
            NativeType::Mixed => "mixed",
            NativeType::Void => "void",
            NativeType::Never => "never",
            NativeType::Null => "null",
            NativeType::True => "true",
            NativeType::False => "false",
            NativeType::Iterable => "iterable",
            NativeType::Callable => "callable",
            NativeType::SelfType => "self",
            NativeType::Static => "static",
            NativeType::Parent => "parent",
        }
    }
}

impl TypeHint {
    /// Whether two optional declarations are written the same (spans
    /// aside).
    pub fn same(a: Option<&TypeHint>, b: Option<&TypeHint>, arenas: &NodeArenas) -> bool {
        a.map(|hint| hint.to_string_with(arenas)) == b.map(|hint| hint.to_string_with(arenas))
    }

    /// Whether this declares `mixed`.
    pub fn is_mixed(&self, arenas: &NodeArenas) -> bool {
        match &self.kind {
            TypeHintKind::Keyword(NativeType::Mixed, _) => true,
            TypeHintKind::Nullable(inner) => inner.is_mixed(arenas),
            // A union holding `mixed` is `mixed`.
            TypeHintKind::Union(members) => arenas
                .type_hints(members)
                .iter()
                .any(|member| member.is_mixed(arenas)),
            _ => false,
        }
    }

    /// Every class the declaration names (keyword types like `int` or
    /// `static` left out), as written, in source order.
    pub fn class_names<'a>(&'a self, arenas: &'a NodeArenas) -> Vec<&'a QualifiedName> {
        let mut out = Vec::new();
        self.collect_class_names(arenas, &mut out);
        out
    }

    fn collect_class_names<'a>(&'a self, arenas: &'a NodeArenas, out: &mut Vec<&'a QualifiedName>) {
        match &self.kind {
            TypeHintKind::Keyword(..) => {}
            TypeHintKind::Name(name) => out.extend(arenas.qualified_names(name)),
            TypeHintKind::Nullable(inner) => inner.collect_class_names(arenas, out),
            TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => {
                for member in arenas.type_hints(members) {
                    member.collect_class_names(arenas, out);
                }
            }
        }
    }

    pub fn to_string_with(&self, arenas: &NodeArenas) -> String {
        format!("{}", TypeHintDisplay { hint: self, arenas })
    }
}

pub struct TypeHintDisplay<'a> {
    hint: &'a TypeHint,
    arenas: &'a NodeArenas,
}

impl std::fmt::Display for TypeHintDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn write_hint(
            hint: &TypeHint,
            arenas: &NodeArenas,
            f: &mut std::fmt::Formatter<'_>,
        ) -> std::fmt::Result {
            match &hint.kind {
                TypeHintKind::Keyword(_, written) => f.write_str(written),
                TypeHintKind::Name(name) => f.write_str(
                    arenas
                        .qualified_name(name)
                        .expect("type-name range contains its qualified name"),
                ),
                TypeHintKind::Nullable(inner) => {
                    f.write_str("?")?;
                    write_hint(inner, arenas, f)
                }
                TypeHintKind::Union(members) => {
                    for (i, member) in arenas.type_hints(members).iter().enumerate() {
                        if i > 0 {
                            f.write_str("|")?;
                        }
                        match &member.kind {
                            TypeHintKind::Intersection(_) => {
                                f.write_str("(")?;
                                write_hint(member, arenas, f)?;
                                f.write_str(")")?;
                            }
                            _ => write_hint(member, arenas, f)?,
                        }
                    }
                    Ok(())
                }
                TypeHintKind::Intersection(members) => {
                    for (i, member) in arenas.type_hints(members).iter().enumerate() {
                        if i > 0 {
                            f.write_str("&")?;
                        }
                        write_hint(member, arenas, f)?;
                    }
                    Ok(())
                }
            }
        }
        write_hint(self.hint, self.arenas, f)
    }
}

/// One variable a `global` statement imports.
#[derive(Debug, Clone)]
pub enum GlobalVar {
    /// `global $name;` (the name is without its `$`).
    Name(Ident),
    /// `global ${expr};` / `global $$name;`: the name is computed.
    Dynamic(ExprId),
}

impl GlobalVar {
    /// The variable's name, when it is written out.
    pub fn name(&self) -> Option<&Name> {
        match self {
            GlobalVar::Name(ident) => Some(&ident.name),
            GlobalVar::Dynamic(_) => None,
        }
    }
}

/// One function-local static variable and its optional initializer.
#[derive(Debug, Clone)]
pub struct StaticVarDecl {
    pub name: Ident,
    pub default: Option<ExprId>,
}

/// How an `ExprKind::Exit` is spelled; the two are the same construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    Exit,
    Die,
}

impl ExitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ExitKind::Exit => "exit",
            ExitKind::Die => "die",
        }
    }
}

/// Which of the four file-inclusion constructs an `ExprKind::Include` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeKind {
    Include,
    IncludeOnce,
    Require,
    RequireOnce,
}

impl IncludeKind {
    /// The keyword, as written in lowercase.
    pub fn as_str(self) -> &'static str {
        match self {
            IncludeKind::Include => "include",
            IncludeKind::IncludeOnce => "include_once",
            IncludeKind::Require => "require",
            IncludeKind::RequireOnce => "require_once",
        }
    }

    /// `require`/`require_once`: a missing file is a fatal error, not a
    /// warning.
    pub fn is_require(self) -> bool {
        matches!(self, IncludeKind::Require | IncludeKind::RequireOnce)
    }
}

/// A cast's target, as the type it converts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastKind {
    Int,
    Float,
    String,
    Bool,
    Array,
    Object,
    Unset,
    /// `(void)` (PHP 8.5): discards the value.
    Void,
}

impl CastKind {
    /// The canonical spelling, as written between the parentheses.
    pub fn keyword(self) -> &'static str {
        match self {
            CastKind::Int => "int",
            CastKind::Float => "float",
            CastKind::String => "string",
            CastKind::Bool => "bool",
            CastKind::Array => "array",
            CastKind::Object => "object",
            CastKind::Unset => "unset",
            CastKind::Void => "void",
        }
    }
}

/// A cast exactly as spelled (`(integer)` and `(int)` are the same cast).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastSpelling {
    Int,
    Integer,
    Float,
    Real,
    Double,
    String,
    Binary,
    Bool,
    Boolean,
    Array,
    Object,
    Unset,
    Void,
}

impl CastSpelling {
    /// Recognizes a cast name (any case), `None` if it isn't one.
    pub fn from_name(name: &str) -> Option<CastSpelling> {
        Some(match name.to_ascii_lowercase().as_str() {
            "int" => CastSpelling::Int,
            "integer" => CastSpelling::Integer,
            "float" => CastSpelling::Float,
            "real" => CastSpelling::Real,
            "double" => CastSpelling::Double,
            "string" => CastSpelling::String,
            "binary" => CastSpelling::Binary,
            "bool" => CastSpelling::Bool,
            "boolean" => CastSpelling::Boolean,
            "array" => CastSpelling::Array,
            "object" => CastSpelling::Object,
            "unset" => CastSpelling::Unset,
            "void" => CastSpelling::Void,
            _ => return None,
        })
    }

    pub fn kind(self) -> CastKind {
        match self {
            CastSpelling::Int | CastSpelling::Integer => CastKind::Int,
            CastSpelling::Float | CastSpelling::Real | CastSpelling::Double => CastKind::Float,
            CastSpelling::String | CastSpelling::Binary => CastKind::String,
            CastSpelling::Bool | CastSpelling::Boolean => CastKind::Bool,
            CastSpelling::Array => CastKind::Array,
            CastSpelling::Object => CastKind::Object,
            CastSpelling::Unset => CastKind::Unset,
            CastSpelling::Void => CastKind::Void,
        }
    }

    /// The spelling as written between the parentheses, lowercased.
    pub fn keyword(self) -> &'static str {
        match self {
            CastSpelling::Integer => "integer",
            CastSpelling::Real => "real",
            CastSpelling::Double => "double",
            CastSpelling::Binary => "binary",
            CastSpelling::Boolean => "boolean",
            other => other.kind().keyword(),
        }
    }
}

/// The operator reaching a member: `->`, or `?->`, which gives `null`
/// instead when the object is `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectOperator {
    Arrow,
    NullsafeArrow,
}

impl ObjectOperator {
    pub fn from_nullsafe(nullsafe: bool) -> ObjectOperator {
        if nullsafe {
            ObjectOperator::NullsafeArrow
        } else {
            ObjectOperator::Arrow
        }
    }

    pub fn is_nullsafe(self) -> bool {
        self == ObjectOperator::NullsafeArrow
    }
}

/// `function (...) use (...) { ... }`.
#[derive(Debug, Clone)]
pub struct ClosureExpr {
    pub params: ParamRange,
    pub uses: ClosureUseRange,
    pub body: StmtIdRange,
    // The labels the body's `goto`s name; see `FunctionBody`.
    pub goto_labels: Box<[GotoLabel]>,
    // Whether the body is a generator; see `FunctionBody`.
    pub is_generator: bool,
    pub is_static: bool,
    pub by_ref: bool,
    // Boxed: keeps `ExprNode` small, since expression parsing recurses.
    pub return_type: Option<Box<TypeHint>>,
    // `#[Attr]` groups on the closure expression itself
    // (`$f = #[Foo] function () {};`), not on any of its parameters.
    pub attrs: AttributeRange,
}

/// `fn (...) => expr`.
#[derive(Debug, Clone)]
pub struct ArrowFnExpr {
    pub params: ParamRange,
    pub body: ExprId,
    // Whether the body is a generator; see `FunctionBody`.
    pub is_generator: bool,
    pub is_static: bool,
    // `fn &() => ...` - returns by reference.
    pub by_ref: bool,
    // See `ClosureExpr::return_type`.
    pub return_type: Option<Box<TypeHint>>,
    // See `ClosureExpr::attrs`.
    pub attrs: AttributeRange,
}

#[derive(Debug, Clone)]
pub enum ClosurePayload {
    Closure(ClosureExpr),
    ArrowFn(ArrowFnExpr),
}

/// An expression. Its real source span is stored in `NodeArenas::expr_spans`
/// at the matching expression ID.
#[derive(Debug, Clone)]
pub struct ExprNode {
    pub id: ExprId,
    pub kind: ExprKind,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Literal(Literal),
    // Moved out of the lexer's interned `Token::Variable` or a string
    // interpolation's variable, without its `$`.
    Variable(Name),
    // `${expr}` - the older "variable variable" syntax: the variable's own
    // name is computed at runtime by evaluating expr, so unlike Variable
    // there's no fixed name here to look up or bind in `locals`.
    DynamicVariable(ExprId),
    ArrayLiteral(ArrayItemRange),
    // `@expr`: the error-control operator, which silences the warnings
    // evaluating `expr` raises. Its value is `expr`'s.
    ErrorSuppress(ExprId),
    // `[...]` or `list(...)` as the target of an assignment or a
    // `foreach`: takes the array apart into the item targets. Only ever
    // the left of an `Assign`, a `foreach` value, or an item target of
    // another pattern.
    Destructure(DestructureItemRange, DestructureSyntax),
    Unary(UnaryOp, ExprId),
    Binary(BinOp, ExprId, ExprId),
    Assign(ExprId, ExprId),
    // `$a = &$b;` - reference assignment, only ever valid on a plain
    // `=` (never a compound one). A genuinely distinct variant, not a
    // flag on `Assign`, specifically so a `nullsafe.byRef` check
    // (`$a = &$b?->c;`) can tell this RHS position apart from an
    // ordinary read - aliasing itself still isn't modeled beyond that
    // (nothing tracks that two variables now share storage; the RHS's
    // own type still just flows through normally, identically to
    // `Assign`). Every other consumer that doesn't care about the
    // by-ref distinction folds this into the same arm as `Assign` via
    // an or-pattern.
    AssignRef(ExprId, ExprId),
    CompoundAssign(BinOp, ExprId, ExprId),
    Ternary(ExprId, Option<ExprId>, ExprId),
    Call(ExprId, ArgRange),
    New(ExprId, ArgRange),
    // The trailing `bool` is `true` for a nullsafe access (`$x?->prop`),
    // `false` for a plain `$x->prop` - the two are otherwise identical
    // syntactically, but real PHP's `?->` implicitly makes the whole
    // access `null` when its base is `null`, which `infer_expr` needs to
    // know to union `Null` into the result.
    // The property/method name: a `Name`, moved straight out of a
    // `Token::Identifier`/`Token::Keyword` (via `Parser::
    // take_name_segment`) - or, for a string-interpolation `$obj->prop`
    // occurrence, `StringPart::PropertyAccess::prop`. A `$obj->$prop` dynamic-by-variable-name
    // access is a `DynamicPropertyAccess`/`DynamicMethodCall` below, not
    // this variant - `$prop`'s own runtime value, not its declared name,
    // is the real member name.
    PropertyAccess(ExprId, Ident, ObjectOperator),
    MethodCall(ExprId, Ident, ArgRange, ObjectOperator),
    // `$obj->{$name}` / `$obj->{$name}(...)` - the property/method name is
    // itself computed from an expression, not statically known, so it
    // can't share PropertyAccess/MethodCall's plain-String name. Still
    // worth its own base+name-expr(+args) checking (undefined variables
    // inside the {} can still be caught), even though the accessed
    // member's own type never can be.
    DynamicPropertyAccess(ExprId, ExprId, ObjectOperator),
    DynamicMethodCall(ExprId, ExprId, ArgRange, ObjectOperator),
    /// `Foo::BAR`.
    ClassConstFetch(StaticTarget, Ident),
    /// `Foo::class` (any case).
    ClassNameFetch(StaticTarget),
    /// `Foo::$bar`; the name is without its `$`.
    StaticPropertyFetch(StaticTarget, Ident),
    /// `Foo::${expr}` / `Foo::$$name`: the property name is computed.
    DynamicStaticPropertyFetch(StaticTarget, ExprId),
    /// `Foo::{$expr}` (PHP 8.3): the constant name is computed.
    DynamicClassConstFetch(StaticTarget, ExprId),
    // `Name` mirrors `InstanceofTarget::Name`'s own (name, name-span)
    // shape at parse time; the instance-of form now stores that payload in
    // the qualified-name owner arena. `ExprNode` is a real, non-`Name` receiver
    // expression (`$obj::method()`) - previously discarded into a
    // permanent `"?"` sentinel string at parse time; now the real,
    // already-parsed receiver `ExprNode` survives into the AST, letting
    // `check_static_call` resolve its inferred type and check the call
    // against it, the same way a literal class name already is.
    StaticCall(StaticTarget, Ident, ArgRange),
    /// `Foo::$name()` / `Foo::{$expr}()`: the method name is computed.
    DynamicStaticCall(StaticTarget, ExprId, ArgRange),
    Index(ExprId, Option<ExprId>),
    Name(QualifiedNameRange),
    Instanceof(ExprId, InstanceofTarget),
    Cast(CastSpelling, ExprId),
    Grouping(ExprId),
    // `clone $x` / PHP 8.5's `clone($x, $overrides)` (the second field is
    // the property-overrides array, when present). Both produce an object
    // of the same type as the first operand, so infer_expr treats them
    // identically - the overrides themselves aren't otherwise modeled.
    Clone(ExprId, Option<ExprId>),
    // `foo(...)` / `$obj->method(...)` (PHP 8.1): a reference to the
    // callable itself, not a call - wraps a Call/MethodCall/StaticCall
    // that was parsed with an empty argument list specifically because of
    // this syntax, so it's never mistaken for (and argument-count-checked
    // as) a real zero-argument call.
    FirstClassCallable(ExprId),
    // `` `cmd $interpolated` ``: runs the command, as `shell_exec()` does.
    // Holds the command string.
    ShellExec(ExprId),
    // `isset($a, $b, ...)`: whether every operand is set and not null.
    Isset(ExprIdRange),
    // `empty(expr)`: whether the operand is unset or falsy.
    Empty(ExprId),
    // `print expr`, which evaluates to `1`.
    Print(ExprId),
    // `exit`/`die`: `None` written bare, `Some` with its parenthesized
    // arguments (PHP 8.4 makes both functions of `string|int $status`).
    Exit(ExitKind, Option<ArgRange>),
    // `eval(code)`.
    Eval(ExprId),
    // `include`/`include_once`/`require`/`require_once path`.
    Include(IncludeKind, ExprId),
    // `throw` as an expression (PHP 8.0+): valid in match arms, ternary
    // branches, ??'s right side, arrow function bodies, etc - as opposed
    // to StmtKind::Throw, a bare `throw expr;` statement, which is still
    // dispatched separately and takes priority at statement position.
    Throw(ExprId),
    NewAnonymousClass(AnonymousClassId),
    // `yield;` / `yield $v;` / `yield $k => $v;` - (key, value). Makes the
    // enclosing function a generator.
    Yield(Option<ExprId>, Option<ExprId>),
    // `yield from $generator;`
    YieldFrom(ExprId),
    Interpolated(ExprIdRange),
    // Boxed: keeps `ExprNode` small, since expression parsing recurses.
    Closure(ClosureId),
    ArrowFn(ClosureId),
    // The arms in written order, `default` among them.
    Match {
        subject: ExprId,
        arms: MatchArmRange,
    },
}

/// One arm of a `match`: `conditions => body`, or `default => body`.
#[derive(Debug, Clone)]
pub struct MatchArm {
    // `None` for the `default` arm.
    pub conditions: Option<ExprIdRange>,
    pub body: ExprId,
    pub span: Span,
}

impl MatchArm {
    /// A non-default arm's conditions and body.
    pub fn conditioned(&self) -> Option<(ExprIdRange, ExprId)> {
        Some((self.conditions.clone()?, self.body))
    }
}

/// The body of a `match`'s `default` arm.
pub fn match_default(arms: &[MatchArm]) -> Option<ExprId> {
    arms.iter()
        .find(|arm| arm.conditions.is_none())
        .map(|arm| arm.body)
}

/// A literal string's text, or a statically-foldable `.`-concatenation of
/// nothing but literal strings (and parenthesized groupings of the same),
/// folded into one owned value. `None` for anything not fully foldable
/// this way (a variable anywhere in the chain, a non-string literal) -
/// unlike `literal_string_key`, this doesn't also accept an int literal,
/// since a format string is never legitimately written as a bare number.
/// Purely syntactic: a concatenation with a variable operand is never
/// resolved here even if a caller's type tracking knows it's a literal
/// value, since this function only sees the raw `ExprNode`.
pub fn literal_string_value<'a>(
    arenas: &'a NodeArenas,
    id: ExprId,
) -> Option<std::borrow::Cow<'a, str>> {
    let id = arenas.peeled(id)?;
    let e = arenas.expr(id)?;
    crate::stack::guard(|| match &e.kind {
        ExprKind::Literal(Literal::String(s)) => {
            let bytes = arenas.string_bytes(s)?;
            std::str::from_utf8(bytes)
                .ok()
                .map(std::borrow::Cow::Borrowed)
        }
        ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
            let l = literal_string_value(arenas, *lhs)?;
            let r = literal_string_value(arenas, *rhs)?;
            Some(std::borrow::Cow::Owned(format!("{l}{r}")))
        }
        _ => None,
    })
}

/// Whether `name` (without its `$`) is one of PHP's superglobals.
pub fn is_superglobal(name: &str) -> bool {
    matches!(
        name,
        "GLOBALS"
            | "_GET"
            | "_POST"
            | "_REQUEST"
            | "_SERVER"
            | "_SESSION"
            | "_COOKIE"
            | "_FILES"
            | "_ENV"
            | "argv"
            | "argc"
    )
}

/// Whether `name` is a PHP or PHPDoc keyword type rather than a class name.
pub fn is_primitive_type_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "int"
            | "integer"
            | "float"
            | "double"
            | "real"
            | "string"
            | "bool"
            | "boolean"
            | "array"
            | "object"
            | "mixed"
            | "void"
            | "never"
            | "noreturn"
            | "never-return"
            | "never-returns"
            | "no-return"
            | "null"
            | "true"
            | "false"
            | "self"
            | "static"
            | "parent"
            | "callable"
            | "iterable"
    )
}

#[cfg(test)]
mod arena_tests {
    use super::*;

    fn arg(value: u32) -> Arg {
        Arg {
            name: None,
            value: ExprId::from_raw(la_arena::RawIdx::from_u32(value)),
            spread: false,
        }
    }

    fn array_item(value: u32) -> ArrayItem {
        ArrayItem {
            key: None,
            value: ExprId::from_raw(la_arena::RawIdx::from_u32(value)),
            spread: false,
            by_ref: false,
        }
    }

    fn parameter(attrs: AttributeRange) -> Param {
        Param {
            name: Ident {
                name: crate::name::Interner::global().name("parameter"),
                name_id: None,
                span: Span::default(),
            },
            type_hint: None,
            type_aware_override: None,
            default: None,
            by_ref: false,
            variadic: false,
            promotion: None,
            hooks: None,
            attrs,
            span: Span::default(),
        }
    }

    fn attribute(args: ArgRange) -> Attribute {
        Attribute {
            name: QualifiedName {
                written: crate::name::Interner::global().name("Attribute"),
                resolved: ResolvedName::Unresolved,
                span: Span::default(),
            },
            args,
            span: Span::default(),
            removal_span: Span::default(),
        }
    }

    fn class_def(arenas: &mut NodeArenas) -> ClassDef {
        let methods = arenas.alloc_methods([]);
        let attrs = arenas.alloc_attributes([]);
        ClassDef {
            kind: ClassKind::Class,
            backing_type: None,
            extends: arenas.alloc_qualified_names([]),
            implements: arenas.alloc_qualified_names([]),
            is_abstract: false,
            is_final: false,
            is_readonly: false,
            properties: arenas.alloc_properties([]),
            methods,
            consts: arenas.alloc_class_consts([]),
            trait_uses: arenas.alloc_trait_uses([]),
            enum_cases: arenas.alloc_enum_cases([]),
            doc_comment: None,
            attrs,
            span: Span::default(),
        }
    }

    #[test]
    fn list_allocators_commit_contiguous_ranges_and_reuse_capacity() {
        let mut arenas = NodeArenas::default();
        let first = arenas.alloc_args([arg(0), arg(1)]);
        assert_eq!(arenas.args(first.clone()).len(), 2);
        assert_eq!(u32::from(first.start().into_raw()), 0);
        assert_eq!(u32::from(first.end().into_raw()), 2);
        let storage = arenas.args.as_mut().as_ptr();

        arenas.clear();
        let second = arenas.alloc_args([arg(2)]);
        assert_eq!(u32::from(second.start().into_raw()), 0);
        assert_eq!(arenas.args.as_mut().as_ptr(), storage);

        let statements = arenas.alloc_stmt_ids([
            StmtId::from_raw(la_arena::RawIdx::from_u32(4)),
            StmtId::from_raw(la_arena::RawIdx::from_u32(5)),
        ]);
        assert_eq!(u32::from(statements.start().into_raw()), 0);
        assert_eq!(arenas.stmt_ids(&statements).len(), 2);
    }

    #[test]
    fn list_ranges_keep_file_ids_across_fragment_append() {
        let mut parent = NodeArenas::default();
        let original = parent.alloc_args([arg(0), arg(1)]);
        let original_stmts = parent.alloc_stmt_ids([
            StmtId::from_raw(la_arena::RawIdx::from_u32(0)),
            StmtId::from_raw(la_arena::RawIdx::from_u32(1)),
        ]);
        let original_expr_ids = parent.alloc_expr_ids([
            ExprId::from_raw(la_arena::RawIdx::from_u32(0)),
            ExprId::from_raw(la_arena::RawIdx::from_u32(1)),
        ]);
        let original_pattern = parent.alloc_destructure_items([DestructureItem {
            key: None,
            target: Some(ExprId::from_raw(la_arena::RawIdx::from_u32(0))),
            by_ref: false,
        }]);
        let original_items = parent.alloc_array_items([array_item(0)]);
        let empty_attribute_range = parent.alloc_attributes([]);
        let original_params = parent.alloc_params([parameter(empty_attribute_range)]);
        let match_condition =
            parent.alloc_expr_ids([ExprId::from_raw(la_arena::RawIdx::from_u32(0))]);
        let original_arms = parent.alloc_match_arms([MatchArm {
            conditions: Some(match_condition),
            body: ExprId::from_raw(la_arena::RawIdx::from_u32(1)),
            span: Span::default(),
        }]);
        let original_elseifs = parent.alloc_elseifs([ElseIf {
            cond: ExprId::from_raw(la_arena::RawIdx::from_u32(0)),
            body: StmtId::from_raw(la_arena::RawIdx::from_u32(0)),
            span: Span::default(),
        }]);
        let original_case_body = parent.alloc_stmt_ids([]);
        let original_cases = parent.alloc_switch_cases([SwitchCase {
            test: None,
            body: original_case_body,
            span: Span::default(),
        }]);
        let original_catch_types = parent.alloc_qualified_names([]);
        let original_catch_body = parent.alloc_stmt_ids([]);
        let original_catches = parent.alloc_catch_clauses([CatchClause {
            types: original_catch_types,
            var: None,
            body: original_catch_body,
            span: Span::default(),
        }]);
        let original_directives = parent.alloc_declare_directives([DeclareDirective {
            name: Ident {
                name: crate::name::Interner::global().name("strict_types"),
                name_id: None,
                span: Span::default(),
            },
            value: ExprId::from_raw(la_arena::RawIdx::from_u32(0)),
        }]);
        let original_globals = parent.alloc_global_vars([GlobalVar::Name(Ident {
            name: crate::name::Interner::global().name("original_global"),
            name_id: None,
            span: Span::default(),
        })]);
        let original_statics = parent.alloc_static_vars([StaticVarDecl {
            name: Ident {
                name: crate::name::Interner::global().name("original_static"),
                name_id: None,
                span: Span::default(),
            },
            default: Some(ExprId::from_raw(la_arena::RawIdx::from_u32(0))),
        }]);
        let empty_args_range = parent.alloc_args([]);
        let original_attributes = parent.alloc_attributes([attribute(empty_args_range)]);
        let original_class_def = class_def(&mut parent);
        let original_class = parent.alloc_class(original_class_def);
        let original_anonymous_class = parent.alloc_anonymous_class(AnonymousClass {
            def: original_class,
            args: original.clone(),
        });
        assert!(parent.set_anonymous_class_name(
            original_anonymous_class,
            crate::name::Name::from("class@anonymous/original:1"),
        ));

        let mut fragment = NodeArenas {
            args_base: u32::try_from(parent.args.len()).unwrap(),
            stmt_ids_base: u32::try_from(parent.stmt_ids.len()).unwrap(),
            expr_ids_base: u32::try_from(parent.expr_ids.len()).unwrap(),
            destructure_items_base: u32::try_from(parent.destructure_items.len()).unwrap(),
            array_items_base: u32::try_from(parent.array_items.len()).unwrap(),
            params_base: u32::try_from(parent.params.len()).unwrap(),
            match_arms_base: u32::try_from(parent.match_arms.len()).unwrap(),
            attributes_base: u32::try_from(parent.attributes.len()).unwrap(),
            elseifs: parent.elseifs.fragment(),
            switch_cases: parent.switch_cases.fragment(),
            catch_clauses: parent.catch_clauses.fragment(),
            declare_directives: parent.declare_directives.fragment(),
            global_vars: parent.global_vars.fragment(),
            static_vars: parent.static_vars.fragment(),
            methods_base: u32::try_from(parent.methods.len()).unwrap(),
            classes_base: u32::try_from(parent.classes.len()).unwrap(),
            anonymous_classes_base: u32::try_from(parent.anonymous_classes.len()).unwrap(),
            ..NodeArenas::default()
        };
        let appended = fragment.alloc_args([arg(2)]);
        let appended_stmts =
            fragment.alloc_stmt_ids([StmtId::from_raw(la_arena::RawIdx::from_u32(2))]);
        let appended_expr_ids =
            fragment.alloc_expr_ids([ExprId::from_raw(la_arena::RawIdx::from_u32(2))]);
        let appended_pattern = fragment.alloc_destructure_items([DestructureItem {
            key: None,
            target: Some(ExprId::from_raw(la_arena::RawIdx::from_u32(3))),
            by_ref: true,
        }]);
        let appended_items = fragment.alloc_array_items([array_item(2)]);
        let empty_fragment_attribute_range = fragment.alloc_attributes([]);
        let appended_params = fragment.alloc_params([parameter(empty_fragment_attribute_range)]);
        let appended_arms = fragment.alloc_match_arms([MatchArm {
            conditions: None,
            body: ExprId::from_raw(la_arena::RawIdx::from_u32(3)),
            span: Span::default(),
        }]);
        let appended_elseifs = fragment.alloc_elseifs([ElseIf {
            cond: ExprId::from_raw(la_arena::RawIdx::from_u32(2)),
            body: StmtId::from_raw(la_arena::RawIdx::from_u32(2)),
            span: Span::default(),
        }]);
        let appended_case_body = fragment.alloc_stmt_ids([]);
        let appended_cases = fragment.alloc_switch_cases([SwitchCase {
            test: Some(ExprId::from_raw(la_arena::RawIdx::from_u32(2))),
            body: appended_case_body,
            span: Span::default(),
        }]);
        let appended_catch_types = fragment.alloc_qualified_names([]);
        let appended_catch_body = fragment.alloc_stmt_ids([]);
        let appended_catches = fragment.alloc_catch_clauses([CatchClause {
            types: appended_catch_types,
            var: None,
            body: appended_catch_body,
            span: Span::default(),
        }]);
        let appended_directives = fragment.alloc_declare_directives([DeclareDirective {
            name: Ident {
                name: crate::name::Interner::global().name("ticks"),
                name_id: None,
                span: Span::default(),
            },
            value: ExprId::from_raw(la_arena::RawIdx::from_u32(2)),
        }]);
        let appended_globals = fragment.alloc_global_vars([GlobalVar::Dynamic(ExprId::from_raw(
            la_arena::RawIdx::from_u32(2),
        ))]);
        let appended_statics = fragment.alloc_static_vars([StaticVarDecl {
            name: Ident {
                name: crate::name::Interner::global().name("fragment_static"),
                name_id: None,
                span: Span::default(),
            },
            default: None,
        }]);
        let empty_fragment_args_range = fragment.alloc_args([]);
        let appended_attributes = fragment.alloc_attributes([attribute(empty_fragment_args_range)]);
        let appended_class_def = class_def(&mut fragment);
        let appended_class = fragment.alloc_class(appended_class_def);
        let appended_anonymous_class = fragment.alloc_anonymous_class(AnonymousClass {
            def: appended_class,
            args: appended.clone(),
        });
        assert!(fragment.set_anonymous_class_name(
            appended_anonymous_class,
            crate::name::Name::from("class@anonymous/fragment:1"),
        ));
        assert_eq!(u32::from(appended.start().into_raw()), 2);
        assert_eq!(
            u32::from(fragment.args(appended.clone())[0].value.into_raw()),
            2
        );
        assert_eq!(u32::from(appended_stmts.start().into_raw()), 2);
        assert_eq!(
            u32::from(fragment.stmt_ids(&appended_stmts)[0].into_raw()),
            2
        );
        assert_eq!(u32::from(appended_expr_ids.start().into_raw()), 3);
        assert_eq!(
            u32::from(fragment.expr_ids(&appended_expr_ids)[0].into_raw()),
            2
        );
        assert_eq!(u32::from(appended_pattern.start().into_raw()), 1);
        assert!(fragment.destructure_items(&appended_pattern)[0].by_ref);
        assert_eq!(
            u32::from(appended_items.start().into_raw()),
            u32::try_from(parent.array_items.len()).unwrap()
        );
        assert_eq!(
            u32::from(appended_params.start().into_raw()),
            u32::try_from(parent.params.len()).unwrap()
        );
        assert_eq!(
            u32::from(appended_arms.start().into_raw()),
            u32::try_from(parent.match_arms.len()).unwrap()
        );
        assert_eq!(
            u32::from(appended_elseifs.start().into_raw()),
            u32::try_from(parent.elseifs.get(&original_elseifs).len()).unwrap()
        );
        assert_eq!(
            u32::from(appended_cases.start().into_raw()),
            u32::try_from(parent.switch_cases.get(&original_cases).len()).unwrap()
        );
        assert_eq!(
            u32::from(appended_catches.start().into_raw()),
            u32::try_from(parent.catch_clauses.get(&original_catches).len()).unwrap()
        );
        assert_eq!(
            u32::from(appended_directives.start().into_raw()),
            u32::try_from(parent.declare_directives.get(&original_directives).len()).unwrap()
        );
        assert_eq!(
            u32::from(appended_globals.start().into_raw()),
            u32::try_from(parent.global_vars.get(&original_globals).len()).unwrap()
        );
        assert_eq!(
            u32::from(appended_statics.start().into_raw()),
            u32::try_from(parent.static_vars.get(&original_statics).len()).unwrap()
        );
        assert_eq!(
            u32::from(appended_attributes.start().into_raw()),
            u32::try_from(parent.attributes.len()).unwrap()
        );
        assert_eq!(u32::from(appended_class.into_raw()), 1);
        assert_eq!(u32::from(appended_anonymous_class.into_raw()), 1);

        parent.append_fragment(fragment);
        assert_eq!(parent.args(original).len(), 2);
        assert_eq!(parent.expr_ids(&original_expr_ids).len(), 2);
        assert_eq!(parent.destructure_items(&original_pattern).len(), 1);
        assert_eq!(parent.stmt_ids(&original_stmts).len(), 2);
        assert_eq!(parent.stmt_ids(&appended_stmts).len(), 1);
        assert_eq!(u32::from(parent.stmt_ids(&appended_stmts)[0].into_raw()), 2);
        assert_eq!(parent.expr_ids(&appended_expr_ids).len(), 1);
        assert_eq!(
            u32::from(parent.expr_ids(&appended_expr_ids)[0].into_raw()),
            2
        );
        assert_eq!(parent.elseifs(original_elseifs).len(), 1);
        assert_eq!(parent.elseifs(appended_elseifs.clone()).len(), 1);
        assert_eq!(
            u32::from(parent.elseifs(appended_elseifs)[0].cond.into_raw()),
            2
        );
        assert_eq!(parent.switch_cases(original_cases.clone()).len(), 1);
        assert_eq!(parent.switch_cases(appended_cases.clone()).len(), 1);
        assert_eq!(
            u32::from(
                parent.switch_cases(appended_cases.clone())[0]
                    .test
                    .unwrap()
                    .into_raw()
            ),
            2
        );
        assert_eq!(parent.catch_clauses(original_catches.clone()).len(), 1);
        assert_eq!(parent.catch_clauses(appended_catches.clone()).len(), 1);
        assert_eq!(
            parent.declare_directives(original_directives.clone()).len(),
            1
        );
        assert_eq!(
            parent.declare_directives(appended_directives.clone()).len(),
            1
        );
        assert_eq!(
            parent.declare_directives(appended_directives.clone())[0]
                .name
                .name,
            "ticks"
        );
        assert_eq!(parent.global_vars(original_globals.clone()).len(), 1);
        assert_eq!(parent.global_vars(appended_globals.clone()).len(), 1);
        assert!(matches!(
            &parent.global_vars(appended_globals.clone())[0],
            GlobalVar::Dynamic(_)
        ));
        assert_eq!(parent.static_vars(original_statics.clone()).len(), 1);
        assert_eq!(parent.static_vars(appended_statics.clone()).len(), 1);
        assert_eq!(
            parent.static_vars(appended_statics.clone())[0].name.name,
            "fragment_static"
        );
        assert_eq!(parent.destructure_items(&appended_pattern).len(), 1);
        assert!(parent.destructure_items(&appended_pattern)[0].by_ref);
        assert_eq!(
            u32::from(parent.args(appended.clone())[0].value.into_raw()),
            2
        );
        assert_eq!(parent.array_items(original_items).len(), 1);
        assert_eq!(
            u32::from(parent.array_items(appended_items)[0].value.into_raw()),
            2
        );
        assert_eq!(parent.params(original_params).len(), 1);
        assert_eq!(parent.params(appended_params).len(), 1);
        assert_eq!(parent.match_arms(original_arms).len(), 1);
        assert_eq!(parent.match_arms(appended_arms).len(), 1);
        assert_eq!(parent.attributes(original_attributes).len(), 1);
        assert_eq!(parent.attributes(appended_attributes).len(), 1);
        assert!(parent.class(original_class).is_some());
        assert!(parent.class(appended_class).is_some());
        assert_eq!(
            parent
                .anonymous_class_name(original_anonymous_class)
                .map(|name| name.as_str()),
            Some("class@anonymous/original:1")
        );
        assert_eq!(
            parent
                .anonymous_class_name(appended_anonymous_class)
                .map(|name| name.as_str()),
            Some("class@anonymous/fragment:1")
        );
    }

    #[test]
    fn node_arena_scratch_clear_reuses_typed_indices() {
        let mut arenas = NodeArenas::default();
        let expression = ExprNode {
            id: SYNTHETIC_EXPR_ID,
            kind: ExprKind::Literal(Literal::Null),
        };
        let statement = StmtNode {
            id: SYNTHETIC_STMT_ID,
            kind: StmtKind::Break(None),
        };

        let expression_id = arenas.exprs.alloc(expression);
        let statement_id = arenas.stmts.alloc(statement);
        arenas.clear();

        assert_eq!(
            arenas.exprs.alloc(ExprNode {
                id: SYNTHETIC_EXPR_ID,
                kind: ExprKind::Literal(Literal::Null),
            }),
            expression_id
        );
        assert_eq!(
            arenas.stmts.alloc(StmtNode {
                id: SYNTHETIC_STMT_ID,
                kind: StmtKind::Break(None),
            }),
            statement_id
        );
        arenas.shrink_to_fit();
    }
}
