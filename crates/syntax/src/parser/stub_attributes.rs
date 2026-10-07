//! The parser-time projection used by phpstorm-stubs. These rules are
//! intentionally activated only by a resolved stub attribute, preserving
//! the historical behavior for any file that names one of them.

use std::sync::Arc;

use crate::ast::*;
use crate::lexer::Lexer;
use crate::name::Name;
use crate::parser::{NameKind, ParseError, Parser};
use crate::span::Span;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Availability,
    TypeAware,
}

impl Parser<'_> {
    fn stub_kind_name(name: &QualifiedName) -> Option<Kind> {
        let name = name
            .resolved_class()
            .unwrap_or_else(|| name.written.as_str());
        match name.rsplit('\\').next().unwrap_or(name) {
            "PhpStormStubsElementAvailable" => Some(Kind::Availability),
            "LanguageLevelTypeAware" | "PhpVersionAware" => Some(Kind::TypeAware),
            _ => None,
        }
    }

    fn stub_kind(&self, attr: &Attribute) -> Option<Kind> {
        Self::stub_kind_name(&attr.name)
    }

    pub(crate) fn note_stub_attribute(&mut self, name: &QualifiedName) {
        self.stub_attributes_mentioned |= Self::stub_kind_name(name).is_some();
    }

    fn attributes(&self, range: AttributeRange) -> Vec<Attribute> {
        self.node_arenas.borrow().attributes(range).to_vec()
    }

    fn is_closed_range(&self, attr: &Attribute) -> bool {
        self.node_arenas
            .borrow()
            .args(attr.args.clone())
            .iter()
            .any(|arg| arg.name.as_ref().is_some_and(|name| name.name == "to"))
    }

    pub(crate) fn stub_drops_function(&self, range: AttributeRange) -> bool {
        if !self.stub_attributes_mentioned {
            return false;
        }
        let attrs = self.attributes(range.clone());
        let drops = attrs.last().is_some_and(|attr| {
            self.stub_kind(attr) == Some(Kind::Availability) && self.is_closed_range(attr)
        });
        if drops {
            self.stub_attributes_changed.set(true);
        }
        drops
    }

    pub(crate) fn strip_stub_attributes(&mut self, range: AttributeRange) -> AttributeRange {
        if !self.stub_attributes_mentioned {
            return range;
        }
        let attrs = self.attributes(range.clone());
        let kept: Vec<_> = attrs
            .into_iter()
            .filter(|attr| self.stub_kind(attr).is_none())
            .collect();
        if kept.len() != range.len() {
            self.stub_attributes_changed.set(true);
            self.node_arenas.borrow_mut().alloc_attributes(kept)
        } else {
            range
        }
    }

    fn string_literal(&self, expr: ExprId) -> Option<String> {
        let arenas = self.node_arenas.borrow();
        let ExprKind::Literal(Literal::String(payload)) = &arenas.expr(expr)?.kind else {
            return None;
        };
        let bytes = match payload {
            StringPayload::Source(span) => self
                .source?
                .get(span.start as usize..span.end as usize)?,
            StringPayload::Decoded(_) | StringPayload::Owned(_) => arenas.string_bytes(payload)?,
        };
        std::str::from_utf8(bytes)
            .ok()
            .map(str::to_owned)
    }

    fn newest_type(&self, attr: &Attribute) -> Option<String> {
        let args = self.node_arenas.borrow().args(attr.args.clone()).to_vec();
        let Some(array) = args.iter().find(|arg| arg.name.is_none()) else {
            return self.default_type(&args);
        };
        let items_range = self
            .node_arenas
            .borrow()
            .expr(array.value)
            .and_then(|expr| match &expr.kind {
                ExprKind::ArrayLiteral(items) => Some(items.clone()),
                _ => None,
            });
        let Some(items_range) = items_range else {
            return self.default_type(&args);
        };
        let items = self.node_arenas.borrow().array_items(items_range).to_vec();
        let mut best: Option<((u32, u32), String)> = None;
        for item in items {
            let (Some(key), Some(value)) = (
                item.key.and_then(|key| self.string_literal(key)),
                self.string_literal(item.value),
            ) else {
                break;
            };
            let Some((major, minor)) = key
                .split_once('.')
                .and_then(|(major, minor)| Some((major.parse().ok()?, minor.parse().ok()?)))
            else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|(version, _)| (major, minor) > *version)
            {
                best = Some(((major, minor), value));
            }
        }
        best.map(|(_, value)| value)
            .or_else(|| self.default_type(&args))
    }

    fn default_type(&self, args: &[Arg]) -> Option<String> {
        args.iter()
            .find(|arg| arg.name.as_ref().is_some_and(|name| name.name == "default"))
            .and_then(|arg| self.string_literal(arg.value))
    }

    pub(crate) fn return_overrides(&self, attrs: AttributeRange) -> Vec<String> {
        if !self.stub_attributes_mentioned {
            return Vec::new();
        }
        self.attributes(attrs)
            .iter()
            .filter(|attr| self.stub_kind(attr) == Some(Kind::TypeAware))
            .filter_map(|attr| self.newest_type(attr))
            .collect()
    }

    pub(crate) fn append_tag(&self, doc: &mut Option<DocCommentRanges>, line: &str) {
        let Some(ranges) = doc.as_ref() else {
            return;
        };
        let Some(text) = self.doc_text(ranges) else {
            return;
        };
        let updated = match text.rfind("*/") {
            Some(idx) => format!("{}\n * {line}\n {}", &text[..idx], &text[idx..]),
            None => return,
        };
        *doc = Some(ranges.with_appended_text(Arc::<str>::from(updated)));
        self.stub_attributes_changed.set(true);
    }

    /// A synthesized parameter override supersedes any stale declaration
    /// `@param` tag for the same variable. Keep unrelated tags intact.
    fn override_param_tag(
        &self,
        doc: &mut Option<DocCommentRanges>,
        line: &str,
        param_name: &str,
    ) {
        let Some(ranges) = doc.as_ref() else {
            return;
        };
        let Some(text) = self.doc_text(ranges) else {
            return;
        };
        let needle = format!("${param_name}");
        let mut replaced = false;
        let mut updated = String::with_capacity(text.len() + line.len() + 8);
        for original_line in text.lines() {
            let trimmed = original_line.trim().trim_start_matches('*').trim();
            if !replaced
                && trimmed.starts_with("@param ")
                && trimmed.split_whitespace().any(|part| part == needle)
            {
                let indent = &original_line[..original_line.len() - original_line.trim_start().len()];
                updated.push_str(indent);
                updated.push_str("* ");
                updated.push_str(line);
                updated.push('\n');
                replaced = true;
            } else {
                updated.push_str(original_line);
                updated.push('\n');
            }
        }
        if !replaced {
            self.append_tag(doc, line);
            return;
        }
        // Preserve the usual closing delimiter and replace only the reader
        // text; its original source spans remain available for diagnostics.
        let updated = updated.trim_end_matches('\n');
        *doc = Some(ranges.with_appended_text(Arc::<str>::from(updated)));
        self.stub_attributes_changed.set(true);
    }

    fn parse_stub_native_type(&self, text: &str, at: Span) -> Option<TypeHint> {
        let stream = Lexer::new_in_php(text).lex_token_stream();
        let mut parser = Parser::from_token_stream(stream);
        let hint = parser.parse_type_hint().ok()??;
        let source_arenas = parser.take_node_arenas();
        Some(self.import_stub_type_hint(hint, &source_arenas, at))
    }

    fn import_stub_type_hint(
        &self,
        mut hint: TypeHint,
        source_arenas: &NodeArenas,
        span: Span,
    ) -> TypeHint {
        hint.span = span;
        hint.kind = match hint.kind {
            TypeHintKind::Keyword(kind, written) => TypeHintKind::Keyword(kind, written),
            TypeHintKind::Name(name_range) => {
                let name = source_arenas
                    .qualified_name(&name_range)
                    .expect("stub type name belongs to its source arena")
                    .written
                    .clone();
                let imported = self.qualified_name(name, span, NameKind::Class);
                TypeHintKind::Name(self.node_arenas.borrow_mut().alloc_qualified_name(imported))
            }
            TypeHintKind::Nullable(inner) => TypeHintKind::Nullable(Box::new(
                self.import_stub_type_hint(*inner, source_arenas, span),
            )),
            TypeHintKind::Union(members) => {
                let imported = source_arenas
                    .type_hints(&members)
                    .iter()
                    .cloned()
                    .map(|member| self.import_stub_type_hint(member, source_arenas, span))
                    .collect::<Vec<_>>();
                TypeHintKind::Union(self.node_arenas.borrow_mut().alloc_type_hints(imported))
            }
            TypeHintKind::Intersection(members) => {
                let imported = source_arenas
                    .type_hints(&members)
                    .iter()
                    .cloned()
                    .map(|member| self.import_stub_type_hint(member, source_arenas, span))
                    .collect::<Vec<_>>();
                TypeHintKind::Intersection(self.node_arenas.borrow_mut().alloc_type_hints(imported))
            }
        };
        hint
    }

    pub(crate) fn transform_stub_params(
        &mut self,
        params: &mut Vec<crate::parser::ParamDraft>,
        function_doc: &mut Option<DocCommentRanges>,
    ) {
        if !self.stub_attributes_mentioned {
            return;
        }

        let names: Vec<Name> = params
            .iter()
            .map(|param| param.node.name.name.clone())
            .collect();
        let mut keep = vec![true; params.len()];
        for index in 0..params.len() {
            let attrs = self.attributes(params[index].node.attrs.clone());
            let superseded = attrs.iter().any(|attr| {
                self.stub_kind(attr) == Some(Kind::Availability) && self.is_closed_range(attr)
            });
            if superseded {
                let repeated = names.get(index + 1) == Some(&names[index])
                    || names[..index].contains(&names[index]);
                if params[index].node.default.is_none() || repeated {
                    keep[index] = false;
                    self.stub_attributes_changed.set(true);
                    continue;
                }
            }
            if let Some(last) = attrs.last()
                && self.stub_kind(last) == Some(Kind::TypeAware)
                && params[index].node.type_hint.is_none()
                && let Some(ty) = self.newest_type(last)
                && !ty.contains(['[', ']'])
            {
                let at = Span {
                    start: params[index].node.name.span.start,
                    end: params[index].node.name.span.start,
                };
                if let Some(hint) = self.parse_stub_native_type(&ty, at) {
                    params[index].node.type_hint = Some(hint);
                    params[index].node.type_aware_override = Some(ty.clone());
                    // Preserve the parameter override on the node itself as
                    // well as in the enclosing declaration's PHPDoc. Readers
                    // can then apply it to this exact parameter without
                    // accidentally borrowing a stale tag from another
                    // declaration or relying on a synthesized hint surviving
                    // a later owner handoff.
                    let doc_text: Arc<str> =
                        format!("/**\n * @param {ty} ${}\n */", names[index]).into();
                    self.node_arenas.borrow_mut().doc_sites.push(NodeDocSite {
                        pos: params[index].node.span.start,
                        line: self.line_index.line(params[index].node.span.start),
                        text: doc_text,
                    });
                    let line = format!("@param {ty} ${}", names[index]);
                    if let Some(previous) = params[..index]
                        .iter_mut()
                        .rev()
                        .find(|param| param.doc_comment.is_some())
                    {
                        self.override_param_tag(
                            &mut previous.doc_comment,
                            &line,
                            names[index].as_str(),
                        );
                    } else {
                        self.override_param_tag(function_doc, &line, names[index].as_str());
                    }
                    self.stub_attributes_changed.set(true);
                }
            }
        }
        let mut index = 0;
        params.retain(|_| {
            let retain = keep[index];
            index += 1;
            retain
        });
        for draft in params {
            draft.node.attrs = self.strip_stub_attributes(draft.node.attrs.clone());
        }
    }

    pub(crate) fn transform_stub_class_attrs(
        &mut self,
        class: &mut ClassDef,
        mut statement_doc: Option<&mut Option<DocCommentRanges>>,
    ) {
        for ty in self.return_overrides(class.attrs.clone()) {
            self.append_tag(&mut class.doc_comment, &format!("@return {ty}"));
            if let Some(doc) = statement_doc.as_deref_mut() {
                self.append_tag(doc, &format!("@return {ty}"));
            }
        }
        class.attrs = self.strip_stub_attributes(class.attrs.clone());
    }

    pub(crate) fn transform_stub_member_attrs(
        &mut self,
        attrs: AttributeRange,
        doc: &mut Option<DocCommentRanges>,
    ) -> AttributeRange {
        for ty in self.return_overrides(attrs.clone()) {
            self.append_tag(doc, &format!("@return {ty}"));
        }
        self.strip_stub_attributes(attrs)
    }

    pub(crate) fn stub_attributes_changed(&self) -> bool {
        self.stub_attributes_changed.get()
    }
}

#[cfg(test)]
mod tests;
