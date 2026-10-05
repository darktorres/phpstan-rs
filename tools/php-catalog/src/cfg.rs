//! The control-flow graph's cleanup after lowering: the blocks no path
//! reaches go, a block that only jumps on is jumped over, and a branch
//! whose arms meet or whose condition is a constant becomes a jump.

use crate::ir::{Block, BlockId, ExprKind, Function, Term};

pub(crate) fn simplify(function: &mut Function) {
    loop {
        let before = function.blocks.len();
        let folded = pass(function);
        if !folded && function.blocks.len() == before {
            return;
        }
    }
}

/// One round: whether a terminator was folded.
fn pass(function: &mut Function) -> bool {
    let count = function.blocks.len();
    if count == 0 {
        return false;
    }
    let mut folded = false;
    for block in &mut function.blocks {
        folded |= fold(&mut block.term);
    }
    let forward: Vec<BlockId> = (0..count as BlockId)
        .map(|block| jump_target(&function.blocks, block))
        .collect();
    for block in &mut function.blocks {
        retarget(&mut block.term, &forward);
    }
    let entry = forward[0];
    let mut order = Vec::with_capacity(count);
    let mut number = vec![BlockId::MAX; count];
    number[entry as usize] = 0;
    order.push(entry);
    let mut next = 0;
    while next < order.len() {
        let block = order[next];
        next += 1;
        for successor in successors(&function.blocks[block as usize].term) {
            if number[successor as usize] == BlockId::MAX {
                number[successor as usize] = order.len() as BlockId;
                order.push(successor);
            }
        }
    }
    let mut old: Vec<Option<Block>> = std::mem::take(&mut function.blocks)
        .into_iter()
        .map(Some)
        .collect();
    function.blocks = order
        .iter()
        .map(|&block| {
            let mut block = old[block as usize].take().expect("each block once");
            retarget(&mut block.term, &number);
            block
        })
        .collect();
    folded
}

/// Where a jump to `block` goes on to: past every block that holds no
/// statement and only jumps.
fn jump_target(blocks: &[Block], block: BlockId) -> BlockId {
    let mut at = block;
    for _ in 0..blocks.len() {
        let next = &blocks[at as usize];
        match next.term {
            Term::Goto(target) if next.stmts.is_empty() && target != at => at = target,
            _ => return at,
        }
    }
    at
}

fn retarget(term: &mut Term, map: &[BlockId]) {
    match term {
        Term::Goto(target) => *target = map[*target as usize],
        Term::Branch { then, els, .. } => {
            *then = map[*then as usize];
            *els = map[*els as usize];
        }
        Term::Switch { cases, default, .. } => {
            for (_, _, target) in cases {
                *target = map[*target as usize];
            }
            *default = map[*default as usize];
        }
        Term::IndirectGoto(targets) => {
            for target in targets.iter_mut() {
                *target = map[*target as usize];
            }
            targets.sort_unstable();
            targets.dedup();
        }
        Term::Return(_) | Term::Unreachable => {}
    }
}

/// A branch to one block, or on a constant, as a jump; a switch whose
/// cases all go where its default does, as a jump. Whether it folded.
fn fold(term: &mut Term) -> bool {
    let jump = match term {
        Term::Branch { cond, then, els } => match cond.kind {
            _ if then == els => Some(*then),
            ExprKind::Int(0) => Some(*els),
            ExprKind::Int(_) => Some(*then),
            _ => None,
        },
        Term::Switch { on, cases, default } => match on.kind {
            ExprKind::Int(value) => Some(
                cases
                    .iter()
                    .find(|(low, high, _)| (*low..=*high).contains(&value))
                    .map_or(*default, |(_, _, target)| *target),
            ),
            _ if cases.iter().all(|(_, _, target)| target == default) => Some(*default),
            _ => None,
        },
        _ => None,
    };
    match jump {
        Some(target) => {
            *term = Term::Goto(target);
            true
        }
        None => false,
    }
}

pub(crate) fn successors(term: &Term) -> Vec<BlockId> {
    match term {
        Term::Goto(target) => vec![*target],
        Term::Branch { then, els, .. } => vec![*then, *els],
        Term::Switch { cases, default, .. } => {
            let mut targets: Vec<BlockId> = cases.iter().map(|(_, _, target)| *target).collect();
            targets.push(*default);
            targets
        }
        Term::IndirectGoto(targets) => targets.clone(),
        Term::Return(_) | Term::Unreachable => Vec::new(),
    }
}
