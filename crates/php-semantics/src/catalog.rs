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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    DigestMismatch(Digest),
    UnsupportedManifest(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRecord {
    pub id: Digest,
    pub graph: BehaviorGraph,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogManifest {
    pub schema_major: u32,
    pub entries: Vec<CatalogEntry>,
    pub graphs: Vec<GraphRecord>,
    pub evidence: Vec<(Digest, EvidenceRecord)>,
}

impl CatalogManifest {
    pub fn canonical_digest(&self) -> Result<Digest, postcard::Error> {
        Ok(Digest(Sha256::digest(postcard::to_allocvec(self)?).into()))
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema_major != crate::SCHEMA_MAJOR {
            return Err(CatalogError::UnsupportedManifest(self.schema_major));
        }
        let mut graph_ids = Vec::new();
        for record in &self.graphs {
            if record
                .graph
                .canonical_digest()
                .map_err(|_| CatalogError::GraphEncoding)?
                != record.id
            {
                return Err(CatalogError::DigestMismatch(record.id));
            }
            record.graph.validate().map_err(CatalogError::Graph)?;
            if graph_ids.contains(&record.id) {
                return Err(CatalogError::DigestMismatch(record.id));
            }
            graph_ids.push(record.id);
        }
        let mut entry_keys = Vec::new();
        for entry in &self.entries {
            if entry.key != entry.key.normalized() || entry_keys.contains(&entry.key) {
                return Err(CatalogError::DuplicateEntry(entry.key.clone()));
            }
            if !graph_ids.contains(&entry.graph) {
                return Err(CatalogError::DigestMismatch(entry.graph));
            }
            entry_keys.push(entry.key.clone());
        }
        let mut evidence_ids = Vec::new();
        for (id, record) in &self.evidence {
            if record.id().map_err(|_| CatalogError::GraphEncoding)? != *id
                || evidence_ids.contains(id)
            {
                return Err(CatalogError::DigestMismatch(*id));
            }
            evidence_ids.push(*id);
        }
        for record in &self.graphs {
            for item in &record.graph.coverage {
                for id in item.evidence.iter().chain(item.assumptions.iter()) {
                    if !evidence_ids.contains(id) {
                        return Err(CatalogError::MissingEvidence(*id));
                    }
                }
            }
        }
        for (_, record) in &self.evidence {
            for dependency in &record.dependencies {
                if !evidence_ids.contains(dependency) {
                    return Err(CatalogError::MissingEvidence(*dependency));
                }
            }
        }
        Ok(())
    }
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

    pub fn manifest(&self) -> CatalogManifest {
        CatalogManifest {
            schema_major: crate::SCHEMA_MAJOR,
            entries: self.entries.values().cloned().collect(),
            graphs: self
                .graphs
                .iter()
                .map(|(id, graph)| GraphRecord {
                    id: *id,
                    graph: graph.clone(),
                })
                .collect(),
            evidence: self
                .evidence
                .iter()
                .map(|(id, record)| (*id, record.clone()))
                .collect(),
        }
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

    #[test]
    fn canonical_manifest_round_trips_and_rejects_tampered_graph_identity() {
        let mut catalog = Catalog::default();
        let evidence = catalog.add_evidence(evidence_record()).unwrap();
        catalog
            .add_entry(function(b"core", b"strlen"), graph(evidence))
            .unwrap();
        let manifest = catalog.manifest();
        manifest.validate().unwrap();
        let json = manifest.to_json().unwrap();
        assert!(json.contains("\"base64\""));
        let decoded: CatalogManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, manifest);
        assert_eq!(
            decoded.canonical_digest().unwrap(),
            manifest.canonical_digest().unwrap()
        );

        let mut tampered = manifest;
        tampered.graphs[0].id = Digest([0; 32]);
        assert_eq!(
            tampered.validate(),
            Err(CatalogError::DigestMismatch(Digest([0; 32])))
        );
    }
}
