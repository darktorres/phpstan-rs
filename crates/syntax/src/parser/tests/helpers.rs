use super::super::*;
use crate::parse::ParsedFile;
use crate::span::Span;
use std::ops::{Deref, Index};

/// Test view over a normally parsed file. All payload reads still resolve
/// through the owning ParsedFile/NodeArenas; this only keeps tree navigation
/// concise in the parser tests.
pub(super) struct TestParsedFile(ParsedFile);

pub(super) fn parse(src: &str) -> TestParsedFile {
    TestParsedFile(crate::parse::parse(src).expect("PHP parses"))
}

impl Deref for TestParsedFile {
    type Target = ParsedFile;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Index<usize> for TestParsedFile {
    type Output = StmtNode;

    fn index(&self, index: usize) -> &Self::Output {
        self.0
            .arenas
            .stmt(self.0.statements()[index])
            .expect("statement id belongs to parsed file")
    }
}

impl TestParsedFile {
    pub(super) fn iter(&self) -> impl Iterator<Item = &StmtNode> {
        self.0.statements().iter().map(|id| {
            self.0
                .arenas
                .stmt(*id)
                .expect("statement id belongs to parsed file")
        })
    }

    pub(super) fn len(&self) -> usize {
        self.0.statements().len()
    }

    pub(super) fn stmt(&self, id: StmtId) -> &StmtNode {
        self.0
            .arenas
            .stmt(id)
            .expect("statement id belongs to parsed file")
    }

    pub(super) fn expr(&self, id: ExprId) -> &ExprNode {
        self.0
            .arenas
            .expr(id)
            .expect("expression id belongs to parsed file")
    }

    pub(super) fn expr_span(&self, id: ExprId) -> Span {
        self.0
            .arenas
            .expr_span(id)
            .expect("expression id belongs to parsed file")
    }

    pub(super) fn function(&self, id: FunctionId) -> &FunctionDecl {
        self.0
            .arenas
            .function(id)
            .expect("function id belongs to parsed file")
    }

    pub(super) fn class_def(&self, id: ClassId) -> &ClassDef {
        self.0
            .arenas
            .class(id)
            .expect("class id belongs to parsed file")
    }

    pub(super) fn anonymous_class(&self, id: AnonymousClassId) -> &AnonymousClass {
        self.0
            .arenas
            .anonymous_class(id)
            .expect("anonymous-class id belongs to parsed file")
    }

    pub(super) fn args(&self, range: ArgRange) -> &[Arg] {
        self.0.arenas.args(range)
    }

    pub(super) fn array_items(&self, range: ArrayItemRange) -> &[ArrayItem] {
        self.0.arenas.array_items(range)
    }

    pub(super) fn params(&self, range: ParamRange) -> &[Param] {
        self.0.arenas.params(range)
    }

    pub(super) fn attributes(&self, range: AttributeRange) -> &[Attribute] {
        self.0.arenas.attributes(range)
    }

    pub(super) fn methods(&self, range: MethodRange) -> &[MethodDecl] {
        self.0.arenas.methods(range)
    }

    pub(super) fn class_methods(&self, id: ClassId) -> &[MethodDecl] {
        self.0.arenas.methods(self.class_def(id).methods.clone())
    }

    pub(super) fn doc_comment_text(&self, ranges: &DocCommentRanges) -> Option<String> {
        self.0
            .doc_comment_text(ranges)
            .map(|text| text.into_owned())
    }

    pub(super) fn string_bytes<'a>(&'a self, payload: &'a StringPayload) -> Option<&'a [u8]> {
        self.0.string_bytes(payload)
    }

    pub(super) fn string_text(&self, payload: &StringPayload) -> Option<String> {
        std::str::from_utf8(self.string_bytes(payload)?)
            .ok()
            .map(str::to_owned)
    }
}

/// The text of `src` a `Span` covers - lets a test assert a span actually
/// covers the identifier text it claims, not just that it exists.
/// Single-line spans only.
pub(super) fn text_at(src: &str, span: Span) -> String {
    let text = src
        .get(span.start as usize..span.end as usize)
        .unwrap_or_else(|| panic!("{span:?} out of range in {src:?}"));
    assert!(
        !text.contains('\n'),
        "text_at only supports a single-line span, got {span:?}"
    );
    text.to_string()
}

/// The line and character column of the byte offset `offset` of `src`.
pub(super) fn position(src: &str, offset: u32) -> crate::span::LineCol {
    crate::span::LineIndex::new(src.as_bytes()).line_col(offset)
}
