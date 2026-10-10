//! Shared, checker-independent vocabulary and evaluator for PHP behavior.
//!
//! This crate deliberately knows nothing about PHPStan's type system or the
//! native compiler importer. A host supplies values, state, predicates, and
//! event transfers; the graph preserves ordering and distinct exit kinds.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

mod catalog;
mod php_value;
mod target;
pub use catalog::{
    AssumptionGraphValidationError, AssumptionKind, AssumptionProjection, AssumptionRecord,
    AssumptionScope, AssumptionValidation, AssumptionValidator, AssumptionValidatorExecutor,
    Catalog, CatalogEntry, CatalogError, CatalogManifest, CatalogTargetSelection, EntryKey,
    EvidenceKind, EvidenceRecord, GraphRecord, Projection, Selection, SupportNode, ValidatorPin,
    ValidatorRevocation, ValidatorTrustDecision, ValidatorTrustError, ValidatorTrustPolicy,
    ValidatorTrustRoot, VariantSelection,
};
pub use php_value::{
    ArrayId, ArrayKey, Binding, ObjectId, PhpCallbackOutcome, PhpFlowHost, PhpState, PhpValue,
    ReferenceId, StateError,
};
pub use target::{
    Applicability, ReleaseVersion, TargetError, TargetPredicate, TargetProfile, TargetScenario,
    TargetSelection, TargetSet, VersionRange,
};

pub const SCHEMA_MAJOR: u32 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Digest(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Bytes(pub Vec<u8>);

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("base64", &BASE64.encode(&self.0))?;
            map.end()
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BytesVisitor {
            human_readable: bool,
        }
        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = Bytes;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("base64 byte object or binary byte sequence")
            }
            fn visit_bytes<E: serde::de::Error>(self, value: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(value.to_vec()))
            }
            fn visit_byte_buf<E: serde::de::Error>(self, value: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(value))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Bytes, A::Error> {
                let mut bytes = Vec::new();
                while let Some(byte) = seq.next_element()? {
                    bytes.push(byte);
                }
                Ok(Bytes(bytes))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Bytes, A::Error> {
                if !self.human_readable {
                    return Err(serde::de::Error::custom("expected binary byte sequence"));
                }
                let mut encoded = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key != "base64" || encoded.is_some() {
                        return Err(serde::de::Error::custom("expected one base64 field"));
                    }
                    encoded = Some(map.next_value::<String>()?);
                }
                let encoded = encoded.ok_or_else(|| serde::de::Error::missing_field("base64"))?;
                BASE64
                    .decode(encoded)
                    .map(Bytes)
                    .map_err(serde::de::Error::custom)
            }
        }
        if deserializer.is_human_readable() {
            deserializer.deserialize_map(BytesVisitor {
                human_readable: true,
            })
        } else {
            deserializer.deserialize_bytes(BytesVisitor {
                human_readable: false,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Domain {
    Invocation,
    UserRead,
    UserWrite,
    ExternalRead,
    ExternalWrite,
    Diagnostic,
    Autoload,
    Output,
    Release,
    Retention,
    Return,
    PhpThrow,
    NativeUnwind,
    Fatal,
    Suspension,
}

impl Domain {
    pub const ALL: [Self; 15] = [
        Self::Invocation,
        Self::UserRead,
        Self::UserWrite,
        Self::ExternalRead,
        Self::ExternalWrite,
        Self::Diagnostic,
        Self::Autoload,
        Self::Output,
        Self::Release,
        Self::Retention,
        Self::Return,
        Self::PhpThrow,
        Self::NativeUnwind,
        Self::Fatal,
        Self::Suspension,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Coverage {
    Closed,
    Open { reasons: Vec<Bytes> },
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Precision {
    Exact,
    Joined,
    Widened,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainCoverage {
    pub domain: Domain,
    pub coverage: Coverage,
    pub precision: Precision,
    /// Other semantic domains required to justify this domain's facts.
    pub dependencies: Vec<Domain>,
    pub evidence: Vec<Digest>,
    pub assumptions: Vec<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operand {
    Argument(u32),
    Receiver,
    Result(u32),
    Literal(Bytes),
    /// A source/model-backed integer envelope for a normal return.
    IntRange {
        min: Option<i64>,
        max: Option<i64>,
    },
    /// PHP's byte length of a string argument, with a nonnegative range
    /// fallback when the host cannot enumerate constant strings.
    StringByteLength(u32),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Read {
        storage: Operand,
    },
    Write {
        storage: Operand,
        value: Operand,
    },
    Invoke {
        callable: Operand,
        arguments: Vec<Operand>,
        result: u32,
    },
    /// Release a value held by the native operation after conversion.
    /// A host may attach destructor effects and abrupt outcomes to this
    /// ordered point; the catalog currently leaves those consequences open.
    Release {
        value: Operand,
    },
    /// A diagnostic that is observable when this ordered event is reached.
    /// The argument index identifies the PHP call operand used for its span.
    EmitDiagnostic {
        kind: DiagnosticKind,
        argument: u32,
    },
    Unknown {
        domains: Vec<Domain>,
        reason: Bytes,
    },
    /// PHP value/reference operations executed by the PHP-specific host.
    PhpFlow {
        instruction: PhpFlowInstruction,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DiagnosticKind {
    NullToInternalScalarParameterDeprecated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhpFlowInstruction {
    Assign {
        destination: Bytes,
        source: Bytes,
    },
    Unset {
        local: Bytes,
    },
    BindReference {
        left: Bytes,
        right: Bytes,
    },
    ArraySet {
        array: Bytes,
        key: ArrayKey,
        source: Bytes,
    },
    ArrayGet {
        destination: Bytes,
        array: Bytes,
        key: ArrayKey,
    },
    ArrayBindReference {
        array: Bytes,
        key: ArrayKey,
        local: Bytes,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Predicate {
    Always,
    IsLiteral {
        value: Operand,
        expected: Bytes,
    },
    /// A typed-by-host predicate for state or target facts outside the shared
    /// operand vocabulary. Hosts must return `Maybe` when an ID is unknown or
    /// cannot be decided soundly.
    Host(Bytes),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Terminator {
    Goto(u32),
    Branch {
        predicate: Predicate,
        yes: u32,
        no: u32,
    },
    Return(Option<Operand>),
    Throw(Operand),
    Fatal(Operand),
    Suspend(Operand),
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub events: Vec<Event>,
    pub terminator: Terminator,
    pub exceptional: ExceptionalSuccessors,
}

impl Block {
    pub fn new(events: Vec<Event>, terminator: Terminator) -> Self {
        Self {
            events,
            terminator,
            exceptional: ExceptionalSuccessors::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionalSuccessors {
    pub php_throw: Option<u32>,
    pub native_unwind: Option<u32>,
    pub fatal: Option<u32>,
    pub suspended: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehaviorGraph {
    pub schema_major: u32,
    pub entry: u32,
    pub blocks: Vec<Block>,
    pub coverage: Vec<DomainCoverage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    UnsupportedSchema(u32),
    Empty,
    InvalidEntry(u32),
    InvalidSuccessor {
        block: u32,
        successor: u32,
    },
    DuplicateCoverage(Domain),
    MissingCoverage(Domain),
    OpenDomainWithoutUnknown(Domain),
    MissingEvidence(Domain),
    OpenWithoutReason(Domain),
    UnknownTransferWithoutReason,
    UnknownTransferWithoutOpenCoverage(Domain),
    DuplicateDomainDependency(Domain),
    ClosedDomainDependsOnOpen {
        domain: Domain,
        dependency: Domain,
    },
    ClosedDomainDependsOnNotApplicable {
        domain: Domain,
        dependency: Domain,
    },
    UnestablishedAssumption(Domain),
    CyclicDomainDependency(Domain),
    ApplicableDomainMarkedNotApplicable(Domain),
    UnresolvedValue,
    ResultDefinedOnOnlySomePaths(u32),
    EventWithoutOutcome,
    NonMonotoneJoin(u32),
    NonPostFixpoint(u32),
    InvalidCertificateBlock(u32),
    CertificateGraphMismatch,
    CanonicalEncodingFailed,
    ResumeWithoutPendingOutcome,
    InvalidComposedCoverage,
    MissingRequiredDomainDependency {
        domain: Domain,
        dependency: Domain,
    },
    UnsupportedCleanupReplacement {
        pending: CompletionKind,
        replacement: CompletionKind,
    },
    NonCanonicalOrder(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssumptionCertificateError {
    MissingContext,
    InvalidTrustPolicy,
    ContextChanged,
    Validation(AssumptionGraphValidationError),
    Graph(GraphError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssumptionEvaluationError {
    Validation(AssumptionGraphValidationError),
    Graph(GraphError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    PhpThrow,
    NativeUnwind,
    Fatal,
    Suspended,
    Normal,
}

fn pending_completion_kind<V>(pending: &PendingOutcome<V>) -> CompletionKind {
    match pending {
        PendingOutcome::PhpThrow(_) => CompletionKind::PhpThrow,
        PendingOutcome::NativeUnwind(_) => CompletionKind::NativeUnwind,
        PendingOutcome::Fatal(_) => CompletionKind::Fatal,
        PendingOutcome::Suspended(_) => CompletionKind::Suspended,
    }
}

fn check_cleanup_replacement<V>(
    pending: &Option<PendingOutcome<V>>,
    replacement: CompletionKind,
) -> Result<(), GraphError> {
    if let Some(pending) = pending {
        let pending = pending_completion_kind(pending);
        // Evidence-backed PHP cleanup cases: a destructor return preserves
        // the pending PHP throw, while a PHP throw from that destructor
        // replaces it. Other completion-kind combinations remain fail-closed.
        if pending == CompletionKind::PhpThrow
            && matches!(
                replacement,
                CompletionKind::Normal | CompletionKind::PhpThrow
            )
        {
            return Ok(());
        }
        return Err(GraphError::UnsupportedCleanupReplacement {
            pending,
            replacement,
        });
    }
    Ok(())
}

impl BehaviorGraph {
    /// Encode in the schema's canonical field/vector order and identify the
    /// result with SHA-256. Semantically unordered vectors are normalized for
    /// identity; validation still rejects noncanonical serialized input.
    pub fn canonical_digest(&self) -> Result<Digest, postcard::Error> {
        let canonical = self.canonicalized_order();
        let bytes = postcard::to_allocvec(&canonical)?;
        Ok(Digest(Sha256::digest(bytes).into()))
    }

    pub(crate) fn canonicalized_order(&self) -> Self {
        let mut canonical = self.clone();
        canonical.coverage.sort_by_key(|item| {
            Domain::ALL
                .iter()
                .position(|domain| *domain == item.domain)
                .unwrap_or(usize::MAX)
        });
        for item in &mut canonical.coverage {
            item.dependencies.sort_by_key(|dependency| {
                Domain::ALL
                    .iter()
                    .position(|domain| domain == dependency)
                    .unwrap_or(usize::MAX)
            });
            item.evidence.sort();
            item.assumptions.sort();
            if let Coverage::Open { reasons } = &mut item.coverage {
                reasons.sort();
            }
        }
        for block in &mut canonical.blocks {
            for event in &mut block.events {
                if let Event::Unknown { domains, .. } = event {
                    domains.sort_by_key(|domain| {
                        Domain::ALL
                            .iter()
                            .position(|candidate| candidate == domain)
                            .unwrap_or(usize::MAX)
                    });
                }
            }
        }
        canonical
    }

    pub fn validate(&self) -> Result<(), GraphError> {
        self.validate_with_assumptions(&[])
    }

    pub(crate) fn validate_structure(&self) -> Result<(), GraphError> {
        self.validate_inner(None)
    }

    pub(crate) fn validate_with_assumptions(
        &self,
        established: &[Digest],
    ) -> Result<(), GraphError> {
        self.validate_inner(Some(established))
    }

    fn validate_inner(&self, established: Option<&[Digest]>) -> Result<(), GraphError> {
        if self.schema_major != SCHEMA_MAJOR {
            return Err(GraphError::UnsupportedSchema(self.schema_major));
        }
        if self.blocks.is_empty() {
            return Err(GraphError::Empty);
        }
        if self.entry as usize >= self.blocks.len() {
            return Err(GraphError::InvalidEntry(self.entry));
        }
        for (i, block) in self.blocks.iter().enumerate() {
            let mut successors: Vec<u32> = match block.terminator {
                Terminator::Goto(n) => vec![n],
                Terminator::Branch { yes, no, .. } => vec![yes, no],
                _ => vec![],
            };
            successors.extend(
                [
                    block.exceptional.php_throw,
                    block.exceptional.native_unwind,
                    block.exceptional.fatal,
                    block.exceptional.suspended,
                ]
                .into_iter()
                .flatten(),
            );
            for successor in successors {
                if successor as usize >= self.blocks.len() {
                    return Err(GraphError::InvalidSuccessor {
                        block: i as u32,
                        successor,
                    });
                }
            }
        }
        let mut colors = vec![0u8; self.blocks.len()];
        fn visit(id: u32, graph: &BehaviorGraph, colors: &mut [u8]) -> Result<(), GraphError> {
            match colors[id as usize] {
                1 | 2 => return Ok(()),
                _ => {}
            }
            colors[id as usize] = 1;
            let block = &graph.blocks[id as usize];
            let mut next = match block.terminator {
                Terminator::Goto(n) => vec![n],
                Terminator::Branch { yes, no, .. } => vec![yes, no],
                _ => vec![],
            };
            next.extend(
                [
                    block.exceptional.php_throw,
                    block.exceptional.native_unwind,
                    block.exceptional.fatal,
                    block.exceptional.suspended,
                ]
                .into_iter()
                .flatten(),
            );
            for child in next {
                visit(child, graph, colors)?;
            }
            colors[id as usize] = 2;
            Ok(())
        }
        visit(self.entry, self, &mut colors)?;
        let mut seen = Vec::new();
        for item in &self.coverage {
            let domain = &item.domain;
            if seen.contains(domain) {
                return Err(GraphError::DuplicateCoverage(*domain));
            }
            seen.push(*domain);
        }
        for domain in Domain::ALL {
            if !seen.contains(&domain) {
                return Err(GraphError::MissingCoverage(domain));
            }
        }
        if self.coverage.iter().map(|item| item.domain).ne(Domain::ALL) {
            return Err(GraphError::NonCanonicalOrder("graph coverage"));
        }
        for item in &self.coverage {
            if item.dependencies.windows(2).any(|pair| {
                Domain::ALL.iter().position(|domain| *domain == pair[0])
                    >= Domain::ALL.iter().position(|domain| *domain == pair[1])
            }) {
                return Err(GraphError::NonCanonicalOrder("domain dependencies"));
            }
            if item.evidence.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(GraphError::NonCanonicalOrder("coverage evidence"));
            }
            if item.assumptions.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(GraphError::NonCanonicalOrder("coverage assumptions"));
            }
            if let Coverage::Open { reasons } = &item.coverage
                && reasons.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(GraphError::NonCanonicalOrder("open coverage reasons"));
            }
        }
        let unknown_events: Vec<&Event> = self
            .blocks
            .iter()
            .enumerate()
            .filter(|(i, _)| colors[*i] == 2)
            .flat_map(|(_, block)| block.events.iter())
            .filter(|event| matches!(event, Event::Unknown { .. }))
            .collect();
        let unknown_domains: Vec<Domain> = unknown_events
            .iter()
            .flat_map(|event| match event {
                Event::Unknown { domains, reason } => {
                    if reason.0.is_empty() || domains.is_empty() {
                        vec![]
                    } else {
                        domains.clone()
                    }
                }
                _ => unreachable!(),
            })
            .collect();
        if unknown_events.iter().any(|event| matches!(event, Event::Unknown { domains, reason } if domains.is_empty() || reason.0.is_empty())) {
            return Err(GraphError::UnknownTransferWithoutReason);
        }
        for event in &unknown_events {
            if let Event::Unknown { domains, .. } = event
                && domains.windows(2).any(|pair| {
                    Domain::ALL.iter().position(|domain| *domain == pair[0])
                        >= Domain::ALL.iter().position(|domain| *domain == pair[1])
                })
            {
                return Err(GraphError::NonCanonicalOrder("unknown event domains"));
            }
        }
        let mut required_domains = Vec::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if colors[index] != 2 {
                continue;
            }
            for event in &block.events {
                let required = event_domains(event);
                for domain in required {
                    if !required_domains.contains(&domain) {
                        required_domains.push(domain);
                    }
                }
            }
            let required = match block.terminator {
                Terminator::Return(_) => Some(Domain::Return),
                Terminator::Throw(_) => Some(Domain::PhpThrow),
                Terminator::Fatal(_) => Some(Domain::Fatal),
                Terminator::Suspend(_) => Some(Domain::Suspension),
                _ => None,
            };
            if let Some(domain) = required
                && !required_domains.contains(&domain)
            {
                required_domains.push(domain);
            }
        }
        for domain in required_domains {
            if self.coverage.iter().any(|item| {
                item.domain == domain && matches!(item.coverage, Coverage::NotApplicable)
            }) {
                return Err(GraphError::ApplicableDomainMarkedNotApplicable(domain));
            }
        }
        for item in &self.coverage {
            if matches!(item.coverage, Coverage::Open { ref reasons } if reasons.is_empty()) {
                return Err(GraphError::OpenWithoutReason(item.domain));
            }
            if !matches!(item.coverage, Coverage::Open { .. }) && item.evidence.is_empty() {
                return Err(GraphError::MissingEvidence(item.domain));
            }
            if matches!(item.coverage, Coverage::Open { .. })
                && !unknown_domains.contains(&item.domain)
            {
                return Err(GraphError::OpenDomainWithoutUnknown(item.domain));
            }
        }
        for domain in unknown_domains {
            if !self
                .coverage
                .iter()
                .any(|item| item.domain == domain && matches!(item.coverage, Coverage::Open { .. }))
            {
                return Err(GraphError::UnknownTransferWithoutOpenCoverage(domain));
            }
        }
        validate_domain_dependencies_with_assumptions(&self.coverage, established)?;
        validate_required_domain_dependencies(self, &colors, &self.coverage)?;
        Ok(())
    }
}

fn transitive_domain_dependencies(domain: Domain, coverage: &[DomainCoverage]) -> Vec<Domain> {
    let mut pending = coverage
        .iter()
        .find(|item| item.domain == domain)
        .map_or_else(Vec::new, |item| item.dependencies.clone());
    let mut seen = vec![domain];
    let mut dependencies = Vec::new();
    while let Some(dependency) = pending.pop() {
        if seen.contains(&dependency) {
            continue;
        }
        seen.push(dependency);
        dependencies.push(dependency);
        if let Some(item) = coverage.iter().find(|item| item.domain == dependency) {
            pending.extend(item.dependencies.iter().copied());
        }
    }
    dependencies.sort_by_key(|dependency| {
        Domain::ALL
            .iter()
            .position(|candidate| candidate == dependency)
    });
    dependencies
}

fn validate_domain_dependencies(coverage: &[DomainCoverage]) -> Result<(), GraphError> {
    validate_domain_dependencies_with_assumptions(coverage, Some(&[]))
}

fn validate_domain_dependencies_with_assumptions(
    coverage: &[DomainCoverage],
    established: Option<&[Digest]>,
) -> Result<(), GraphError> {
    for item in coverage {
        if matches!(item.coverage, Coverage::Closed)
            && established.is_some_and(|established| {
                item.assumptions
                    .iter()
                    .any(|assumption| !established.contains(assumption))
            })
        {
            return Err(GraphError::UnestablishedAssumption(item.domain));
        }
        let mut dependencies = Vec::new();
        for dependency in &item.dependencies {
            if dependencies.contains(dependency) {
                return Err(GraphError::DuplicateDomainDependency(item.domain));
            }
            dependencies.push(*dependency);
        }
    }

    fn visit(
        domain: Domain,
        coverage: &[DomainCoverage],
        colors: &mut [u8],
    ) -> Result<(), GraphError> {
        let index = Domain::ALL
            .iter()
            .position(|candidate| *candidate == domain)
            .ok_or(GraphError::DuplicateDomainDependency(domain))?;
        match colors[index] {
            1 => return Err(GraphError::CyclicDomainDependency(domain)),
            2 => return Ok(()),
            _ => {}
        }
        colors[index] = 1;
        if let Some(item) = coverage.iter().find(|item| item.domain == domain) {
            for dependency in &item.dependencies {
                visit(*dependency, coverage, colors)?;
            }
        }
        colors[index] = 2;
        Ok(())
    }

    let mut colors = vec![0u8; Domain::ALL.len()];
    for item in coverage {
        visit(item.domain, coverage, &mut colors)?;
    }

    for item in coverage {
        if matches!(item.coverage, Coverage::Closed) {
            let dependencies = transitive_domain_dependencies(item.domain, coverage);
            if let Some(dependency) = dependencies.iter().find(|dependency| {
                coverage.iter().any(|candidate| {
                    candidate.domain == **dependency
                        && matches!(candidate.coverage, Coverage::Open { .. })
                })
            }) {
                return Err(GraphError::ClosedDomainDependsOnOpen {
                    domain: item.domain,
                    dependency: *dependency,
                });
            }
            if let Some(dependency) = dependencies.iter().find(|dependency| {
                coverage.iter().any(|candidate| {
                    candidate.domain == **dependency
                        && matches!(candidate.coverage, Coverage::NotApplicable)
                })
            }) {
                return Err(GraphError::ClosedDomainDependsOnNotApplicable {
                    domain: item.domain,
                    dependency: *dependency,
                });
            }
        }
    }
    Ok(())
}

/// Semantic domains directly exercised by an event contract. This table is
/// shared by applicability and projection-dependency validation so adding a
/// transfer cannot silently bypass either check.
fn event_domains(event: &Event) -> Vec<Domain> {
    match event {
        Event::Read { .. } => vec![Domain::UserRead],
        Event::Write { .. } => vec![Domain::UserWrite],
        Event::Invoke { .. } => vec![Domain::Invocation],
        Event::Release { .. } => vec![Domain::Release],
        Event::EmitDiagnostic { .. } => vec![Domain::Diagnostic],
        Event::PhpFlow { instruction } => match instruction {
            PhpFlowInstruction::Assign { .. }
            | PhpFlowInstruction::BindReference { .. }
            | PhpFlowInstruction::ArraySet { .. }
            | PhpFlowInstruction::ArrayGet { .. }
            | PhpFlowInstruction::ArrayBindReference { .. } => {
                vec![Domain::UserRead, Domain::UserWrite]
            }
            PhpFlowInstruction::Unset { .. } => vec![Domain::UserWrite],
        },
        Event::Unknown { domains, .. } => domains.clone(),
    }
}

fn validate_required_domain_dependencies(
    graph: &BehaviorGraph,
    reachable: &[u8],
    coverage: &[DomainCoverage],
) -> Result<(), GraphError> {
    let mut normal_reachable = vec![false; graph.blocks.len()];
    let mut pending = vec![graph.entry];
    while let Some(block_id) = pending.pop() {
        let index = block_id as usize;
        if normal_reachable[index] {
            continue;
        }
        normal_reachable[index] = true;
        match graph.blocks[index].terminator {
            Terminator::Goto(to) => pending.push(to),
            Terminator::Branch { yes, no, .. } => {
                pending.push(yes);
                pending.push(no);
            }
            Terminator::Return(_)
            | Terminator::Throw(_)
            | Terminator::Fatal(_)
            | Terminator::Suspend(_)
            | Terminator::Resume => {}
        }
    }
    // Cleanup entered for a pending PHP throw can itself return normally.
    // Include those cleanup blocks in Return's dependency analysis, but do not
    // treat a cleanup path ending in Resume or another completion as normal.
    let mut normal_predecessors = vec![Vec::new(); graph.blocks.len()];
    let mut can_reach_return = vec![false; graph.blocks.len()];
    let mut reverse_worklist = Vec::new();
    for (index, block) in graph.blocks.iter().enumerate() {
        match block.terminator {
            Terminator::Goto(to) => normal_predecessors[to as usize].push(index as u32),
            Terminator::Branch { yes, no, .. } => {
                normal_predecessors[yes as usize].push(index as u32);
                normal_predecessors[no as usize].push(index as u32);
            }
            Terminator::Return(_) => {
                can_reach_return[index] = true;
                reverse_worklist.push(index as u32);
            }
            _ => {}
        }
    }
    while let Some(block_id) = reverse_worklist.pop() {
        for predecessor in &normal_predecessors[block_id as usize] {
            let predecessor_index = *predecessor as usize;
            if !can_reach_return[predecessor_index] {
                can_reach_return[predecessor_index] = true;
                reverse_worklist.push(*predecessor);
            }
        }
    }
    let cleanup_entries: Vec<u32> = graph
        .blocks
        .iter()
        .enumerate()
        .filter(|(index, _)| reachable[*index] == 2)
        .filter_map(|(_, block)| block.exceptional.php_throw)
        .filter(|target| can_reach_return[*target as usize])
        .collect();
    let mut pending = cleanup_entries;
    while let Some(block_id) = pending.pop() {
        let index = block_id as usize;
        if normal_reachable[index] || !can_reach_return[index] {
            continue;
        }
        normal_reachable[index] = true;
        match graph.blocks[index].terminator {
            Terminator::Goto(to) if can_reach_return[to as usize] => pending.push(to),
            Terminator::Branch { yes, no, .. } => {
                if can_reach_return[yes as usize] {
                    pending.push(yes);
                }
                if can_reach_return[no as usize] {
                    pending.push(no);
                }
            }
            _ => {}
        }
    }
    let mut projections = Vec::new();
    for (index, block) in graph.blocks.iter().enumerate() {
        if reachable[index] != 2 {
            continue;
        }
        let projection = match block.terminator {
            Terminator::Return(_) => Some(Domain::Return),
            Terminator::Throw(_) => Some(Domain::PhpThrow),
            Terminator::Fatal(_) => Some(Domain::Fatal),
            Terminator::Suspend(_) => Some(Domain::Suspension),
            Terminator::Goto(_) | Terminator::Branch { .. } | Terminator::Resume => None,
        };
        if let Some(projection) = projection
            && !projections.contains(&projection)
        {
            projections.push(projection);
        }
    }

    let has_return_projection = projections.contains(&Domain::Return);
    let mut required = Vec::<(Domain, Domain)>::new();
    for (index, block) in graph.blocks.iter().enumerate() {
        if reachable[index] != 2 {
            continue;
        }
        for event in &block.events {
            match event {
                // A normal-return projection includes the post-event user
                // state. Its declared result dependencies must therefore
                // include each modeled read/write domain on the graph.
                Event::Read { .. } if has_return_projection && normal_reachable[index] => {
                    required.push((Domain::Return, Domain::UserRead));
                }
                Event::Write { .. } if has_return_projection && normal_reachable[index] => {
                    required.push((Domain::Return, Domain::UserWrite));
                }
                Event::PhpFlow { instruction }
                    if has_return_projection && normal_reachable[index] =>
                {
                    let domains = event_domains(event);
                    match instruction {
                        PhpFlowInstruction::Assign { .. }
                        | PhpFlowInstruction::BindReference { .. }
                        | PhpFlowInstruction::ArraySet { .. }
                        | PhpFlowInstruction::ArrayGet { .. }
                        | PhpFlowInstruction::ArrayBindReference { .. }
                        | PhpFlowInstruction::Unset { .. } => {
                            required.extend(
                                domains
                                    .into_iter()
                                    .map(|dependency| (Domain::Return, dependency)),
                            );
                        }
                    }
                }
                Event::Invoke { .. } => {
                    for projection in [
                        Domain::PhpThrow,
                        Domain::NativeUnwind,
                        Domain::Fatal,
                        Domain::Suspension,
                    ] {
                        required.push((projection, Domain::Invocation));
                    }
                }
                _ => {}
            }
        }
    }

    // A projection that consumes an invocation result depends on the event
    // contract that produced it. Merely having an unrelated invocation in the
    // graph does not make a value-only projection depend on callback effects.
    for (index, block) in graph.blocks.iter().enumerate() {
        if reachable[index] != 2 || !normal_reachable[index] {
            continue;
        }
        let output_operand = match &block.terminator {
            Terminator::Return(Some(operand)) => Some((Domain::Return, operand)),
            Terminator::Throw(operand) => Some((Domain::PhpThrow, operand)),
            Terminator::Fatal(operand) => Some((Domain::Fatal, operand)),
            Terminator::Suspend(operand) => Some((Domain::Suspension, operand)),
            _ => None,
        };
        if let Some((projection, Operand::Result(result_id))) = output_operand
            && graph.blocks.iter().enumerate().any(|(index, producer)| {
                reachable[index] == 2
                    && producer.events.iter().any(
                        |event| matches!(event, Event::Invoke { result, .. } if result == result_id),
                    )
            })
        {
            required.push((projection, Domain::Invocation));
        }
        if let Terminator::Branch {
            predicate:
                Predicate::IsLiteral {
                    value: Operand::Result(result_id),
                    ..
                },
            ..
        } = &block.terminator
            && projections.contains(&Domain::Return)
            && graph.blocks.iter().enumerate().any(|(producer_index, producer)| {
                reachable[producer_index] == 2
                    && producer.events.iter().any(
                        |event| matches!(event, Event::Invoke { result, .. } if result == result_id),
                    )
            })
        {
            required.push((Domain::Return, Domain::Invocation));
        }
    }

    required.sort_by_key(|(domain, dependency)| {
        (
            Domain::ALL.iter().position(|candidate| candidate == domain),
            Domain::ALL
                .iter()
                .position(|candidate| candidate == dependency),
        )
    });
    required.dedup();
    for (domain, dependency) in required {
        if coverage.iter().any(|item| {
            item.domain == domain
                && matches!(item.coverage, Coverage::Closed)
                && !item.dependencies.contains(&dependency)
        }) {
            return Err(GraphError::MissingRequiredDomainDependency { domain, dependency });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    Yes,
    No,
    Maybe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exit<V, S> {
    Normal { value: Option<V>, state: S },
    PhpThrow { value: V, state: S },
    NativeUnwind { value: V, state: S },
    Fatal { value: V, state: S },
    Suspended { value: V, state: S },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingOutcome<V> {
    PhpThrow(V),
    NativeUnwind(V),
    Fatal(V),
    Suspended(V),
}

/// One abstract state entering a graph block, or a transfer result offered to
/// a successor during evaluator discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertifiedFrame<V, S> {
    pub block: u32,
    pub state: S,
    pub results: Vec<(u32, V)>,
    pub pending: Option<PendingOutcome<V>>,
}

/// Replayable inputs, the final per-block invariant, and the observable exits
/// used by an independent one-step post-fixpoint validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssumptionCertificateContext {
    pub entry_key: EntryKey,
    pub target: TargetScenario,
    pub loaded_program: Option<Digest>,
    pub policy_key: Digest,
    pub assumptions: Vec<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostFixpointCertificate<V, S> {
    pub graph_digest: Digest,
    pub initial: CertifiedFrame<V, S>,
    pub arguments: Vec<V>,
    pub receiver: V,
    pub solution: Vec<CertifiedFrame<V, S>>,
    pub candidates: Vec<CertifiedFrame<V, S>>,
    pub exits: Vec<Exit<V, S>>,
    pub coverage: Vec<DomainCoverage>,
    pub assumption_context: Option<AssumptionCertificateContext>,
}

impl<V: Clone + PartialEq, S: Clone + PartialEq> PostFixpointCertificate<V, S> {
    pub fn validate<H>(&self, graph: &BehaviorGraph, host: &H) -> Result<(), GraphError>
    where
        H: SemanticHost<Value = V, State = S>,
    {
        self.validate_inner(graph, host, &[])
    }

    pub fn validate_with_assumption_context<H>(
        &self,
        graph: &BehaviorGraph,
        host: &H,
        catalog: &Catalog,
        entry_key: &EntryKey,
        target: &TargetScenario,
        loaded_program: Option<Digest>,
        policy: &ValidatorTrustPolicy,
        executors: &[&dyn AssumptionValidatorExecutor],
    ) -> Result<(), AssumptionCertificateError>
    where
        H: SemanticHost<Value = V, State = S>,
    {
        let Some(context) = &self.assumption_context else {
            return Err(AssumptionCertificateError::MissingContext);
        };
        let policy_key = policy
            .cache_key()
            .map_err(|_| AssumptionCertificateError::InvalidTrustPolicy)?;
        if context.entry_key != *entry_key
            || context.target != *target
            || context.loaded_program != loaded_program
            || context.policy_key != policy_key
        {
            return Err(AssumptionCertificateError::ContextChanged);
        }
        let validated = catalog
            .validate_graph_assumptions(graph, entry_key, target, loaded_program, policy, executors)
            .map_err(AssumptionCertificateError::Validation)?;
        if validated != context.assumptions {
            return Err(AssumptionCertificateError::ContextChanged);
        }
        self.validate_inner(graph, host, &validated)
            .map_err(AssumptionCertificateError::Graph)
    }

    fn validate_inner<H>(
        &self,
        graph: &BehaviorGraph,
        host: &H,
        established_assumptions: &[Digest],
    ) -> Result<(), GraphError>
    where
        H: SemanticHost<Value = V, State = S>,
    {
        graph.validate_with_assumptions(established_assumptions)?;
        let graph_digest = graph
            .canonical_digest()
            .map_err(|_| GraphError::CanonicalEncodingFailed)?;
        if graph_digest != self.graph_digest {
            return Err(GraphError::CertificateGraphMismatch);
        }
        for frame in std::iter::once(&self.initial)
            .chain(self.solution.iter())
            .chain(&self.candidates)
        {
            if frame.block as usize >= graph.blocks.len() {
                return Err(GraphError::InvalidCertificateBlock(frame.block));
            }
        }
        for candidate in &self.candidates {
            let covered = self.solution.iter().any(|record| {
                record.block == candidate.block
                    && pending_kind(&record.pending) == pending_kind(&candidate.pending)
                    && certified_frame_subsumes(host, record, candidate)
            });
            if !covered {
                return Err(GraphError::NonPostFixpoint(candidate.block));
            }
        }
        if !self.solution.iter().any(|record| {
            record.block == self.initial.block
                && pending_kind(&record.pending) == pending_kind(&self.initial.pending)
                && certified_frame_subsumes(host, record, &self.initial)
        }) {
            return Err(GraphError::NonPostFixpoint(self.initial.block));
        }
        let mut replayed_coverage = graph.coverage.clone();
        for frame in &self.solution {
            let (successors, exits) = replay_certificate_frame(
                graph,
                host,
                &self.arguments,
                &self.receiver,
                frame,
                &mut replayed_coverage,
            )?;
            for successor in successors {
                let covered = self.solution.iter().any(|record| {
                    record.block == successor.block
                        && pending_kind(&record.pending) == pending_kind(&successor.pending)
                        && certified_frame_subsumes(host, record, &successor)
                });
                if !covered {
                    return Err(GraphError::NonPostFixpoint(successor.block));
                }
            }
            for exit in exits {
                if !self
                    .exits
                    .iter()
                    .any(|record| exit_subsumes(host, record, &exit))
                {
                    return Err(GraphError::NonPostFixpoint(frame.block));
                }
            }
        }
        if self.coverage.len() != Domain::ALL.len()
            || self.coverage.iter().enumerate().any(|(index, item)| {
                self.coverage[..index]
                    .iter()
                    .any(|previous| previous.domain == item.domain)
            })
        {
            return Err(GraphError::InvalidComposedCoverage);
        }
        if self.coverage.iter().any(|item| {
            item.evidence.is_empty()
                || matches!(item.coverage, Coverage::Open { ref reasons } if reasons.is_empty())
        }) {
            return Err(GraphError::InvalidComposedCoverage);
        }
        validate_domain_dependencies_with_assumptions(
            &self.coverage,
            Some(established_assumptions),
        )?;
        for replayed in &replayed_coverage {
            let Some(certified) = self
                .coverage
                .iter()
                .find(|item| item.domain == replayed.domain)
            else {
                return Err(GraphError::InvalidComposedCoverage);
            };
            if !coverage_subsumes(certified, replayed) {
                return Err(GraphError::InvalidComposedCoverage);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation<V, S> {
    pub exits: Vec<Exit<V, S>>,
    /// Graph-level derivation coverage is returned with execution results so
    /// consumers cannot mistake an empty projection for a closed negative.
    pub coverage: Vec<DomainCoverage>,
    pub post_fixpoint: PostFixpointCertificate<V, S>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventExit<V, S> {
    Normal {
        result: Option<(u32, V)>,
        state: S,
        coverage: Vec<DomainCoverage>,
    },
    PhpThrow {
        value: V,
        state: S,
        coverage: Vec<DomainCoverage>,
    },
    NativeUnwind {
        value: V,
        state: S,
        coverage: Vec<DomainCoverage>,
    },
    Fatal {
        value: V,
        state: S,
        coverage: Vec<DomainCoverage>,
    },
    Suspended {
        value: V,
        state: S,
        coverage: Vec<DomainCoverage>,
    },
}

impl<V, S> EventExit<V, S> {
    fn coverage(&self) -> &[DomainCoverage] {
        match self {
            Self::Normal { coverage, .. }
            | Self::PhpThrow { coverage, .. }
            | Self::NativeUnwind { coverage, .. }
            | Self::Fatal { coverage, .. }
            | Self::Suspended { coverage, .. } => coverage,
        }
    }
}

pub trait SemanticHost {
    type Value: Clone + PartialEq;
    type State: Clone + PartialEq;

    fn resolve(
        &self,
        operand: &Operand,
        arguments: &[Self::Value],
        receiver: &Self::Value,
        results: &[(u32, Self::Value)],
    ) -> Option<Self::Value>;
    /// Decide a graph predicate from the current arguments, results, and
    /// state. Return `Maybe` whenever the host cannot prove either answer.
    fn test(
        &self,
        predicate: &Predicate,
        arguments: &[Self::Value],
        receiver: &Self::Value,
        results: &[(u32, Self::Value)],
        state: &Self::State,
    ) -> Truth;
    fn event(
        &self,
        event: &Event,
        arguments: &[Self::Value],
        receiver: &Self::Value,
        results: &[(u32, Self::Value)],
        state: Self::State,
    ) -> Vec<EventExit<Self::Value, Self::State>>;
    fn unknown_value(&self) -> Self::Value;
    fn join_value(&self, left: Self::Value, right: Self::Value) -> Self::Value;
    fn widen_value(&self, old: Self::Value, next: Self::Value) -> Self::Value;
    fn value_subsumes(&self, broad: &Self::Value, narrow: &Self::Value) -> bool;
    fn join_state(&self, left: Self::State, right: Self::State) -> Self::State;
    fn widen_state(&self, old: Self::State, next: Self::State) -> Self::State;
    fn state_subsumes(&self, broad: &Self::State, narrow: &Self::State) -> bool;
}

fn precision_rank(precision: Precision) -> u8 {
    match precision {
        Precision::Exact => 0,
        Precision::Joined => 1,
        Precision::Widened => 2,
    }
}

fn propagate_open_dependencies(coverage: &mut [DomainCoverage]) {
    loop {
        let mut changed = false;
        for index in 0..coverage.len() {
            if !matches!(coverage[index].coverage, Coverage::Closed) {
                continue;
            }
            let domain = coverage[index].domain;
            let dependencies = transitive_domain_dependencies(domain, coverage);
            let open_dependencies: Vec<_> = dependencies
                .iter()
                .filter_map(|dependency| {
                    coverage
                        .iter()
                        .find(|item| item.domain == *dependency)
                        .filter(|item| matches!(item.coverage, Coverage::Open { .. }))
                        .cloned()
                })
                .collect();
            if open_dependencies.is_empty() {
                continue;
            }
            let mut reasons = Vec::new();
            for dependency in &open_dependencies {
                if let Coverage::Open { reasons: open } = &dependency.coverage {
                    reasons.extend(open.iter().cloned());
                }
                coverage[index]
                    .evidence
                    .extend(dependency.evidence.iter().copied());
                coverage[index]
                    .assumptions
                    .extend(dependency.assumptions.iter().copied());
                if precision_rank(dependency.precision) > precision_rank(coverage[index].precision)
                {
                    coverage[index].precision = dependency.precision;
                }
            }
            reasons.sort();
            reasons.dedup();
            coverage[index].evidence.sort();
            coverage[index].evidence.dedup();
            coverage[index].assumptions.sort();
            coverage[index].assumptions.dedup();
            coverage[index].coverage = Coverage::Open { reasons };
            changed = true;
        }
        if !changed {
            break;
        }
    }
}

fn merge_coverage(
    current: &mut Vec<DomainCoverage>,
    additions: &[DomainCoverage],
) -> Result<(), GraphError> {
    if additions.is_empty() {
        return Ok(());
    }
    let mut seen = Vec::new();
    for item in additions {
        if seen.contains(&item.domain) {
            return Err(GraphError::InvalidComposedCoverage);
        }
        seen.push(item.domain);
        if matches!(item.coverage, Coverage::Open { ref reasons } if reasons.is_empty())
            || item.evidence.is_empty()
        {
            return Err(GraphError::InvalidComposedCoverage);
        }
        let mut dependencies = Vec::new();
        for dependency in &item.dependencies {
            if dependencies.contains(dependency) {
                return Err(GraphError::InvalidComposedCoverage);
            }
            dependencies.push(*dependency);
        }
    }
    if Domain::ALL.iter().any(|domain| !seen.contains(domain)) {
        return Err(GraphError::InvalidComposedCoverage);
    }

    let mut merged = Vec::with_capacity(Domain::ALL.len());
    for domain in Domain::ALL {
        let Some(mut item) = current.iter().find(|item| item.domain == domain).cloned() else {
            return Err(GraphError::InvalidComposedCoverage);
        };
        let Some(addition) = additions.iter().find(|item| item.domain == domain) else {
            merged.push(item);
            continue;
        };
        item.coverage = match (item.coverage, addition.coverage.clone()) {
            (Coverage::Open { reasons: mut left }, Coverage::Open { reasons: right }) => {
                left.extend(right);
                left.sort();
                left.dedup();
                Coverage::Open { reasons: left }
            }
            (Coverage::Open { reasons }, _) | (_, Coverage::Open { reasons }) => {
                Coverage::Open { reasons }
            }
            (Coverage::Closed, _) | (_, Coverage::Closed) => Coverage::Closed,
            (Coverage::NotApplicable, Coverage::NotApplicable) => Coverage::NotApplicable,
        };
        if precision_rank(addition.precision) > precision_rank(item.precision) {
            item.precision = addition.precision;
        }
        item.evidence.extend(addition.evidence.iter().copied());
        item.evidence.sort();
        item.evidence.dedup();
        item.dependencies
            .extend(addition.dependencies.iter().copied());
        item.dependencies
            .sort_by_key(|dependency| Domain::ALL.iter().position(|d| d == dependency));
        item.dependencies.dedup();
        item.assumptions
            .extend(addition.assumptions.iter().copied());
        item.assumptions.sort();
        item.assumptions.dedup();
        merged.push(item);
    }
    propagate_open_dependencies(&mut merged);
    validate_domain_dependencies(&merged)?;
    *current = merged;
    Ok(())
}

fn merge_graph_coverage(
    graph: &BehaviorGraph,
    current: &mut Vec<DomainCoverage>,
    additions: &[DomainCoverage],
) -> Result<(), GraphError> {
    merge_coverage(current, additions)?;
    let mut composed_graph = graph.clone();
    composed_graph.coverage.clone_from(current);
    let mut reachable = vec![false; graph.blocks.len()];
    let mut pending = vec![graph.entry];
    while let Some(block_id) = pending.pop() {
        let index = block_id as usize;
        if reachable[index] {
            continue;
        }
        reachable[index] = true;
        let block = &graph.blocks[index];
        match block.terminator {
            Terminator::Goto(to) => pending.push(to),
            Terminator::Branch { yes, no, .. } => {
                pending.push(yes);
                pending.push(no);
            }
            Terminator::Return(_)
            | Terminator::Throw(_)
            | Terminator::Fatal(_)
            | Terminator::Suspend(_)
            | Terminator::Resume => {}
        }
        pending.extend(
            [
                block.exceptional.php_throw,
                block.exceptional.native_unwind,
                block.exceptional.fatal,
                block.exceptional.suspended,
            ]
            .into_iter()
            .flatten(),
        );
    }
    let reachable = reachable
        .into_iter()
        .map(|is_reachable| u8::from(is_reachable) * 2)
        .collect::<Vec<_>>();
    validate_required_domain_dependencies(&composed_graph, &reachable, current)
}

/// Execute a finite graph with a finite host-provided state join. Cycles are
/// evaluated with host joins and widening after three growing updates. The
/// host's transfer, join, widening, and subsumption methods must be monotone.
#[derive(Debug, Clone)]
struct Frame<V, S> {
    block: u32,
    state: S,
    results: Vec<(u32, V)>,
    pending: Option<PendingOutcome<V>>,
    growing_updates: u32,
}

fn pending_kind<V>(pending: &Option<PendingOutcome<V>>) -> u8 {
    match pending {
        None => 0,
        Some(PendingOutcome::PhpThrow(_)) => 1,
        Some(PendingOutcome::NativeUnwind(_)) => 2,
        Some(PendingOutcome::Fatal(_)) => 3,
        Some(PendingOutcome::Suspended(_)) => 4,
    }
}

fn merge_pending<H: SemanticHost>(
    host: &H,
    left: Option<PendingOutcome<H::Value>>,
    right: Option<PendingOutcome<H::Value>>,
    widen: bool,
) -> Option<PendingOutcome<H::Value>> {
    let merge = |a, b| {
        if widen {
            host.widen_value(a, b)
        } else {
            host.join_value(a, b)
        }
    };
    match (left, right) {
        (None, None) => None,
        (Some(PendingOutcome::PhpThrow(a)), Some(PendingOutcome::PhpThrow(b))) => {
            Some(PendingOutcome::PhpThrow(merge(a, b)))
        }
        (Some(PendingOutcome::NativeUnwind(a)), Some(PendingOutcome::NativeUnwind(b))) => {
            Some(PendingOutcome::NativeUnwind(merge(a, b)))
        }
        (Some(PendingOutcome::Fatal(a)), Some(PendingOutcome::Fatal(b))) => {
            Some(PendingOutcome::Fatal(merge(a, b)))
        }
        (Some(PendingOutcome::Suspended(a)), Some(PendingOutcome::Suspended(b))) => {
            Some(PendingOutcome::Suspended(merge(a, b)))
        }
        _ => unreachable!("frames of distinct pending-outcome kind are not joined"),
    }
}

fn merge_results<H: SemanticHost>(
    host: &H,
    left: &[(u32, H::Value)],
    right: &[(u32, H::Value)],
    widen: bool,
) -> Result<Vec<(u32, H::Value)>, GraphError> {
    let mut ids: Vec<u32> = left.iter().chain(right).map(|(id, _)| *id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let a = left
                .iter()
                .find(|(key, _)| *key == id)
                .map(|(_, value)| value.clone());
            let b = right
                .iter()
                .find(|(key, _)| *key == id)
                .map(|(_, value)| value.clone());
            let value = match (a, b) {
                (Some(a), Some(b)) if widen => host.widen_value(a, b),
                (Some(a), Some(b)) => host.join_value(a, b),
                (Some(_), None) | (None, Some(_)) => {
                    return Err(GraphError::ResultDefinedOnOnlySomePaths(id));
                }
                (None, None) => unreachable!(),
            };
            Ok((id, value))
        })
        .collect()
}

fn frame_subsumes<H: SemanticHost>(
    host: &H,
    broad: &Frame<H::Value, H::State>,
    narrow: &Frame<H::Value, H::State>,
) -> bool {
    if !host.state_subsumes(&broad.state, &narrow.state)
        || pending_kind(&broad.pending) != pending_kind(&narrow.pending)
    {
        return false;
    }
    let pending_values = match (&broad.pending, &narrow.pending) {
        (None, None) => true,
        (Some(PendingOutcome::PhpThrow(a)), Some(PendingOutcome::PhpThrow(b)))
        | (Some(PendingOutcome::NativeUnwind(a)), Some(PendingOutcome::NativeUnwind(b)))
        | (Some(PendingOutcome::Fatal(a)), Some(PendingOutcome::Fatal(b)))
        | (Some(PendingOutcome::Suspended(a)), Some(PendingOutcome::Suspended(b))) => {
            host.value_subsumes(a, b)
        }
        _ => false,
    };
    pending_values
        && broad.results.len() == narrow.results.len()
        && narrow.results.iter().all(|(id, value)| {
            broad
                .results
                .iter()
                .find(|(other, _)| other == id)
                .is_some_and(|(_, broad_value)| host.value_subsumes(broad_value, value))
        })
}

fn certified_frame_subsumes<H: SemanticHost>(
    host: &H,
    broad: &CertifiedFrame<H::Value, H::State>,
    narrow: &CertifiedFrame<H::Value, H::State>,
) -> bool {
    frame_subsumes(
        host,
        &Frame {
            block: broad.block,
            state: broad.state.clone(),
            results: broad.results.clone(),
            pending: broad.pending.clone(),
            growing_updates: 0,
        },
        &Frame {
            block: narrow.block,
            state: narrow.state.clone(),
            results: narrow.results.clone(),
            pending: narrow.pending.clone(),
            growing_updates: 0,
        },
    )
}

fn certified_frame<V: Clone, S: Clone>(frame: &Frame<V, S>) -> CertifiedFrame<V, S> {
    CertifiedFrame {
        block: frame.block,
        state: frame.state.clone(),
        results: frame.results.clone(),
        pending: frame.pending.clone(),
    }
}

fn exit_subsumes<H: SemanticHost>(
    host: &H,
    broad: &Exit<H::Value, H::State>,
    narrow: &Exit<H::Value, H::State>,
) -> bool {
    match (broad, narrow) {
        (
            Exit::Normal {
                value: a,
                state: sa,
            },
            Exit::Normal {
                value: b,
                state: sb,
            },
        ) => {
            let value_covered = match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => host.value_subsumes(a, b),
                _ => false,
            };
            value_covered && host.state_subsumes(sa, sb)
        }
        (
            Exit::PhpThrow {
                value: a,
                state: sa,
            },
            Exit::PhpThrow {
                value: b,
                state: sb,
            },
        )
        | (
            Exit::NativeUnwind {
                value: a,
                state: sa,
            },
            Exit::NativeUnwind {
                value: b,
                state: sb,
            },
        )
        | (
            Exit::Fatal {
                value: a,
                state: sa,
            },
            Exit::Fatal {
                value: b,
                state: sb,
            },
        )
        | (
            Exit::Suspended {
                value: a,
                state: sa,
            },
            Exit::Suspended {
                value: b,
                state: sb,
            },
        ) => host.value_subsumes(a, b) && host.state_subsumes(sa, sb),
        _ => false,
    }
}

fn coverage_subsumes(broad: &DomainCoverage, narrow: &DomainCoverage) -> bool {
    if broad.domain != narrow.domain
        || precision_rank(broad.precision) < precision_rank(narrow.precision)
    {
        return false;
    }
    let status_covers = match (&broad.coverage, &narrow.coverage) {
        (Coverage::Open { reasons: broad }, Coverage::Open { reasons: narrow }) => {
            narrow.iter().all(|reason| broad.contains(reason))
        }
        (Coverage::Open { reasons }, _) => !reasons.is_empty(),
        (Coverage::Closed, Coverage::Closed)
        | (Coverage::NotApplicable, Coverage::NotApplicable) => true,
        _ => false,
    };
    status_covers
        && narrow
            .evidence
            .iter()
            .all(|item| broad.evidence.contains(item))
        && narrow
            .assumptions
            .iter()
            .all(|item| broad.assumptions.contains(item))
        && narrow
            .dependencies
            .iter()
            .all(|item| broad.dependencies.contains(item))
}

/// Independently replays one invariant frame through the graph's events and
/// terminator. Every produced successor and observable exit must be covered
/// by the persisted certificate.
fn replay_certificate_frame<H: SemanticHost>(
    graph: &BehaviorGraph,
    host: &H,
    arguments: &[H::Value],
    receiver: &H::Value,
    frame: &CertifiedFrame<H::Value, H::State>,
    replayed_coverage: &mut Vec<DomainCoverage>,
) -> Result<
    (
        Vec<CertifiedFrame<H::Value, H::State>>,
        Vec<Exit<H::Value, H::State>>,
    ),
    GraphError,
> {
    let block = &graph.blocks[frame.block as usize];
    let mut paths = vec![(
        frame.state.clone(),
        frame.results.clone(),
        frame.pending.clone(),
    )];
    let mut successors = Vec::new();
    let mut exits = Vec::new();
    for event in &block.events {
        let mut next = Vec::new();
        for (state, values, pending) in paths {
            let outcomes = host.event(event, arguments, receiver, &values, state);
            if outcomes.is_empty() {
                return Err(GraphError::EventWithoutOutcome);
            }
            for outcome in outcomes {
                if matches!(event, Event::Invoke { .. }) {
                    let coverage = outcome.coverage();
                    if coverage.is_empty()
                        || coverage
                            .iter()
                            .find(|item| item.domain == Domain::Invocation)
                            .is_none_or(|item| matches!(item.coverage, Coverage::NotApplicable))
                    {
                        return Err(GraphError::InvalidComposedCoverage);
                    }
                }
                merge_graph_coverage(graph, replayed_coverage, outcome.coverage())?;
                match outcome {
                    EventExit::Normal { result, state, .. } => {
                        let mut next_values = values.clone();
                        if let Some((id, value)) = result {
                            next_values.retain(|(existing, _)| *existing != id);
                            next_values.push((id, value));
                        }
                        next.push((state, next_values, pending.clone()));
                    }
                    EventExit::PhpThrow { value, state, .. } => {
                        check_cleanup_replacement(&pending, CompletionKind::PhpThrow)?;
                        if let Some(to) = block.exceptional.php_throw {
                            successors.push(CertifiedFrame {
                                block: to,
                                state,
                                results: values.clone(),
                                pending: Some(PendingOutcome::PhpThrow(value)),
                            });
                        } else {
                            exits.push(Exit::PhpThrow { value, state });
                        }
                    }
                    EventExit::NativeUnwind { value, state, .. } => {
                        check_cleanup_replacement(&pending, CompletionKind::NativeUnwind)?;
                        if let Some(to) = block.exceptional.native_unwind {
                            successors.push(CertifiedFrame {
                                block: to,
                                state,
                                results: values.clone(),
                                pending: Some(PendingOutcome::NativeUnwind(value)),
                            });
                        } else {
                            exits.push(Exit::NativeUnwind { value, state });
                        }
                    }
                    EventExit::Fatal { value, state, .. } => {
                        check_cleanup_replacement(&pending, CompletionKind::Fatal)?;
                        if let Some(to) = block.exceptional.fatal {
                            successors.push(CertifiedFrame {
                                block: to,
                                state,
                                results: values.clone(),
                                pending: Some(PendingOutcome::Fatal(value)),
                            });
                        } else {
                            exits.push(Exit::Fatal { value, state });
                        }
                    }
                    EventExit::Suspended { value, state, .. } => {
                        check_cleanup_replacement(&pending, CompletionKind::Suspended)?;
                        if let Some(to) = block.exceptional.suspended {
                            successors.push(CertifiedFrame {
                                block: to,
                                state,
                                results: values.clone(),
                                pending: Some(PendingOutcome::Suspended(value)),
                            });
                        } else {
                            exits.push(Exit::Suspended { value, state });
                        }
                    }
                }
            }
        }
        paths = next;
    }
    for (state, results, pending) in paths {
        match &block.terminator {
            Terminator::Goto(to) => successors.push(CertifiedFrame {
                block: *to,
                state,
                results,
                pending,
            }),
            Terminator::Branch { predicate, yes, no } => {
                let target = |truth| match truth {
                    Truth::Yes => vec![*yes],
                    Truth::No => vec![*no],
                    Truth::Maybe => vec![*yes, *no],
                };
                for to in target(host.test(predicate, arguments, receiver, &results, &state)) {
                    successors.push(CertifiedFrame {
                        block: to,
                        state: state.clone(),
                        results: results.clone(),
                        pending: pending.clone(),
                    });
                }
            }
            Terminator::Return(value) => {
                check_cleanup_replacement(&pending, CompletionKind::Normal)?;
                let value = value
                    .as_ref()
                    .map(|operand| {
                        host.resolve(operand, arguments, receiver, &results)
                            .ok_or(GraphError::UnresolvedValue)
                    })
                    .transpose()?;
                exits.push(Exit::Normal { value, state });
            }
            Terminator::Throw(operand) => {
                check_cleanup_replacement(&pending, CompletionKind::PhpThrow)?;
                let value = host
                    .resolve(operand, arguments, receiver, &results)
                    .ok_or(GraphError::UnresolvedValue)?;
                if let Some(to) = block.exceptional.php_throw {
                    successors.push(CertifiedFrame {
                        block: to,
                        state,
                        results,
                        pending: Some(PendingOutcome::PhpThrow(value)),
                    });
                } else {
                    exits.push(Exit::PhpThrow { value, state });
                }
            }
            Terminator::Fatal(operand) => {
                check_cleanup_replacement(&pending, CompletionKind::Fatal)?;
                let value = host
                    .resolve(operand, arguments, receiver, &results)
                    .ok_or(GraphError::UnresolvedValue)?;
                if let Some(to) = block.exceptional.fatal {
                    successors.push(CertifiedFrame {
                        block: to,
                        state,
                        results,
                        pending: Some(PendingOutcome::Fatal(value)),
                    });
                } else {
                    exits.push(Exit::Fatal { value, state });
                }
            }
            Terminator::Suspend(operand) => {
                check_cleanup_replacement(&pending, CompletionKind::Suspended)?;
                let value = host
                    .resolve(operand, arguments, receiver, &results)
                    .ok_or(GraphError::UnresolvedValue)?;
                if let Some(to) = block.exceptional.suspended {
                    successors.push(CertifiedFrame {
                        block: to,
                        state,
                        results,
                        pending: Some(PendingOutcome::Suspended(value)),
                    });
                } else {
                    exits.push(Exit::Suspended { value, state });
                }
            }
            Terminator::Resume => match pending {
                Some(PendingOutcome::PhpThrow(value)) => {
                    exits.push(Exit::PhpThrow { value, state })
                }
                Some(PendingOutcome::NativeUnwind(value)) => {
                    exits.push(Exit::NativeUnwind { value, state })
                }
                Some(PendingOutcome::Fatal(value)) => exits.push(Exit::Fatal { value, state }),
                Some(PendingOutcome::Suspended(value)) => {
                    exits.push(Exit::Suspended { value, state })
                }
                None => return Err(GraphError::ResumeWithoutPendingOutcome),
            },
        }
    }
    Ok((successors, exits))
}

fn enqueue<H: SemanticHost>(
    host: &H,
    frame: Frame<H::Value, H::State>,
    records: &mut Vec<Frame<H::Value, H::State>>,
    by_block: &mut [Vec<usize>],
    queue: &mut Vec<usize>,
    precision: &mut Precision,
    candidates: &mut Vec<Frame<H::Value, H::State>>,
) -> Result<(), GraphError> {
    candidates.push(frame.clone());
    let index = by_block[frame.block as usize]
        .iter()
        .copied()
        .find(|&i| pending_kind(&records[i].pending) == pending_kind(&frame.pending));
    let Some(index) = index else {
        let index = records.len();
        by_block[frame.block as usize].push(index);
        records.push(frame);
        queue.push(index);
        return Ok(());
    };
    if frame_subsumes(host, &records[index], &frame) {
        return Ok(());
    }
    let old = &records[index];
    let widen = old.growing_updates >= 3;
    let mut merged = Frame {
        block: old.block,
        state: if widen {
            host.widen_state(old.state.clone(), frame.state.clone())
        } else {
            host.join_state(old.state.clone(), frame.state.clone())
        },
        results: merge_results(host, &old.results, &frame.results, widen)?,
        pending: merge_pending(host, old.pending.clone(), frame.pending.clone(), widen),
        growing_updates: old.growing_updates + 1,
    };
    if !frame_subsumes(host, &merged, old) || !frame_subsumes(host, &merged, &frame) {
        return Err(GraphError::NonMonotoneJoin(frame.block));
    }
    if merged.state == old.state && merged.results == old.results && merged.pending == old.pending {
        return Ok(());
    }
    let next_precision = if widen {
        Precision::Widened
    } else {
        Precision::Joined
    };
    if precision_rank(next_precision) > precision_rank(*precision) {
        *precision = next_precision;
    }
    if widen {
        merged.growing_updates = old.growing_updates + 1;
    }
    records[index] = merged;
    if !queue.contains(&index) {
        queue.push(index);
    }
    Ok(())
}

fn push_exit<V: PartialEq, S: PartialEq>(exits: &mut Vec<Exit<V, S>>, exit: Exit<V, S>) {
    if !exits.contains(&exit) {
        exits.push(exit);
    }
}

pub fn evaluate<H: SemanticHost>(
    graph: &BehaviorGraph,
    arguments: &[H::Value],
    receiver: &H::Value,
    initial: H::State,
    host: &mut H,
) -> Result<Evaluation<H::Value, H::State>, GraphError> {
    evaluate_inner(graph, arguments, receiver, initial, &[], None, host)
}

pub fn evaluate_with_assumption_context<H: SemanticHost>(
    graph: &BehaviorGraph,
    catalog: &Catalog,
    entry_key: &EntryKey,
    target: &TargetScenario,
    loaded_program: Option<Digest>,
    policy: &ValidatorTrustPolicy,
    executors: &[&dyn AssumptionValidatorExecutor],
    arguments: &[H::Value],
    receiver: &H::Value,
    initial: H::State,
    host: &mut H,
) -> Result<Evaluation<H::Value, H::State>, AssumptionEvaluationError> {
    let assumptions = catalog
        .validate_graph_assumptions(graph, entry_key, target, loaded_program, policy, executors)
        .map_err(AssumptionEvaluationError::Validation)?;
    let context = AssumptionCertificateContext {
        entry_key: entry_key.clone(),
        target: target.clone(),
        loaded_program,
        policy_key: policy.cache_key().map_err(|_| {
            AssumptionEvaluationError::Validation(AssumptionGraphValidationError::Trust(
                ValidatorTrustError::PolicyEncoding,
            ))
        })?,
        assumptions: assumptions.clone(),
    };
    evaluate_inner(
        graph,
        arguments,
        receiver,
        initial,
        &assumptions,
        Some(context),
        host,
    )
    .map_err(AssumptionEvaluationError::Graph)
}

fn evaluate_inner<H: SemanticHost>(
    graph: &BehaviorGraph,
    arguments: &[H::Value],
    receiver: &H::Value,
    initial: H::State,
    established_assumptions: &[Digest],
    assumption_context: Option<AssumptionCertificateContext>,
    host: &mut H,
) -> Result<Evaluation<H::Value, H::State>, GraphError> {
    graph.validate_with_assumptions(established_assumptions)?;
    let initial_frame = Frame {
        block: graph.entry,
        state: initial,
        results: Vec::<(u32, H::Value)>::new(),
        pending: None,
        growing_updates: 0,
    };
    let initial_certificate_frame = certified_frame(&initial_frame);
    let mut records = vec![initial_frame];
    let mut by_block = vec![Vec::new(); graph.blocks.len()];
    by_block[graph.entry as usize].push(0);
    let mut queue = vec![0usize];
    let mut evaluation_precision = Precision::Exact;
    let mut exits = Vec::new();
    let mut transfer_candidates = vec![records[0].clone()];
    let mut evaluation_coverage = graph.coverage.clone();
    while let Some(frame_id) = queue.pop() {
        let frame = records[frame_id].clone();
        let block_id = frame.block;
        let block = &graph.blocks[block_id as usize];
        let mut paths = vec![(frame.state, frame.results, frame.pending)];
        for event in &block.events {
            let mut next = Vec::new();
            for (state, values, pending) in paths {
                let outcomes = host.event(event, arguments, receiver, &values, state);
                if outcomes.is_empty() {
                    return Err(GraphError::EventWithoutOutcome);
                }
                for outcome in outcomes {
                    if matches!(event, Event::Invoke { .. }) {
                        let coverage = outcome.coverage();
                        if coverage.is_empty()
                            || coverage
                                .iter()
                                .find(|item| item.domain == Domain::Invocation)
                                .is_none_or(|item| matches!(item.coverage, Coverage::NotApplicable))
                        {
                            return Err(GraphError::InvalidComposedCoverage);
                        }
                    }
                    match outcome {
                        EventExit::Normal {
                            result,
                            state,
                            coverage,
                        } => {
                            merge_graph_coverage(graph, &mut evaluation_coverage, &coverage)?;
                            let mut next_values = values.clone();
                            if let Some((id, value)) = result {
                                next_values.retain(|(existing, _)| *existing != id);
                                next_values.push((id, value));
                            }
                            next.push((state, next_values, pending.clone()));
                        }
                        EventExit::PhpThrow {
                            value,
                            state,
                            coverage,
                        } => {
                            check_cleanup_replacement(&pending, CompletionKind::PhpThrow)?;
                            merge_graph_coverage(graph, &mut evaluation_coverage, &coverage)?;
                            if let Some(to) = block.exceptional.php_throw {
                                enqueue(
                                    host,
                                    Frame {
                                        block: to,
                                        state,
                                        results: values.clone(),
                                        pending: Some(PendingOutcome::PhpThrow(value)),
                                        growing_updates: 0,
                                    },
                                    &mut records,
                                    &mut by_block,
                                    &mut queue,
                                    &mut evaluation_precision,
                                    &mut transfer_candidates,
                                )?;
                            } else {
                                push_exit(&mut exits, Exit::PhpThrow { value, state });
                            }
                        }
                        EventExit::NativeUnwind {
                            value,
                            state,
                            coverage,
                        } => {
                            check_cleanup_replacement(&pending, CompletionKind::NativeUnwind)?;
                            merge_graph_coverage(graph, &mut evaluation_coverage, &coverage)?;
                            if let Some(to) = block.exceptional.native_unwind {
                                enqueue(
                                    host,
                                    Frame {
                                        block: to,
                                        state,
                                        results: values.clone(),
                                        pending: Some(PendingOutcome::NativeUnwind(value)),
                                        growing_updates: 0,
                                    },
                                    &mut records,
                                    &mut by_block,
                                    &mut queue,
                                    &mut evaluation_precision,
                                    &mut transfer_candidates,
                                )?;
                            } else {
                                push_exit(&mut exits, Exit::NativeUnwind { value, state });
                            }
                        }
                        EventExit::Fatal {
                            value,
                            state,
                            coverage,
                        } => {
                            check_cleanup_replacement(&pending, CompletionKind::Fatal)?;
                            merge_graph_coverage(graph, &mut evaluation_coverage, &coverage)?;
                            if let Some(to) = block.exceptional.fatal {
                                enqueue(
                                    host,
                                    Frame {
                                        block: to,
                                        state,
                                        results: values.clone(),
                                        pending: Some(PendingOutcome::Fatal(value)),
                                        growing_updates: 0,
                                    },
                                    &mut records,
                                    &mut by_block,
                                    &mut queue,
                                    &mut evaluation_precision,
                                    &mut transfer_candidates,
                                )?;
                            } else {
                                push_exit(&mut exits, Exit::Fatal { value, state });
                            }
                        }
                        EventExit::Suspended {
                            value,
                            state,
                            coverage,
                        } => {
                            check_cleanup_replacement(&pending, CompletionKind::Suspended)?;
                            merge_graph_coverage(graph, &mut evaluation_coverage, &coverage)?;
                            if let Some(to) = block.exceptional.suspended {
                                enqueue(
                                    host,
                                    Frame {
                                        block: to,
                                        state,
                                        results: values.clone(),
                                        pending: Some(PendingOutcome::Suspended(value)),
                                        growing_updates: 0,
                                    },
                                    &mut records,
                                    &mut by_block,
                                    &mut queue,
                                    &mut evaluation_precision,
                                    &mut transfer_candidates,
                                )?;
                            } else {
                                push_exit(&mut exits, Exit::Suspended { value, state });
                            }
                        }
                    }
                }
            }
            paths = next;
        }
        for (state, results, pending) in paths {
            match &block.terminator {
                Terminator::Goto(to) => enqueue(
                    host,
                    Frame {
                        block: *to,
                        state,
                        results,
                        pending,
                        growing_updates: 0,
                    },
                    &mut records,
                    &mut by_block,
                    &mut queue,
                    &mut evaluation_precision,
                    &mut transfer_candidates,
                )?,
                Terminator::Branch { predicate, yes, no } => {
                    match host.test(predicate, arguments, receiver, &results, &state) {
                        Truth::Yes => enqueue(
                            host,
                            Frame {
                                block: *yes,
                                state,
                                results,
                                pending,
                                growing_updates: 0,
                            },
                            &mut records,
                            &mut by_block,
                            &mut queue,
                            &mut evaluation_precision,
                            &mut transfer_candidates,
                        )?,
                        Truth::No => enqueue(
                            host,
                            Frame {
                                block: *no,
                                state,
                                results,
                                pending,
                                growing_updates: 0,
                            },
                            &mut records,
                            &mut by_block,
                            &mut queue,
                            &mut evaluation_precision,
                            &mut transfer_candidates,
                        )?,
                        Truth::Maybe => {
                            enqueue(
                                host,
                                Frame {
                                    block: *yes,
                                    state: state.clone(),
                                    results: results.clone(),
                                    pending: pending.clone(),
                                    growing_updates: 0,
                                },
                                &mut records,
                                &mut by_block,
                                &mut queue,
                                &mut evaluation_precision,
                                &mut transfer_candidates,
                            )?;
                            enqueue(
                                host,
                                Frame {
                                    block: *no,
                                    state,
                                    results,
                                    pending,
                                    growing_updates: 0,
                                },
                                &mut records,
                                &mut by_block,
                                &mut queue,
                                &mut evaluation_precision,
                                &mut transfer_candidates,
                            )?;
                        }
                    }
                }
                Terminator::Return(value) => {
                    check_cleanup_replacement(&pending, CompletionKind::Normal)?;
                    let value = value
                        .as_ref()
                        .map(|v| {
                            host.resolve(v, arguments, receiver, &results)
                                .ok_or(GraphError::UnresolvedValue)
                        })
                        .transpose()?;
                    push_exit(&mut exits, Exit::Normal { value, state });
                }
                Terminator::Throw(value) => {
                    check_cleanup_replacement(&pending, CompletionKind::PhpThrow)?;
                    let value = host
                        .resolve(value, arguments, receiver, &results)
                        .ok_or(GraphError::UnresolvedValue)?;
                    if let Some(to) = block.exceptional.php_throw {
                        enqueue(
                            host,
                            Frame {
                                block: to,
                                state,
                                results,
                                pending: Some(PendingOutcome::PhpThrow(value)),
                                growing_updates: 0,
                            },
                            &mut records,
                            &mut by_block,
                            &mut queue,
                            &mut evaluation_precision,
                            &mut transfer_candidates,
                        )?;
                    } else {
                        push_exit(&mut exits, Exit::PhpThrow { value, state });
                    }
                }
                Terminator::Fatal(value) => {
                    check_cleanup_replacement(&pending, CompletionKind::Fatal)?;
                    let value = host
                        .resolve(value, arguments, receiver, &results)
                        .ok_or(GraphError::UnresolvedValue)?;
                    if let Some(to) = block.exceptional.fatal {
                        enqueue(
                            host,
                            Frame {
                                block: to,
                                state,
                                results,
                                pending: Some(PendingOutcome::Fatal(value)),
                                growing_updates: 0,
                            },
                            &mut records,
                            &mut by_block,
                            &mut queue,
                            &mut evaluation_precision,
                            &mut transfer_candidates,
                        )?;
                    } else {
                        push_exit(&mut exits, Exit::Fatal { value, state });
                    }
                }
                Terminator::Suspend(value) => {
                    check_cleanup_replacement(&pending, CompletionKind::Suspended)?;
                    let value = host
                        .resolve(value, arguments, receiver, &results)
                        .ok_or(GraphError::UnresolvedValue)?;
                    if let Some(to) = block.exceptional.suspended {
                        enqueue(
                            host,
                            Frame {
                                block: to,
                                state,
                                results,
                                pending: Some(PendingOutcome::Suspended(value)),
                                growing_updates: 0,
                            },
                            &mut records,
                            &mut by_block,
                            &mut queue,
                            &mut evaluation_precision,
                            &mut transfer_candidates,
                        )?;
                    } else {
                        push_exit(&mut exits, Exit::Suspended { value, state });
                    }
                }
                Terminator::Resume => match pending {
                    Some(PendingOutcome::PhpThrow(value)) => {
                        push_exit(&mut exits, Exit::PhpThrow { value, state })
                    }
                    Some(PendingOutcome::NativeUnwind(value)) => {
                        push_exit(&mut exits, Exit::NativeUnwind { value, state })
                    }
                    Some(PendingOutcome::Fatal(value)) => {
                        push_exit(&mut exits, Exit::Fatal { value, state })
                    }
                    Some(PendingOutcome::Suspended(value)) => {
                        push_exit(&mut exits, Exit::Suspended { value, state })
                    }
                    None => return Err(GraphError::ResumeWithoutPendingOutcome),
                },
            }
        }
    }
    if evaluation_precision != Precision::Exact {
        for item in &mut evaluation_coverage {
            if !matches!(item.coverage, Coverage::NotApplicable) {
                if precision_rank(evaluation_precision) > precision_rank(item.precision) {
                    item.precision = evaluation_precision;
                }
            }
        }
    }
    let post_fixpoint = PostFixpointCertificate {
        graph_digest: graph
            .canonical_digest()
            .map_err(|_| GraphError::CanonicalEncodingFailed)?,
        initial: initial_certificate_frame,
        arguments: arguments.to_vec(),
        receiver: receiver.clone(),
        solution: records.iter().map(certified_frame).collect(),
        candidates: transfer_candidates.iter().map(certified_frame).collect(),
        exits: exits.clone(),
        coverage: evaluation_coverage.clone(),
        assumption_context,
    };
    post_fixpoint.validate_inner(graph, host, established_assumptions)?;
    Ok(Evaluation {
        exits,
        coverage: evaluation_coverage,
        post_fixpoint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Host {
        throw_invoke: bool,
        both_invoke_outcomes: bool,
        native_unwind_invoke: bool,
        fatal_invoke: bool,
        suspended_invoke: bool,
        maybe_always: bool,
        growing_loop: bool,
        bad_join: bool,
        bad_value_join: bool,
        bad_widening: bool,
        nested_open: Option<Domain>,
        incomplete_nested_coverage: bool,
        omit_nested_coverage: bool,
        invocation_not_applicable: bool,
    }
    fn count_interval(state: &[String]) -> (u32, Option<u32>) {
        let Some(encoded) = state.first().and_then(|item| item.strip_prefix("count:")) else {
            return (0, Some(0));
        };
        let (min, max) = encoded.split_once(':').expect("count interval delimiter");
        (
            min.parse().expect("count lower bound"),
            (max != "*").then(|| max.parse().expect("count upper bound")),
        )
    }

    fn encode_count_interval(min: u32, max: Option<u32>) -> Vec<String> {
        vec![format!(
            "count:{min}:{}",
            max.map_or_else(|| "*".into(), |n| n.to_string())
        )]
    }

    impl SemanticHost for Host {
        type Value = String;
        type State = Vec<String>;

        fn resolve(
            &self,
            operand: &Operand,
            args: &[String],
            receiver: &String,
            results: &[(u32, String)],
        ) -> Option<String> {
            match operand {
                Operand::Argument(i) => args.get(*i as usize).cloned(),
                Operand::Receiver => Some(receiver.clone()),
                Operand::Result(i) => results
                    .iter()
                    .find(|(id, _)| id == i)
                    .map(|(_, v)| v.clone()),
                Operand::Literal(v) => Some(String::from_utf8_lossy(&v.0).into_owned()),
                Operand::IntRange { .. } => None,
                Operand::StringByteLength(_) => None,
                Operand::Unknown => None,
            }
        }
        fn test(
            &self,
            p: &Predicate,
            args: &[String],
            receiver: &String,
            results: &[(u32, String)],
            state: &Vec<String>,
        ) -> Truth {
            match p {
                Predicate::Always => {
                    if self.maybe_always {
                        Truth::Maybe
                    } else {
                        Truth::Yes
                    }
                }
                Predicate::IsLiteral { value, expected } => {
                    match self.resolve(value, args, receiver, results) {
                        Some(v) if v.as_bytes() == expected.0 => Truth::Yes,
                        Some(_) => Truth::No,
                        None => Truth::Maybe,
                    }
                }
                Predicate::Host(id) if id.0 == b"state-has-invoke" => {
                    if state.iter().any(|event| event == "invoke") {
                        Truth::Yes
                    } else if state.iter().any(|event| event == "<joined>") {
                        Truth::Maybe
                    } else {
                        Truth::No
                    }
                }
                Predicate::Host(_) => Truth::Maybe,
            }
        }
        fn event(
            &self,
            event: &Event,
            args: &[String],
            receiver: &String,
            _results: &[(u32, String)],
            mut state: Vec<String>,
        ) -> Vec<EventExit<String, Vec<String>>> {
            let mut result = None;
            match event {
                Event::Read { .. } => state.push("read".into()),
                Event::Write { .. } => {
                    if self.growing_loop {
                        let (min, max) = count_interval(&state);
                        state =
                            encode_count_interval(min + 1, max.map(|max| max.saturating_add(1)));
                    } else {
                        state.push("write".into());
                    }
                }
                Event::Invoke {
                    result: result_id, ..
                } => {
                    state.push("invoke".into());
                    let mut coverage = composed_coverage(self.nested_open);
                    if self.invocation_not_applicable {
                        coverage
                            .iter_mut()
                            .find(|item| item.domain == Domain::Invocation)
                            .unwrap()
                            .coverage = Coverage::NotApplicable;
                    }
                    if self.incomplete_nested_coverage {
                        coverage.pop();
                    }
                    if self.omit_nested_coverage {
                        coverage.clear();
                    }
                    if self.native_unwind_invoke {
                        return vec![EventExit::NativeUnwind {
                            value: "native exception".into(),
                            state,
                            coverage,
                        }];
                    }
                    if self.fatal_invoke || self.suspended_invoke {
                        let domain = if self.fatal_invoke {
                            Domain::Fatal
                        } else {
                            Domain::Suspension
                        };
                        let coverage = coverage
                            .into_iter()
                            .map(|mut item| {
                                if item.domain == domain {
                                    item.coverage = Coverage::Open {
                                        reasons: vec![Bytes(
                                            b"observed abrupt invocation".to_vec(),
                                        )],
                                    };
                                    item.assumptions = vec![Digest([3; 32])];
                                }
                                item
                            })
                            .collect();
                        if self.fatal_invoke {
                            return vec![EventExit::Fatal {
                                value: "fatal callback".into(),
                                state,
                                coverage,
                            }];
                        }
                        return vec![EventExit::Suspended {
                            value: "suspended callback".into(),
                            state,
                            coverage,
                        }];
                    }
                    if self.throw_invoke {
                        return vec![EventExit::PhpThrow {
                            value: "exception".into(),
                            state,
                            coverage,
                        }];
                    }
                    result = Some((*result_id, "callback-result".into()));
                    if self.both_invoke_outcomes {
                        let throwing_state = state.clone();
                        return vec![
                            EventExit::Normal {
                                result,
                                state,
                                coverage: coverage.clone(),
                            },
                            EventExit::PhpThrow {
                                value: "exception".into(),
                                state: throwing_state,
                                coverage,
                            },
                        ];
                    }
                    return vec![EventExit::Normal {
                        result,
                        state,
                        coverage,
                    }];
                }
                Event::Release { .. } => state.push("release".into()),
                Event::PhpFlow { instruction } => {
                    state.push(format!("php-flow:{instruction:?}"));
                }
                Event::EmitDiagnostic { kind, argument } => {
                    state.push(format!("diagnostic:{kind:?}:{argument}"));
                }
                Event::Unknown { domains, .. } => state.push(format!("unknown:{domains:?}")),
            }
            let _ = (args, receiver);
            vec![EventExit::Normal {
                result,
                state,
                coverage: vec![],
            }]
        }

        fn unknown_value(&self) -> String {
            "<unknown>".into()
        }
        fn join_value(&self, a: String, b: String) -> String {
            if self.bad_value_join {
                return a;
            }
            if a == b { a } else { "<unknown>".into() }
        }
        fn widen_value(&self, _old: String, _next: String) -> String {
            "<unknown>".into()
        }
        fn value_subsumes(&self, broad: &String, narrow: &String) -> bool {
            broad == narrow || broad == "<unknown>"
        }
        fn join_state(&self, a: Vec<String>, b: Vec<String>) -> Vec<String> {
            if self.bad_join {
                return a;
            }
            if self.growing_loop {
                let (a_min, a_max) = count_interval(&a);
                let (b_min, b_max) = count_interval(&b);
                return encode_count_interval(
                    a_min.min(b_min),
                    match (a_max, b_max) {
                        (Some(a), Some(b)) => Some(a.max(b)),
                        _ => None,
                    },
                );
            }
            if a == b { a } else { vec!["<joined>".into()] }
        }
        fn widen_state(&self, old: Vec<String>, next: Vec<String>) -> Vec<String> {
            if self.bad_widening {
                return old;
            }
            if self.growing_loop {
                let (old_min, _) = count_interval(&old);
                let (next_min, _) = count_interval(&next);
                encode_count_interval(old_min.min(next_min), None)
            } else {
                vec!["<joined>".into()]
            }
        }
        fn state_subsumes(&self, broad: &Vec<String>, narrow: &Vec<String>) -> bool {
            if self.growing_loop {
                let (broad_min, broad_max) = count_interval(broad);
                let (narrow_min, narrow_max) = count_interval(narrow);
                broad_min <= narrow_min
                    && broad_max.is_none_or(|max| narrow_max.is_some_and(|n| n <= max))
            } else {
                broad == narrow || broad == &["<joined>".to_string()]
            }
        }
    }

    #[test]
    fn event_domain_contract_matches_literal_expected_mapping() {
        let operand = Operand::Literal(Bytes(b"x".to_vec()));
        let cases = [
            (
                Event::Read {
                    storage: operand.clone(),
                },
                vec![Domain::UserRead],
            ),
            (
                Event::Write {
                    storage: operand.clone(),
                    value: operand.clone(),
                },
                vec![Domain::UserWrite],
            ),
            (
                Event::Invoke {
                    callable: operand.clone(),
                    arguments: vec![],
                    result: 0,
                },
                vec![Domain::Invocation],
            ),
            (
                Event::Release {
                    value: operand.clone(),
                },
                vec![Domain::Release],
            ),
            (
                Event::EmitDiagnostic {
                    kind: DiagnosticKind::NullToInternalScalarParameterDeprecated,
                    argument: 0,
                },
                vec![Domain::Diagnostic],
            ),
            (
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::Assign {
                        destination: Bytes(b"a".to_vec()),
                        source: Bytes(b"b".to_vec()),
                    },
                },
                vec![Domain::UserRead, Domain::UserWrite],
            ),
            (
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::Unset {
                        local: Bytes(b"a".to_vec()),
                    },
                },
                vec![Domain::UserWrite],
            ),
            (
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::BindReference {
                        left: Bytes(b"a".to_vec()),
                        right: Bytes(b"b".to_vec()),
                    },
                },
                vec![Domain::UserRead, Domain::UserWrite],
            ),
            (
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::ArraySet {
                        array: Bytes(b"a".to_vec()),
                        key: ArrayKey::Int(0),
                        source: Bytes(b"b".to_vec()),
                    },
                },
                vec![Domain::UserRead, Domain::UserWrite],
            ),
            (
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::ArrayGet {
                        destination: Bytes(b"a".to_vec()),
                        array: Bytes(b"b".to_vec()),
                        key: ArrayKey::Int(0),
                    },
                },
                vec![Domain::UserRead, Domain::UserWrite],
            ),
            (
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::ArrayBindReference {
                        array: Bytes(b"a".to_vec()),
                        key: ArrayKey::Int(0),
                        local: Bytes(b"b".to_vec()),
                    },
                },
                vec![Domain::UserRead, Domain::UserWrite],
            ),
            (
                Event::Unknown {
                    domains: vec![Domain::Output, Domain::UserWrite],
                    reason: Bytes(b"opaque".to_vec()),
                },
                vec![Domain::Output, Domain::UserWrite],
            ),
        ];
        for (event, expected) in cases {
            assert_eq!(event_domains(&event), expected, "{event:?}");
        }
    }

    fn graph(events: Vec<Event>, terminator: Terminator) -> BehaviorGraph {
        let mut applicable = Vec::new();
        for event in &events {
            let required = match event {
                Event::Read { .. } => vec![Domain::UserRead],
                Event::Write { .. } => vec![Domain::UserWrite],
                Event::Invoke { .. } => vec![Domain::Invocation],
                Event::Release { .. } => vec![Domain::Release],
                Event::EmitDiagnostic { .. } => vec![Domain::Diagnostic],
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::Assign { .. },
                } => vec![Domain::UserRead, Domain::UserWrite],
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::Unset { .. },
                } => vec![Domain::UserWrite],
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::BindReference { .. },
                } => vec![Domain::UserRead, Domain::UserWrite],
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::ArraySet { .. },
                } => vec![Domain::UserRead, Domain::UserWrite],
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::ArrayGet { .. },
                } => vec![Domain::UserRead, Domain::UserWrite],
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::ArrayBindReference { .. },
                } => vec![Domain::UserRead, Domain::UserWrite],
                Event::Unknown { domains, .. } => domains.clone(),
            };
            for domain in required {
                if !applicable.contains(&domain) {
                    applicable.push(domain);
                }
            }
        }
        let exit_domain = match terminator {
            Terminator::Return(_) => Some(Domain::Return),
            Terminator::Throw(_) => Some(Domain::PhpThrow),
            Terminator::Fatal(_) => Some(Domain::Fatal),
            Terminator::Suspend(_) => Some(Domain::Suspension),
            _ => None,
        };
        if let Some(domain) = exit_domain
            && !applicable.contains(&domain)
        {
            applicable.push(domain);
        }
        let mut coverage = coverage_for(&applicable);
        if matches!(terminator, Terminator::Return(_)) {
            coverage
                .iter_mut()
                .find(|item| item.domain == Domain::Return)
                .expect("return coverage")
                .dependencies
                .clear();
            for event in &events {
                let dependencies = match event {
                    Event::Read { .. } => vec![Domain::UserRead],
                    Event::Write { .. } => vec![Domain::UserWrite],
                    Event::PhpFlow {
                        instruction:
                            PhpFlowInstruction::Assign { .. }
                            | PhpFlowInstruction::BindReference { .. }
                            | PhpFlowInstruction::ArraySet { .. }
                            | PhpFlowInstruction::ArrayGet { .. }
                            | PhpFlowInstruction::ArrayBindReference { .. },
                    } => vec![Domain::UserRead, Domain::UserWrite],
                    Event::PhpFlow {
                        instruction: PhpFlowInstruction::Unset { .. },
                    } => vec![Domain::UserWrite],
                    _ => vec![],
                };
                coverage
                    .iter_mut()
                    .find(|item| item.domain == Domain::Return)
                    .expect("return coverage")
                    .dependencies
                    .extend(dependencies);
            }
            if let Terminator::Return(Some(Operand::Result(result_id))) = &terminator
                && events.iter().any(
                    |event| matches!(event, Event::Invoke { result, .. } if result == result_id),
                )
            {
                coverage
                    .iter_mut()
                    .find(|item| item.domain == Domain::Return)
                    .expect("return coverage")
                    .dependencies
                    .push(Domain::Invocation);
            }
            let returned = coverage
                .iter_mut()
                .find(|item| item.domain == Domain::Return)
                .expect("return coverage");
            returned.dependencies.sort_by_key(|dependency| {
                Domain::ALL.iter().position(|domain| domain == dependency)
            });
            returned.dependencies.dedup();
        }
        BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![Block::new(events, terminator)],
            coverage,
        }
    }
    fn test_certificate(
        graph: &BehaviorGraph,
        solution: Vec<Frame<String, Vec<String>>>,
        candidates: Vec<Frame<String, Vec<String>>>,
    ) -> PostFixpointCertificate<String, Vec<String>> {
        PostFixpointCertificate {
            graph_digest: graph.canonical_digest().unwrap(),
            initial: solution
                .first()
                .map(certified_frame)
                .expect("test solution"),
            arguments: vec![],
            receiver: receiver(),
            solution: solution.iter().map(certified_frame).collect(),
            candidates: candidates.iter().map(certified_frame).collect(),
            exits: vec![],
            coverage: graph.coverage.clone(),
            assumption_context: None,
        }
    }
    fn coverage_for(applicable: &[Domain]) -> Vec<DomainCoverage> {
        let mut coverage: Vec<_> = Domain::ALL
            .into_iter()
            .map(|domain| DomainCoverage {
                domain,
                coverage: if applicable.contains(&domain) {
                    Coverage::Closed
                } else {
                    Coverage::NotApplicable
                },
                precision: Precision::Exact,
                dependencies: vec![],
                evidence: vec![Digest([1; 32])],
                assumptions: vec![],
            })
            .collect();
        if applicable.contains(&Domain::Return) {
            let dependencies = [Domain::Invocation, Domain::UserRead, Domain::UserWrite];
            coverage
                .iter_mut()
                .find(|item| item.domain == Domain::Return)
                .expect("return coverage")
                .dependencies
                .extend(
                    dependencies
                        .into_iter()
                        .filter(|dependency| applicable.contains(dependency)),
                );
        }
        if applicable.contains(&Domain::Invocation) {
            for projection in [
                Domain::PhpThrow,
                Domain::NativeUnwind,
                Domain::Fatal,
                Domain::Suspension,
            ] {
                if applicable.contains(&projection) {
                    coverage
                        .iter_mut()
                        .find(|item| item.domain == projection)
                        .expect("exit coverage")
                        .dependencies
                        .push(Domain::Invocation);
                }
            }
        }
        coverage
    }

    struct AcceptingAssumptionValidator {
        identity: AssumptionValidator,
        implementation: Digest,
    }

    impl AssumptionValidatorExecutor for AcceptingAssumptionValidator {
        fn validator(&self) -> AssumptionValidator {
            self.identity.clone()
        }

        fn implementation(&self) -> Digest {
            self.implementation
        }

        fn validate(
            &self,
            _: &AssumptionRecord,
            evidence: &[EvidenceRecord],
            _: &[AssumptionRecord],
            _: &TargetScenario,
            _: Option<Digest>,
        ) -> Applicability {
            assert!(!evidence.is_empty());
            Applicability::Yes
        }
    }

    fn assumption_target() -> TargetScenario {
        TargetScenario {
            release: VersionRange::minor(8, 5),
            os: Some(Bytes(b"linux".to_vec())),
            arch: Some(Bytes(b"x86_64".to_vec())),
            sapi: Some(Bytes(b"cli".to_vec())),
            zts: Some(false),
            debug: Some(false),
            extensions: Some(vec![]),
            ini: Some(vec![]),
        }
    }

    fn assumption_trust_policy(
        validator: AssumptionValidator,
        implementation: Digest,
    ) -> ValidatorTrustPolicy {
        ValidatorTrustPolicy {
            revision: 1,
            roots: vec![ValidatorTrustRoot {
                id: Bytes(b"phase-f-root".to_vec()),
                fingerprint: Digest([0x91; 32]),
            }],
            pins: vec![ValidatorPin {
                validator,
                implementation,
                root: Bytes(b"phase-f-root".to_vec()),
            }],
            revocations: vec![],
        }
    }

    #[test]
    fn trusted_assumption_flows_through_evaluation_and_certificate_replay() {
        let validator = AssumptionValidator {
            id: Bytes(b"phase-f-validator".to_vec()),
            version: Bytes(b"1".to_vec()),
        };
        let implementation = Digest([0x92; 32]);
        let evidence = EvidenceRecord {
            kind: EvidenceKind::ModelContract,
            source_digest: Digest([0x93; 32]),
            site: Some(Bytes(b"assumption integration test".to_vec())),
            rule_version: Bytes(b"test-v1".to_vec()),
            assumptions: vec![],
            dependencies: vec![],
        };
        let mut catalog = Catalog::default();
        let evidence_id = catalog.add_evidence(evidence).unwrap();
        let assumption = AssumptionRecord {
            kind: AssumptionKind::TargetProfile,
            claim: Bytes(b"the validation test target is supported".to_vec()),
            scope: AssumptionScope {
                target: TargetPredicate::any(),
                loaded_program: None,
            },
            prerequisites: vec![],
            supporting_evidence: vec![evidence_id],
            validator: validator.clone(),
        };
        let assumption_id = catalog.add_assumption(assumption).unwrap();
        let mut graph = graph(vec![], Terminator::Return(None));
        for item in &mut graph.coverage {
            item.evidence = vec![evidence_id];
        }
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .assumptions = vec![assumption_id];
        let key = EntryKey::Function {
            extension: Bytes(b"core".to_vec()),
            name: Bytes(b"assumption_test".to_vec()),
        };
        catalog.add_entry(key.clone(), graph.clone()).unwrap();
        let catalog = Catalog::from_json(&catalog.manifest().to_json().unwrap()).unwrap();

        let executor = AcceptingAssumptionValidator {
            identity: validator.clone(),
            implementation,
        };
        let executors: [&dyn AssumptionValidatorExecutor; 1] = [&executor];
        let policy = assumption_trust_policy(validator, implementation);
        let target = assumption_target();
        assert_eq!(
            evaluate(&graph, &[], &receiver(), vec![], &mut Host::default()),
            Err(GraphError::UnestablishedAssumption(Domain::Return))
        );
        let evaluation = evaluate_with_assumption_context(
            &graph,
            &catalog,
            &key,
            &target,
            None,
            &policy,
            &executors,
            &[],
            &receiver(),
            vec![],
            &mut Host::default(),
        )
        .unwrap();
        evaluation
            .post_fixpoint
            .validate_with_assumption_context(
                &graph,
                &Host::default(),
                &catalog,
                &key,
                &target,
                None,
                &policy,
                &executors,
            )
            .unwrap();
        let persisted: PostFixpointCertificate<String, Vec<String>> =
            serde_json::from_str(&serde_json::to_string(&evaluation.post_fixpoint).unwrap())
                .unwrap();
        persisted
            .validate_with_assumption_context(
                &graph,
                &Host::default(),
                &catalog,
                &key,
                &target,
                None,
                &policy,
                &executors,
            )
            .unwrap();

        let mut revoked = policy.clone();
        revoked.revision = 2;
        revoked.revocations.push(ValidatorRevocation {
            validator_id: Bytes(b"phase-f-validator".to_vec()),
            version: Bytes(b"1".to_vec()),
            effective_revision: 2,
        });
        assert_eq!(
            persisted.validate_with_assumption_context(
                &graph,
                &Host::default(),
                &catalog,
                &key,
                &target,
                None,
                &revoked,
                &executors,
            ),
            Err(AssumptionCertificateError::ContextChanged)
        );
        let mut other_target = target.clone();
        other_target.os = Some(Bytes(b"windows".to_vec()));
        assert_eq!(
            persisted.validate_with_assumption_context(
                &graph,
                &Host::default(),
                &catalog,
                &key,
                &other_target,
                None,
                &policy,
                &executors,
            ),
            Err(AssumptionCertificateError::ContextChanged)
        );
        let other_key = EntryKey::Function {
            extension: Bytes(b"core".to_vec()),
            name: Bytes(b"different_function".to_vec()),
        };
        assert_eq!(
            persisted.validate_with_assumption_context(
                &graph,
                &Host::default(),
                &catalog,
                &other_key,
                &target,
                None,
                &policy,
                &executors,
            ),
            Err(AssumptionCertificateError::ContextChanged)
        );
    }

    fn close_unmodeled_domains(graph: &mut BehaviorGraph) {
        for item in &mut graph.coverage {
            if matches!(item.coverage, Coverage::NotApplicable) {
                item.coverage = Coverage::Closed;
            }
        }
        if graph.blocks.iter().any(|block| {
            block
                .events
                .iter()
                .any(|event| matches!(event, Event::Invoke { .. }))
        }) {
            for projection in [
                Domain::PhpThrow,
                Domain::NativeUnwind,
                Domain::Fatal,
                Domain::Suspension,
            ] {
                let item = graph
                    .coverage
                    .iter_mut()
                    .find(|item| item.domain == projection)
                    .unwrap();
                if !item.dependencies.contains(&Domain::Invocation) {
                    item.dependencies.push(Domain::Invocation);
                }
            }
        }
    }

    #[test]
    fn removing_required_state_dependency_from_closed_return_is_rejected() {
        let mut result_graph = graph(
            vec![Event::Write {
                storage: Operand::Argument(0),
                value: Operand::Literal(Bytes(b"written".to_vec())),
            }],
            Terminator::Return(None),
        );
        let returned = result_graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        assert!(returned.dependencies.contains(&Domain::UserWrite));
        returned.dependencies.clear();
        assert_eq!(
            result_graph.validate(),
            Err(GraphError::MissingRequiredDomainDependency {
                domain: Domain::Return,
                dependency: Domain::UserWrite,
            })
        );

        result_graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .dependencies
            .push(Domain::UserWrite);
        assert_eq!(result_graph.validate(), Ok(()));
    }

    #[test]
    fn cleanup_return_dependencies_include_supported_pending_throw_path() {
        let mut graph = graph(
            vec![],
            Terminator::Throw(Operand::Literal(Bytes(b"pending".to_vec()))),
        );
        graph.blocks[0].exceptional.php_throw = Some(1);
        graph.blocks.push(Block::new(
            vec![Event::Write {
                storage: Operand::Argument(0),
                value: Operand::Literal(Bytes(b"cleanup-write".to_vec())),
            }],
            Terminator::Return(None),
        ));
        graph.coverage = coverage_for(&[Domain::UserWrite, Domain::PhpThrow, Domain::Return]);
        let returned = graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        assert!(returned.dependencies.contains(&Domain::UserWrite));
        returned.dependencies.clear();
        assert_eq!(
            graph.validate(),
            Err(GraphError::MissingRequiredDomainDependency {
                domain: Domain::Return,
                dependency: Domain::UserWrite,
            })
        );
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .dependencies
            .push(Domain::UserWrite);
        assert_eq!(graph.validate(), Ok(()));
    }

    #[test]
    fn result_projection_requires_invocation_dependency_only_when_consumed() {
        let mut result_graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 7,
            }],
            Terminator::Return(Some(Operand::Result(7))),
        );
        let returned = result_graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        assert!(returned.dependencies.contains(&Domain::Invocation));
        returned.dependencies.clear();
        assert_eq!(
            result_graph.validate(),
            Err(GraphError::MissingRequiredDomainDependency {
                domain: Domain::Return,
                dependency: Domain::Invocation,
            })
        );

        result_graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .dependencies
            .push(Domain::Invocation);
        assert_eq!(result_graph.validate(), Ok(()));

        let unrelated_invoke = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 7,
            }],
            Terminator::Return(Some(Operand::IntRange {
                min: Some(0),
                max: None,
            })),
        );
        assert!(
            unrelated_invoke
                .coverage
                .iter()
                .find(|item| item.domain == Domain::Return)
                .unwrap()
                .dependencies
                .is_empty()
        );
        assert_eq!(unrelated_invoke.validate(), Ok(()));
    }

    #[test]
    fn return_guard_on_invocation_result_requires_invocation_dependency() {
        let mut graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Invoke {
                        callable: Operand::Argument(0),
                        arguments: vec![],
                        result: 7,
                    }],
                    Terminator::Branch {
                        predicate: Predicate::IsLiteral {
                            value: Operand::Result(7),
                            expected: Bytes(b"expected".to_vec()),
                        },
                        yes: 1,
                        no: 2,
                    },
                ),
                Block::new(vec![], Terminator::Return(None)),
                Block::new(vec![], Terminator::Return(None)),
            ],
            coverage: coverage_for(&[Domain::Invocation, Domain::Return]),
        };
        let returned = graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        returned.dependencies.clear();
        assert_eq!(
            graph.validate(),
            Err(GraphError::MissingRequiredDomainDependency {
                domain: Domain::Return,
                dependency: Domain::Invocation,
            })
        );
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .dependencies
            .push(Domain::Invocation);
        assert_eq!(graph.validate(), Ok(()));
    }

    #[test]
    fn invocation_exception_projection_requires_invocation_dependency() {
        let mut throw_graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 7,
            }],
            Terminator::Throw(Operand::Literal(Bytes(b"throw".to_vec()))),
        );
        let thrown = throw_graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::PhpThrow)
            .unwrap();
        assert!(thrown.dependencies.contains(&Domain::Invocation));
        thrown.dependencies.clear();
        assert_eq!(
            throw_graph.validate(),
            Err(GraphError::MissingRequiredDomainDependency {
                domain: Domain::PhpThrow,
                dependency: Domain::Invocation,
            })
        );
        throw_graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::PhpThrow)
            .unwrap()
            .dependencies
            .push(Domain::Invocation);
        assert_eq!(throw_graph.validate(), Ok(()));

        let mut unwind_graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 7,
            }],
            Terminator::Return(None),
        );
        unwind_graph.coverage = coverage_for(&Domain::ALL);
        let unwind = unwind_graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::NativeUnwind)
            .unwrap();
        assert!(unwind.dependencies.contains(&Domain::Invocation));
        unwind.dependencies.clear();
        assert_eq!(
            unwind_graph.validate(),
            Err(GraphError::MissingRequiredDomainDependency {
                domain: Domain::NativeUnwind,
                dependency: Domain::Invocation,
            })
        );
    }

    #[test]
    fn composed_closed_invocation_exits_require_dependency_edges() {
        let graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 7,
            }],
            Terminator::Return(None),
        );
        for projection in [
            Domain::PhpThrow,
            Domain::NativeUnwind,
            Domain::Fatal,
            Domain::Suspension,
        ] {
            let mut composed = graph.coverage.clone();
            let mut complete_child = coverage_for(&Domain::ALL);
            complete_child
                .iter_mut()
                .find(|item| item.domain == projection)
                .unwrap()
                .dependencies
                .clear();
            assert_eq!(
                merge_graph_coverage(&graph, &mut composed, &complete_child),
                Err(GraphError::MissingRequiredDomainDependency {
                    domain: projection,
                    dependency: Domain::Invocation,
                })
            );
        }
    }
    fn receiver() -> String {
        "receiver".into()
    }
    fn open_coverage(domain: Domain, reason: &[u8]) -> DomainCoverage {
        DomainCoverage {
            domain,
            coverage: Coverage::Open {
                reasons: vec![Bytes(reason.to_vec())],
            },
            precision: Precision::Widened,
            dependencies: vec![],
            evidence: vec![Digest([2; 32])],
            assumptions: vec![],
        }
    }

    fn dependency_coverage(
        domain: Domain,
        coverage: Coverage,
        dependencies: Vec<Domain>,
    ) -> DomainCoverage {
        DomainCoverage {
            domain,
            coverage,
            precision: Precision::Exact,
            dependencies,
            evidence: vec![Digest([4; 32])],
            assumptions: vec![],
        }
    }

    fn composed_coverage(open_domain: Option<Domain>) -> Vec<DomainCoverage> {
        Domain::ALL
            .into_iter()
            .map(|domain| DomainCoverage {
                domain,
                coverage: if Some(domain) == open_domain {
                    Coverage::Open {
                        reasons: vec![Bytes(b"nested boundary".to_vec())],
                    }
                } else {
                    Coverage::Closed
                },
                precision: Precision::Exact,
                dependencies: if matches!(
                    domain,
                    Domain::PhpThrow | Domain::NativeUnwind | Domain::Fatal | Domain::Suspension
                ) {
                    vec![Domain::Invocation]
                } else {
                    vec![]
                },
                evidence: vec![Digest([2; 32])],
                assumptions: if Some(domain) == open_domain {
                    vec![Digest([3; 32])]
                } else {
                    vec![]
                },
            })
            .collect()
    }

    #[test]
    fn callback_result_and_mutation_feed_ordered_return() {
        let graph = graph(
            vec![
                Event::Write {
                    storage: Operand::Argument(0),
                    value: Operand::Literal(Bytes(b"changed".to_vec())),
                },
                Event::Invoke {
                    callable: Operand::Argument(1),
                    arguments: vec![],
                    result: 4,
                },
            ],
            Terminator::Return(Some(Operand::Result(4))),
        );
        let mut host = Host::default();
        let exits = evaluate(
            &graph,
            &["slot".into(), "callback".into()],
            &receiver(),
            vec![],
            &mut host,
        )
        .unwrap()
        .exits;
        assert_eq!(
            exits,
            vec![Exit::Normal {
                value: Some("callback-result".into()),
                state: vec!["write".into(), "invoke".into()]
            }]
        );
    }

    #[test]
    fn callback_result_selects_branch_before_the_next_callback() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Invoke {
                        callable: Operand::Argument(0),
                        arguments: vec![],
                        result: 3,
                    }],
                    Terminator::Branch {
                        predicate: Predicate::IsLiteral {
                            value: Operand::Result(3),
                            expected: Bytes(b"callback-result".to_vec()),
                        },
                        yes: 1,
                        no: 2,
                    },
                ),
                Block::new(
                    vec![
                        Event::Write {
                            storage: Operand::Argument(1),
                            value: Operand::Literal(Bytes(b"between-callbacks".to_vec())),
                        },
                        Event::Invoke {
                            callable: Operand::Argument(0),
                            arguments: vec![],
                            result: 4,
                        },
                    ],
                    Terminator::Return(Some(Operand::Result(4))),
                ),
                Block::new(
                    vec![],
                    Terminator::Return(Some(Operand::Literal(Bytes(b"wrong-branch".to_vec())))),
                ),
            ],
            coverage: coverage_for(&[Domain::Invocation, Domain::UserWrite, Domain::Return]),
        };
        let mut host = Host::default();
        let evaluation = evaluate(
            &graph,
            &["callback".into(), "slot".into()],
            &receiver(),
            vec![],
            &mut host,
        )
        .unwrap();
        assert_eq!(
            evaluation.exits,
            vec![Exit::Normal {
                value: Some("callback-result".into()),
                state: vec!["invoke".into(), "write".into(), "invoke".into()],
            }]
        );
    }

    #[test]
    fn php_flow_host_composes_callback_writes_with_normal_and_throw_states() {
        let evidence = Digest([9; 32]);
        let mut host = PhpFlowHost::new(evidence);
        host.argument_locations = vec![None, Some(Bytes(b"alias".to_vec()))];
        host.callbacks.insert(
            Bytes(b"mutate".to_vec()),
            vec![
                PhpCallbackOutcome {
                    writes: vec![(0, PhpValue::Int(1))],
                    normal: true,
                    result: Some(PhpValue::Int(7)),
                    throws: None,
                },
                PhpCallbackOutcome {
                    writes: vec![(0, PhpValue::Int(2))],
                    normal: false,
                    result: None,
                    throws: Some(PhpValue::Int(99)),
                },
            ],
        );
        let mut invoke = Block::new(
            vec![
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::BindReference {
                        left: Bytes(b"alias".to_vec()),
                        right: Bytes(b"slot".to_vec()),
                    },
                },
                Event::Invoke {
                    callable: Operand::Argument(0),
                    arguments: vec![Operand::Argument(1)],
                    result: 4,
                },
            ],
            Terminator::Return(Some(Operand::Result(4))),
        );
        invoke.exceptional.php_throw = Some(1);
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![invoke, Block::new(vec![], Terminator::Resume)],
            coverage: coverage_for(&[
                Domain::Invocation,
                Domain::UserRead,
                Domain::UserWrite,
                Domain::Return,
                Domain::PhpThrow,
            ]),
        };
        let mut initial = PhpState::new();
        initial.set_local(Bytes(b"slot".to_vec()), PhpValue::Int(0));
        let evaluation = evaluate(
            &graph,
            &[
                PhpValue::Callable(Bytes(b"mutate".to_vec())),
                PhpValue::Unknown,
            ],
            &PhpValue::Unknown,
            initial,
            &mut host,
        )
        .unwrap();

        assert!(evaluation.exits.contains(&Exit::Normal {
            value: Some(PhpValue::Int(7)),
            state: {
                let mut state = PhpState::new();
                state.set_local(Bytes(b"slot".to_vec()), PhpValue::Int(0));
                state.bind_reference(Bytes(b"alias".to_vec()), Bytes(b"slot".to_vec()));
                state.set_local(Bytes(b"alias".to_vec()), PhpValue::Int(1));
                state
            },
        }));
        assert!(evaluation.exits.contains(&Exit::PhpThrow {
            value: PhpValue::Int(99),
            state: {
                let mut state = PhpState::new();
                state.set_local(Bytes(b"slot".to_vec()), PhpValue::Int(0));
                state.bind_reference(Bytes(b"alias".to_vec()), Bytes(b"slot".to_vec()));
                state.set_local(Bytes(b"alias".to_vec()), PhpValue::Int(2));
                state
            },
        }));
        evaluation.post_fixpoint.validate(&graph, &host).unwrap();
    }

    #[test]
    fn php_flow_host_uses_callback_result_to_select_next_invocation() {
        let mut host = PhpFlowHost::new(Digest([8; 32]));
        host.argument_locations = vec![None, None];
        host.callbacks.insert(
            Bytes(b"first".to_vec()),
            vec![PhpCallbackOutcome {
                writes: vec![],
                normal: true,
                result: Some(PhpValue::String(Bytes(b"continue".to_vec()))),
                throws: None,
            }],
        );
        host.callbacks.insert(
            Bytes(b"second".to_vec()),
            vec![PhpCallbackOutcome {
                writes: vec![],
                normal: true,
                result: Some(PhpValue::String(Bytes(b"finished".to_vec()))),
                throws: None,
            }],
        );
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Invoke {
                        callable: Operand::Argument(0),
                        arguments: vec![],
                        result: 3,
                    }],
                    Terminator::Branch {
                        predicate: Predicate::IsLiteral {
                            value: Operand::Result(3),
                            expected: Bytes(b"continue".to_vec()),
                        },
                        yes: 1,
                        no: 2,
                    },
                ),
                Block::new(
                    vec![Event::Invoke {
                        callable: Operand::Argument(1),
                        arguments: vec![],
                        result: 4,
                    }],
                    Terminator::Return(Some(Operand::Result(4))),
                ),
                Block::new(
                    vec![],
                    Terminator::Return(Some(Operand::Literal(Bytes(b"wrong-branch".to_vec())))),
                ),
            ],
            coverage: coverage_for(&[Domain::Invocation, Domain::Return]),
        };
        let evaluation = evaluate(
            &graph,
            &[
                PhpValue::Callable(Bytes(b"first".to_vec())),
                PhpValue::Callable(Bytes(b"second".to_vec())),
            ],
            &PhpValue::Unknown,
            PhpState::new(),
            &mut host,
        )
        .unwrap();
        assert_eq!(
            evaluation.exits,
            vec![Exit::Normal {
                value: Some(PhpValue::String(Bytes(b"finished".to_vec()))),
                state: PhpState::new(),
            }]
        );
        evaluation.post_fixpoint.validate(&graph, &host).unwrap();
    }

    #[test]
    fn php_flow_host_joins_distinct_normal_callback_branches_conservatively() {
        let mut host = PhpFlowHost::new(Digest([15; 32]));
        host.argument_locations = vec![None, Some(Bytes(b"slot".to_vec()))];
        host.callbacks.insert(
            Bytes(b"branching".to_vec()),
            vec![
                PhpCallbackOutcome {
                    writes: vec![(0, PhpValue::Int(1))],
                    normal: true,
                    result: Some(PhpValue::Int(10)),
                    throws: None,
                },
                PhpCallbackOutcome {
                    writes: vec![(0, PhpValue::Int(2))],
                    normal: true,
                    result: Some(PhpValue::Int(20)),
                    throws: None,
                },
            ],
        );
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Invoke {
                        callable: Operand::Argument(0),
                        arguments: vec![Operand::Argument(1)],
                        result: 4,
                    }],
                    Terminator::Branch {
                        predicate: Predicate::Host(Bytes(b"uncertain-branch".to_vec())),
                        yes: 1,
                        no: 2,
                    },
                ),
                Block::new(vec![], Terminator::Goto(3)),
                Block::new(vec![], Terminator::Goto(3)),
                Block::new(vec![], Terminator::Return(Some(Operand::Result(4)))),
            ],
            coverage: coverage_for(&[
                Domain::Invocation,
                Domain::UserWrite,
                Domain::Return,
                Domain::PhpThrow,
            ]),
        };
        let mut initial = PhpState::new();
        initial.set_local(Bytes(b"slot".to_vec()), PhpValue::Int(0));
        let evaluation = evaluate(
            &graph,
            &[
                PhpValue::Callable(Bytes(b"branching".to_vec())),
                PhpValue::Unknown,
            ],
            &PhpValue::Unknown,
            initial,
            &mut host,
        )
        .unwrap();

        assert_eq!(evaluation.exits.len(), 1);
        assert!(matches!(
            &evaluation.exits[0],
            Exit::Normal {
                value: Some(PhpValue::Unknown),
                state,
            } if state.local(b"slot") == Some(PhpValue::Unknown)
        ));
        evaluation.post_fixpoint.validate(&graph, &host).unwrap();
    }

    #[test]
    fn php_flow_array_reference_copy_tracks_callback_writes_across_cleanup() {
        let mut host = PhpFlowHost::new(Digest([14; 32]));
        host.argument_locations = vec![None, Some(Bytes(b"alias".to_vec()))];
        host.callbacks.insert(
            Bytes(b"mutate".to_vec()),
            vec![
                PhpCallbackOutcome {
                    writes: vec![(0, PhpValue::Int(1))],
                    normal: true,
                    result: Some(PhpValue::Null),
                    throws: None,
                },
                PhpCallbackOutcome {
                    writes: vec![(0, PhpValue::Int(2))],
                    normal: false,
                    result: None,
                    throws: Some(PhpValue::Int(99)),
                },
            ],
        );
        let key = ArrayKey::String(Bytes(b"value".to_vec()));
        let mut body = Block::new(
            vec![
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::BindReference {
                        left: Bytes(b"alias".to_vec()),
                        right: Bytes(b"slot".to_vec()),
                    },
                },
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::ArrayBindReference {
                        array: Bytes(b"source".to_vec()),
                        key: key.clone(),
                        local: Bytes(b"slot".to_vec()),
                    },
                },
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::Assign {
                        destination: Bytes(b"copy".to_vec()),
                        source: Bytes(b"source".to_vec()),
                    },
                },
                Event::Invoke {
                    callable: Operand::Argument(0),
                    arguments: vec![Operand::Argument(1)],
                    result: 0,
                },
                Event::PhpFlow {
                    instruction: PhpFlowInstruction::ArrayGet {
                        destination: Bytes(b"observed".to_vec()),
                        array: Bytes(b"copy".to_vec()),
                        key: key.clone(),
                    },
                },
            ],
            Terminator::Return(Some(Operand::Result(0))),
        );
        body.exceptional.php_throw = Some(1);
        let cleanup = Block::new(
            vec![Event::PhpFlow {
                instruction: PhpFlowInstruction::ArrayGet {
                    destination: Bytes(b"observed".to_vec()),
                    array: Bytes(b"copy".to_vec()),
                    key: key.clone(),
                },
            }],
            Terminator::Resume,
        );
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![body, cleanup],
            coverage: coverage_for(&[
                Domain::Invocation,
                Domain::UserRead,
                Domain::UserWrite,
                Domain::Return,
                Domain::PhpThrow,
            ]),
        };
        let mut initial = PhpState::new();
        initial.set_local(Bytes(b"slot".to_vec()), PhpValue::Int(0));
        let source = initial.new_array();
        initial.set_local(Bytes(b"source".to_vec()), source);
        let evaluation = evaluate(
            &graph,
            &[
                PhpValue::Callable(Bytes(b"mutate".to_vec())),
                PhpValue::Unknown,
            ],
            &PhpValue::Unknown,
            initial,
            &mut host,
        )
        .unwrap();

        assert!(evaluation.exits.iter().any(|exit| matches!(
            exit,
            Exit::Normal { value: Some(PhpValue::Null), state }
                if state.local(b"slot") == Some(PhpValue::Int(1))
                    && state.local(b"observed") == Some(PhpValue::Int(1))
        )));
        assert!(evaluation.exits.iter().any(|exit| matches!(
            exit,
            Exit::PhpThrow { value: PhpValue::Int(99), state }
                if state.local(b"slot") == Some(PhpValue::Int(2))
                    && state.local(b"observed") == Some(PhpValue::Int(2))
        )));
        evaluation.post_fixpoint.validate(&graph, &host).unwrap();
    }

    #[test]
    fn php_flow_array_copy_and_object_identity_execute_through_graph() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![Block::new(
                vec![
                    Event::PhpFlow {
                        instruction: PhpFlowInstruction::Assign {
                            destination: Bytes(b"copy".to_vec()),
                            source: Bytes(b"source".to_vec()),
                        },
                    },
                    Event::PhpFlow {
                        instruction: PhpFlowInstruction::ArraySet {
                            array: Bytes(b"copy".to_vec()),
                            key: ArrayKey::String(Bytes(b"key".to_vec())),
                            source: Bytes(b"replacement".to_vec()),
                        },
                    },
                    Event::PhpFlow {
                        instruction: PhpFlowInstruction::ArrayGet {
                            destination: Bytes(b"read_back".to_vec()),
                            array: Bytes(b"copy".to_vec()),
                            key: ArrayKey::String(Bytes(b"key".to_vec())),
                        },
                    },
                    Event::PhpFlow {
                        instruction: PhpFlowInstruction::Assign {
                            destination: Bytes(b"object_copy".to_vec()),
                            source: Bytes(b"object".to_vec()),
                        },
                    },
                ],
                Terminator::Return(Some(Operand::Literal(Bytes(b"done".to_vec())))),
            )],
            coverage: coverage_for(&[Domain::UserRead, Domain::UserWrite, Domain::Return]),
        };
        let mut host = PhpFlowHost::new(Digest([12; 32]));
        let mut initial = PhpState::new();
        let source_array = initial.new_array();
        initial.set_local(Bytes(b"source".to_vec()), source_array);
        initial
            .array_set(
                b"source",
                ArrayKey::String(Bytes(b"key".to_vec())),
                PhpValue::Int(1),
            )
            .unwrap();
        initial.set_local(Bytes(b"replacement".to_vec()), PhpValue::Int(2));
        let object = initial.new_object();
        initial.set_local(Bytes(b"object".to_vec()), object.clone());

        let evaluation = evaluate(&graph, &[], &PhpValue::Unknown, initial, &mut host).unwrap();
        let Exit::Normal { state, .. } = &evaluation.exits[0] else {
            panic!("expected normal return");
        };
        assert_eq!(state.local(b"read_back"), Some(PhpValue::Int(2)));
        assert_eq!(state.local(b"object_copy"), Some(object));
        assert_eq!(
            state.array_get(b"source", &ArrayKey::String(Bytes(b"key".to_vec()))),
            Ok(Some(PhpValue::Int(1)))
        );
        evaluation.post_fixpoint.validate(&graph, &host).unwrap();
    }

    #[test]
    fn php_flow_missing_array_key_keeps_unknown_outcomes_open() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![Block::new(
                vec![Event::PhpFlow {
                    instruction: PhpFlowInstruction::ArrayGet {
                        destination: Bytes(b"value".to_vec()),
                        array: Bytes(b"array".to_vec()),
                        key: ArrayKey::Int(7),
                    },
                }],
                Terminator::Return(Some(Operand::Literal(Bytes(b"done".to_vec())))),
            )],
            coverage: coverage_for(&[
                Domain::UserRead,
                Domain::UserWrite,
                Domain::PhpThrow,
                Domain::Return,
                Domain::Fatal,
            ]),
        };
        let mut host = PhpFlowHost::new(Digest([13; 32]));
        let mut initial = PhpState::new();
        let array = initial.new_array();
        initial.set_local(Bytes(b"array".to_vec()), array);
        let evaluation = evaluate(&graph, &[], &PhpValue::Unknown, initial, &mut host).unwrap();

        assert!(
            evaluation
                .exits
                .iter()
                .any(|exit| matches!(exit, Exit::Normal { .. }))
        );
        assert!(
            evaluation
                .exits
                .iter()
                .any(|exit| matches!(exit, Exit::PhpThrow { .. }))
        );
        for domain in [
            Domain::UserRead,
            Domain::UserWrite,
            Domain::PhpThrow,
            Domain::Fatal,
        ] {
            assert!(matches!(
                evaluation
                    .coverage
                    .iter()
                    .find(|item| item.domain == domain)
                    .unwrap()
                    .coverage,
                Coverage::Open { .. }
            ));
        }
        evaluation.post_fixpoint.validate(&graph, &host).unwrap();
    }

    #[test]
    fn php_flow_host_keeps_unknown_callback_effects_open_and_havocs_state() {
        let mut host = PhpFlowHost::new(Digest([10; 32]));
        host.argument_locations = vec![None, Some(Bytes(b"slot".to_vec()))];
        let graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![Operand::Argument(1)],
                result: 0,
            }],
            Terminator::Return(Some(Operand::Result(0))),
        );
        let mut initial = PhpState::new();
        initial.set_local(Bytes(b"slot".to_vec()), PhpValue::Int(4));
        let evaluation = evaluate(
            &graph,
            &[
                PhpValue::Callable(Bytes(b"unmodeled".to_vec())),
                PhpValue::Unknown,
            ],
            &PhpValue::Unknown,
            initial,
            &mut host,
        )
        .unwrap();

        assert_eq!(evaluation.exits.len(), 5);
        assert!(evaluation.exits.iter().all(|exit| match exit {
            Exit::Normal { state, .. }
            | Exit::PhpThrow { state, .. }
            | Exit::NativeUnwind { state, .. }
            | Exit::Fatal { state, .. }
            | Exit::Suspended { state, .. } => {
                state.local(b"slot") == Some(PhpValue::Unknown)
            }
        }));
        assert!(
            evaluation
                .coverage
                .iter()
                .all(|coverage| matches!(coverage.coverage, Coverage::Open { .. }))
        );
        let mut falsely_closed = evaluation.post_fixpoint.clone();
        falsely_closed
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .coverage = Coverage::Closed;
        assert_eq!(
            falsely_closed.validate(&graph, &host),
            Err(GraphError::ClosedDomainDependsOnOpen {
                domain: Domain::Return,
                dependency: Domain::Invocation,
            })
        );
    }

    #[test]
    fn php_flow_host_keeps_unmodeled_read_exceptions_visible() {
        let mut host = PhpFlowHost::new(Digest([11; 32]));
        let graph = graph(
            vec![Event::Read {
                storage: Operand::Argument(0),
            }],
            Terminator::Return(Some(Operand::Literal(Bytes(b"read".to_vec())))),
        );
        let mut initial = PhpState::new();
        initial.set_local(Bytes(b"slot".to_vec()), PhpValue::Int(5));
        let evaluation = evaluate(
            &graph,
            &[PhpValue::Unknown],
            &PhpValue::Unknown,
            initial,
            &mut host,
        )
        .unwrap();

        assert!(
            evaluation
                .exits
                .iter()
                .any(|exit| matches!(exit, Exit::PhpThrow { .. }))
        );
        assert!(
            evaluation
                .exits
                .iter()
                .any(|exit| matches!(exit, Exit::NativeUnwind { .. }))
        );
        assert!(
            evaluation
                .exits
                .iter()
                .any(|exit| matches!(exit, Exit::Fatal { .. }))
        );
        assert!(
            evaluation
                .exits
                .iter()
                .any(|exit| matches!(exit, Exit::Suspended { .. }))
        );
        assert!(
            evaluation
                .coverage
                .iter()
                .all(|coverage| matches!(coverage.coverage, Coverage::Open { .. }))
        );
    }

    #[test]
    fn host_predicate_observes_callback_state_after_invocation() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Invoke {
                        callable: Operand::Argument(0),
                        arguments: vec![],
                        result: 0,
                    }],
                    Terminator::Branch {
                        predicate: Predicate::Host(Bytes(b"state-has-invoke".to_vec())),
                        yes: 1,
                        no: 2,
                    },
                ),
                Block::new(
                    vec![],
                    Terminator::Return(Some(Operand::Literal(Bytes(b"called".to_vec())))),
                ),
                Block::new(
                    vec![],
                    Terminator::Return(Some(Operand::Literal(Bytes(b"not-called".to_vec())))),
                ),
            ],
            coverage: coverage_for(&[Domain::Invocation, Domain::Return]),
        };
        let mut host = Host::default();
        let evaluation = evaluate(&graph, &[], &receiver(), vec![], &mut host).unwrap();
        assert_eq!(
            evaluation.exits,
            vec![Exit::Normal {
                value: Some("called".to_string()),
                state: vec!["invoke".to_string()],
            }]
        );
    }

    #[test]
    fn state_guard_after_a_join_keeps_both_possible_branches() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![],
                    Terminator::Branch {
                        predicate: Predicate::Host(Bytes(b"unknown-guard".to_vec())),
                        yes: 1,
                        no: 2,
                    },
                ),
                Block::new(
                    vec![
                        Event::Write {
                            storage: Operand::Argument(0),
                            value: Operand::Literal(Bytes(b"changed".to_vec())),
                        },
                        Event::Invoke {
                            callable: Operand::Argument(1),
                            arguments: vec![],
                            result: 0,
                        },
                    ],
                    Terminator::Goto(3),
                ),
                Block::new(
                    vec![Event::Invoke {
                        callable: Operand::Argument(1),
                        arguments: vec![],
                        result: 0,
                    }],
                    Terminator::Goto(3),
                ),
                Block::new(
                    vec![],
                    Terminator::Branch {
                        predicate: Predicate::Host(Bytes(b"state-has-invoke".to_vec())),
                        yes: 4,
                        no: 5,
                    },
                ),
                Block::new(
                    vec![],
                    Terminator::Return(Some(Operand::Literal(Bytes(b"yes".to_vec())))),
                ),
                Block::new(
                    vec![],
                    Terminator::Return(Some(Operand::Literal(Bytes(b"no".to_vec())))),
                ),
            ],
            coverage: coverage_for(&[Domain::Invocation, Domain::UserWrite, Domain::Return]),
        };
        let mut host = Host::default();
        let evaluation = evaluate(
            &graph,
            &["slot".into(), "callback".into()],
            &receiver(),
            vec![],
            &mut host,
        )
        .unwrap();
        assert!(evaluation.exits.contains(&Exit::Normal {
            value: Some("yes".into()),
            state: vec!["<joined>".into()],
        }));
        assert!(evaluation.exits.contains(&Exit::Normal {
            value: Some("no".into()),
            state: vec!["<joined>".into()],
        }));
    }

    #[test]
    fn joining_paths_with_a_partially_defined_result_is_incomplete() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![],
                    Terminator::Branch {
                        predicate: Predicate::Host(Bytes(b"unknown-guard".to_vec())),
                        yes: 1,
                        no: 2,
                    },
                ),
                Block::new(
                    vec![Event::Invoke {
                        callable: Operand::Argument(0),
                        arguments: vec![],
                        result: 9,
                    }],
                    Terminator::Goto(3),
                ),
                Block::new(vec![], Terminator::Goto(3)),
                Block::new(vec![], Terminator::Return(Some(Operand::Result(9)))),
            ],
            coverage: coverage_for(&[Domain::Invocation, Domain::Return]),
        };
        let mut host = Host::default();
        assert_eq!(
            evaluate(&graph, &[], &receiver(), vec![], &mut host),
            Err(GraphError::UnresolvedValue)
        );
    }

    #[test]
    fn unsupported_host_predicate_keeps_both_branches() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![],
                    Terminator::Branch {
                        predicate: Predicate::Host(Bytes(b"unrecognized".to_vec())),
                        yes: 1,
                        no: 2,
                    },
                ),
                Block::new(
                    vec![],
                    Terminator::Return(Some(Operand::Literal(Bytes(b"yes".to_vec())))),
                ),
                Block::new(
                    vec![],
                    Terminator::Return(Some(Operand::Literal(Bytes(b"no".to_vec())))),
                ),
            ],
            coverage: coverage_for(&[Domain::Return]),
        };
        let mut host = Host::default();
        let evaluation = evaluate(&graph, &[], &receiver(), vec![], &mut host).unwrap();
        let mut values: Vec<_> = evaluation
            .exits
            .into_iter()
            .map(|exit| match exit {
                Exit::Normal { value, .. } => value.unwrap(),
                _ => panic!("unexpected non-normal exit"),
            })
            .collect();
        values.sort();
        assert_eq!(values, vec!["no", "yes"]);
    }

    #[test]
    fn open_child_coverage_opens_a_closed_parent_domain() {
        let mut graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 0,
            }],
            Terminator::Return(Some(Operand::Result(0))),
        );
        let returned = graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        returned.coverage = Coverage::Closed;

        let mut host = Host {
            nested_open: Some(Domain::Return),
            ..Host::default()
        };
        let evaluation = evaluate(&graph, &[], &receiver(), vec![], &mut host).unwrap();
        let returned = evaluation
            .coverage
            .iter()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        assert_eq!(
            returned.coverage,
            Coverage::Open {
                reasons: vec![Bytes(b"nested boundary".to_vec())]
            }
        );
        assert_eq!(returned.evidence, vec![Digest([1; 32]), Digest([2; 32])]);
        assert_eq!(returned.assumptions, vec![Digest([3; 32])]);
    }

    #[test]
    fn open_child_dependency_opens_the_parent_result_domain() {
        let mut graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 0,
            }],
            Terminator::Return(Some(Operand::Result(0))),
        );
        let returned = graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        returned.coverage = Coverage::Closed;
        returned.dependencies.push(Domain::UserWrite);
        graph
            .coverage
            .iter_mut()
            .find(|item| item.domain == Domain::UserWrite)
            .unwrap()
            .coverage = Coverage::Closed;
        let mut host = Host {
            nested_open: Some(Domain::UserWrite),
            ..Host::default()
        };

        let evaluation = evaluate(&graph, &[], &receiver(), vec![], &mut host).unwrap();
        let returned = evaluation
            .coverage
            .iter()
            .find(|item| item.domain == Domain::Return)
            .unwrap();
        assert_eq!(
            returned.coverage,
            Coverage::Open {
                reasons: vec![Bytes(b"nested boundary".to_vec())]
            }
        );
        assert!(returned.dependencies.contains(&Domain::UserWrite));
    }

    #[test]
    fn closed_open_and_not_applicable_dependency_pairs_have_explicit_rules() {
        let open = || Coverage::Open {
            reasons: vec![Bytes(b"open".to_vec())],
        };
        let cases = [
            (Coverage::Closed, Coverage::Closed, Ok(())),
            (
                Coverage::Closed,
                open(),
                Err(GraphError::ClosedDomainDependsOnOpen {
                    domain: Domain::Return,
                    dependency: Domain::Diagnostic,
                }),
            ),
            (
                Coverage::Closed,
                Coverage::NotApplicable,
                Err(GraphError::ClosedDomainDependsOnNotApplicable {
                    domain: Domain::Return,
                    dependency: Domain::Diagnostic,
                }),
            ),
            (open(), Coverage::Closed, Ok(())),
            (open(), open(), Ok(())),
            (open(), Coverage::NotApplicable, Ok(())),
            (Coverage::NotApplicable, Coverage::Closed, Ok(())),
            (Coverage::NotApplicable, open(), Ok(())),
            (Coverage::NotApplicable, Coverage::NotApplicable, Ok(())),
        ];
        for (parent, dependency, expected) in cases {
            let coverage = vec![
                dependency_coverage(Domain::Return, parent, vec![Domain::Diagnostic]),
                dependency_coverage(Domain::Diagnostic, dependency, vec![]),
            ];
            assert_eq!(validate_domain_dependencies(&coverage), expected);
        }
    }

    #[test]
    fn closed_dependency_through_not_applicable_is_checked_transitively() {
        let coverage = vec![
            dependency_coverage(Domain::Return, Coverage::Closed, vec![Domain::Diagnostic]),
            dependency_coverage(
                Domain::Diagnostic,
                Coverage::NotApplicable,
                vec![Domain::UserWrite],
            ),
            open_coverage(Domain::UserWrite, b"unmodeled write"),
        ];
        assert_eq!(
            validate_domain_dependencies(&coverage),
            Err(GraphError::ClosedDomainDependsOnOpen {
                domain: Domain::Return,
                dependency: Domain::UserWrite,
            })
        );

        let no_open_leaf = vec![
            dependency_coverage(Domain::Return, Coverage::Closed, vec![Domain::Diagnostic]),
            dependency_coverage(Domain::Diagnostic, Coverage::NotApplicable, vec![]),
        ];
        assert_eq!(
            validate_domain_dependencies(&no_open_leaf),
            Err(GraphError::ClosedDomainDependsOnNotApplicable {
                domain: Domain::Return,
                dependency: Domain::Diagnostic,
            })
        );
    }

    #[test]
    fn open_dependency_propagates_through_not_applicable_intermediate() {
        let mut coverage = vec![
            dependency_coverage(Domain::Return, Coverage::Closed, vec![Domain::Diagnostic]),
            dependency_coverage(
                Domain::Diagnostic,
                Coverage::NotApplicable,
                vec![Domain::UserWrite],
            ),
            open_coverage(Domain::UserWrite, b"unmodeled write"),
        ];
        propagate_open_dependencies(&mut coverage);
        assert_eq!(
            coverage
                .iter()
                .find(|item| item.domain == Domain::Return)
                .unwrap()
                .coverage,
            Coverage::Open {
                reasons: vec![Bytes(b"unmodeled write".to_vec())]
            }
        );
        assert_eq!(validate_domain_dependencies(&coverage), Ok(()));

        let mut composed = coverage_for(&[Domain::Return]);
        composed
            .iter_mut()
            .find(|item| item.domain == Domain::Return)
            .unwrap()
            .dependencies
            .push(Domain::Diagnostic);
        composed
            .iter_mut()
            .find(|item| item.domain == Domain::Diagnostic)
            .unwrap()
            .dependencies
            .push(Domain::UserWrite);
        let mut child = coverage_for(&[]);
        child
            .iter_mut()
            .find(|item| item.domain == Domain::UserWrite)
            .unwrap()
            .coverage = Coverage::Open {
            reasons: vec![Bytes(b"child write unknown".to_vec())],
        };
        merge_coverage(&mut composed, &child).unwrap();
        assert_eq!(
            composed
                .iter()
                .find(|item| item.domain == Domain::Return)
                .unwrap()
                .coverage,
            Coverage::Open {
                reasons: vec![Bytes(b"child write unknown".to_vec())]
            }
        );
        assert_eq!(
            composed
                .iter()
                .find(|item| item.domain == Domain::Diagnostic)
                .unwrap()
                .coverage,
            Coverage::NotApplicable
        );
    }

    #[test]
    fn dependency_cycles_through_open_and_not_applicable_domains_are_rejected() {
        let coverage = vec![
            dependency_coverage(
                Domain::Return,
                Coverage::Open {
                    reasons: vec![Bytes(b"open".to_vec())],
                },
                vec![Domain::Diagnostic],
            ),
            dependency_coverage(
                Domain::Diagnostic,
                Coverage::NotApplicable,
                vec![Domain::Return],
            ),
        ];
        assert_eq!(
            validate_domain_dependencies(&coverage),
            Err(GraphError::CyclicDomainDependency(Domain::Return))
        );
    }

    #[test]
    fn incomplete_child_coverage_rejects_the_evaluation() {
        let graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 0,
            }],
            Terminator::Return(Some(Operand::Result(0))),
        );
        let mut host = Host {
            incomplete_nested_coverage: true,
            ..Host::default()
        };
        assert_eq!(
            evaluate(&graph, &[], &receiver(), vec![], &mut host),
            Err(GraphError::InvalidComposedCoverage)
        );
    }

    #[test]
    fn omitted_child_coverage_rejects_the_evaluation() {
        let graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 0,
            }],
            Terminator::Return(Some(Operand::Result(0))),
        );
        let mut host = Host {
            omit_nested_coverage: true,
            ..Host::default()
        };
        assert_eq!(
            evaluate(&graph, &[], &receiver(), vec![], &mut host),
            Err(GraphError::InvalidComposedCoverage)
        );
    }

    #[test]
    fn invoked_child_cannot_mark_invocation_not_applicable() {
        let graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 0,
            }],
            Terminator::Return(Some(Operand::Result(0))),
        );
        let mut host = Host {
            invocation_not_applicable: true,
            ..Host::default()
        };
        assert_eq!(
            evaluate(&graph, &[], &receiver(), vec![], &mut host),
            Err(GraphError::InvalidComposedCoverage)
        );
    }

    #[test]
    fn callback_throw_preserves_prior_write_and_skips_later_events() {
        let graph = graph(
            vec![
                Event::Write {
                    storage: Operand::Argument(0),
                    value: Operand::Literal(Bytes(b"changed".to_vec())),
                },
                Event::Invoke {
                    callable: Operand::Argument(1),
                    arguments: vec![],
                    result: 4,
                },
                Event::Write {
                    storage: Operand::Argument(0),
                    value: Operand::Literal(Bytes(b"too-late".to_vec())),
                },
            ],
            Terminator::Return(None),
        );
        let mut host = Host {
            throw_invoke: true,
            ..Host::default()
        };
        let exits = evaluate(
            &graph,
            &["slot".into(), "callback".into()],
            &receiver(),
            vec![],
            &mut host,
        )
        .unwrap()
        .exits;
        assert_eq!(
            exits,
            vec![Exit::PhpThrow {
                value: "exception".into(),
                state: vec!["write".into(), "invoke".into()]
            }]
        );
    }

    #[test]
    fn pending_callback_throw_runs_cleanup_then_resumes() {
        let mut g = graph(
            vec![
                Event::Write {
                    storage: Operand::Argument(0),
                    value: Operand::Literal(Bytes(b"before".to_vec())),
                },
                Event::Invoke {
                    callable: Operand::Argument(1),
                    arguments: vec![],
                    result: 4,
                },
                Event::Write {
                    storage: Operand::Argument(0),
                    value: Operand::Literal(Bytes(b"after".to_vec())),
                },
            ],
            Terminator::Return(None),
        );
        g.blocks[0].exceptional.php_throw = Some(1);
        g.blocks.push(Block::new(
            vec![Event::Write {
                storage: Operand::Argument(0),
                value: Operand::Literal(Bytes(b"cleanup".to_vec())),
            }],
            Terminator::Resume,
        ));
        let mut host = Host {
            throw_invoke: true,
            ..Host::default()
        };
        let exits = evaluate(
            &g,
            &["slot".into(), "callback".into()],
            &receiver(),
            vec![],
            &mut host,
        )
        .unwrap()
        .exits;
        assert_eq!(
            exits,
            vec![Exit::PhpThrow {
                value: "exception".into(),
                state: vec!["write".into(), "invoke".into(), "write".into()],
            }]
        );
    }

    #[test]
    fn cleanup_return_overrides_pending_throw() {
        let mut g = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 1,
            }],
            Terminator::Return(None),
        );
        g.blocks[0].exceptional.php_throw = Some(1);
        g.blocks.push(Block::new(
            vec![],
            Terminator::Return(Some(Operand::Literal(Bytes(b"cleanup-return".to_vec())))),
        ));
        let mut host = Host {
            throw_invoke: true,
            ..Host::default()
        };
        let exits = evaluate(&g, &["callback".into()], &receiver(), vec![], &mut host)
            .unwrap()
            .exits;
        assert_eq!(
            exits,
            vec![Exit::Normal {
                value: Some("cleanup-return".into()),
                state: vec!["invoke".into()],
            }]
        );
    }

    #[test]
    fn cleanup_replacement_matrix_allows_evidenced_php_throw_replacement_only() {
        let pending = [
            PendingOutcome::PhpThrow("throw".to_owned()),
            PendingOutcome::NativeUnwind("unwind".to_owned()),
            PendingOutcome::Fatal("fatal".to_owned()),
            PendingOutcome::Suspended("suspended".to_owned()),
        ];
        let replacements = [
            CompletionKind::Normal,
            CompletionKind::PhpThrow,
            CompletionKind::NativeUnwind,
            CompletionKind::Fatal,
            CompletionKind::Suspended,
        ];
        for (pending_index, pending) in pending.iter().enumerate() {
            for (replacement_index, replacement) in replacements.iter().enumerate() {
                let result = check_cleanup_replacement(&Some(pending.clone()), *replacement);
                let supported = pending_index == 0 && matches!(replacement_index, 0 | 1);
                assert_eq!(
                    result.is_ok(),
                    supported,
                    "pending={pending:?}, replacement={replacement:?}: {result:?}"
                );
                if !supported {
                    assert_eq!(
                        result,
                        Err(GraphError::UnsupportedCleanupReplacement {
                            pending: pending_completion_kind(pending),
                            replacement: *replacement,
                        })
                    );
                }
            }
        }
        assert_eq!(
            check_cleanup_replacement::<String>(&None, CompletionKind::Normal),
            Ok(())
        );
        assert_eq!(
            check_cleanup_replacement::<String>(&None, CompletionKind::PhpThrow),
            Ok(())
        );
    }

    #[test]
    fn evaluator_enforces_every_pending_completion_replacement_cell() {
        let pending_kinds = [
            CompletionKind::PhpThrow,
            CompletionKind::NativeUnwind,
            CompletionKind::Fatal,
            CompletionKind::Suspended,
        ];
        let replacement_kinds = [
            CompletionKind::Normal,
            CompletionKind::PhpThrow,
            CompletionKind::NativeUnwind,
            CompletionKind::Fatal,
            CompletionKind::Suspended,
        ];
        for pending_kind in pending_kinds {
            for replacement_kind in replacement_kinds {
                let mut graph = if pending_kind == CompletionKind::NativeUnwind
                    || (pending_kind == CompletionKind::PhpThrow
                        && replacement_kind != CompletionKind::NativeUnwind)
                {
                    graph(
                        vec![Event::Invoke {
                            callable: Operand::Argument(0),
                            arguments: vec![],
                            result: 1,
                        }],
                        Terminator::Return(None),
                    )
                } else if pending_kind == CompletionKind::PhpThrow {
                    graph(
                        vec![],
                        Terminator::Throw(Operand::Literal(Bytes(b"initial throw".to_vec()))),
                    )
                } else {
                    let terminator = match pending_kind {
                        CompletionKind::Fatal => {
                            Terminator::Fatal(Operand::Literal(Bytes(b"fatal".to_vec())))
                        }
                        CompletionKind::Suspended => {
                            Terminator::Suspend(Operand::Literal(Bytes(b"suspended".to_vec())))
                        }
                        _ => unreachable!(),
                    };
                    graph(vec![], terminator)
                };
                match pending_kind {
                    CompletionKind::PhpThrow => graph.blocks[0].exceptional.php_throw = Some(1),
                    CompletionKind::NativeUnwind => {
                        graph.blocks[0].exceptional.native_unwind = Some(1)
                    }
                    CompletionKind::Fatal => graph.blocks[0].exceptional.fatal = Some(1),
                    CompletionKind::Suspended => graph.blocks[0].exceptional.suspended = Some(1),
                    CompletionKind::Normal => unreachable!(),
                }
                let replacement = match replacement_kind {
                    CompletionKind::Normal => {
                        Terminator::Return(Some(Operand::Literal(Bytes(b"return".to_vec()))))
                    }
                    CompletionKind::PhpThrow => {
                        Terminator::Throw(Operand::Literal(Bytes(b"throw".to_vec())))
                    }
                    CompletionKind::NativeUnwind => Terminator::Resume,
                    CompletionKind::Fatal => {
                        Terminator::Fatal(Operand::Literal(Bytes(b"fatal".to_vec())))
                    }
                    CompletionKind::Suspended => {
                        Terminator::Suspend(Operand::Literal(Bytes(b"suspended".to_vec())))
                    }
                };
                let replacement_events = if replacement_kind == CompletionKind::NativeUnwind {
                    vec![Event::Invoke {
                        callable: Operand::Argument(1),
                        arguments: vec![],
                        result: 2,
                    }]
                } else {
                    vec![]
                };
                graph
                    .blocks
                    .push(Block::new(replacement_events, replacement));
                close_unmodeled_domains(&mut graph);
                let mut host = Host {
                    throw_invoke: pending_kind == CompletionKind::PhpThrow,
                    native_unwind_invoke: pending_kind == CompletionKind::NativeUnwind
                        || replacement_kind == CompletionKind::NativeUnwind,
                    ..Host::default()
                };
                let arguments = if pending_kind == CompletionKind::NativeUnwind
                    || (pending_kind == CompletionKind::PhpThrow
                        && replacement_kind != CompletionKind::NativeUnwind)
                {
                    vec!["callback".into()]
                } else {
                    vec!["cleanup".into()]
                };
                let arguments = if replacement_kind == CompletionKind::NativeUnwind
                    && pending_kind != CompletionKind::NativeUnwind
                {
                    vec!["cleanup".into()]
                } else if pending_kind == CompletionKind::NativeUnwind {
                    vec!["callback".into(), "cleanup".into()]
                } else {
                    arguments
                };
                let result = evaluate(&graph, &arguments, &receiver(), vec![], &mut host);
                let supported = pending_kind == CompletionKind::PhpThrow
                    && matches!(
                        replacement_kind,
                        CompletionKind::Normal | CompletionKind::PhpThrow
                    );
                assert_eq!(
                    result.is_ok(),
                    supported,
                    "pending={pending_kind:?}, replacement={replacement_kind:?}: {result:?}"
                );
                if !supported {
                    assert_eq!(
                        result.unwrap_err(),
                        GraphError::UnsupportedCleanupReplacement {
                            pending: pending_kind,
                            replacement: replacement_kind,
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn evaluator_rejects_native_unwind_replacement_during_cleanup() {
        let mut graph = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 1,
            }],
            Terminator::Return(None),
        );
        graph.blocks[0].exceptional.native_unwind = Some(1);
        let cleanup = Block::new(
            vec![Event::Invoke {
                callable: Operand::Argument(1),
                arguments: vec![],
                result: 2,
            }],
            Terminator::Resume,
        );
        graph.blocks.push(cleanup);
        close_unmodeled_domains(&mut graph);

        let mut host = Host {
            native_unwind_invoke: true,
            ..Host::default()
        };
        let result = evaluate(
            &graph,
            &["initial".into(), "cleanup".into()],
            &receiver(),
            vec![],
            &mut host,
        );
        assert_eq!(
            result,
            Err(GraphError::UnsupportedCleanupReplacement {
                pending: CompletionKind::NativeUnwind,
                replacement: CompletionKind::NativeUnwind,
            })
        );
    }

    #[test]
    fn certificate_replay_rejects_unsupported_cleanup_replacement() {
        let mut graph = graph(
            vec![],
            Terminator::Fatal(Operand::Literal(Bytes(b"fatal".to_vec()))),
        );
        graph.blocks[0].exceptional.fatal = Some(1);
        graph.blocks.push(Block::new(
            vec![],
            Terminator::Return(Some(Operand::Literal(Bytes(b"cleanup".to_vec())))),
        ));
        for item in &mut graph.coverage {
            if matches!(item.coverage, Coverage::NotApplicable) {
                item.coverage = Coverage::Closed;
            }
        }
        let host = Host::default();
        let initial = Frame {
            block: 0,
            state: vec![],
            results: vec![],
            pending: None,
            growing_updates: 0,
        };
        let cleanup = Frame {
            block: 1,
            state: vec![],
            results: vec![],
            pending: Some(PendingOutcome::Fatal("fatal".into())),
            growing_updates: 0,
        };
        let certificate = test_certificate(&graph, vec![initial, cleanup], vec![]);
        assert_eq!(
            certificate.validate(&graph, &host),
            Err(GraphError::UnsupportedCleanupReplacement {
                pending: CompletionKind::Fatal,
                replacement: CompletionKind::Normal,
            })
        );
    }

    #[test]
    fn cleanup_resume_preserves_every_pending_completion_kind() {
        for pending_kind in [
            CompletionKind::PhpThrow,
            CompletionKind::NativeUnwind,
            CompletionKind::Fatal,
            CompletionKind::Suspended,
        ] {
            let mut graph = match pending_kind {
                CompletionKind::PhpThrow | CompletionKind::NativeUnwind => graph(
                    vec![Event::Invoke {
                        callable: Operand::Argument(0),
                        arguments: vec![],
                        result: 1,
                    }],
                    Terminator::Return(None),
                ),
                CompletionKind::Fatal => graph(
                    vec![],
                    Terminator::Fatal(Operand::Literal(Bytes(b"fatal".to_vec()))),
                ),
                CompletionKind::Suspended => graph(
                    vec![],
                    Terminator::Suspend(Operand::Literal(Bytes(b"suspended".to_vec()))),
                ),
                CompletionKind::Normal => unreachable!(),
            };
            match pending_kind {
                CompletionKind::PhpThrow => graph.blocks[0].exceptional.php_throw = Some(1),
                CompletionKind::NativeUnwind => graph.blocks[0].exceptional.native_unwind = Some(1),
                CompletionKind::Fatal => graph.blocks[0].exceptional.fatal = Some(1),
                CompletionKind::Suspended => graph.blocks[0].exceptional.suspended = Some(1),
                CompletionKind::Normal => unreachable!(),
            }
            graph.blocks.push(Block::new(vec![], Terminator::Resume));
            close_unmodeled_domains(&mut graph);
            let mut host = Host {
                throw_invoke: pending_kind == CompletionKind::PhpThrow,
                native_unwind_invoke: pending_kind == CompletionKind::NativeUnwind,
                ..Host::default()
            };
            let arguments = if matches!(
                pending_kind,
                CompletionKind::PhpThrow | CompletionKind::NativeUnwind
            ) {
                vec!["callback".into()]
            } else {
                vec![]
            };
            let exits = evaluate(&graph, &arguments, &receiver(), vec![], &mut host)
                .unwrap()
                .exits;
            assert_eq!(exits.len(), 1, "pending={pending_kind:?}: {exits:?}");
            assert!(
                matches!(
                    (&exits[0], pending_kind),
                    (Exit::PhpThrow { .. }, CompletionKind::PhpThrow)
                        | (Exit::NativeUnwind { .. }, CompletionKind::NativeUnwind)
                        | (Exit::Fatal { .. }, CompletionKind::Fatal)
                        | (Exit::Suspended { .. }, CompletionKind::Suspended)
                ),
                "resume changed pending={pending_kind:?}: {exits:?}"
            );
        }
    }

    #[test]
    fn pending_throw_survives_cleanup_branch_join_and_resume() {
        let mut g = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 1,
            }],
            Terminator::Return(None),
        );
        g.blocks[0].exceptional.php_throw = Some(1);
        g.blocks.push(Block::new(
            vec![],
            Terminator::Branch {
                predicate: Predicate::Always,
                yes: 2,
                no: 3,
            },
        ));
        g.blocks.push(Block::new(
            vec![Event::Write {
                storage: Operand::Argument(1),
                value: Operand::Literal(Bytes(b"left".to_vec())),
            }],
            Terminator::Goto(4),
        ));
        g.blocks.push(Block::new(
            vec![Event::Read {
                storage: Operand::Argument(1),
            }],
            Terminator::Goto(4),
        ));
        g.blocks.push(Block::new(vec![], Terminator::Resume));
        close_unmodeled_domains(&mut g);
        let mut host = Host {
            throw_invoke: true,
            maybe_always: true,
            ..Host::default()
        };
        let exits = evaluate(
            &g,
            &["callback".into(), "slot".into()],
            &receiver(),
            vec![],
            &mut host,
        )
        .unwrap()
        .exits;
        assert!(
            exits.iter().all(|exit| matches!(
                exit,
                Exit::PhpThrow { value, .. } if value == "exception"
            )),
            "branch/join must preserve the pending throw: {exits:?}"
        );
        assert!(
            exits
                .iter()
                .any(|exit| matches!(exit, Exit::PhpThrow { state, .. } if state == &["<joined>".to_owned()])),
            "both branch states must reach the join: {exits:?}"
        );
    }

    #[test]
    fn native_unwind_remains_distinct_from_php_throw() {
        let g = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 1,
            }],
            Terminator::Return(None),
        );
        let mut host = Host {
            native_unwind_invoke: true,
            ..Host::default()
        };
        let exits = evaluate(&g, &["native".into()], &receiver(), vec![], &mut host)
            .unwrap()
            .exits;
        assert_eq!(
            exits,
            vec![Exit::NativeUnwind {
                value: "native exception".into(),
                state: vec!["invoke".into()],
            }]
        );
    }

    #[test]
    fn callback_fatal_and_suspension_remain_distinct_open_exits() {
        for (fatal, expected, domain) in [
            (true, "fatal callback", Domain::Fatal),
            (false, "suspended callback", Domain::Suspension),
        ] {
            let graph = graph(
                vec![Event::Invoke {
                    callable: Operand::Argument(0),
                    arguments: vec![],
                    result: 1,
                }],
                Terminator::Return(None),
            );
            let mut host = Host {
                fatal_invoke: fatal,
                suspended_invoke: !fatal,
                ..Host::default()
            };
            let evaluation = evaluate(&graph, &["callback".into()], &receiver(), vec![], &mut host)
                .expect("abrupt callback outcome evaluates");
            assert_eq!(evaluation.exits.len(), 1);
            assert!(
                matches!(
                    (&evaluation.exits[0], domain),
                    (Exit::Fatal { value, state }, Domain::Fatal)
                        if value == expected && state == &["invoke".to_owned()]
                ) || matches!(
                    (&evaluation.exits[0], domain),
                    (Exit::Suspended { value, state }, Domain::Suspension)
                        if value == expected && state == &["invoke".to_owned()]
                )
            );
            assert!(evaluation.coverage.iter().any(|coverage| {
                coverage.domain == domain && matches!(coverage.coverage, Coverage::Open { .. })
            }));
            evaluation
                .post_fixpoint
                .validate(&graph, &host)
                .expect("certificate preserves the distinct abrupt outcome");
        }
    }

    #[test]
    fn callback_normal_and_throw_paths_keep_separate_results_and_states() {
        let graph = graph(
            vec![
                Event::Invoke {
                    callable: Operand::Argument(0),
                    arguments: vec![],
                    result: 9,
                },
                Event::Write {
                    storage: Operand::Argument(1),
                    value: Operand::Literal(Bytes(b"after".to_vec())),
                },
            ],
            Terminator::Return(Some(Operand::Result(9))),
        );
        let mut host = Host {
            both_invoke_outcomes: true,
            ..Host::default()
        };
        let exits = evaluate(
            &graph,
            &["callback".into(), "slot".into()],
            &receiver(),
            vec![],
            &mut host,
        )
        .unwrap()
        .exits;
        assert_eq!(exits.len(), 2);
        assert!(exits.contains(&Exit::Normal {
            value: Some("callback-result".into()),
            state: vec!["invoke".into(), "write".into()]
        }));
        assert!(exits.contains(&Exit::PhpThrow {
            value: "exception".into(),
            state: vec!["invoke".into()]
        }));
    }

    #[test]
    fn unknown_transfer_retains_its_domain_mask() {
        let domains = vec![Domain::UserWrite, Domain::Return];
        let mut graph = graph(
            vec![Event::Unknown {
                domains: domains.clone(),
                reason: Bytes(b"opaque call".to_vec()),
            }],
            Terminator::Return(None),
        );
        let index = graph
            .coverage
            .iter()
            .position(|c| c.domain == Domain::UserWrite)
            .unwrap();
        graph.coverage[index] = open_coverage(Domain::UserWrite, b"opaque call");
        let index = graph
            .coverage
            .iter()
            .position(|c| c.domain == Domain::Return)
            .unwrap();
        graph.coverage[index] = open_coverage(Domain::Return, b"opaque call");
        let mut host = Host::default();
        let evaluation = evaluate(&graph, &[], &receiver(), vec![], &mut host).unwrap();
        assert_eq!(evaluation.coverage, graph.coverage);
        assert_eq!(
            evaluation.exits,
            vec![Exit::Normal {
                value: None,
                state: vec![format!("unknown:{domains:?}")]
            }]
        );
    }

    #[test]
    fn reachable_return_cannot_be_marked_not_applicable() {
        let mut graph = graph(vec![], Terminator::Return(None));
        let index = graph
            .coverage
            .iter()
            .position(|item| item.domain == Domain::Return)
            .unwrap();
        graph.coverage[index].coverage = Coverage::NotApplicable;
        assert_eq!(
            graph.validate(),
            Err(GraphError::ApplicableDomainMarkedNotApplicable(
                Domain::Return
            ))
        );
    }

    #[test]
    fn fatal_and_suspension_remain_distinct_exits() {
        for (terminator, expected) in [
            (
                Terminator::Fatal(Operand::Literal(Bytes(b"fatal".to_vec()))),
                Exit::Fatal {
                    value: "fatal".into(),
                    state: vec![],
                },
            ),
            (
                Terminator::Suspend(Operand::Literal(Bytes(b"yield".to_vec()))),
                Exit::Suspended {
                    value: "yield".into(),
                    state: vec![],
                },
            ),
        ] {
            let graph = graph(vec![], terminator);
            let mut host = Host::default();
            let evaluation = evaluate(&graph, &[], &receiver(), vec![], &mut host).unwrap();
            assert_eq!(evaluation.exits, vec![expected]);
        }
    }

    #[test]
    fn cyclic_graph_reaches_a_stable_joined_exit() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Write {
                        storage: Operand::Argument(0),
                        value: Operand::Literal(Bytes(b"loop".to_vec())),
                    }],
                    Terminator::Branch {
                        predicate: Predicate::Always,
                        yes: 0,
                        no: 1,
                    },
                ),
                Block::new(vec![], Terminator::Return(None)),
            ],
            coverage: coverage_for(&[Domain::UserWrite, Domain::Return]),
        };
        let mut host = Host {
            maybe_always: true,
            ..Host::default()
        };
        let result = evaluate(&graph, &["x".into()], &receiver(), vec![], &mut host).unwrap();
        assert!(
            result
                .exits
                .iter()
                .any(|exit| matches!(exit, Exit::Normal { .. }))
        );
        assert!(result.exits.len() <= 2);
        assert!(result.coverage.iter().all(|item| {
            matches!(item.coverage, Coverage::NotApplicable) || item.precision != Precision::Exact
        }));
        for domain in [Domain::UserWrite, Domain::Return] {
            assert_eq!(
                result
                    .coverage
                    .iter()
                    .find(|item| item.domain == domain)
                    .unwrap()
                    .precision,
                Precision::Joined
            );
        }
    }

    #[test]
    fn widening_preserves_the_transferred_lower_bound_and_reflects_precision() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Write {
                        storage: Operand::Argument(0),
                        value: Operand::Literal(Bytes(b"loop".to_vec())),
                    }],
                    Terminator::Branch {
                        predicate: Predicate::Always,
                        yes: 0,
                        no: 1,
                    },
                ),
                Block::new(vec![], Terminator::Return(None)),
            ],
            coverage: coverage_for(&[Domain::UserWrite, Domain::Return]),
        };
        let mut host = Host {
            maybe_always: true,
            growing_loop: true,
            ..Host::default()
        };
        // The entry interval [0,1] transfers to [1,2]. Widening drops the
        // upper bound, so this asserts only that the transferred lower bound
        // remains 1; it does not test upper-bound preservation.
        let initial = encode_count_interval(0, Some(1));
        let result = evaluate(&graph, &[], &receiver(), initial.clone(), &mut host).unwrap();
        assert!(
            result
                .exits
                .iter()
                .any(|exit| matches!(exit, Exit::Normal { .. }))
        );
        assert!(result.exits.iter().any(|exit| match exit {
            Exit::Normal { state, .. } => count_interval(state) == (1, None),
            _ => false,
        }));
        for domain in [Domain::UserWrite, Domain::Return] {
            assert_eq!(
                result
                    .coverage
                    .iter()
                    .find(|item| item.domain == domain)
                    .unwrap()
                    .precision,
                Precision::Widened
            );
        }
    }

    #[test]
    fn evaluator_rejects_a_join_that_drops_a_reachable_state() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Write {
                        storage: Operand::Argument(0),
                        value: Operand::Literal(Bytes(b"loop".to_vec())),
                    }],
                    Terminator::Branch {
                        predicate: Predicate::Always,
                        yes: 0,
                        no: 1,
                    },
                ),
                Block::new(vec![], Terminator::Return(None)),
            ],
            coverage: coverage_for(&[Domain::UserWrite, Domain::Return]),
        };
        let mut host = Host {
            maybe_always: true,
            bad_join: true,
            ..Host::default()
        };
        assert_eq!(
            evaluate(&graph, &[], &receiver(), vec![], &mut host),
            Err(GraphError::NonMonotoneJoin(0))
        );
    }

    #[test]
    fn evaluator_rejects_a_value_join_that_drops_a_reachable_result() {
        let host = Host {
            bad_value_join: true,
            ..Host::default()
        };
        let old = Frame {
            block: 0,
            state: vec![],
            results: vec![(7, "old result".into())],
            pending: None,
            growing_updates: 0,
        };
        let incoming = Frame {
            block: 0,
            state: vec![],
            results: vec![(7, "new result".into())],
            pending: None,
            growing_updates: 0,
        };
        let mut records = vec![old];
        let mut by_block = vec![vec![0]];
        let mut queue = vec![];
        let mut precision = Precision::Exact;
        assert_eq!(
            enqueue(
                &host,
                incoming,
                &mut records,
                &mut by_block,
                &mut queue,
                &mut precision,
                &mut Vec::new(),
            ),
            Err(GraphError::NonMonotoneJoin(0))
        );
    }

    #[test]
    fn post_fixpoint_validator_rejects_a_dropped_transfer_candidate() {
        let host = Host::default();
        let graph = graph(vec![], Terminator::Return(None));
        let candidate = Frame {
            block: 0,
            state: vec!["write".into()],
            results: vec![],
            pending: None,
            growing_updates: 0,
        };
        let incomplete_solution = Frame {
            block: 0,
            state: vec![],
            results: vec![],
            pending: None,
            growing_updates: 0,
        };
        assert_eq!(
            test_certificate(&graph, vec![incomplete_solution], vec![candidate])
                .validate(&graph, &host),
            Err(GraphError::NonPostFixpoint(0))
        );
    }

    #[test]
    fn published_post_fixpoint_certificate_is_revalidatable_and_graph_bound() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Write {
                        storage: Operand::Argument(0),
                        value: Operand::Literal(Bytes(b"write".to_vec())),
                    }],
                    Terminator::Goto(1),
                ),
                Block::new(vec![], Terminator::Return(None)),
            ],
            coverage: coverage_for(&[Domain::UserWrite, Domain::Return]),
        };
        let mut host = Host::default();
        let initial = vec!["entry-state".to_string()];
        let result = evaluate(&graph, &[], &receiver(), initial.clone(), &mut host).unwrap();
        assert!(
            result
                .post_fixpoint
                .solution
                .iter()
                .any(|frame| frame.block == graph.entry && frame.state == initial)
        );
        assert_eq!(
            result.exits,
            vec![Exit::Normal {
                value: None,
                state: vec!["entry-state".into(), "write".into()],
            }]
        );
        result.post_fixpoint.validate(&graph, &host).unwrap();

        let encoded = serde_json::to_vec(&result.post_fixpoint).unwrap();
        let persisted: PostFixpointCertificate<String, Vec<String>> =
            serde_json::from_slice(&encoded).unwrap();
        persisted.validate(&graph, &host).unwrap();

        let mut omitted = persisted.clone();
        omitted.solution.retain(|frame| frame.block != 1);
        omitted.candidates.retain(|frame| frame.block != 1);
        assert_eq!(
            omitted.validate(&graph, &host),
            Err(GraphError::NonPostFixpoint(1))
        );

        let mut missing_exit = persisted.clone();
        missing_exit.exits.clear();
        assert_eq!(
            missing_exit.validate(&graph, &host),
            Err(GraphError::NonPostFixpoint(1))
        );

        let mut incomplete = result.post_fixpoint.clone();
        incomplete
            .solution
            .iter_mut()
            .find(|frame| frame.block == 1)
            .unwrap()
            .state
            .clear();
        assert_eq!(
            incomplete.validate(&graph, &host),
            Err(GraphError::NonPostFixpoint(1))
        );

        let mut changed_graph = graph.clone();
        changed_graph.blocks[0].events.clear();
        assert_eq!(
            result.post_fixpoint.validate(&changed_graph, &host),
            Err(GraphError::CertificateGraphMismatch)
        );
    }

    #[test]
    fn post_fixpoint_validator_rejects_a_dropped_result_or_pending_throw() {
        let host = Host::default();
        let graph = graph(vec![], Terminator::Return(None));
        let empty_solution = Frame {
            block: 0,
            state: vec![],
            results: vec![],
            pending: None,
            growing_updates: 0,
        };
        let result_candidate = Frame {
            block: 0,
            state: vec![],
            results: vec![(3, "callback result".into())],
            pending: None,
            growing_updates: 0,
        };
        assert_eq!(
            test_certificate(&graph, vec![empty_solution.clone()], vec![result_candidate])
                .validate(&graph, &host),
            Err(GraphError::NonPostFixpoint(0))
        );
        let throw_candidate = Frame {
            block: 0,
            state: vec![],
            results: vec![],
            pending: Some(PendingOutcome::PhpThrow("exception".into())),
            growing_updates: 0,
        };
        assert_eq!(
            test_certificate(&graph, vec![empty_solution], vec![throw_candidate])
                .validate(&graph, &host),
            Err(GraphError::NonPostFixpoint(0))
        );
    }

    #[test]
    fn evaluator_rejects_a_widening_that_drops_a_reachable_state() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![
                Block::new(
                    vec![Event::Write {
                        storage: Operand::Argument(0),
                        value: Operand::Literal(Bytes(b"loop".to_vec())),
                    }],
                    Terminator::Branch {
                        predicate: Predicate::Always,
                        yes: 0,
                        no: 1,
                    },
                ),
                Block::new(vec![], Terminator::Return(None)),
            ],
            coverage: coverage_for(&[Domain::UserWrite, Domain::Return]),
        };
        let mut host = Host {
            maybe_always: true,
            growing_loop: true,
            bad_widening: true,
            ..Host::default()
        };
        assert_eq!(
            evaluate(&graph, &[], &receiver(), vec![], &mut host),
            Err(GraphError::NonMonotoneJoin(0))
        );
    }

    #[test]
    fn graph_identity_is_stable_and_order_sensitive() {
        let first = graph(
            vec![Event::Read {
                storage: Operand::Argument(0),
            }],
            Terminator::Return(None),
        );
        let second = graph(
            vec![Event::Write {
                storage: Operand::Argument(0),
                value: Operand::Literal(Bytes(b"v".to_vec())),
            }],
            Terminator::Return(None),
        );
        assert_eq!(
            first.canonical_digest().unwrap(),
            first.canonical_digest().unwrap()
        );
        assert_ne!(
            first.canonical_digest().unwrap(),
            second.canonical_digest().unwrap()
        );
    }

    #[test]
    fn graph_rejects_noncanonical_unordered_vectors_but_keeps_event_order() {
        let mut permuted = graph(vec![], Terminator::Return(None));
        permuted.coverage.swap(0, 1);
        let canonical = graph(vec![], Terminator::Return(None));
        assert_eq!(
            permuted.canonical_digest().unwrap(),
            canonical.canonical_digest().unwrap()
        );
        assert_eq!(
            permuted.validate(),
            Err(GraphError::NonCanonicalOrder("graph coverage"))
        );

        let mut permuted = graph(vec![], Terminator::Return(None));
        permuted.coverage[0].evidence = vec![Digest([8; 32]), Digest([7; 32])];
        let mut canonical_evidence = permuted.clone();
        canonical_evidence.coverage[0].evidence.reverse();
        assert_eq!(
            permuted.canonical_digest().unwrap(),
            canonical_evidence.canonical_digest().unwrap()
        );
        assert_eq!(
            permuted.validate(),
            Err(GraphError::NonCanonicalOrder("coverage evidence"))
        );

        let mut permuted = graph(vec![], Terminator::Return(None));
        permuted.coverage[0].assumptions = vec![Digest([8; 32]), Digest([7; 32])];
        let mut canonical_assumptions = permuted.clone();
        canonical_assumptions.coverage[0].assumptions.reverse();
        assert_eq!(
            permuted.canonical_digest().unwrap(),
            canonical_assumptions.canonical_digest().unwrap()
        );
        assert_eq!(
            permuted.validate(),
            Err(GraphError::NonCanonicalOrder("coverage assumptions"))
        );

        let mut permuted = graph(vec![], Terminator::Return(None));
        permuted.coverage[0].dependencies = vec![Domain::UserWrite, Domain::Invocation];
        assert_eq!(
            permuted.validate(),
            Err(GraphError::NonCanonicalOrder("domain dependencies"))
        );

        let mut permuted = graph(vec![], Terminator::Return(None));
        permuted.coverage[0].coverage = Coverage::Open {
            reasons: vec![Bytes(b"zeta".to_vec()), Bytes(b"alpha".to_vec())],
        };
        assert_eq!(
            permuted.validate(),
            Err(GraphError::NonCanonicalOrder("open coverage reasons"))
        );

        let permuted = graph(
            vec![Event::Unknown {
                domains: vec![Domain::UserWrite, Domain::Invocation],
                reason: Bytes(b"unknown".to_vec()),
            }],
            Terminator::Return(None),
        );
        assert_eq!(
            permuted.validate(),
            Err(GraphError::NonCanonicalOrder("unknown event domains"))
        );

        let first = graph(
            vec![
                Event::Read {
                    storage: Operand::Argument(0),
                },
                Event::Write {
                    storage: Operand::Argument(1),
                    value: Operand::Unknown,
                },
            ],
            Terminator::Return(None),
        );
        let second = graph(
            vec![
                Event::Write {
                    storage: Operand::Argument(1),
                    value: Operand::Unknown,
                },
                Event::Read {
                    storage: Operand::Argument(0),
                },
            ],
            Terminator::Return(None),
        );
        assert!(first.validate().is_ok());
        assert!(second.validate().is_ok());
        assert_ne!(
            first.canonical_digest().unwrap(),
            second.canonical_digest().unwrap()
        );
    }

    #[test]
    fn arbitrary_bytes_round_trip_as_explicit_base64_json_and_binary() {
        let bytes = Bytes(vec![0, 0xff, b'/', b'\n']);
        let json = serde_json::to_value(&bytes).unwrap();
        assert_eq!(json, serde_json::json!({"base64": "AP8vCg=="}));
        assert_eq!(serde_json::from_value::<Bytes>(json).unwrap(), bytes);
        let postcard = postcard::to_allocvec(&bytes).unwrap();
        assert_eq!(postcard::from_bytes::<Bytes>(&postcard).unwrap(), bytes);
    }

    #[test]
    fn open_domain_requires_an_executable_unknown_transfer() {
        let mut g = graph(vec![], Terminator::Return(None));
        let index = g
            .coverage
            .iter()
            .position(|c| c.domain == Domain::UserWrite)
            .unwrap();
        g.coverage[index] = open_coverage(Domain::UserWrite, b"missing body");
        assert_eq!(
            g.validate(),
            Err(GraphError::OpenDomainWithoutUnknown(Domain::UserWrite))
        );
    }

    #[test]
    fn every_domain_requires_a_coverage_decision_and_evidence() {
        let mut g = graph(vec![], Terminator::Return(None));
        g.coverage.clear();
        assert_eq!(
            g.validate(),
            Err(GraphError::MissingCoverage(Domain::Invocation))
        );

        let mut g = graph(vec![], Terminator::Return(None));
        g.coverage[0].evidence.clear();
        assert_eq!(
            g.validate(),
            Err(GraphError::MissingEvidence(Domain::Invocation))
        );
    }

    #[test]
    fn open_domain_requires_a_reason() {
        let mut g = graph(
            vec![Event::Unknown {
                domains: vec![Domain::UserWrite],
                reason: Bytes(b"opaque".to_vec()),
            }],
            Terminator::Return(None),
        );
        let index = g
            .coverage
            .iter()
            .position(|c| c.domain == Domain::UserWrite)
            .unwrap();
        g.coverage[index] = DomainCoverage {
            domain: Domain::UserWrite,
            coverage: Coverage::Open { reasons: vec![] },
            precision: Precision::Widened,
            dependencies: vec![],
            evidence: vec![Digest([3; 32])],
            assumptions: vec![],
        };
        assert_eq!(
            g.validate(),
            Err(GraphError::OpenWithoutReason(Domain::UserWrite))
        );
    }

    #[test]
    fn unresolved_return_is_an_incomplete_evaluation_not_no_return() {
        let g = graph(vec![], Terminator::Return(Some(Operand::Unknown)));
        let mut host = Host::default();
        assert_eq!(
            evaluate(&g, &[], &receiver(), vec![], &mut host),
            Err(GraphError::UnresolvedValue)
        );
    }

    #[test]
    fn unknown_transfer_cannot_be_marked_closed_or_reasonless() {
        let graph_with_unknown = |reason: &[u8]| {
            graph(
                vec![Event::Unknown {
                    domains: vec![Domain::Return],
                    reason: Bytes(reason.to_vec()),
                }],
                Terminator::Return(None),
            )
        };
        let mut g = graph_with_unknown(b"unknown body");
        assert_eq!(
            g.validate(),
            Err(GraphError::UnknownTransferWithoutOpenCoverage(
                Domain::Return
            ))
        );
        let index = g
            .coverage
            .iter()
            .position(|c| c.domain == Domain::Return)
            .unwrap();
        g.coverage[index] = open_coverage(Domain::Return, b"unknown body");
        let mut event_graph = graph_with_unknown(b"");
        let index = event_graph
            .coverage
            .iter()
            .position(|c| c.domain == Domain::Return)
            .unwrap();
        event_graph.coverage[index] = open_coverage(Domain::Return, b"unknown body");
        assert_eq!(
            event_graph.validate(),
            Err(GraphError::UnknownTransferWithoutReason)
        );
        assert_eq!(g.validate(), Ok(()));
    }

    /// Passes when evaluation fails closed, or keeps a pending exit matching
    /// `kept` and reports no normal or PHP-throw exit in its place.
    fn assert_pending_exit_kept(
        graph: &mut BehaviorGraph,
        arguments: &[String],
        mut host: Host,
        kept: impl Fn(&Exit<String, Vec<String>>) -> bool,
    ) {
        // The helper only derives applicability from the entry block, so a
        // cleanup block's own domains start as not applicable. Close them so the
        // graph is valid and an error cannot make the check pass vacuously.
        close_unmodeled_domains(graph);
        assert_eq!(graph.validate(), Ok(()), "the test graph must be valid");
        match evaluate(graph, arguments, &receiver(), vec![], &mut host) {
            Ok(evaluation) => {
                assert!(
                    evaluation
                        .exits
                        .iter()
                        .all(|exit| !matches!(exit, Exit::Normal { .. } | Exit::PhpThrow { .. })),
                    "cleanup replacement leaked a normal/PHP-throw exit: {:?}",
                    evaluation.exits
                );
                assert!(
                    evaluation.exits.iter().any(kept),
                    "pending exit replaced: {:?}",
                    evaluation.exits
                );
            }
            Err(error) => assert!(
                matches!(error, GraphError::UnsupportedCleanupReplacement { .. }),
                "cleanup must fail closed for the unsupported replacement cell, got {error:?}"
            ),
        }
    }

    #[test]
    fn cleanup_return_does_not_replace_a_pending_fatal() {
        let mut g = graph(
            vec![],
            Terminator::Fatal(Operand::Literal(Bytes(b"boom".to_vec()))),
        );
        g.blocks[0].exceptional.fatal = Some(1);
        g.blocks.push(Block::new(
            vec![],
            Terminator::Return(Some(Operand::Literal(Bytes(b"cleanup-return".to_vec())))),
        ));
        assert_pending_exit_kept(&mut g, &[], Host::default(), |exit| {
            matches!(exit, Exit::Fatal { .. })
        });
    }

    #[test]
    fn cleanup_throw_does_not_replace_a_pending_fatal() {
        let mut g = graph(
            vec![],
            Terminator::Fatal(Operand::Literal(Bytes(b"boom".to_vec()))),
        );
        g.blocks[0].exceptional.fatal = Some(1);
        g.blocks.push(Block::new(
            vec![],
            Terminator::Throw(Operand::Literal(Bytes(b"cleanup-throw".to_vec()))),
        ));
        assert_pending_exit_kept(&mut g, &[], Host::default(), |exit| {
            matches!(exit, Exit::Fatal { .. })
        });
    }

    #[test]
    fn cleanup_return_does_not_replace_a_pending_native_unwind() {
        let mut g = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 1,
            }],
            Terminator::Return(None),
        );
        g.blocks[0].exceptional.native_unwind = Some(1);
        g.blocks.push(Block::new(
            vec![],
            Terminator::Return(Some(Operand::Literal(Bytes(b"cleanup-return".to_vec())))),
        ));
        let host = Host {
            native_unwind_invoke: true,
            ..Host::default()
        };
        assert_pending_exit_kept(&mut g, &["native".into()], host, |exit| {
            matches!(exit, Exit::NativeUnwind { .. })
        });
    }

    #[test]
    fn cleanup_throw_does_not_replace_a_pending_native_unwind() {
        let mut g = graph(
            vec![Event::Invoke {
                callable: Operand::Argument(0),
                arguments: vec![],
                result: 1,
            }],
            Terminator::Return(None),
        );
        g.blocks[0].exceptional.native_unwind = Some(1);
        g.blocks.push(Block::new(
            vec![],
            Terminator::Throw(Operand::Literal(Bytes(b"cleanup-throw".to_vec()))),
        ));
        let host = Host {
            native_unwind_invoke: true,
            ..Host::default()
        };
        assert_pending_exit_kept(&mut g, &["native".into()], host, |exit| {
            matches!(exit, Exit::NativeUnwind { .. })
        });
    }

    #[test]
    fn cleanup_return_does_not_replace_a_pending_suspension() {
        let mut g = graph(
            vec![],
            Terminator::Suspend(Operand::Literal(Bytes(b"suspended".to_vec()))),
        );
        g.blocks[0].exceptional.suspended = Some(1);
        g.blocks.push(Block::new(
            vec![],
            Terminator::Return(Some(Operand::Literal(Bytes(b"cleanup-return".to_vec())))),
        ));
        assert_pending_exit_kept(&mut g, &[], Host::default(), |exit| {
            matches!(exit, Exit::Suspended { .. })
        });
    }

    #[test]
    fn cleanup_throw_does_not_replace_a_pending_suspension() {
        let mut g = graph(
            vec![],
            Terminator::Suspend(Operand::Literal(Bytes(b"suspended".to_vec()))),
        );
        g.blocks[0].exceptional.suspended = Some(1);
        g.blocks.push(Block::new(
            vec![],
            Terminator::Throw(Operand::Literal(Bytes(b"cleanup-throw".to_vec()))),
        ));
        assert_pending_exit_kept(&mut g, &[], Host::default(), |exit| {
            matches!(exit, Exit::Suspended { .. })
        });
    }
}
