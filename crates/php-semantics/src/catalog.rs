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
        let mut canonical = self.clone();
        canonical.assumptions.sort();
        canonical.dependencies.sort();
        Ok(Digest(
            Sha256::digest(postcard::to_allocvec(&canonical)?).into(),
        ))
    }

    fn validate_canonical_order(&self) -> Result<(), CatalogError> {
        require_strictly_sorted(&self.assumptions, "evidence assumptions")?;
        require_strictly_sorted(&self.dependencies, "evidence dependencies")
    }
}

fn require_strictly_sorted<T: Ord>(values: &[T], field: &'static str) -> Result<(), CatalogError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(CatalogError::NonCanonicalOrder(field));
    }
    Ok(())
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

/// Deployment-owned trust configuration. It is intentionally kept outside
/// the catalog manifest: a catalog cannot authorize its own validator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorTrustPolicy {
    /// Incremented whenever pins or revocations change. Consumers must include
    /// the policy digest in any cached assumption decision.
    pub revision: u64,
    pub roots: Vec<ValidatorTrustRoot>,
    pub pins: Vec<ValidatorPin>,
    pub revocations: Vec<ValidatorRevocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorTrustRoot {
    pub id: crate::Bytes,
    /// Fingerprint of the root configuration managed by the deployment.
    pub fingerprint: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorPin {
    pub validator: AssumptionValidator,
    /// Digest reported by the loaded validator implementation at execution.
    pub implementation: Digest,
    pub root: crate::Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorRevocation {
    pub validator_id: crate::Bytes,
    pub version: crate::Bytes,
    pub effective_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatorTrustError {
    EmptyRoot,
    EmptyPin,
    MissingRoot(crate::Bytes),
    DuplicateRoot(crate::Bytes),
    DuplicatePin(AssumptionValidator),
    DuplicateRevocation(crate::Bytes, crate::Bytes),
    FutureRevocation(crate::Bytes, crate::Bytes),
    PolicyEncoding,
    InvalidTarget(crate::TargetError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatorTrustDecision {
    Trusted { root: crate::Bytes },
    Untrusted,
    Revoked { effective_revision: u64 },
}

/// Deployment-loaded code that evaluates an assumption. Implementations must
/// validate the claim against every supplied evidence record and the requested
/// target/program context. Their declared digest is checked against the
/// administrator-owned trust policy before this method is called.
pub trait AssumptionValidatorExecutor {
    fn validator(&self) -> AssumptionValidator;
    fn implementation(&self) -> Digest;
    fn validate(
        &self,
        assumption: &AssumptionRecord,
        supporting_evidence: &[EvidenceRecord],
        established_prerequisites: &[AssumptionRecord],
        target: &TargetScenario,
        loaded_program: Option<Digest>,
    ) -> Applicability;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstablishedAssumption {
    pub assumption: Digest,
    pub target: TargetScenario,
    pub loaded_program: Option<Digest>,
    pub policy_key: Digest,
    pub root: crate::Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssumptionValidation {
    Established(Box<EstablishedAssumption>),
    NotApplicable,
    ContextUnknown,
    UntrustedValidator,
    RevokedValidator { effective_revision: u64 },
    InvalidClaim,
    PrerequisiteNotEstablished(Digest),
    SupportCycle(Digest),
    MissingAssumption(Digest),
    MissingEvidence(Digest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssumptionGraphValidationError {
    Graph(GraphError),
    Trust(ValidatorTrustError),
    EntryNotApplicable(EntryKey),
    NotEstablished {
        assumption: Digest,
        decision: AssumptionValidation,
    },
}

impl ValidatorTrustPolicy {
    pub fn validate(&self) -> Result<(), ValidatorTrustError> {
        for (index, root) in self.roots.iter().enumerate() {
            if root.id.0.is_empty() {
                return Err(ValidatorTrustError::EmptyRoot);
            }
            if self.roots[..index].iter().any(|old| old.id == root.id) {
                return Err(ValidatorTrustError::DuplicateRoot(root.id.clone()));
            }
        }
        for (index, pin) in self.pins.iter().enumerate() {
            if pin.validator.id.0.is_empty()
                || pin.validator.version.0.is_empty()
                || pin.root.0.is_empty()
            {
                return Err(ValidatorTrustError::EmptyPin);
            }
            if !self.roots.iter().any(|root| root.id == pin.root) {
                return Err(ValidatorTrustError::MissingRoot(pin.root.clone()));
            }
            if self.pins[..index].iter().any(|old| {
                old.validator.id == pin.validator.id
                    && old.validator.version == pin.validator.version
            }) {
                return Err(ValidatorTrustError::DuplicatePin(pin.validator.clone()));
            }
        }
        for (index, revocation) in self.revocations.iter().enumerate() {
            let identity = (revocation.validator_id.clone(), revocation.version.clone());
            if self.revocations[..index]
                .iter()
                .any(|old| old.validator_id == identity.0 && old.version == identity.1)
            {
                return Err(ValidatorTrustError::DuplicateRevocation(
                    identity.0, identity.1,
                ));
            }
            if revocation.effective_revision > self.revision {
                return Err(ValidatorTrustError::FutureRevocation(
                    identity.0, identity.1,
                ));
            }
        }
        Ok(())
    }

    pub fn decide(
        &self,
        validator: &AssumptionValidator,
        implementation: Digest,
    ) -> Result<ValidatorTrustDecision, ValidatorTrustError> {
        self.validate()?;
        if let Some(revocation) = self.revocations.iter().find(|revocation| {
            revocation.validator_id == validator.id && revocation.version == validator.version
        }) {
            return Ok(ValidatorTrustDecision::Revoked {
                effective_revision: revocation.effective_revision,
            });
        }
        Ok(self
            .pins
            .iter()
            .find(|pin| pin.validator == *validator && pin.implementation == implementation)
            .map_or(ValidatorTrustDecision::Untrusted, |pin| {
                ValidatorTrustDecision::Trusted {
                    root: pin.root.clone(),
                }
            }))
    }

    /// Stable invalidation key for downstream caches. A change to revision,
    /// pinning, root fingerprints, or revocation changes this digest.
    pub fn cache_key(&self) -> Result<Digest, postcard::Error> {
        let mut canonical = self.clone();
        canonical.roots.sort_by(|a, b| a.id.cmp(&b.id));
        canonical.pins.sort_by(|a, b| {
            a.validator
                .id
                .cmp(&b.validator.id)
                .then_with(|| a.validator.version.cmp(&b.validator.version))
                .then_with(|| a.implementation.cmp(&b.implementation))
        });
        canonical.revocations.sort_by(|a, b| {
            a.validator_id
                .cmp(&b.validator_id)
                .then_with(|| a.version.cmp(&b.version))
        });
        let mut hasher = Sha256::new();
        hasher.update(b"php-semantics-validator-trust-policy-v1\0");
        hasher.update(postcard::to_allocvec(&canonical)?);
        Ok(Digest(hasher.finalize().into()))
    }
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
        let mut canonical = self.clone();
        canonical.prerequisites.sort();
        canonical.supporting_evidence.sort();
        let encoded = postcard::to_allocvec(&canonical)?;
        let mut hasher = Sha256::new();
        hasher.update(b"php-semantics-assumption-v1\0");
        hasher.update(crate::SCHEMA_MAJOR.to_le_bytes());
        hasher.update(encoded);
        Ok(Digest(hasher.finalize().into()))
    }

    fn validate_canonical_order(&self) -> Result<(), CatalogError> {
        require_strictly_sorted(&self.prerequisites, "assumption prerequisites")?;
        require_strictly_sorted(&self.supporting_evidence, "assumption supporting evidence")
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
    NonCanonicalOrder(&'static str),
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
        let mut canonical = self.clone();
        for entry in &mut canonical.entries {
            entry.applicability = entry.applicability.canonicalized_order();
        }
        canonical.entries.sort_by(|a, b| {
            a.key
                .cmp(&b.key)
                .then_with(|| a.graph.cmp(&b.graph))
                .then_with(|| {
                    postcard::to_allocvec(&a.applicability)
                        .unwrap_or_default()
                        .cmp(&postcard::to_allocvec(&b.applicability).unwrap_or_default())
                })
        });
        for record in &mut canonical.graphs {
            record.graph = record.graph.canonicalized_order();
        }
        canonical.graphs.sort_by_key(|record| record.id);
        for (_, record) in &mut canonical.evidence {
            record.assumptions.sort();
            record.dependencies.sort();
        }
        canonical.evidence.sort_by_key(|(id, _)| *id);
        for (_, record) in &mut canonical.assumptions {
            record.prerequisites.sort();
            record.supporting_evidence.sort();
        }
        canonical.assumptions.sort_by_key(|(id, _)| *id);
        Ok(Digest(
            Sha256::digest(postcard::to_allocvec(&canonical)?).into(),
        ))
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema_major != crate::SCHEMA_MAJOR {
            return Err(CatalogError::UnsupportedManifest(self.schema_major));
        }
        validate_manifest_order(self)?;
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
            record
                .graph
                .validate_structure()
                .map_err(CatalogError::Graph)?;
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
            record.validate_canonical_order()?;
            if record.id().map_err(|_| CatalogError::GraphEncoding)? != *id
                || evidence_ids.contains(id)
            {
                return Err(CatalogError::DigestMismatch(*id));
            }
            evidence_ids.push(*id);
        }
        let mut assumption_ids = Vec::new();
        for (id, record) in &self.assumptions {
            record.validate_canonical_order()?;
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

fn validate_manifest_order(manifest: &CatalogManifest) -> Result<(), CatalogError> {
    if manifest.entries.windows(2).any(|pair| {
        let left = &pair[0];
        let right = &pair[1];
        left.key > right.key
            || (left.key == right.key
                && (left.graph > right.graph
                    || (left.graph == right.graph
                        && postcard::to_allocvec(&left.applicability).unwrap_or_default()
                            > postcard::to_allocvec(&right.applicability).unwrap_or_default())))
    }) {
        return Err(CatalogError::NonCanonicalOrder("manifest entries"));
    }
    require_strictly_sorted_by(&manifest.graphs, |record| record.id, "manifest graphs")?;
    require_strictly_sorted_by(&manifest.evidence, |(id, _)| *id, "manifest evidence")?;
    require_strictly_sorted_by(&manifest.assumptions, |(id, _)| *id, "manifest assumptions")
}

fn require_strictly_sorted_by<T, K: Ord>(
    values: &[T],
    key: impl Fn(&T) -> K,
    field: &'static str,
) -> Result<(), CatalogError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        return Err(CatalogError::NonCanonicalOrder(field));
    }
    Ok(())
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
        let evidence = manifest.evidence.into_iter().collect();
        let assumptions = manifest.assumptions.into_iter().collect();
        let mut catalog = Self {
            evidence,
            assumptions,
            ..Self::default()
        };
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
        record.validate_canonical_order()?;
        let id = record.id().map_err(|_| CatalogError::GraphEncoding)?;
        if self.evidence.get(&id).is_some_and(|old| old != &record) {
            return Err(CatalogError::ConflictingEvidence(id));
        }
        self.evidence.insert(id, record);
        Ok(id)
    }

    pub fn add_assumption(&mut self, record: AssumptionRecord) -> Result<Digest, CatalogError> {
        record.validate_canonical_order()?;
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
        graph.validate_structure().map_err(CatalogError::Graph)?;
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

    /// Whether the catalog has an entry for the function `name` of
    /// `extension`, whatever the targets it covers.
    pub fn has_function(&self, extension: &str, name: &str) -> bool {
        let key = EntryKey::Function {
            extension: crate::Bytes(extension.as_bytes().to_vec()),
            name: crate::Bytes(name.as_bytes().to_vec()),
        };
        self.entries.contains_key(&key.normalized())
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
        self.project_inner(key, domain, false)
    }

    pub fn project_with_assumption_context(
        &self,
        key: &EntryKey,
        domain: Domain,
        target: &TargetScenario,
        loaded_program: Option<Digest>,
        policy: &ValidatorTrustPolicy,
        executors: &[&dyn AssumptionValidatorExecutor],
    ) -> Result<Option<AssumptionProjection<'_>>, AssumptionGraphValidationError> {
        let Some(selection) = self.select_for_scenario(key, target) else {
            return Ok(None);
        };
        let assumptions = self.validate_graph_assumptions(
            selection.graph,
            key,
            target,
            loaded_program,
            policy,
            executors,
        )?;
        let policy_key = policy.cache_key().map_err(|_| {
            AssumptionGraphValidationError::Trust(ValidatorTrustError::PolicyEncoding)
        })?;
        Ok(
            Self::project_selection(selection, domain, true).map(|projection| {
                AssumptionProjection {
                    projection,
                    context: crate::AssumptionCertificateContext {
                        entry_key: key.normalized(),
                        target: target.clone(),
                        loaded_program,
                        policy_key,
                        assumptions,
                    },
                }
            }),
        )
    }

    fn select_for_scenario(
        &self,
        key: &EntryKey,
        target: &TargetScenario,
    ) -> Option<Selection<'_>> {
        let entry = self
            .entries
            .get(&key.normalized())?
            .iter()
            .find(|entry| entry.applicability.test(target) == Applicability::Yes)?;
        Some(Selection {
            entry,
            graph: self
                .graphs
                .get(&entry.graph)
                .expect("entry graph is installed with the entry"),
        })
    }

    fn project_inner(
        &self,
        key: &EntryKey,
        domain: Domain,
        allow_closed_assumptions: bool,
    ) -> Option<Projection<'_>> {
        let selection = self.select(key)?;
        Self::project_selection(selection, domain, allow_closed_assumptions)
    }

    fn project_selection(
        selection: Selection<'_>,
        domain: Domain,
        allow_closed_assumptions: bool,
    ) -> Option<Projection<'_>> {
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
        if !allow_closed_assumptions
            && std::iter::once(coverage)
                .chain(dependencies.iter().copied())
                .any(|item| {
                    matches!(item.coverage, crate::Coverage::Closed) && !item.assumptions.is_empty()
                })
        {
            return None;
        }
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

    /// Execute one assumption validator through a deployment-pinned policy.
    /// This result is contextual. Cache it only under the assumption ID,
    /// target, loaded-program identity, and returned policy key.
    /// It does not mutate graph coverage; callers still need to preserve the
    /// result and its context when consuming a claim.
    pub fn validate_assumption(
        &self,
        id: Digest,
        target: &TargetScenario,
        loaded_program: Option<Digest>,
        policy: &ValidatorTrustPolicy,
        executors: &[&dyn AssumptionValidatorExecutor],
    ) -> Result<AssumptionValidation, ValidatorTrustError> {
        policy.validate()?;
        target
            .validate()
            .map_err(ValidatorTrustError::InvalidTarget)?;
        let policy_key = policy
            .cache_key()
            .map_err(|_| ValidatorTrustError::PolicyEncoding)?;
        struct Scope<'a> {
            catalog: &'a Catalog,
            target: &'a TargetScenario,
            loaded_program: Option<Digest>,
            policy: &'a ValidatorTrustPolicy,
            policy_key: Digest,
            executors: &'a [&'a dyn AssumptionValidatorExecutor],
        }
        fn visit(
            scope: &Scope<'_>,
            id: Digest,
            active: &mut Vec<Digest>,
            memo: &mut BTreeMap<Digest, AssumptionValidation>,
        ) -> AssumptionValidation {
            let Scope {
                catalog,
                target,
                loaded_program,
                policy,
                policy_key,
                executors,
            } = *scope;
            fn collect_evidence(
                scope: &Scope<'_>,
                evidence_id: Digest,
                active_assumptions: &mut Vec<Digest>,
                memo: &mut BTreeMap<Digest, AssumptionValidation>,
                active_evidence: &mut Vec<Digest>,
                seen_evidence: &mut Vec<Digest>,
                collected: &mut Vec<EvidenceRecord>,
            ) -> Result<(), AssumptionValidation> {
                let Scope { catalog, .. } = *scope;
                if seen_evidence.contains(&evidence_id) {
                    return Ok(());
                }
                if active_evidence.contains(&evidence_id) {
                    return Err(AssumptionValidation::SupportCycle(evidence_id));
                }
                let Some(record) = catalog.evidence.get(&evidence_id) else {
                    return Err(AssumptionValidation::MissingEvidence(evidence_id));
                };
                active_evidence.push(evidence_id);
                for assumption_id in &record.assumptions {
                    match visit(scope, *assumption_id, active_assumptions, memo) {
                        AssumptionValidation::Established(_) => {}
                        _ => {
                            active_evidence.pop();
                            return Err(AssumptionValidation::PrerequisiteNotEstablished(
                                *assumption_id,
                            ));
                        }
                    }
                }
                for dependency in &record.dependencies {
                    if let Err(error) = collect_evidence(
                        scope,
                        *dependency,
                        active_assumptions,
                        memo,
                        active_evidence,
                        seen_evidence,
                        collected,
                    ) {
                        active_evidence.pop();
                        return Err(error);
                    }
                }
                active_evidence.pop();
                seen_evidence.push(evidence_id);
                collected.push(record.clone());
                Ok(())
            }

            if active.contains(&id) {
                return AssumptionValidation::SupportCycle(id);
            }
            if let Some(cached) = memo.get(&id) {
                return cached.clone();
            }
            let Some(record) = catalog.assumptions.get(&id) else {
                return AssumptionValidation::MissingAssumption(id);
            };
            match record.applicability(target, loaded_program) {
                Applicability::No => return AssumptionValidation::NotApplicable,
                Applicability::Maybe => return AssumptionValidation::ContextUnknown,
                Applicability::Yes => {}
            }

            let matching = executors
                .iter()
                .filter(|executor| executor.validator() == record.validator)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return AssumptionValidation::UntrustedValidator;
            }
            let executor = matching[0];
            let trust = match policy.decide(&record.validator, executor.implementation()) {
                Ok(trust) => trust,
                Err(_) => return AssumptionValidation::UntrustedValidator,
            };
            let root = match trust {
                ValidatorTrustDecision::Trusted { root } => root,
                ValidatorTrustDecision::Untrusted => {
                    return AssumptionValidation::UntrustedValidator;
                }
                ValidatorTrustDecision::Revoked { effective_revision } => {
                    return AssumptionValidation::RevokedValidator { effective_revision };
                }
            };

            active.push(id);
            let mut prerequisites = Vec::with_capacity(record.prerequisites.len());
            for prerequisite_id in &record.prerequisites {
                match visit(scope, *prerequisite_id, active, memo) {
                    AssumptionValidation::Established(_) => {
                        let Some(prerequisite) = catalog.assumptions.get(prerequisite_id) else {
                            active.pop();
                            return AssumptionValidation::MissingAssumption(*prerequisite_id);
                        };
                        prerequisites.push(prerequisite.clone());
                    }
                    _ => {
                        active.pop();
                        return AssumptionValidation::PrerequisiteNotEstablished(*prerequisite_id);
                    }
                }
            }
            let mut evidence = Vec::with_capacity(record.supporting_evidence.len());
            let mut seen_evidence = Vec::new();
            let mut active_evidence = Vec::new();
            for evidence_id in &record.supporting_evidence {
                if let Err(error) = collect_evidence(
                    scope,
                    *evidence_id,
                    active,
                    memo,
                    &mut active_evidence,
                    &mut seen_evidence,
                    &mut evidence,
                ) {
                    active.pop();
                    return error;
                }
            }
            let outcome =
                executor.validate(record, &evidence, &prerequisites, target, loaded_program);
            active.pop();
            let result = match outcome {
                Applicability::Yes => {
                    AssumptionValidation::Established(Box::new(EstablishedAssumption {
                        assumption: id,
                        target: target.clone(),
                        loaded_program,
                        policy_key,
                        root,
                    }))
                }
                Applicability::No => AssumptionValidation::InvalidClaim,
                Applicability::Maybe => AssumptionValidation::ContextUnknown,
            };
            memo.insert(id, result.clone());
            result
        }

        let scope = Scope {
            catalog: self,
            target,
            loaded_program,
            policy,
            policy_key,
            executors,
        };
        Ok(visit(&scope, id, &mut Vec::new(), &mut BTreeMap::new()))
    }

    pub fn validate_graph_assumptions(
        &self,
        graph: &BehaviorGraph,
        key: &EntryKey,
        target: &TargetScenario,
        loaded_program: Option<Digest>,
        policy: &ValidatorTrustPolicy,
        executors: &[&dyn AssumptionValidatorExecutor],
    ) -> Result<Vec<Digest>, AssumptionGraphValidationError> {
        graph
            .validate_structure()
            .map_err(AssumptionGraphValidationError::Graph)?;
        let graph_id = graph.canonical_digest().map_err(|_| {
            AssumptionGraphValidationError::Graph(GraphError::CanonicalEncodingFailed)
        })?;
        let applies = self.entries.get(&key.normalized()).is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry.graph == graph_id && entry.applicability.test(target) == Applicability::Yes
            })
        });
        if !applies {
            return Err(AssumptionGraphValidationError::EntryNotApplicable(
                key.clone(),
            ));
        }
        let mut required = Vec::new();
        for item in &graph.coverage {
            if !matches!(item.coverage, crate::Coverage::Closed) {
                continue;
            }
            for id in &item.assumptions {
                if !required.contains(id) {
                    required.push(*id);
                }
            }
        }
        for id in &required {
            let decision = self
                .validate_assumption(*id, target, loaded_program, policy, executors)
                .map_err(AssumptionGraphValidationError::Trust)?;
            if !matches!(decision, AssumptionValidation::Established(_)) {
                return Err(AssumptionGraphValidationError::NotEstablished {
                    assumption: *id,
                    decision,
                });
            }
        }
        required.sort();
        Ok(required)
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

#[derive(Debug)]
pub struct AssumptionProjection<'a> {
    pub projection: Projection<'a>,
    pub context: crate::AssumptionCertificateContext,
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

    struct TestValidator {
        identity: AssumptionValidator,
        implementation: Digest,
        result: Applicability,
        calls: std::cell::Cell<usize>,
    }

    impl AssumptionValidatorExecutor for TestValidator {
        fn validator(&self) -> AssumptionValidator {
            self.identity.clone()
        }

        fn implementation(&self) -> Digest {
            self.implementation
        }

        fn validate(
            &self,
            _: &AssumptionRecord,
            supporting_evidence: &[EvidenceRecord],
            _: &[AssumptionRecord],
            _: &TargetScenario,
            _: Option<Digest>,
        ) -> Applicability {
            assert!(!supporting_evidence.is_empty());
            self.calls.set(self.calls.get() + 1);
            self.result
        }
    }

    fn trust_policy(
        validator: AssumptionValidator,
        implementation: Digest,
    ) -> ValidatorTrustPolicy {
        ValidatorTrustPolicy {
            revision: 1,
            roots: vec![ValidatorTrustRoot {
                id: Bytes(b"root".to_vec()),
                fingerprint: Digest([0xA1; 32]),
            }],
            pins: vec![ValidatorPin {
                validator,
                implementation,
                root: Bytes(b"root".to_vec()),
            }],
            revocations: vec![],
        }
    }

    #[test]
    fn assumption_validation_executes_only_the_exact_trusted_implementation() {
        let mut catalog = Catalog::default();
        let evidence = catalog.add_evidence(evidence_record()).unwrap();
        let assumption = assumption_record(evidence);
        let id = catalog.add_assumption(assumption.clone()).unwrap();
        let implementation = Digest([0x53; 32]);
        let executor = TestValidator {
            identity: assumption.validator.clone(),
            implementation,
            result: Applicability::Yes,
            calls: std::cell::Cell::new(0),
        };
        let policy = trust_policy(assumption.validator.clone(), implementation);
        assert!(matches!(
            catalog.validate_assumption(id, &scenario(None), None, &policy, &[&executor]),
            Ok(AssumptionValidation::Established(_))
        ));
        assert_eq!(executor.calls.get(), 1);

        let stale_executor = TestValidator {
            identity: assumption.validator,
            implementation: Digest([0x54; 32]),
            result: Applicability::Yes,
            calls: std::cell::Cell::new(0),
        };
        assert_eq!(
            catalog.validate_assumption(id, &scenario(None), None, &policy, &[&stale_executor]),
            Ok(AssumptionValidation::UntrustedValidator)
        );
        assert_eq!(stale_executor.calls.get(), 0);

        let stale_version_executor = TestValidator {
            identity: AssumptionValidator {
                version: Bytes(b"0".to_vec()),
                ..catalog.explain_assumption(id).unwrap().validator.clone()
            },
            implementation: Digest([0x53; 32]),
            result: Applicability::Yes,
            calls: std::cell::Cell::new(0),
        };
        assert_eq!(
            catalog.validate_assumption(
                id,
                &scenario(None),
                None,
                &policy,
                &[&stale_version_executor]
            ),
            Ok(AssumptionValidation::UntrustedValidator)
        );
        assert_eq!(stale_version_executor.calls.get(), 0);
    }

    #[test]
    fn assumption_validation_is_revoked_and_context_scoped() {
        let mut catalog = Catalog::default();
        let evidence = catalog.add_evidence(evidence_record()).unwrap();
        let mut assumption = assumption_record(evidence);
        assumption.scope.loaded_program = Some(Digest([0x71; 32]));
        let id = catalog.add_assumption(assumption.clone()).unwrap();
        let implementation = Digest([0x72; 32]);
        let executor = TestValidator {
            identity: assumption.validator.clone(),
            implementation,
            result: Applicability::Yes,
            calls: std::cell::Cell::new(0),
        };
        let policy = trust_policy(assumption.validator.clone(), implementation);
        assert_eq!(
            catalog.validate_assumption(id, &scenario(None), None, &policy, &[&executor]),
            Ok(AssumptionValidation::ContextUnknown)
        );
        let mut revoked = policy;
        revoked.revision = 2;
        revoked.revocations.push(ValidatorRevocation {
            validator_id: assumption.validator.id.clone(),
            version: assumption.validator.version.clone(),
            effective_revision: 2,
        });
        assert_eq!(
            catalog.validate_assumption(
                id,
                &scenario(None),
                Some(Digest([0x71; 32])),
                &revoked,
                &[&executor]
            ),
            Ok(AssumptionValidation::RevokedValidator {
                effective_revision: 2,
            })
        );
    }

    #[test]
    fn assumption_validation_checks_prerequisite_and_transitive_evidence_closure() {
        let mut catalog = Catalog::default();
        let base_evidence = catalog.add_evidence(evidence_record()).unwrap();
        let prerequisite = assumption_record(base_evidence);
        let prerequisite_id = catalog.add_assumption(prerequisite.clone()).unwrap();
        let derived_evidence = EvidenceRecord {
            assumptions: vec![prerequisite_id],
            dependencies: vec![base_evidence],
            ..evidence_record()
        };
        let derived_evidence_id = catalog.add_evidence(derived_evidence).unwrap();
        let mut assumption = assumption_record(derived_evidence_id);
        assumption.claim = Bytes(b"derived claim conditioned on prerequisite".to_vec());
        assumption.prerequisites = vec![prerequisite_id];
        let id = catalog.add_assumption(assumption.clone()).unwrap();
        let implementation = Digest([0x73; 32]);
        let executor = TestValidator {
            identity: assumption.validator.clone(),
            implementation,
            result: Applicability::Yes,
            calls: std::cell::Cell::new(0),
        };
        let policy = trust_policy(assumption.validator, implementation);
        assert!(matches!(
            catalog.validate_assumption(id, &scenario(None), None, &policy, &[&executor]),
            Ok(AssumptionValidation::Established(_))
        ));
        // The shared prerequisite is executed once despite being reached both
        // through the explicit prerequisite edge and evidence provenance.
        assert_eq!(executor.calls.get(), 2);
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
    fn validator_trust_requires_exact_root_version_and_implementation_pin() {
        let validator = AssumptionValidator {
            id: Bytes(b"profile-checker".to_vec()),
            version: Bytes(b"3.1.0".to_vec()),
        };
        let implementation = Digest([0x31; 32]);
        let policy = ValidatorTrustPolicy {
            revision: 4,
            roots: vec![ValidatorTrustRoot {
                id: Bytes(b"deployment-root-1".to_vec()),
                fingerprint: Digest([0xA1; 32]),
            }],
            pins: vec![ValidatorPin {
                validator: validator.clone(),
                implementation,
                root: Bytes(b"deployment-root-1".to_vec()),
            }],
            revocations: vec![],
        };
        assert_eq!(
            policy.decide(&validator, implementation),
            Ok(ValidatorTrustDecision::Trusted {
                root: Bytes(b"deployment-root-1".to_vec()),
            })
        );
        assert_eq!(
            policy.decide(&validator, Digest([0x32; 32])),
            Ok(ValidatorTrustDecision::Untrusted)
        );
        let stale = AssumptionValidator {
            version: Bytes(b"3.0.9".to_vec()),
            ..validator.clone()
        };
        assert_eq!(
            policy.decide(&stale, implementation),
            Ok(ValidatorTrustDecision::Untrusted)
        );
    }

    #[test]
    fn validator_revocation_changes_decision_and_invalidates_cache_key() {
        let validator = AssumptionValidator {
            id: Bytes(b"profile-checker".to_vec()),
            version: Bytes(b"3.1.0".to_vec()),
        };
        let implementation = Digest([0x31; 32]);
        let mut policy = ValidatorTrustPolicy {
            revision: 4,
            roots: vec![ValidatorTrustRoot {
                id: Bytes(b"deployment-root-1".to_vec()),
                fingerprint: Digest([0xA1; 32]),
            }],
            pins: vec![ValidatorPin {
                validator: validator.clone(),
                implementation,
                root: Bytes(b"deployment-root-1".to_vec()),
            }],
            revocations: vec![],
        };
        let before = policy.cache_key().unwrap();
        policy.revision = 5;
        policy.revocations.push(ValidatorRevocation {
            validator_id: validator.id.clone(),
            version: validator.version.clone(),
            effective_revision: 5,
        });
        assert_ne!(before, policy.cache_key().unwrap());
        assert_eq!(
            policy.decide(&validator, implementation),
            Ok(ValidatorTrustDecision::Revoked {
                effective_revision: 5,
            })
        );
    }

    #[test]
    fn validator_trust_policy_rejects_unrooted_and_future_revocations() {
        let validator = AssumptionValidator {
            id: Bytes(b"profile-checker".to_vec()),
            version: Bytes(b"3.1.0".to_vec()),
        };
        let policy = ValidatorTrustPolicy {
            revision: 1,
            roots: vec![],
            pins: vec![ValidatorPin {
                validator: validator.clone(),
                implementation: Digest([0x31; 32]),
                root: Bytes(b"missing-root".to_vec()),
            }],
            revocations: vec![],
        };
        assert_eq!(
            policy.validate(),
            Err(ValidatorTrustError::MissingRoot(Bytes(
                b"missing-root".to_vec()
            )))
        );
        let policy = ValidatorTrustPolicy {
            revision: 1,
            roots: vec![ValidatorTrustRoot {
                id: Bytes(b"root".to_vec()),
                fingerprint: Digest([0xA1; 32]),
            }],
            pins: vec![],
            revocations: vec![ValidatorRevocation {
                validator_id: validator.id,
                version: validator.version,
                effective_revision: 2,
            }],
        };
        assert!(matches!(
            policy.validate(),
            Err(ValidatorTrustError::FutureRevocation(_, _))
        ));
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
        catalog
            .add_entry(function(b"core", b"strlen"), graph)
            .unwrap();
        assert!(
            catalog
                .project(&function(b"core", b"strlen"), Domain::Return)
                .is_none()
        );
        let implementation = Digest([0x81; 32]);
        let executor = TestValidator {
            identity: catalog
                .explain_assumption(assumption_id)
                .unwrap()
                .validator
                .clone(),
            implementation,
            result: Applicability::Yes,
            calls: std::cell::Cell::new(0),
        };
        let policy = trust_policy(executor.identity.clone(), implementation);
        assert!(
            catalog
                .project_with_assumption_context(
                    &function(b"core", b"strlen"),
                    Domain::Return,
                    &scenario(None),
                    None,
                    &policy,
                    &[&executor]
                )
                .unwrap()
                .is_some()
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
    fn evidence_assumption_order_is_rejected_at_catalog_admission() {
        let first = Digest([7; 32]);
        let second = Digest([8; 32]);
        let mut descending = evidence_record();
        descending.assumptions = vec![second, first];
        let mut ascending = descending.clone();
        ascending.assumptions.reverse();
        assert_eq!(ascending.id().unwrap(), descending.id().unwrap());
        let mut catalog = Catalog::default();
        assert_eq!(
            catalog.add_evidence(descending),
            Err(CatalogError::NonCanonicalOrder("evidence assumptions"))
        );
    }

    #[test]
    fn assumption_and_evidence_reference_lists_reject_permutations_and_duplicates() {
        let first = Digest([7; 32]);
        let second = Digest([8; 32]);
        let mut catalog = Catalog::default();
        let mut evidence = evidence_record();
        evidence.dependencies = vec![second, first];
        assert_eq!(
            catalog.add_evidence(evidence),
            Err(CatalogError::NonCanonicalOrder("evidence dependencies"))
        );
        let mut assumption = assumption_record(first);
        assumption.prerequisites = vec![second, first];
        let mut permuted_assumption = assumption.clone();
        permuted_assumption.prerequisites.reverse();
        assert_eq!(assumption.id().unwrap(), permuted_assumption.id().unwrap());
        assert_eq!(
            catalog.add_assumption(assumption),
            Err(CatalogError::NonCanonicalOrder("assumption prerequisites"))
        );
        let mut assumption = assumption_record(first);
        assumption.supporting_evidence = vec![second, first];
        assert_eq!(
            catalog.add_assumption(assumption),
            Err(CatalogError::NonCanonicalOrder(
                "assumption supporting evidence"
            ))
        );
        let mut evidence = evidence_record();
        evidence.assumptions = vec![first, first];
        assert_eq!(
            catalog.add_evidence(evidence),
            Err(CatalogError::NonCanonicalOrder("evidence assumptions"))
        );
    }

    #[test]
    fn manifest_record_vectors_must_be_sorted_by_content_id() {
        let mut catalog = Catalog::default();
        let mut first = evidence_record();
        first.source_digest = Digest([1; 32]);
        let mut second = evidence_record();
        second.source_digest = Digest([2; 32]);
        catalog.add_evidence(first).unwrap();
        catalog.add_evidence(second).unwrap();
        let manifest = catalog.manifest();
        let mut permuted = manifest.clone();
        permuted.evidence.reverse();
        assert_eq!(
            manifest.canonical_digest().unwrap(),
            permuted.canonical_digest().unwrap()
        );
        assert_eq!(
            permuted.validate(),
            Err(CatalogError::NonCanonicalOrder("manifest evidence"))
        );

        let mut unsorted = evidence_record();
        unsorted.assumptions = vec![Digest([8; 32]), Digest([7; 32])];
        let id = unsorted.id().unwrap();
        let noncanonical_record = CatalogManifest {
            schema_major: crate::SCHEMA_MAJOR,
            entries: vec![],
            graphs: vec![],
            evidence: vec![(id, unsorted)],
            assumptions: vec![],
        };
        assert_eq!(
            noncanonical_record.validate(),
            Err(CatalogError::NonCanonicalOrder("evidence assumptions"))
        );
    }

    #[test]
    fn manifest_entries_graphs_and_assumptions_reject_permutations() {
        let mut manifest = CatalogManifest {
            schema_major: crate::SCHEMA_MAJOR,
            entries: vec![],
            graphs: vec![],
            evidence: vec![],
            assumptions: vec![],
        };
        manifest.entries = vec![
            CatalogEntry {
                key: EntryKey::Function {
                    extension: Bytes(b"core".to_vec()),
                    name: Bytes(b"a".to_vec()),
                },
                graph: Digest([1; 32]),
                applicability: TargetPredicate::any(),
            },
            CatalogEntry {
                key: EntryKey::Function {
                    extension: Bytes(b"core".to_vec()),
                    name: Bytes(b"b".to_vec()),
                },
                graph: Digest([2; 32]),
                applicability: TargetPredicate::any(),
            },
        ];
        let entry_digest = manifest.canonical_digest().unwrap();
        manifest.entries.reverse();
        assert_eq!(entry_digest, manifest.canonical_digest().unwrap());
        assert_eq!(
            manifest.validate(),
            Err(CatalogError::NonCanonicalOrder("manifest entries"))
        );
        manifest.entries.clear();
        manifest.graphs = vec![
            GraphRecord {
                id: Digest([1; 32]),
                graph: graph(Digest([3; 32])),
            },
            GraphRecord {
                id: Digest([2; 32]),
                graph: graph(Digest([4; 32])),
            },
        ];
        let graph_digest = manifest.canonical_digest().unwrap();
        manifest.graphs.reverse();
        assert_eq!(graph_digest, manifest.canonical_digest().unwrap());
        assert_eq!(
            manifest.validate(),
            Err(CatalogError::NonCanonicalOrder("manifest graphs"))
        );
        manifest.graphs.clear();
        manifest.assumptions = vec![
            (Digest([1; 32]), assumption_record(Digest([3; 32]))),
            (Digest([2; 32]), assumption_record(Digest([4; 32]))),
        ];
        let assumption_digest = manifest.canonical_digest().unwrap();
        manifest.assumptions.reverse();
        assert_eq!(assumption_digest, manifest.canonical_digest().unwrap());
        assert_eq!(
            manifest.validate(),
            Err(CatalogError::NonCanonicalOrder("manifest assumptions"))
        );

        let mut with_target_set = CatalogManifest {
            schema_major: crate::SCHEMA_MAJOR,
            entries: vec![CatalogEntry {
                key: EntryKey::Function {
                    extension: Bytes(b"core".to_vec()),
                    name: Bytes(b"targeted".to_vec()),
                },
                graph: Digest([3; 32]),
                applicability: TargetPredicate {
                    extensions: vec![
                        (Bytes(b"curl".to_vec()), true),
                        (Bytes(b"zlib".to_vec()), true),
                    ],
                    ..TargetPredicate::any()
                },
            }],
            graphs: vec![],
            evidence: vec![],
            assumptions: vec![],
        };
        let original_digest = with_target_set.canonical_digest().unwrap();
        with_target_set.entries[0]
            .applicability
            .extensions
            .reverse();
        assert_eq!(original_digest, with_target_set.canonical_digest().unwrap());
        assert_eq!(
            with_target_set.validate(),
            Err(CatalogError::InvalidTargetPredicate(
                TargetError::NonCanonicalOrder("target extensions")
            ))
        );
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

#[cfg(test)]
mod has_function_tests {
    use super::Catalog;

    #[test]
    fn the_embedded_strlen_entry_is_found_by_any_spelling_of_its_name() {
        let catalog = Catalog::from_json(include_str!("../data/php-8.5.4-strlen.json"))
            .expect("the embedded catalog validates");
        assert!(catalog.has_function("standard", "strlen"));
        assert!(catalog.has_function("STANDARD", "STRLEN"));
        assert!(!catalog.has_function("standard", "strtolower"));
        assert!(!catalog.has_function("core", "strlen"));
    }
}
