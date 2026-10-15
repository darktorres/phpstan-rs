//! What is kept of function and method bodies, per file rather than in the
//! registry: a declaration's provenance finds its body's summary and flow.
//! The statements themselves are kept only in a debug build, which checks
//! the summaries against them.

use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;
use syntax::Source;
use syntax::ast::{FunctionDecl, MethodDecl, Stmt};
use syntax::name_resolution::{NameContext, StoredContext, for_each_in_context};
use syntax::span::Span;
use syntax::visit::{self, Visitor};

use crate::registry::{BodySummary, InitFlow, Provenance};

/// Where the body of a function or method is found from its provenance.
pub trait BodySource: Sync {
    /// The statements of the body of the function or method declared where
    /// `provenance` records, when its file is at hand.
    #[cfg(debug_assertions)]
    fn body(&self, provenance: &Provenance) -> Option<&Arc<Vec<Stmt>>>;

    /// What the passes read of that body.
    fn summary(&self, provenance: &Provenance) -> Option<&Arc<BodySummary>>;

    /// The flow of that body, when it is a method's.
    fn init_flow(&self, provenance: &Provenance) -> Option<&Arc<InitFlow>>;
}

/// No bodies at all.
pub struct NoBodies;

impl BodySource for NoBodies {
    #[cfg(debug_assertions)]
    fn body(&self, _: &Provenance) -> Option<&Arc<Vec<Stmt>>> {
        None
    }

    fn summary(&self, _: &Provenance) -> Option<&Arc<BodySummary>> {
        None
    }

    fn init_flow(&self, _: &Provenance) -> Option<&Arc<InitFlow>> {
        None
    }
}

/// What the passes read of one body's statements.
#[derive(Debug, Clone)]
struct Read {
    #[cfg(debug_assertions)]
    stmts: Option<Arc<Vec<Stmt>>>,
    summary: Arc<BodySummary>,
    /// A method's flow, which a call to it is followed through.
    flow: Option<Arc<InitFlow>>,
}

impl Read {
    fn of(body: &syntax::ast::FunctionBody, is_method: bool) -> Read {
        Read {
            #[cfg(debug_assertions)]
            stmts: Some(body.stmts.clone()),
            summary: Arc::new(BodySummary::of(&body.stmts)),
            flow: is_method.then(|| Arc::new(InitFlow::of(&body.stmts, &body.goto_labels))),
        }
    }
}

/// A body that is read when something asks for it: where its text is, and
/// the names in effect where it is declared.
#[derive(Debug, Clone)]
struct Skimmed {
    /// The byte offsets of the body in its file, from its `{` to its `}`.
    start: u32,
    end: u32,
    /// Where its text is in the file's kept text.
    at: u32,
    /// Which of the file's contexts is in effect at its declaration.
    context: u32,
    is_method: bool,
    /// What the body is read as; `None` when its text does not parse,
    /// which a body of a file that parsed does.
    read: OnceLock<Option<Read>>,
}

#[derive(Debug, Clone)]
enum Body {
    Read(Read),
    Skimmed(Skimmed),
}

/// One file's function and method bodies, anonymous classes' included,
/// by the span of their name.
///
/// A file with no declaration inside a body and no anonymous class keeps
/// each body as its text, which is parsed when a pass reads it: the
/// summaries and flows are made then, from the text with the names in
/// effect where the body is declared, and are what the statements give.
#[derive(Debug, Default, Clone)]
pub struct FileBodies {
    bodies: FxHashMap<Span, Body>,
    /// The name contexts the skimmed bodies are declared in.
    contexts: Vec<NameContext>,
    /// The bytes of the skimmed bodies, one after another.
    text: Arc<[u8]>,
}

impl FileBodies {
    pub fn of(stmts: &[Stmt]) -> FileBodies {
        FileBodies::collect(stmts, true, None)
    }

    /// The bodies of a file whose facts are known: its expressions are
    /// walked only when a body can be in one, in an anonymous class or a
    /// closure. With `source`, the file's bytes, its bodies are kept as
    /// text when its facts allow.
    pub fn of_file(
        stmts: &[Stmt],
        facts: &syntax::FileFacts,
        source: Option<Source<'_>>,
    ) -> FileBodies {
        let nothing_declared_in_bodies = !facts.has_anonymous_class
            && !facts.has_nested_declaration
            && !facts.has_declaration_in_body;
        let skim = source
            .filter(|_| nothing_declared_in_bodies)
            .map(|source| match source {
                Source::Text(text) => text.as_bytes(),
                Source::Bytes(bytes) => bytes,
            });
        let bodies = FileBodies::collect(
            stmts,
            facts.has_anonymous_class || facts.has_nested_declaration,
            skim,
        );
        // What a walk of the expressions too finds.
        #[cfg(debug_assertions)]
        {
            let eager = FileBodies::collect(stmts, true, None);
            assert_eq!(
                bodies.bodies.len(),
                eager.bodies.len(),
                "a file whose facts say it has no body in an expression has"
            );
            // What the text of each skimmed body is read as.
            for (span, body) in &bodies.bodies {
                let Body::Skimmed(_) = body else { continue };
                let expected = eager.summary(*span).expect("a body of the eager walk");
                let read = bodies
                    .summary(*span)
                    .expect("a skimmed body of a parsed file parses");
                assert_eq!(
                    format!("{read:?}"),
                    format!("{expected:?}"),
                    "a body read from its text and from the file's tree differ in their summaries"
                );
                assert_eq!(
                    bodies.init_flow(*span).map(|flow| format!("{flow:?}")),
                    eager.init_flow(*span).map(|flow| format!("{flow:?}")),
                    "a body read from its text and from the file's tree differ in their flows"
                );
            }
        }
        bodies
    }

    /// `self` as it was decoded, with the bytes of its file: the text of
    /// each skimmed body is taken from them.
    pub fn with_source(mut self, source: &[u8]) -> FileBodies {
        let mut skimmed: Vec<&mut Skimmed> = self
            .bodies
            .values_mut()
            .filter_map(|body| match body {
                Body::Skimmed(skimmed) => Some(skimmed),
                Body::Read(_) => None,
            })
            .collect();
        skimmed.sort_unstable_by_key(|skimmed| skimmed.start);
        let mut text = Vec::new();
        for body in skimmed {
            body.at = text.len() as u32;
            let range = body.start as usize..body.end as usize;
            // A file whose bytes are not the stored ones has no body here;
            // the file's key makes that not happen.
            match source.get(range) {
                Some(bytes) => text.extend_from_slice(bytes),
                None => body.end = body.start,
            }
        }
        self.text = text.into();
        self
    }

    fn collect(stmts: &[Stmt], in_expressions: bool, skim: Option<&[u8]>) -> FileBodies {
        struct Collect<'s> {
            bodies: FxHashMap<Span, Body>,
            // Whether the next function declaration visited is a method.
            method: bool,
            in_expressions: bool,
            // The file's bytes, when its bodies are skimmed.
            skim: Option<&'s [u8]>,
            contexts: Vec<NameContext>,
            // The context in effect where the declaration being visited is.
            context: u32,
            text: Vec<u8>,
        }
        impl Collect<'_> {
            /// Notes the context in effect from here on.
            fn enter(&mut self, ctx: &NameContext) {
                let at = match self
                    .contexts
                    .iter()
                    .rposition(|held| held.version() == ctx.version())
                {
                    Some(at) => at,
                    None => {
                        self.contexts.push(ctx.clone());
                        self.contexts.len() - 1
                    }
                };
                self.context = at as u32;
            }

            /// The body of `function`, as text, when the file's bytes
            /// hold it.
            fn skimmed(
                &mut self,
                function: &FunctionDecl,
                body: &syntax::ast::FunctionBody,
                is_method: bool,
            ) -> Option<Body> {
                let (start, end) = (body.open.start, function.span.end);
                let bytes = self.skim?.get(start as usize..end as usize)?;
                debug_assert!(bytes.first() == Some(&b'{') && bytes.last() == Some(&b'}'));
                let at = self.text.len() as u32;
                self.text.extend_from_slice(bytes);
                Some(Body::Skimmed(Skimmed {
                    start,
                    end,
                    at,
                    context: self.context,
                    is_method,
                    read: OnceLock::new(),
                }))
            }
        }
        impl<'ast> Visitor<'ast> for Collect<'_> {
            fn visit_expr(&mut self, expr: &'ast syntax::ast::Expr) {
                if self.in_expressions {
                    visit::walk_expr(self, expr);
                }
            }

            fn visit_method_decl(&mut self, method: &'ast MethodDecl) {
                self.method = true;
                visit::walk_method_decl(self, method);
            }

            fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
                let is_method = std::mem::take(&mut self.method);
                if let Some(body) = &function.body {
                    let body_kept = self
                        .skimmed(function, body, is_method)
                        .unwrap_or_else(|| Body::Read(Read::of(body, is_method)));
                    let skimmed = matches!(body_kept, Body::Skimmed(_));
                    self.bodies.insert(function.name.span, body_kept);
                    // A file that is skimmed declares nothing in a body.
                    if skimmed {
                        return;
                    }
                }
                visit::walk_function_decl(self, function);
            }
        }
        let mut collect = Collect {
            bodies: FxHashMap::default(),
            method: false,
            in_expressions,
            skim,
            contexts: Vec::new(),
            context: 0,
            text: Vec::new(),
        };
        if skim.is_some() {
            for_each_in_context(stmts, |stmt, ctx| {
                collect.enter(ctx);
                collect.visit_stmt(stmt);
            });
        } else {
            visit::walk_stmts(&mut collect, stmts);
        }
        FileBodies {
            bodies: collect.bodies,
            contexts: collect.contexts,
            text: collect.text.into(),
        }
    }

    /// What the body at `span` is read as.
    fn read(&self, span: Span) -> Option<&Read> {
        match self.bodies.get(&span)? {
            Body::Read(read) => Some(read),
            Body::Skimmed(skimmed) => skimmed.read.get_or_init(|| self.parse(skimmed)).as_ref(),
        }
    }

    /// The body `skimmed` is the text of, read.
    fn parse(&self, skimmed: &Skimmed) -> Option<Read> {
        let length = (skimmed.end - skimmed.start) as usize;
        let bytes = self.text.get(skimmed.at as usize..)?.get(..length)?;
        let context = self.contexts.get(skimmed.context as usize)?;
        let body = syntax::parse_function_body(bytes, skimmed.start, context).ok()?;
        Some(Read::of(&body, skimmed.is_method))
    }

    /// The span of the name of every declaration with a body, in order.
    #[cfg(test)]
    pub(crate) fn spans(&self) -> Vec<Span> {
        let mut spans: Vec<Span> = self.bodies.keys().copied().collect();
        spans.sort_by_key(|span| (span.start, span.end));
        spans
    }

    /// Whether the body at `span` is kept as text.
    #[cfg(test)]
    pub(crate) fn is_skimmed(&self, span: Span) -> bool {
        matches!(self.bodies.get(&span), Some(Body::Skimmed(_)))
    }

    /// The statements of the body of the declaration whose name is at `span`.
    #[cfg(debug_assertions)]
    pub fn get(&self, span: Span) -> Option<&Arc<Vec<Stmt>>> {
        self.read(span)?.stmts.as_ref()
    }

    /// What the passes read of the body of the declaration whose name is at
    /// `span`.
    pub fn summary(&self, span: Span) -> Option<&Arc<BodySummary>> {
        self.read(span).map(|read| &read.summary)
    }

    /// The flow of the method whose name is at `span`.
    pub fn init_flow(&self, span: Span) -> Option<&Arc<InitFlow>> {
        self.read(span)?.flow.as_ref()
    }
}

/// A body as stored.
#[derive(serde::Serialize, serde::Deserialize)]
enum StoredBody {
    Read(Arc<BodySummary>, Option<Arc<InitFlow>>),
    /// Where its text is in the file, and which context it is declared in.
    Skimmed {
        start: u32,
        end: u32,
        context: u32,
        is_method: bool,
    },
}

/// A file's bodies as stored: by the span of the declaration's name, in
/// that order, without the statements and without the text of a skimmed
/// body, which is taken from the file's bytes again.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredBodies {
    contexts: Vec<StoredContext>,
    bodies: Vec<(Span, StoredBody)>,
}

impl serde::Serialize for FileBodies {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut bodies: Vec<(&Span, &Body)> = self.bodies.iter().collect();
        bodies.sort_by_key(|(span, _)| (span.start, span.end));
        let stored = StoredBodies {
            contexts: self.contexts.iter().map(NameContext::to_stored).collect(),
            bodies: bodies
                .into_iter()
                .map(|(span, body)| {
                    let stored = match body {
                        Body::Read(read) => {
                            StoredBody::Read(read.summary.clone(), read.flow.clone())
                        }
                        Body::Skimmed(skimmed) => StoredBody::Skimmed {
                            start: skimmed.start,
                            end: skimmed.end,
                            context: skimmed.context,
                            is_method: skimmed.is_method,
                        },
                    };
                    (*span, stored)
                })
                .collect(),
        };
        serde::Serialize::serialize(&stored, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for FileBodies {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<FileBodies, D::Error> {
        let stored: StoredBodies = serde::Deserialize::deserialize(deserializer)?;
        Ok(FileBodies {
            contexts: stored
                .contexts
                .into_iter()
                .map(NameContext::from_stored)
                .collect(),
            bodies: stored
                .bodies
                .into_iter()
                .map(|(span, body)| {
                    let body = match body {
                        StoredBody::Read(summary, flow) => Body::Read(Read {
                            #[cfg(debug_assertions)]
                            stmts: None,
                            summary,
                            flow,
                        }),
                        StoredBody::Skimmed {
                            start,
                            end,
                            context,
                            is_method,
                        } => Body::Skimmed(Skimmed {
                            start,
                            end,
                            at: 0,
                            context,
                            is_method,
                            read: OnceLock::new(),
                        }),
                    };
                    (span, body)
                })
                .collect(),
            text: Arc::default(),
        })
    }
}

/// A single file's bodies answer for any declaration, whatever file it
/// records: a registry built from one program.
impl BodySource for FileBodies {
    #[cfg(debug_assertions)]
    fn body(&self, provenance: &Provenance) -> Option<&Arc<Vec<Stmt>>> {
        self.get(provenance.declaration_span())
    }

    fn summary(&self, provenance: &Provenance) -> Option<&Arc<BodySummary>> {
        self.summary(provenance.declaration_span())
    }

    fn init_flow(&self, provenance: &Provenance) -> Option<&Arc<InitFlow>> {
        self.init_flow(provenance.declaration_span())
    }
}
