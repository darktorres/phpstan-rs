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
    ArrayId, ArrayKey, Binding, ObjectId, PhpState, PhpValue, ReferenceId, StateError,
};
pub use target::{
    Applicability, ReleaseVersion, TargetError, TargetPredicate, TargetScenario, TargetSelection,
    TargetSet, VersionRange,
};

pub const SCHEMA_MAJOR: u32 = 1;

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
    pub evidence: Vec<Digest>,
    pub assumptions: Vec<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operand {
    Argument(u32),
    Receiver,
    Result(u32),
    Literal(Bytes),
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
    UnresolvedValue,
    EventWithoutOutcome,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    Yes,
    No,
    Maybe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit<V, S> {
    Normal { value: Option<V>, state: S },
    PhpThrow { value: V, state: S },
    NativeUnwind { value: V, state: S },
    Fatal { value: V, state: S },
    Suspended { value: V, state: S },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingOutcome<V> {
    PhpThrow(V),
    NativeUnwind(V),
    Fatal(V),
    Suspended(V),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation<V, S> {
    pub exits: Vec<Exit<V, S>>,
    /// Graph-level derivation coverage is returned with execution results so
    /// consumers cannot mistake an empty projection for a closed negative.
    pub coverage: Vec<DomainCoverage>,
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
        &mut self,
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
        item.assumptions
            .extend(addition.assumptions.iter().copied());
        item.assumptions.sort();
        item.assumptions.dedup();
        merged.push(item);
    }
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
) -> Vec<(u32, H::Value)> {
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
            let unknown = host.unknown_value();
            let value = match (a, b) {
                (Some(a), Some(b)) if widen => host.widen_value(a, b),
                (Some(a), Some(b)) => host.join_value(a, b),
                (Some(a), None) if widen => host.widen_value(a, unknown),
                (None, Some(b)) if widen => host.widen_value(unknown, b),
                (Some(a), None) => host.join_value(a, unknown),
                (None, Some(b)) => host.join_value(unknown, b),
                (None, None) => unreachable!(),
            };
            (id, value)
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

fn enqueue<H: SemanticHost>(
    host: &H,
    frame: Frame<H::Value, H::State>,
    records: &mut Vec<Frame<H::Value, H::State>>,
    by_block: &mut [Vec<usize>],
    queue: &mut Vec<usize>,
) {
    let index = by_block[frame.block as usize]
        .iter()
        .copied()
        .find(|&i| pending_kind(&records[i].pending) == pending_kind(&frame.pending));
    let Some(index) = index else {
        let index = records.len();
        by_block[frame.block as usize].push(index);
        records.push(frame);
        queue.push(index);
        return;
    };
    if frame_subsumes(host, &records[index], &frame) {
        return;
    }
    let old = &records[index];
    let widen = old.growing_updates >= 3;
    let mut merged = Frame {
        block: old.block,
        state: if widen {
            host.widen_state(old.state.clone(), frame.state)
        } else {
            host.join_state(old.state.clone(), frame.state)
        },
        results: merge_results(host, &old.results, &frame.results, widen),
        pending: merge_pending(host, old.pending.clone(), frame.pending, widen),
        growing_updates: old.growing_updates + 1,
    };
    if merged.state == old.state && merged.results == old.results && merged.pending == old.pending {
        return;
    }
    if widen {
        merged.growing_updates = old.growing_updates + 1;
    }
    records[index] = merged;
    if !queue.contains(&index) {
        queue.push(index);
    }
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
    let mut records = vec![initial_frame];
    let mut by_block = vec![Vec::new(); graph.blocks.len()];
    by_block[graph.entry as usize].push(0);
    let mut queue = vec![0usize];
    let mut exits = Vec::new();
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
                    if matches!(event, Event::Invoke { .. }) && outcome.coverage().is_empty() {
                        return Err(GraphError::InvalidComposedCoverage);
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
                                );
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
                                );
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
                                );
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
                                );
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
                ),
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
                        ),
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
                        ),
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
                            );
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
                            );
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
                        );
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
                        );
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
                        );
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
    Ok(Evaluation {
        exits,
        coverage: evaluation_coverage,
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
        nested_open: Option<Domain>,
        incomplete_nested_coverage: bool,
        omit_nested_coverage: bool,
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
                    } else {
                        Truth::No
                    }
                }
                Predicate::Host(_) => Truth::Maybe,
            }
        }
        fn event(
            &mut self,
            event: &Event,
            args: &[String],
            receiver: &String,
            _results: &[(u32, String)],
            mut state: Vec<String>,
        ) -> Vec<EventExit<String, Vec<String>>> {
            let mut result = None;
            match event {
                Event::Read { .. } => state.push("read".into()),
                Event::Write { .. } => state.push("write".into()),
                Event::Invoke {
                    result: result_id, ..
                } => {
                    state.push("invoke".into());
                    let mut coverage = composed_coverage(self.nested_open);
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
            if a == b { a } else { "<unknown>".into() }
        }
        fn widen_value(&self, _old: String, _next: String) -> String {
            "<unknown>".into()
        }
        fn value_subsumes(&self, broad: &String, narrow: &String) -> bool {
            broad == narrow || broad == "<unknown>"
        }
        fn join_state(&self, a: Vec<String>, b: Vec<String>) -> Vec<String> {
            if a == b { a } else { vec!["<joined>".into()] }
        }
        fn widen_state(&self, _old: Vec<String>, _next: Vec<String>) -> Vec<String> {
            vec!["<joined>".into()]
        }
        fn state_subsumes(&self, broad: &Vec<String>, narrow: &Vec<String>) -> bool {
            broad == narrow || broad == &["<joined>".to_string()]
        }
    }

    fn graph(events: Vec<Event>, terminator: Terminator) -> BehaviorGraph {
        BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![Block::new(events, terminator)],
            coverage: Domain::ALL
                .into_iter()
                .map(|domain| DomainCoverage {
                    domain,
                    coverage: Coverage::NotApplicable,
                    precision: Precision::Exact,
                    evidence: vec![Digest([1; 32])],
                    assumptions: vec![],
                })
                .collect(),
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
            coverage: Domain::ALL
                .into_iter()
                .map(|domain| DomainCoverage {
                    domain,
                    coverage: Coverage::NotApplicable,
                    precision: Precision::Exact,
                    evidence: vec![Digest([1; 32])],
                    assumptions: vec![],
                })
                .collect(),
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
    fn cyclic_graph_reaches_a_stable_widened_exit() {
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
            coverage: Domain::ALL
                .into_iter()
                .map(|domain| DomainCoverage {
                    domain,
                    coverage: Coverage::NotApplicable,
                    precision: Precision::Exact,
                    evidence: vec![Digest([1; 32])],
                    assumptions: vec![],
                })
                .collect(),
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
