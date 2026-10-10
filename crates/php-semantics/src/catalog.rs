//! In-memory catalog registry and exact-entry selection for the Phase F slice.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    Applicability, BehaviorGraph, Digest, Domain, DomainCoverage, GraphError, SemanticHost,
    TargetError, TargetPredicate, TargetScenario, TargetSet, evaluate,
};

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

/// The kind of condition on which a catalog fact relies. This is deliberately
/// distinct from `EvidenceKind`: assumptions describe applicability, while
/// evidence describes support for a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssumptionKind {
    SourceBuildIdentity,
    LoadedProgram,
    TargetProfile,
    NativeBehavior,
    EngineInvariant,
    ModelContract,
    DispatchCondition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssumptionScope {
    pub target: TargetPredicate,
    /// Required loaded-program identity, when the assumption is program-local.
    pub loaded_program: Option<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssumptionValidator {
    pub id: crate::Bytes,
    pub version: crate::Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssumptionRecord {
    pub kind: AssumptionKind,
    pub claim: crate::Bytes,
    pub scope: AssumptionScope,
    pub prerequisites: Vec<Digest>,
    pub supporting_evidence: Vec<Digest>,
    pub validator: AssumptionValidator,
}

impl AssumptionRecord {
    pub fn id(&self) -> Result<Digest, postcard::Error> {
        let encoded = postcard::to_allocvec(self)?;
        let mut hasher = Sha256::new();
        hasher.update(b"php-semantics-assumption-v1\0");
        hasher.update(crate::SCHEMA_MAJOR.to_le_bytes());
        hasher.update(encoded);
        Ok(Digest(hasher.finalize().into()))
    }

    /// Applicability is only a context match. It does not mean the named
    /// validator is trusted or that supporting evidence has been checked.
    pub fn applicability(
        &self,
        target: &TargetScenario,
        loaded_program: Option<Digest>,
    ) -> Applicability {
        let target_match = self.scope.target.test(target);
        if target_match == Applicability::No {
            return Applicability::No;
        }
        match (self.scope.loaded_program, loaded_program) {
            (Some(required), Some(actual)) if required != actual => Applicability::No,
            (Some(_), None) => Applicability::Maybe,
            _ => target_match,
        }
    }

    fn is_well_formed(&self) -> bool {
        !self.claim.0.is_empty()
            && !self.validator.id.0.is_empty()
            && !self.validator.version.0.is_empty()
            && !self.supporting_evidence.is_empty()
            && self.scope.target.validate().is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub key: EntryKey,
    pub graph: Digest,
    pub applicability: TargetPredicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    Graph(GraphError),
    GraphEncoding,
    DuplicateEntry(EntryKey),
    ConflictingEvidence(Digest),
    MissingEvidence(Digest),
    MissingAssumption(Digest),
    InvalidAssumption(Digest),
    InvalidTargetPredicate(TargetError),
    UnpropagatedEvidenceAssumption {
        evidence: Digest,
        assumption: Digest,
    },
    CyclicSupport(Vec<SupportNode>),
    DigestMismatch(Digest),
    UnsupportedManifest(u32),
}

/// A node in the catalog's combined evidence/assumption support graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SupportNode {
    Evidence(Digest),
    Assumption(Digest),
}

impl SupportNode {
    fn successors(
        self,
        evidence: &BTreeMap<Digest, EvidenceRecord>,
        assumptions: &BTreeMap<Digest, AssumptionRecord>,
    ) -> Vec<Self> {
        match self {
            Self::Evidence(id) => evidence.get(&id).map_or_else(Vec::new, |record| {
                record
                    .dependencies
                    .iter()
                    .copied()
                    .map(Self::Evidence)
                    .chain(record.assumptions.iter().copied().map(Self::Assumption))
                    .collect()
            }),
            Self::Assumption(id) => assumptions.get(&id).map_or_else(Vec::new, |record| {
                record
                    .prerequisites
                    .iter()
                    .copied()
                    .map(Self::Assumption)
                    .chain(
                        record
                            .supporting_evidence
                            .iter()
                            .copied()
                            .map(Self::Evidence),
                    )
                    .collect()
            }),
        }
    }
}

fn find_support_cycle(
    evidence: &BTreeMap<Digest, EvidenceRecord>,
    assumptions: &BTreeMap<Digest, AssumptionRecord>,
) -> Option<Vec<SupportNode>> {
    let nodes = evidence
        .keys()
        .copied()
        .map(SupportNode::Evidence)
        .chain(assumptions.keys().copied().map(SupportNode::Assumption));
    let mut active = Vec::new();
    let mut active_positions = BTreeMap::new();
    let mut complete = std::collections::BTreeSet::new();
    for node in nodes {
        if complete.contains(&node) {
            continue;
        }
        active.push((node, node.successors(evidence, assumptions), 0));
        active_positions.insert(node, 0);
        while let Some((current, successors, next)) = active.last_mut() {
            if *next == successors.len() {
                let finished = *current;
                active.pop();
                active_positions.remove(&finished);
                complete.insert(finished);
                continue;
            }
            let successor = successors[*next];
            *next += 1;
            if let Some(start) = active_positions.get(&successor).copied() {
                let mut cycle: Vec<_> = active[start..]
                    .iter()
                    .map(|(active_node, _, _)| *active_node)
                    .collect();
                cycle.push(successor);
                return Some(cycle);
            }
            if !complete.contains(&successor) {
                let position = active.len();
                active_positions.insert(successor, position);
                active.push((successor, successor.successors(evidence, assumptions), 0));
            }
        }
    }
    None
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
    pub assumptions: Vec<(Digest, AssumptionRecord)>,
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
        // Check graph structure before content IDs so malformed cyclic input
        // receives the actionable cycle diagnostic instead of only a digest
        // mismatch. Well-formed content-addressed cycles are not constructible
        // without a hash fixed point, but untrusted manifests still need a
        // direct, fail-closed structural check.
        let evidence = self.evidence.iter().cloned().collect();
        let assumptions = self.assumptions.iter().cloned().collect();
        if let Some(cycle) = find_support_cycle(&evidence, &assumptions) {
            return Err(CatalogError::CyclicSupport(cycle));
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
            entry
                .applicability
                .validate()
                .map_err(CatalogError::InvalidTargetPredicate)?;
            if entry.key != entry.key.normalized()
                || entry_keys.contains(&(entry.key.clone(), entry.applicability.clone()))
            {
                return Err(CatalogError::DuplicateEntry(entry.key.clone()));
            }
            if !graph_ids.contains(&entry.graph) {
                return Err(CatalogError::DigestMismatch(entry.graph));
            }
            entry_keys.push((entry.key.clone(), entry.applicability.clone()));
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
        let mut assumption_ids = Vec::new();
        for (id, record) in &self.assumptions {
            if record.id().map_err(|_| CatalogError::GraphEncoding)? != *id
                || assumption_ids.contains(id)
                || !record.is_well_formed()
            {
                return Err(CatalogError::InvalidAssumption(*id));
            }
            assumption_ids.push(*id);
        }
        for record in &self.graphs {
            for item in &record.graph.coverage {
                for id in &item.evidence {
                    if !evidence_ids.contains(id) {
                        return Err(CatalogError::MissingEvidence(*id));
                    }
                    let evidence = self
                        .evidence
                        .iter()
                        .find(|(evidence_id, _)| evidence_id == id)
                        .map(|(_, evidence)| evidence)
                        .expect("validated evidence ID is present");
                    for assumption in &evidence.assumptions {
                        if !assumption_ids.contains(assumption) {
                            return Err(CatalogError::MissingAssumption(*assumption));
                        }
                        if !item.assumptions.contains(assumption) {
                            return Err(CatalogError::UnpropagatedEvidenceAssumption {
                                evidence: *id,
                                assumption: *assumption,
                            });
                        }
                    }
                }
                for id in &item.assumptions {
                    if !assumption_ids.contains(id) {
                        return Err(CatalogError::MissingAssumption(*id));
                    }
                }
            }
        }
        for (_, record) in &self.evidence {
            for id in &record.assumptions {
                if !assumption_ids.contains(id) {
                    return Err(CatalogError::MissingAssumption(*id));
                }
            }
            for dependency in &record.dependencies {
                if !evidence_ids.contains(dependency) {
                    return Err(CatalogError::MissingEvidence(*dependency));
                }
            }
        }
        for (_, record) in &self.assumptions {
            for id in &record.supporting_evidence {
                if !evidence_ids.contains(id) {
                    return Err(CatalogError::MissingEvidence(*id));
                }
            }
            for id in &record.prerequisites {
                if !assumption_ids.contains(id) {
                    return Err(CatalogError::MissingAssumption(*id));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct Catalog {
    graphs: BTreeMap<Digest, BehaviorGraph>,
    entries: BTreeMap<EntryKey, Vec<CatalogEntry>>,
    evidence: BTreeMap<Digest, EvidenceRecord>,
    assumptions: BTreeMap<Digest, AssumptionRecord>,
}

impl Catalog {
    /// Install a decoded artifact only after validating every graph, key,
    /// evidence edge, and schema version. This is the runtime ingestion path;
    /// callers must not execute a manifest by deserializing its fields alone.
    pub fn from_manifest(manifest: CatalogManifest) -> Result<Self, CatalogError> {
        manifest.validate()?;
        let mut catalog = Self::default();
        catalog.evidence = manifest.evidence.into_iter().collect();
        catalog.assumptions = manifest.assumptions.into_iter().collect();
        catalog.graphs = manifest
            .graphs
            .into_iter()
            .map(|record| (record.id, record.graph))
            .collect();
        for entry in manifest.entries {
            catalog
                .entries
                .entry(entry.key.clone())
                .or_default()
                .push(entry);
        }
        Ok(catalog)
    }

    /// Decode JSON and pass it through the same validation boundary as a
    /// generated in-memory manifest.
    pub fn from_json(json: &str) -> Result<Self, ManifestDecodeError> {
        let manifest = serde_json::from_str(json).map_err(ManifestDecodeError::Json)?;
        Self::from_manifest(manifest).map_err(ManifestDecodeError::Catalog)
    }

    pub fn add_evidence(&mut self, record: EvidenceRecord) -> Result<Digest, CatalogError> {
        let id = record.id().map_err(|_| CatalogError::GraphEncoding)?;
        if self.evidence.get(&id).is_some_and(|old| old != &record) {
            return Err(CatalogError::ConflictingEvidence(id));
        }
        self.evidence.insert(id, record);
        Ok(id)
    }

    pub fn add_assumption(&mut self, record: AssumptionRecord) -> Result<Digest, CatalogError> {
        let id = record.id().map_err(|_| CatalogError::GraphEncoding)?;
        if !record.is_well_formed() {
            return Err(CatalogError::InvalidAssumption(id));
        }
        if self.assumptions.get(&id).is_some_and(|old| old != &record) {
            return Err(CatalogError::InvalidAssumption(id));
        }
        self.assumptions.insert(id, record);
        Ok(id)
    }

    pub fn add_entry(
        &mut self,
        key: EntryKey,
        graph: BehaviorGraph,
    ) -> Result<CatalogEntry, CatalogError> {
        self.add_variant(key, TargetPredicate::any(), graph)
    }

    pub fn add_variant(
        &mut self,
        key: EntryKey,
        applicability: TargetPredicate,
        graph: BehaviorGraph,
    ) -> Result<CatalogEntry, CatalogError> {
        applicability
            .validate()
            .map_err(CatalogError::InvalidTargetPredicate)?;
        let key = key.normalized();
        if self.entries.get(&key).is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.applicability == applicability)
        }) {
            return Err(CatalogError::DuplicateEntry(key));
        }
        for domain in &graph.coverage {
            for evidence in &domain.evidence {
                let record = self
                    .evidence
                    .get(evidence)
                    .ok_or(CatalogError::MissingEvidence(*evidence))?;
                for assumption in &record.assumptions {
                    if !self.assumptions.contains_key(assumption) {
                        return Err(CatalogError::MissingAssumption(*assumption));
                    }
                    if !domain.assumptions.contains(assumption) {
                        return Err(CatalogError::UnpropagatedEvidenceAssumption {
                            evidence: *evidence,
                            assumption: *assumption,
                        });
                    }
                }
            }
            for assumption in &domain.assumptions {
                if !self.assumptions.contains_key(assumption) {
                    return Err(CatalogError::MissingAssumption(*assumption));
                }
            }
        }
        self.validate_assumption_references()?;
        graph.validate().map_err(CatalogError::Graph)?;
        let id = graph
            .canonical_digest()
            .map_err(|_| CatalogError::GraphEncoding)?;
        self.graphs.entry(id).or_insert(graph);
        let entry = CatalogEntry {
            key: key.clone(),
            graph: id,
            applicability,
        };
        self.entries.entry(key).or_default().push(entry.clone());
        Ok(entry)
    }

    fn validate_assumption_references(&self) -> Result<(), CatalogError> {
        for record in self.evidence.values() {
            for assumption in &record.assumptions {
                if !self.assumptions.contains_key(assumption) {
                    return Err(CatalogError::MissingAssumption(*assumption));
                }
            }
            for dependency in &record.dependencies {
                if !self.evidence.contains_key(dependency) {
                    return Err(CatalogError::MissingEvidence(*dependency));
                }
            }
        }
        for record in self.assumptions.values() {
            let id = record.id().map_err(|_| CatalogError::GraphEncoding)?;
            if !record.is_well_formed() {
                return Err(CatalogError::InvalidAssumption(id));
            }
            for evidence in &record.supporting_evidence {
                if !self.evidence.contains_key(evidence) {
                    return Err(CatalogError::MissingEvidence(*evidence));
                }
            }
            for prerequisite in &record.prerequisites {
                if !self.assumptions.contains_key(prerequisite) {
                    return Err(CatalogError::MissingAssumption(*prerequisite));
                }
            }
        }
        if let Some(cycle) = find_support_cycle(&self.evidence, &self.assumptions) {
            return Err(CatalogError::CyclicSupport(cycle));
        }
        Ok(())
    }

    pub fn select(&self, key: &EntryKey) -> Option<Selection<'_>> {
        let entry = self
            .entries
            .get(&key.normalized())?
            .iter()
            .find(|entry| entry.applicability == TargetPredicate::any())?;
        Some(Selection {
            entry,
            graph: self
                .graphs
                .get(&entry.graph)
                .expect("entry graph is installed with the entry"),
        })
    }

    pub fn select_for_targets(
        &self,
        key: &EntryKey,
        targets: &TargetSet,
    ) -> CatalogTargetSelection<'_> {
        let variants = self.entries.get(&key.normalized());
        let mut selected = Vec::new();
        let mut uncovered = Vec::new();
        for scenario in &targets.scenarios {
            let mut any_candidate = false;
            let mut maybe = false;
            if let Some(variants) = variants {
                for entry in variants {
                    let applicability = entry.applicability.test(scenario);
                    if applicability == Applicability::No {
                        continue;
                    }
                    any_candidate = true;
                    maybe |= applicability == Applicability::Maybe;
                    selected.push(VariantSelection {
                        entry,
                        graph: self
                            .graphs
                            .get(&entry.graph)
                            .expect("entry graph installed atomically"),
                        scenario: scenario.clone(),
                        applicability,
                    });
                }
            }
            if !any_candidate || maybe {
                uncovered.push(scenario.clone());
            }
        }
        CatalogTargetSelection {
            variants: selected,
            uncovered,
        }
    }

    pub fn project(&self, key: &EntryKey, domain: Domain) -> Option<Projection<'_>> {
        let selection = self.select(key)?;
        let coverage = selection
            .graph
            .coverage
            .iter()
            .find(|item| item.domain == domain)?;
        let mut pending = coverage.dependencies.clone();
        let mut seen = vec![domain];
        let mut dependencies = Vec::new();
        while let Some(required) = pending.pop() {
            if seen.contains(&required) {
                continue;
            }
            seen.push(required);
            let item = selection
                .graph
                .coverage
                .iter()
                .find(|item| item.domain == required)?;
            dependencies.push(item);
            pending.extend(item.dependencies.iter().copied());
        }
        dependencies.sort_by_key(|item| {
            crate::Domain::ALL
                .iter()
                .position(|domain| *domain == item.domain)
        });
        Some(Projection {
            entry: selection.entry,
            coverage,
            dependencies,
        })
    }

    pub fn explain(&self, id: Digest) -> Option<&EvidenceRecord> {
        self.evidence.get(&id)
    }
    pub fn explain_assumption(&self, id: Digest) -> Option<&AssumptionRecord> {
        self.assumptions.get(&id)
    }
    pub fn entry_count(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }

    pub fn manifest(&self) -> CatalogManifest {
        let mut entries: Vec<_> = self.entries.values().flatten().cloned().collect();
        entries.sort_by(|a, b| {
            a.key
                .cmp(&b.key)
                .then_with(|| a.graph.cmp(&b.graph))
                .then_with(|| {
                    postcard::to_allocvec(&a.applicability)
                        .unwrap_or_default()
                        .cmp(&postcard::to_allocvec(&b.applicability).unwrap_or_default())
                })
        });
        CatalogManifest {
            schema_major: crate::SCHEMA_MAJOR,
            entries,
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
            assumptions: self
                .assumptions
                .iter()
                .map(|(id, record)| (*id, record.clone()))
                .collect(),
        }
    }
}

#[derive(Debug)]
pub enum ManifestDecodeError {
    Json(serde_json::Error),
    Catalog(CatalogError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogTargetSelection<'a> {
    pub variants: Vec<VariantSelection<'a>>,
    pub uncovered: Vec<TargetScenario>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantSelection<'a> {
    pub entry: &'a CatalogEntry,
    pub graph: &'a BehaviorGraph,
    pub scenario: TargetScenario,
    pub applicability: Applicability,
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

#[derive(Debug, Clone)]
pub struct Projection<'a> {
    pub entry: &'a CatalogEntry,
    pub coverage: &'a DomainCoverage,
    /// Transitive domain dependencies in canonical `Domain::ALL` order.
    pub dependencies: Vec<&'a DomainCoverage>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Applicability, Block, Bytes, Coverage, Event, GraphError, SCHEMA_MAJOR, TargetPredicate,
        TargetScenario, TargetSet, Terminator, VersionRange,
    };

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

    fn assumption_record(evidence: Digest) -> AssumptionRecord {
        AssumptionRecord {
            kind: AssumptionKind::TargetProfile,
            claim: Bytes(b"the selected target matches this tested profile".to_vec()),
            scope: AssumptionScope {
                target: TargetPredicate::any(),
                loaded_program: None,
            },
            prerequisites: vec![],
            supporting_evidence: vec![evidence],
            validator: AssumptionValidator {
                id: Bytes(b"php-semantics-test-validator".to_vec()),
                version: Bytes(b"1".to_vec()),
            },
        }
    }

    fn support_graph_records(
        evidence_edges: &[(u8, Vec<Digest>, Vec<Digest>)],
        assumption_edges: &[(u8, Vec<Digest>, Vec<Digest>)],
    ) -> (
        BTreeMap<Digest, EvidenceRecord>,
        BTreeMap<Digest, AssumptionRecord>,
    ) {
        let evidence = evidence_edges
            .iter()
            .map(|(id, assumptions, dependencies)| {
                let mut record = evidence_record();
                record.assumptions = assumptions.clone();
                record.dependencies = dependencies.clone();
                (Digest([*id; 32]), record)
            })
            .collect();
        let assumptions = assumption_edges
            .iter()
            .map(|(id, prerequisites, supporting_evidence)| {
                let mut record = assumption_record(Digest([0; 32]));
                record.prerequisites = prerequisites.clone();
                record.supporting_evidence = supporting_evidence.clone();
                (Digest([*id; 32]), record)
            })
            .collect();
        (evidence, assumptions)
    }

    #[test]
    fn support_graph_reports_self_and_multi_node_assumption_cycles() {
        let (evidence, assumptions) =
            support_graph_records(&[], &[(1, vec![Digest([1; 32])], vec![])]);
        assert_eq!(
            find_support_cycle(&evidence, &assumptions),
            Some(vec![
                SupportNode::Assumption(Digest([1; 32])),
                SupportNode::Assumption(Digest([1; 32]))
            ])
        );

        let (evidence, assumptions) = support_graph_records(
            &[],
            &[
                (1, vec![Digest([2; 32])], vec![]),
                (2, vec![Digest([1; 32])], vec![]),
            ],
        );
        let cycle = find_support_cycle(&evidence, &assumptions).unwrap();
        assert_eq!(cycle.first(), cycle.last());
        assert_eq!(cycle.len(), 3);
        assert!(
            cycle
                .iter()
                .all(|node| matches!(node, SupportNode::Assumption(_)))
        );
    }

    #[test]
    fn manifest_reports_assumption_cycles_before_content_id_mismatches() {
        let (evidence, assumptions) = support_graph_records(
            &[],
            &[
                (1, vec![Digest([2; 32])], vec![]),
                (2, vec![Digest([1; 32])], vec![]),
            ],
        );
        let manifest = CatalogManifest {
            schema_major: SCHEMA_MAJOR,
            entries: vec![],
            graphs: vec![],
            evidence: evidence.into_iter().collect(),
            assumptions: assumptions.into_iter().collect(),
        };
        assert_eq!(
            manifest.validate(),
            Err(CatalogError::CyclicSupport(vec![
                SupportNode::Assumption(Digest([1; 32])),
                SupportNode::Assumption(Digest([2; 32])),
                SupportNode::Assumption(Digest([1; 32])),
            ]))
        );
    }

    #[test]
    fn support_graph_reports_cross_namespace_cycles_and_accepts_dags() {
        let (evidence, assumptions) = support_graph_records(
            &[(1, vec![Digest([2; 32])], vec![])],
            &[(2, vec![], vec![Digest([1; 32])])],
        );
        let cycle = find_support_cycle(&evidence, &assumptions).unwrap();
        assert_eq!(cycle.first(), cycle.last());
        assert!(
            cycle
                .iter()
                .any(|node| matches!(node, SupportNode::Evidence(_)))
        );
        assert!(
            cycle
                .iter()
                .any(|node| matches!(node, SupportNode::Assumption(_)))
        );

        let (evidence, assumptions) = support_graph_records(
            &[
                (1, vec![], vec![]),
                (2, vec![Digest([3; 32])], vec![Digest([1; 32])]),
            ],
            &[
                (3, vec![], vec![Digest([1; 32])]),
                (4, vec![Digest([3; 32])], vec![Digest([2; 32])]),
            ],
        );
        assert_eq!(find_support_cycle(&evidence, &assumptions), None);
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
                    dependencies: vec![],
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

    fn scenario(os: Option<&[u8]>) -> TargetScenario {
        TargetScenario {
            release: VersionRange::minor(8, 5),
            os: os.map(|value| Bytes(value.to_vec())),
            arch: Some(Bytes(b"x86_64".to_vec())),
            sapi: Some(Bytes(b"cli".to_vec())),
            zts: Some(false),
            debug: Some(false),
            extensions: Some(vec![(Bytes(b"session".to_vec()), true)]),
            ini: Some(vec![]),
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
    fn graph_rejects_closed_return_coverage_with_open_domain_dependency() {
        let mut catalog = Catalog::default();
        let record = evidence_record();
        let id = catalog.add_evidence(record).unwrap();
        let mut graph = graph(id);
        graph.blocks[0].events.push(Event::Unknown {
            domains: vec![Domain::UserWrite],
            reason: Bytes(b"unmodeled write".to_vec()),
        });
        let returned = graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        returned.dependencies.push(Domain::UserWrite);
        let write = graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::UserWrite)
            .unwrap();
        write.coverage = Coverage::Open {
            reasons: vec![Bytes(b"unmodeled write".to_vec())],
        };

        assert_eq!(
            catalog.add_entry(function(b"core", b"strlen"), graph),
            Err(CatalogError::Graph(GraphError::ClosedDomainDependsOnOpen {
                domain: Domain::Return,
                dependency: Domain::UserWrite,
            }))
        );
    }

    #[test]
    fn closed_return_refinement_cannot_drop_an_open_throw_path() {
        let mut catalog = Catalog::default();
        let evidence = catalog.add_evidence(evidence_record()).unwrap();
        let mut graph = graph(evidence);
        graph.blocks[0].events.push(Event::Unknown {
            domains: vec![Domain::PhpThrow],
            reason: Bytes(b"callback may throw".to_vec()),
        });
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .dependencies
            .push(Domain::PhpThrow);
        let throws = graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::PhpThrow)
            .unwrap();
        throws.coverage = Coverage::Open {
            reasons: vec![Bytes(b"callback may throw".to_vec())],
        };

        assert_eq!(
            catalog.add_entry(function(b"core", b"map"), graph),
            Err(CatalogError::Graph(GraphError::ClosedDomainDependsOnOpen {
                domain: Domain::Return,
                dependency: Domain::PhpThrow,
            }))
        );
    }

    #[test]
    fn graph_rejects_cyclic_closed_domain_dependencies_without_a_certificate() {
        let mut catalog = Catalog::default();
        let id = catalog.add_evidence(evidence_record()).unwrap();
        let mut graph = graph(id);
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .dependencies
            .push(Domain::Invocation);
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Invocation)
            .unwrap()
            .dependencies
            .push(Domain::Return);

        assert_eq!(
            catalog.add_entry(function(b"core", b"strlen"), graph),
            Err(CatalogError::Graph(GraphError::CyclicDomainDependency(
                Domain::Invocation
            )))
        );
    }

    #[test]
    fn projection_includes_transitive_domain_dependencies() {
        let mut catalog = Catalog::default();
        let id = catalog.add_evidence(evidence_record()).unwrap();
        let mut graph = graph(id);
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .dependencies
            .push(Domain::Invocation);
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Invocation)
            .unwrap()
            .dependencies
            .push(Domain::UserRead);
        catalog
            .add_entry(function(b"core", b"strlen"), graph)
            .unwrap();

        let projection = catalog
            .project(&function(b"core", b"strlen"), Domain::Return)
            .unwrap();
        assert_eq!(
            projection
                .dependencies
                .iter()
                .map(|item| item.domain)
                .collect::<Vec<_>>(),
            vec![Domain::Invocation, Domain::UserRead]
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
        let loaded = Catalog::from_json(&json).unwrap();
        assert!(loaded.select(&function(b"CORE", b"STRLEN")).is_some());

        let mut tampered = manifest;
        tampered.graphs[0].id = Digest([0; 32]);
        assert_eq!(
            tampered.validate(),
            Err(CatalogError::DigestMismatch(Digest([0; 32])))
        );
    }

    #[test]
    fn manifest_rejects_the_previous_schema_major() {
        let mut catalog = Catalog::default();
        let evidence = catalog.add_evidence(evidence_record()).unwrap();
        catalog
            .add_entry(function(b"core", b"strlen"), graph(evidence))
            .unwrap();
        let mut manifest = catalog.manifest();
        manifest.schema_major = SCHEMA_MAJOR - 1;
        assert_eq!(
            manifest.validate(),
            Err(CatalogError::UnsupportedManifest(SCHEMA_MAJOR - 1))
        );
    }

    #[test]
    fn target_selection_keeps_unknown_remainder_and_selects_exact_scenario() {
        let mut catalog = Catalog::default();
        let evidence = catalog.add_evidence(evidence_record()).unwrap();
        let linux = TargetPredicate {
            os: Some(Bytes(b"linux".to_vec())),
            ..TargetPredicate::any()
        };
        let windows = TargetPredicate {
            os: Some(Bytes(b"windows".to_vec())),
            ..TargetPredicate::any()
        };
        let key = function(b"core", b"strlen");
        catalog
            .add_variant(key.clone(), linux, graph(evidence))
            .unwrap();
        catalog
            .add_variant(key.clone(), windows, graph(evidence))
            .unwrap();

        let known = TargetSet::new(vec![scenario(Some(b"linux"))]).unwrap();
        let selection = catalog.select_for_targets(&key, &known);
        assert_eq!(selection.variants.len(), 1);
        assert_eq!(selection.variants[0].applicability, Applicability::Yes);
        assert!(selection.uncovered.is_empty());

        let unknown = TargetSet::new(vec![scenario(None)]).unwrap();
        let selection = catalog.select_for_targets(&key, &unknown);
        assert_eq!(selection.variants.len(), 2);
        assert!(
            selection
                .variants
                .iter()
                .all(|candidate| candidate.applicability == Applicability::Maybe)
        );
        assert_eq!(selection.uncovered.len(), 1);
    }

    #[test]
    fn closed_return_cannot_reach_an_open_domain_through_not_applicable() {
        let mut catalog = Catalog::default();
        let id = catalog.add_evidence(evidence_record()).unwrap();
        let mut graph = graph(id);
        graph.blocks[0].events.push(Event::Unknown {
            domains: vec![Domain::UserWrite],
            reason: Bytes(b"unmodeled write".to_vec()),
        });
        for item in &mut graph.coverage {
            match item.domain {
                Domain::Return => item.dependencies.push(Domain::Diagnostic),
                Domain::Diagnostic => {
                    item.coverage = Coverage::NotApplicable;
                    item.dependencies.push(Domain::UserWrite);
                }
                Domain::UserWrite => {
                    item.coverage = Coverage::Open {
                        reasons: vec![Bytes(b"unmodeled write".to_vec())],
                    }
                }
                _ => {}
            }
        }
        let key = function(b"core", b"strlen");
        assert_eq!(
            catalog.add_entry(key, graph),
            Err(CatalogError::Graph(GraphError::ClosedDomainDependsOnOpen {
                domain: Domain::Return,
                dependency: Domain::UserWrite,
            }))
        );
    }

    #[test]
    fn closed_return_cannot_depend_on_not_applicable_domain() {
        let mut catalog = Catalog::default();
        let id = catalog.add_evidence(evidence_record()).unwrap();
        let mut graph = graph(id);
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .dependencies
            .push(Domain::Diagnostic);
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Diagnostic)
            .unwrap()
            .coverage = Coverage::NotApplicable;
        assert_eq!(
            catalog.add_entry(function(b"core", b"strlen"), graph),
            Err(CatalogError::Graph(
                GraphError::ClosedDomainDependsOnNotApplicable {
                    domain: Domain::Return,
                    dependency: Domain::Diagnostic,
                }
            ))
        );
    }

    #[test]
    fn typed_assumptions_are_separate_and_round_trip_with_coverage() {
        let mut catalog = Catalog::default();
        let evidence_id = catalog.add_evidence(evidence_record()).unwrap();
        let prerequisite = assumption_record(evidence_id);
        let prerequisite_id = catalog.add_assumption(prerequisite.clone()).unwrap();
        let mut assumption = assumption_record(evidence_id);
        assumption.scope.loaded_program = Some(Digest([0x55; 32]));
        assumption.prerequisites = vec![prerequisite_id];
        let assumption_id = catalog.add_assumption(assumption.clone()).unwrap();
        let mut graph = graph(evidence_id);
        graph.blocks[0].events.push(Event::Unknown {
            domains: vec![Domain::Return],
            reason: Bytes(b"assumption validation is not implemented".to_vec()),
        });
        let returned = graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        returned.coverage = crate::Coverage::Open {
            reasons: vec![Bytes(
                b"assumption validation awaits a trusted validator".to_vec(),
            )],
        };
        returned.assumptions = vec![assumption_id];
        catalog
            .add_entry(function(b"core", b"strlen"), graph)
            .unwrap();
        let manifest = catalog.manifest();
        manifest.validate().unwrap();
        assert_eq!(manifest.assumptions.len(), 2);
        let loaded = Catalog::from_json(&manifest.to_json().unwrap()).unwrap();
        assert_eq!(loaded.explain_assumption(assumption_id), Some(&assumption));
        assert_eq!(
            loaded.explain_assumption(prerequisite_id),
            Some(&prerequisite)
        );
        let mut tampered = manifest;
        let tampered_index = tampered
            .assumptions
            .iter()
            .position(|(id, _)| *id == assumption_id)
            .unwrap();
        tampered.assumptions[tampered_index].1.claim.0.push(b'!');
        assert_eq!(
            tampered.validate(),
            Err(CatalogError::InvalidAssumption(assumption_id))
        );
    }

    #[test]
    fn manifest_rejects_unresolved_evidence_assumptions() {
        let unresolved = Digest([0xCD; 32]);
        let mut catalog = Catalog::default();
        let id = catalog.add_evidence(evidence_record()).unwrap();
        catalog
            .add_entry(function(b"core", b"strlen"), graph(id))
            .unwrap();
        let mut manifest = catalog.manifest();
        manifest.evidence[0].1.assumptions = vec![unresolved];
        let updated_evidence_id = manifest.evidence[0].1.id().unwrap();
        manifest.evidence[0].0 = updated_evidence_id;
        for item in &mut manifest.graphs[0].graph.coverage {
            item.evidence = vec![updated_evidence_id];
        }
        let updated_graph_id = manifest.graphs[0].graph.canonical_digest().unwrap();
        manifest.graphs[0].id = updated_graph_id;
        manifest.entries[0].graph = updated_graph_id;
        assert!(matches!(
            Catalog::from_manifest(manifest),
            Err(CatalogError::MissingAssumption(id)) if id == unresolved
        ));
    }

    #[test]
    fn coverage_assumption_cannot_resolve_in_the_evidence_namespace() {
        let mut catalog = Catalog::default();
        let evidence_id = catalog.add_evidence(evidence_record()).unwrap();
        let mut graph = graph(evidence_id);
        graph.coverage[0].assumptions = vec![evidence_id];
        assert_eq!(
            catalog.add_entry(function(b"core", b"strlen"), graph),
            Err(CatalogError::MissingAssumption(evidence_id))
        );
    }

    #[test]
    fn closed_coverage_cannot_rely_on_an_unestablished_assumption() {
        let mut catalog = Catalog::default();
        let evidence_id = catalog.add_evidence(evidence_record()).unwrap();
        let assumption_id = catalog
            .add_assumption(assumption_record(evidence_id))
            .unwrap();
        let mut graph = graph(evidence_id);
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .assumptions = vec![assumption_id];
        assert_eq!(
            graph.validate(),
            Err(GraphError::UnestablishedAssumption(Domain::Return))
        );
        assert_eq!(
            catalog.add_entry(function(b"core", b"strlen"), graph),
            Err(CatalogError::Graph(GraphError::UnestablishedAssumption(
                Domain::Return
            )))
        );
    }

    #[test]
    fn evidence_assumptions_must_be_propagated_to_coverage() {
        let support = evidence_record();
        let support_id = support.id().unwrap();
        let assumption = assumption_record(support_id);
        let assumption_id = assumption.id().unwrap();
        let mut conditioned_evidence = evidence_record();
        conditioned_evidence.assumptions = vec![assumption_id];
        let conditioned_id = conditioned_evidence.id().unwrap();
        let graph = graph(conditioned_id);
        let graph_id = graph.canonical_digest().unwrap();
        let manifest = CatalogManifest {
            schema_major: SCHEMA_MAJOR,
            entries: vec![CatalogEntry {
                key: function(b"core", b"strlen"),
                graph: graph_id,
                applicability: TargetPredicate::any(),
            }],
            graphs: vec![GraphRecord {
                id: graph_id,
                graph: graph.clone(),
            }],
            evidence: vec![
                (support_id, support),
                (conditioned_id, conditioned_evidence),
            ],
            assumptions: vec![(assumption_id, assumption)],
        };

        assert_eq!(
            manifest.validate(),
            Err(CatalogError::UnpropagatedEvidenceAssumption {
                evidence: conditioned_id,
                assumption: assumption_id
            })
        );

        let mut catalog = Catalog::default();
        catalog.add_evidence(evidence_record()).unwrap();
        catalog
            .add_assumption(assumption_record(support_id))
            .unwrap();
        let mut evidence_without_condition = evidence_record();
        evidence_without_condition.assumptions = vec![assumption_id];
        catalog.add_evidence(evidence_without_condition).unwrap();
        assert!(matches!(
            catalog.add_entry(function(b"core", b"strlen"), graph),
            Err(CatalogError::UnpropagatedEvidenceAssumption { .. })
        ));
    }

    #[test]
    fn assumption_records_require_valid_claim_validator_scope_and_resolved_edges() {
        let evidence_id = evidence_record().id().unwrap();
        let mut catalog = Catalog::default();
        let mut assumption = assumption_record(evidence_id);
        assumption.claim = Bytes(vec![]);
        assert!(matches!(
            catalog.add_assumption(assumption),
            Err(CatalogError::InvalidAssumption(_))
        ));
        let mut assumption = assumption_record(evidence_id);
        assumption.validator.version = Bytes(vec![]);
        assert!(matches!(
            catalog.add_assumption(assumption),
            Err(CatalogError::InvalidAssumption(_))
        ));
        let mut assumption = assumption_record(evidence_id);
        assumption.scope.target.release = Some(VersionRange {
            min: crate::ReleaseVersion {
                major: 8,
                minor: 6,
                patch: 0,
            },
            max: crate::ReleaseVersion {
                major: 8,
                minor: 5,
                patch: u16::MAX,
            },
        });
        assert!(matches!(
            catalog.add_assumption(assumption),
            Err(CatalogError::InvalidAssumption(_))
        ));
        let mut assumption = assumption_record(evidence_id);
        assumption.supporting_evidence.clear();
        assert!(matches!(
            catalog.add_assumption(assumption),
            Err(CatalogError::InvalidAssumption(_))
        ));

        let assumption = assumption_record(evidence_id);
        let assumption_id = catalog.add_assumption(assumption).unwrap();
        let manifest = catalog.manifest();
        assert_eq!(
            manifest.validate(),
            Err(CatalogError::MissingEvidence(evidence_id))
        );
        let mut manifest = manifest;
        manifest.evidence.push((evidence_id, evidence_record()));
        manifest.assumptions[0].1.prerequisites = vec![Digest([0xAA; 32])];
        let stale_id = manifest.assumptions[0].0;
        manifest.assumptions[0].0 = manifest.assumptions[0].1.id().unwrap();
        assert_ne!(stale_id, manifest.assumptions[0].0);
        assert_eq!(
            manifest.validate(),
            Err(CatalogError::MissingAssumption(Digest([0xAA; 32])))
        );
        assert_ne!(assumption_id, Digest([0; 32]));
    }

    #[test]
    fn assumption_applicability_requires_matching_target_and_program_context() {
        let evidence_id = Digest([9; 32]);
        let program_id = Digest([0x33; 32]);
        let mut assumption = assumption_record(evidence_id);
        assumption.scope.loaded_program = Some(program_id);
        assert_eq!(
            assumption.applicability(&scenario(Some(b"linux")), Some(program_id)),
            Applicability::Yes
        );
        assert_eq!(
            assumption.applicability(&scenario(Some(b"linux")), None),
            Applicability::Maybe
        );
        assert_eq!(
            assumption.applicability(&scenario(Some(b"linux")), Some(Digest([0x44; 32]))),
            Applicability::No
        );
        assumption.scope.target.os = Some(Bytes(b"windows".to_vec()));
        assert_eq!(
            assumption.applicability(&scenario(Some(b"linux")), Some(program_id)),
            Applicability::No
        );
    }

    #[test]
    #[ignore = "RV-5: permuted unordered assumption vectors get distinct identities; remove when canonicalized or rejected"]
    fn permuted_evidence_assumptions_share_one_identity_or_are_rejected() {
        let first = Digest([7; 32]);
        let second = Digest([8; 32]);
        let mut ascending = evidence_record();
        ascending.assumptions = vec![first, second];
        let mut descending = evidence_record();
        descending.assumptions = vec![second, first];
        let mut catalog = Catalog::default();
        if let (Ok(a), Ok(b)) = (
            catalog.add_evidence(ascending),
            catalog.add_evidence(descending),
        ) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn inverted_release_predicate_is_rejected_on_add() {
        let mut catalog = Catalog::default();
        let id = catalog.add_evidence(evidence_record()).unwrap();
        let inverted = TargetPredicate {
            release: Some(VersionRange {
                min: crate::ReleaseVersion {
                    major: 8,
                    minor: 5,
                    patch: 4,
                },
                max: crate::ReleaseVersion {
                    major: 8,
                    minor: 5,
                    patch: 0,
                },
            }),
            ..TargetPredicate::any()
        };
        assert_eq!(
            catalog.add_variant(function(b"core", b"strlen"), inverted, graph(id)),
            Err(CatalogError::InvalidTargetPredicate(
                crate::TargetError::InvalidVersionRange
            ))
        );
    }

    #[test]
    fn valid_release_boundaries_are_accepted_on_add_and_manifest_load() {
        let mut catalog = Catalog::default();
        let id = catalog.add_evidence(evidence_record()).unwrap();
        let key = function(b"core", b"versioned");
        let predicates = [
            TargetPredicate::any(),
            TargetPredicate {
                release: Some(VersionRange::exact(crate::ReleaseVersion {
                    major: 0,
                    minor: 0,
                    patch: 0,
                })),
                ..TargetPredicate::any()
            },
            TargetPredicate {
                release: Some(VersionRange::exact(crate::ReleaseVersion {
                    major: u16::MAX,
                    minor: u16::MAX,
                    patch: u16::MAX,
                })),
                ..TargetPredicate::any()
            },
            TargetPredicate {
                release: Some(VersionRange::minor(8, 5)),
                ..TargetPredicate::any()
            },
        ];
        for predicate in predicates {
            catalog
                .add_variant(key.clone(), predicate, graph(id))
                .unwrap();
        }
        Catalog::from_manifest(catalog.manifest()).unwrap();
    }

    #[test]
    fn manifest_load_rejects_inverted_release_predicates() {
        let mut catalog = Catalog::default();
        let id = catalog.add_evidence(evidence_record()).unwrap();
        catalog
            .add_variant(
                function(b"core", b"versioned"),
                TargetPredicate {
                    release: Some(VersionRange::minor(8, 5)),
                    ..TargetPredicate::any()
                },
                graph(id),
            )
            .unwrap();
        let mut manifest = catalog.manifest();
        manifest.entries[0].applicability.release = Some(VersionRange {
            min: crate::ReleaseVersion {
                major: 8,
                minor: 5,
                patch: 4,
            },
            max: crate::ReleaseVersion {
                major: 8,
                minor: 5,
                patch: 0,
            },
        });
        let error = CatalogError::InvalidTargetPredicate(crate::TargetError::InvalidVersionRange);
        assert_eq!(manifest.validate(), Err(error.clone()));
        assert!(matches!(
            Catalog::from_manifest(manifest),
            Err(actual) if actual == error
        ));
    }
}
