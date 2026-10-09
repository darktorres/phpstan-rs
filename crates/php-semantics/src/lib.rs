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

pub const SCHEMA_MAJOR: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Digest(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    IsLiteral { value: Operand, expected: Bytes },
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub events: Vec<Event>,
    pub terminator: Terminator,
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
    Cycle(u32),
    DuplicateCoverage(Domain),
    MissingCoverage(Domain),
    OpenDomainWithoutUnknown(Domain),
    MissingEvidence(Domain),
    OpenWithoutReason(Domain),
    UnresolvedValue,
    EventWithoutOutcome,
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
            let successors: Vec<u32> = match block.terminator {
                Terminator::Goto(n) => vec![n],
                Terminator::Branch { yes, no, .. } => vec![yes, no],
                _ => vec![],
            };
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
                1 => return Err(GraphError::Cycle(id)),
                2 => return Ok(()),
                _ => {}
            }
            colors[id as usize] = 1;
            let block = &graph.blocks[id as usize];
            let next = match block.terminator {
                Terminator::Goto(n) => vec![n],
                Terminator::Branch { yes, no, .. } => vec![yes, no],
                _ => vec![],
            };
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
        let unknown_domains: Vec<Domain> = self
            .blocks
            .iter()
            .enumerate()
            .filter(|(i, _)| colors[*i] == 2)
            .flat_map(|(_, block)| block.events.iter())
            .flat_map(|event| match event {
                Event::Unknown { domains, .. } => domains.clone(),
                _ => Vec::new(),
            })
            .collect();
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
    Fatal { value: V, state: S },
    Suspended { value: V, state: S },
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
    Normal { result: Option<(u32, V)>, state: S },
    PhpThrow { value: V, state: S },
    Fatal { value: V, state: S },
    Suspended { value: V, state: S },
}

pub trait SemanticHost {
    type Value: Clone;
    type State: Clone;

    fn resolve(
        &self,
        operand: &Operand,
        arguments: &[Self::Value],
        receiver: &Self::Value,
        results: &[(u32, Self::Value)],
    ) -> Option<Self::Value>;
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
}

/// Execute a finite graph with a finite host-provided state join. Cycles are
/// rejected here: production loop evaluation must provide explicit widening;
/// this seed evaluator must never silently truncate an unfinished worklist.
pub fn evaluate<H: SemanticHost>(
    graph: &BehaviorGraph,
    arguments: &[H::Value],
    receiver: &H::Value,
    initial: H::State,
    host: &mut H,
) -> Result<Evaluation<H::Value, H::State>, GraphError> {
    graph.validate()?;
    let mut work = vec![(graph.entry, initial, Vec::<(u32, H::Value)>::new())];
    let mut exits = Vec::new();
    while let Some((block_id, state, results)) = work.pop() {
        let block = &graph.blocks[block_id as usize];
        let mut paths = vec![(state, results)];
        for event in &block.events {
            let mut next = Vec::new();
            for (state, values) in paths {
                let outcomes = host.event(event, arguments, receiver, &values, state);
                if outcomes.is_empty() {
                    return Err(GraphError::EventWithoutOutcome);
                }
                for outcome in outcomes {
                    match outcome {
                        EventExit::Normal { result, state } => {
                            let mut next_values = values.clone();
                            if let Some((id, value)) = result {
                                next_values.retain(|(existing, _)| *existing != id);
                                next_values.push((id, value));
                            }
                            next.push((state, next_values));
                        }
                        EventExit::PhpThrow { value, state } => {
                            exits.push(Exit::PhpThrow { value, state })
                        }
                        EventExit::Fatal { value, state } => {
                            exits.push(Exit::Fatal { value, state })
                        }
                        EventExit::Suspended { value, state } => {
                            exits.push(Exit::Suspended { value, state })
                        }
                    }
                }
            }
            paths = next;
        }
        for (state, results) in paths {
            match &block.terminator {
                Terminator::Goto(to) => work.push((*to, state, results)),
                Terminator::Branch { predicate, yes, no } => {
                    match host.test(predicate, arguments, receiver, &results, &state) {
                        Truth::Yes => work.push((*yes, state, results)),
                        Truth::No => work.push((*no, state, results)),
                        Truth::Maybe => {
                            work.push((*yes, state.clone(), results.clone()));
                            work.push((*no, state, results));
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
                    exits.push(Exit::Normal { value, state });
                }
                Terminator::Throw(value) => {
                    let value = host
                        .resolve(value, arguments, receiver, &results)
                        .ok_or(GraphError::UnresolvedValue)?;
                    exits.push(Exit::PhpThrow { value, state });
                }
                Terminator::Fatal(value) => {
                    let value = host
                        .resolve(value, arguments, receiver, &results)
                        .ok_or(GraphError::UnresolvedValue)?;
                    exits.push(Exit::Fatal { value, state });
                }
                Terminator::Suspend(value) => {
                    let value = host
                        .resolve(value, arguments, receiver, &results)
                        .ok_or(GraphError::UnresolvedValue)?;
                    exits.push(Exit::Suspended { value, state });
                }
            }
        }
    }
    Ok(Evaluation {
        exits,
        coverage: graph.coverage.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Host {
        throw_invoke: bool,
        both_invoke_outcomes: bool,
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
            _: &Vec<String>,
        ) -> Truth {
            match p {
                Predicate::Always => Truth::Yes,
                Predicate::IsLiteral { value, expected } => {
                    match self.resolve(value, args, receiver, results) {
                        Some(v) if v.as_bytes() == expected.0 => Truth::Yes,
                        Some(_) => Truth::No,
                        None => Truth::Maybe,
                    }
                }
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
                    if self.throw_invoke {
                        return vec![EventExit::PhpThrow {
                            value: "exception".into(),
                            state,
                        }];
                    }
                    result = Some((*result_id, "callback-result".into()));
                    if self.both_invoke_outcomes {
                        let throwing_state = state.clone();
                        return vec![
                            EventExit::Normal { result, state },
                            EventExit::PhpThrow {
                                value: "exception".into(),
                                state: throwing_state,
                            },
                        ];
                    }
                }
                Event::Unknown { domains, .. } => state.push(format!("unknown:{domains:?}")),
            }
            let _ = (args, receiver);
            vec![EventExit::Normal { result, state }]
        }
    }

    fn graph(events: Vec<Event>, terminator: Terminator) -> BehaviorGraph {
        BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![Block { events, terminator }],
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
    fn cyclic_graph_is_rejected_instead_of_truncated() {
        let graph = graph(vec![], Terminator::Goto(0));
        assert_eq!(graph.validate(), Err(GraphError::Cycle(0)));
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
}
