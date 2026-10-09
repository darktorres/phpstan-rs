//! In-memory catalog registry and exact-entry selection for the Phase F slice.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{BehaviorGraph, Digest, Domain, DomainCoverage, GraphError, SemanticHost, evaluate};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EntryKey {
    Function {
        extension: crate::Bytes,
        name: crate::Bytes,
    },
    Method {
        extension: crate::Bytes,
        class: crate::Bytes,
        name: crate::Bytes,
    },
    Opcode {
        number: u32,
        operand_kinds: Vec<u8>,
    },
    Construct {
        kind: crate::Bytes,
        context: crate::Bytes,
    },
}

impl EntryKey {
    fn normalized(&self) -> Self {
        fn fold(value: &crate::Bytes) -> crate::Bytes {
            crate::Bytes(value.0.iter().map(u8::to_ascii_lowercase).collect())
        }
        match self {
            Self::Function { extension, name } => Self::Function {
                extension: fold(extension),
                name: fold(name),
            },
            Self::Method {
                extension,
                class,
                name,
            } => Self::Method {
                extension: fold(extension),
                class: fold(class),
                name: fold(name),
            },
            Self::Opcode {
                number,
                operand_kinds,
            } => Self::Opcode {
                number: *number,
                operand_kinds: operand_kinds.clone(),
            },
            Self::Construct { kind, context } => Self::Construct {
                kind: fold(kind),
                context: fold(context),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceKind {
    SourceTransfer,
    ModelContract,
    DispatchResolution,
    Invariant,
    Widening,
    UnknownBoundary,
    Observation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub kind: EvidenceKind,
    pub source_digest: Digest,
    pub site: Option<crate::Bytes>,
    pub rule_version: crate::Bytes,
    pub assumptions: Vec<Digest>,
    pub dependencies: Vec<Digest>,
}

impl EvidenceRecord {
    pub fn id(&self) -> Result<Digest, postcard::Error> {
        Ok(Digest(Sha256::digest(postcard::to_allocvec(self)?).into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub key: EntryKey,
    pub graph: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    Graph(GraphError),
    GraphEncoding,
    DuplicateEntry(EntryKey),
    ConflictingEvidence(Digest),
    MissingEvidence(Digest),
}

#[derive(Debug, Default)]
pub struct Catalog {
    graphs: BTreeMap<Digest, BehaviorGraph>,
    entries: BTreeMap<EntryKey, CatalogEntry>,
    evidence: BTreeMap<Digest, EvidenceRecord>,
}

impl Catalog {
    pub fn add_evidence(&mut self, record: EvidenceRecord) -> Result<Digest, CatalogError> {
        let id = record.id().map_err(|_| CatalogError::GraphEncoding)?;
        if self.evidence.get(&id).is_some_and(|old| old != &record) {
            return Err(CatalogError::ConflictingEvidence(id));
        }
        self.evidence.insert(id, record);
        Ok(id)
    }

    pub fn add_entry(
        &mut self,
        key: EntryKey,
        graph: BehaviorGraph,
    ) -> Result<CatalogEntry, CatalogError> {
        graph.validate().map_err(CatalogError::Graph)?;
        let key = key.normalized();
        if self.entries.contains_key(&key) {
            return Err(CatalogError::DuplicateEntry(key));
        }
        for domain in &graph.coverage {
            for evidence in domain.evidence.iter().chain(domain.assumptions.iter()) {
                if !self.evidence.contains_key(evidence) {
                    return Err(CatalogError::MissingEvidence(*evidence));
                }
            }
        }
        let id = graph
            .canonical_digest()
            .map_err(|_| CatalogError::GraphEncoding)?;
        self.graphs.entry(id).or_insert(graph);
        let entry = CatalogEntry {
            key: key.clone(),
            graph: id,
        };
        self.entries.insert(key, entry.clone());
        Ok(entry)
    }

    pub fn select(&self, key: &EntryKey) -> Option<Selection<'_>> {
        let entry = self.entries.get(&key.normalized())?;
        Some(Selection {
            entry,
            graph: self
                .graphs
                .get(&entry.graph)
                .expect("entry graph is installed with the entry"),
        })
    }

    pub fn project(&self, key: &EntryKey, domain: Domain) -> Option<Projection<'_>> {
        let selection = self.select(key)?;
        let coverage = selection
            .graph
            .coverage
            .iter()
            .find(|item| item.domain == domain)?;
        Some(Projection {
            entry: selection.entry,
            coverage,
        })
    }

    pub fn explain(&self, id: Digest) -> Option<&EvidenceRecord> {
        self.evidence.get(&id)
    }
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Selection<'a> {
    pub entry: &'a CatalogEntry,
    pub graph: &'a BehaviorGraph,
}

impl Selection<'_> {
    pub fn evaluate<H: SemanticHost>(
        &self,
        arguments: &[H::Value],
        receiver: &H::Value,
        initial: H::State,
        host: &mut H,
    ) -> Result<crate::Evaluation<H::Value, H::State>, GraphError> {
        evaluate(self.graph, arguments, receiver, initial, host)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Projection<'a> {
    pub entry: &'a CatalogEntry,
    pub coverage: &'a DomainCoverage,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, Bytes, Coverage, SCHEMA_MAJOR, Terminator};

    fn evidence_record() -> EvidenceRecord {
        EvidenceRecord {
            kind: EvidenceKind::ModelContract,
            source_digest: Digest([9; 32]),
            site: Some(Bytes(b"reference-contract:strlen".to_vec())),
            rule_version: Bytes(b"test-v1".to_vec()),
            assumptions: vec![],
            dependencies: vec![],
        }
    }

    fn graph(evidence: Digest) -> BehaviorGraph {
        BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![Block::new(
                vec![],
                Terminator::Return(Some(crate::Operand::Literal(Bytes(b"result".to_vec())))),
            )],
            coverage: Domain::ALL
                .into_iter()
                .map(|domain| DomainCoverage {
                    domain,
                    coverage: Coverage::Closed,
                    precision: crate::Precision::Exact,
                    evidence: vec![evidence],
                    assumptions: vec![],
                })
                .collect(),
        }
    }

    fn function(extension: &[u8], name: &[u8]) -> EntryKey {
        EntryKey::Function {
            extension: Bytes(extension.to_vec()),
            name: Bytes(name.to_vec()),
        }
    }

    #[test]
    fn exact_selection_folds_ascii_symbols_and_preserves_missing_entries() {
        let mut catalog = Catalog::default();
        let evidence = catalog.add_evidence(evidence_record()).unwrap();
        catalog
            .add_entry(function(b"Core", b"strlen"), graph(evidence))
            .unwrap();
        assert!(catalog.select(&function(b"CORE", b"STRLEN")).is_some());
        assert!(catalog.select(&function(b"core", b"other")).is_none());
        assert_eq!(catalog.entry_count(), 1);
    }

    #[test]
    fn catalog_rejects_missing_evidence_and_explains_accepted_contract() {
        let mut catalog = Catalog::default();
        let record = evidence_record();
        let id = record.id().unwrap();
        assert_eq!(
            catalog.add_entry(function(b"core", b"strlen"), graph(id)),
            Err(CatalogError::MissingEvidence(id))
        );
        catalog.add_evidence(record.clone()).unwrap();
        catalog
            .add_entry(function(b"core", b"strlen"), graph(id))
            .unwrap();
        assert_eq!(catalog.explain(id), Some(&record));
        assert_eq!(
            catalog
                .project(&function(b"CORE", b"STRLEN"), Domain::Return)
                .unwrap()
                .coverage
                .coverage,
            Coverage::Closed
        );
    }
}
