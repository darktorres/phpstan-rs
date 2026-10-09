//! Shared, checker-independent vocabulary and evaluator for PHP behavior.
//!
//! This crate deliberately knows nothing about PHPStan's type system or the
//! native compiler importer. A host supplies values, state, predicates, and
//! event transfers; the graph preserves ordering and distinct exit kinds.

use serde::{Deserialize, Serialize};

pub const SCHEMA_MAJOR: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Bytes(pub Vec<u8>);

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Coverage {
    Closed,
    Open { reasons: Vec<Bytes> },
    NotApplicable,
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
    pub coverage: Vec<(Domain, Coverage)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    UnsupportedSchema(u32),
    Empty,
    InvalidEntry(u32),
    InvalidSuccessor { block: u32, successor: u32 },
    Cycle(u32),
    DuplicateCoverage(Domain),
    OpenDomainWithoutUnknown(Domain),
}

impl BehaviorGraph {
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
        for (domain, _) in &self.coverage {
            if seen.contains(domain) {
                return Err(GraphError::DuplicateCoverage(*domain));
            }
            seen.push(*domain);
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
        for (domain, coverage) in &self.coverage {
            if matches!(coverage, Coverage::Open { .. }) && !unknown_domains.contains(domain) {
                return Err(GraphError::OpenDomainWithoutUnknown(*domain));
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
) -> Result<Vec<Exit<H::Value, H::State>>, GraphError> {
    graph.validate()?;
    let mut work = vec![(graph.entry, initial, Vec::<(u32, H::Value)>::new())];
    let mut exits = Vec::new();
    while let Some((block_id, state, results)) = work.pop() {
        let block = &graph.blocks[block_id as usize];
        let mut paths = vec![(state, results)];
        for event in &block.events {
            let mut next = Vec::new();
            for (state, values) in paths {
                for outcome in host.event(event, arguments, receiver, &values, state) {
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
                Terminator::Return(value) => exits.push(Exit::Normal {
                    value: value
                        .as_ref()
                        .and_then(|v| host.resolve(v, arguments, receiver, &results)),
                    state,
                }),
                Terminator::Throw(value) => {
                    if let Some(value) = host.resolve(value, arguments, receiver, &results) {
                        exits.push(Exit::PhpThrow { value, state });
                    }
                }
                Terminator::Fatal(value) => {
                    if let Some(value) = host.resolve(value, arguments, receiver, &results) {
                        exits.push(Exit::Fatal { value, state });
                    }
                }
                Terminator::Suspend(value) => {
                    if let Some(value) = host.resolve(value, arguments, receiver, &results) {
                        exits.push(Exit::Suspended { value, state });
                    }
                }
            }
        }
    }
    Ok(exits)
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
            coverage: vec![],
        }
    }
    fn receiver() -> String {
        "receiver".into()
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
        .unwrap();
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
        .unwrap();
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
        .unwrap();
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
        let graph = graph(
            vec![Event::Unknown {
                domains: domains.clone(),
                reason: Bytes(b"opaque call".to_vec()),
            }],
            Terminator::Return(None),
        );
        let mut host = Host::default();
        let exits = evaluate(&graph, &[], &receiver(), vec![], &mut host).unwrap();
        assert_eq!(
            exits,
            vec![Exit::Normal {
                value: None,
                state: vec![format!("unknown:{domains:?}")]
            }]
        );
    }

    #[test]
    fn cyclic_graph_is_rejected_instead_of_truncated() {
        let graph = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            coverage: vec![],
            blocks: vec![Block {
                events: vec![],
                terminator: Terminator::Goto(0),
            }],
        };
        assert_eq!(graph.validate(), Err(GraphError::Cycle(0)));
    }

    #[test]
    fn open_domain_requires_an_executable_unknown_transfer() {
        let g = BehaviorGraph {
            schema_major: SCHEMA_MAJOR,
            entry: 0,
            blocks: vec![Block {
                events: vec![],
                terminator: Terminator::Return(None),
            }],
            coverage: vec![(
                Domain::UserWrite,
                Coverage::Open {
                    reasons: vec![Bytes(b"missing body".to_vec())],
                },
            )],
        };
        assert_eq!(
            g.validate(),
            Err(GraphError::OpenDomainWithoutUnknown(Domain::UserWrite))
        );
    }
}
