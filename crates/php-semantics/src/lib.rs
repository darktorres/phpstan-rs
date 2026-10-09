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
    Catalog, CatalogEntry, CatalogError, CatalogManifest, CatalogTargetSelection, EntryKey,
    EvidenceKind, EvidenceRecord, GraphRecord, Projection, Selection, VariantSelection,
};
pub use php_value::{
    ArrayId, ArrayKey, Binding, ObjectId, PhpCallbackOutcome, PhpFlowHost, PhpState, PhpValue,
    ReferenceId, StateError,
};
pub use target::{
    Applicability, ReleaseVersion, TargetError, TargetPredicate, TargetScenario, TargetSelection,
    TargetSet, VersionRange,
};

pub const SCHEMA_MAJOR: u32 = 6;

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
    Fatal,
    Suspension,
}

impl Domain {
    pub const ALL: [Self; 14] = [
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
    Unknown {
        domains: Vec<Domain>,
        reason: Bytes,
    },
    /// PHP value/reference operations executed by the PHP-specific host.
    PhpFlow {
        instruction: PhpFlowInstruction,
    },
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
    InvalidSuccessor { block: u32, successor: u32 },
    DuplicateCoverage(Domain),
    MissingCoverage(Domain),
    OpenDomainWithoutUnknown(Domain),
    MissingEvidence(Domain),
    OpenWithoutReason(Domain),
    UnknownTransferWithoutReason,
    UnknownTransferWithoutOpenCoverage(Domain),
    DuplicateDomainDependency(Domain),
    ClosedDomainDependsOnOpen { domain: Domain, dependency: Domain },
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
}

impl BehaviorGraph {
    /// Encode in the schema's canonical field/vector order and identify the
    /// result with SHA-256. Producers must sort semantically unordered lists
    /// before calling this method.
    pub fn canonical_digest(&self) -> Result<Digest, postcard::Error> {
        let bytes = postcard::to_allocvec(self)?;
        Ok(Digest(Sha256::digest(bytes).into()))
    }

    pub fn validate(&self) -> Result<(), GraphError> {
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
        let mut required_domains = Vec::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if colors[index] != 2 {
                continue;
            }
            for event in &block.events {
                let required = match event {
                    Event::Read { .. } => vec![Domain::UserRead],
                    Event::Write { .. } => vec![Domain::UserWrite],
                    Event::Invoke { .. } => vec![Domain::Invocation],
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
        validate_domain_dependencies(&self.coverage)?;
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
        Ok(())
    }
}

fn validate_domain_dependencies(coverage: &[DomainCoverage]) -> Result<(), GraphError> {
    for item in coverage {
        let mut dependencies = Vec::new();
        for dependency in &item.dependencies {
            if dependencies.contains(dependency) {
                return Err(GraphError::DuplicateDomainDependency(item.domain));
            }
            dependencies.push(*dependency);
            if matches!(item.coverage, Coverage::Closed)
                && coverage.iter().any(|candidate| {
                    candidate.domain == *dependency
                        && matches!(candidate.coverage, Coverage::Open { .. })
                })
            {
                return Err(GraphError::ClosedDomainDependsOnOpen {
                    domain: item.domain,
                    dependency: *dependency,
                });
            }
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
        if let Some(item) = coverage.iter().find(|item| item.domain == domain)
            && matches!(item.coverage, Coverage::Closed)
        {
            for dependency in &item.dependencies {
                if coverage.iter().any(|candidate| {
                    candidate.domain == *dependency
                        && matches!(candidate.coverage, Coverage::Closed)
                }) {
                    visit(*dependency, coverage, colors)?;
                }
            }
        }
        colors[index] = 2;
        Ok(())
    }

    let mut colors = vec![0u8; Domain::ALL.len()];
    for item in coverage {
        if matches!(item.coverage, Coverage::Closed) {
            visit(item.domain, coverage, &mut colors)?;
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
pub struct PostFixpointCertificate<V, S> {
    pub graph_digest: Digest,
    pub initial: CertifiedFrame<V, S>,
    pub arguments: Vec<V>,
    pub receiver: V,
    pub solution: Vec<CertifiedFrame<V, S>>,
    pub candidates: Vec<CertifiedFrame<V, S>>,
    pub exits: Vec<Exit<V, S>>,
    pub coverage: Vec<DomainCoverage>,
}

impl<V: Clone + PartialEq, S: Clone + PartialEq> PostFixpointCertificate<V, S> {
    pub fn validate<H>(&self, graph: &BehaviorGraph, host: &H) -> Result<(), GraphError>
    where
        H: SemanticHost<Value = V, State = S>,
    {
        graph.validate()?;
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
        validate_domain_dependencies(&self.coverage)?;
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
            let dependencies = coverage[index].dependencies.clone();
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
                merge_coverage(replayed_coverage, outcome.coverage())?;
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
    graph.validate()?;
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
                            merge_coverage(&mut evaluation_coverage, &coverage)?;
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
                            merge_coverage(&mut evaluation_coverage, &coverage)?;
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
                            merge_coverage(&mut evaluation_coverage, &coverage)?;
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
                            merge_coverage(&mut evaluation_coverage, &coverage)?;
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
                            merge_coverage(&mut evaluation_coverage, &coverage)?;
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
    };
    post_fixpoint.validate(graph, host)?;
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
                Event::PhpFlow { instruction } => {
                    state.push(format!("php-flow:{instruction:?}"));
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

    fn graph(events: Vec<Event>, terminator: Terminator) -> BehaviorGraph {
        let mut applicable = Vec::new();
        for event in &events {
            let required = match event {
                Event::Read { .. } => vec![Domain::UserRead],
                Event::Write { .. } => vec![Domain::UserWrite],
                Event::Invoke { .. } => vec![Domain::Invocation],
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
        BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![Block::new(events, terminator)],
            coverage: coverage_for(&applicable),
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
        }
    }
    fn coverage_for(applicable: &[Domain]) -> Vec<DomainCoverage> {
        Domain::ALL
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
            .collect()
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
                dependencies: vec![],
                evidence: vec![Digest([2; 32])],
                assumptions: vec![Digest([3; 32])],
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
            Err(GraphError::InvalidComposedCoverage)
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
    fn widening_is_reflected_in_domain_precision() {
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
        let result = evaluate(&graph, &[], &receiver(), vec![], &mut host).unwrap();
        assert!(
            result
                .exits
                .iter()
                .any(|exit| matches!(exit, Exit::Normal { .. }))
        );
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
        let result = evaluate(&graph, &[], &receiver(), vec![], &mut host).unwrap();
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
}
