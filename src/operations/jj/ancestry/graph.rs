use super::{JjAncestryError as E, JjHeadClosure};
use crate::model::jj_observation::{MAX_JJ_OBSERVATION_OPERATIONS, is_root};
use crate::operations::jj::baseline::MAX_JJ_BASELINE_HEADS;
use crate::operations::jj::operation::MAX_OPERATION_PARENTS;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct TopologyNode<'a> {
    pub(super) id: &'a str,
    pub(super) parents: &'a [String],
}

pub(super) struct GraphOrder {
    pub(super) operation_indices: Vec<usize>,
    pub(super) head_closures: Vec<JjHeadClosure>,
    pub(super) reached_baseline_ids: Vec<String>,
    pub(super) reaches_root: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Unvisited,
    Active,
    Complete,
}

struct Frame {
    index: usize,
    next_parent: usize,
}

const ROOT_MASK: u64 = 1 << MAX_JJ_BASELINE_HEADS;
const _: () = assert!(MAX_JJ_BASELINE_HEADS < u64::BITS as usize);

struct Terminals<'a> {
    baseline: BTreeMap<&'a str, u64>,
}

impl Terminals<'_> {
    fn mask(&self, id: &str) -> Option<u64> {
        if is_root(id) {
            Some(ROOT_MASK)
        } else {
            self.baseline.get(id).copied()
        }
    }

    fn reached(&self, mask: u64) -> Vec<String> {
        self.baseline
            .iter()
            .filter(|(_, bit)| mask & **bit != 0)
            .map(|(id, _)| (*id).to_owned())
            .collect()
    }
}

/// The caller validates native evidence, identifiers, uniqueness and boundary
/// overlap first. Only the durable baseline receipt supplies terminal IDs;
/// observed membership or an anchor's unverified parents cannot widen this cut.
pub(super) fn order_to_baseline(
    heads: &[String],
    nodes: &[TopologyNode<'_>],
    baseline_heads: &[String],
) -> Result<GraphOrder, E> {
    if nodes.len() > MAX_JJ_OBSERVATION_OPERATIONS {
        return Err(E::Input("topology operation count limit exceeded"));
    }
    if heads.len() > MAX_JJ_BASELINE_HEADS || baseline_heads.len() > MAX_JJ_BASELINE_HEADS {
        return Err(E::Input("topology head or cutoff count limit exceeded"));
    }
    if nodes
        .iter()
        .any(|node| node.parents.len() > MAX_OPERATION_PARENTS)
    {
        return Err(E::Input("topology parent count limit exceeded"));
    }

    let index: BTreeMap<_, _> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id, index))
        .collect();
    let terminals = Terminals {
        baseline: baseline_heads
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .enumerate()
            .map(|(index, id)| (id, 1u64 << index))
            .collect(),
    };
    let mut roots: Vec<_> = heads.iter().map(String::as_str).collect();
    roots.sort_unstable();
    let mut colors = vec![Color::Unvisited; nodes.len()];
    let mut stack: Vec<Frame> = Vec::with_capacity(nodes.len());
    let mut ordered = Vec::with_capacity(nodes.len());

    for &root in &roots {
        if terminals.mask(root).is_some() {
            continue;
        }
        let start = *index
            .get(root)
            .ok_or(E::Input("missing ancestry head operation"))?;
        if colors[start] == Color::Complete {
            continue;
        }
        colors[start] = Color::Active;
        stack.push(Frame {
            index: start,
            next_parent: 0,
        });

        while let Some(frame) = stack.last_mut() {
            let current = frame.index;
            let Some(parent) = nodes[current].parents.get(frame.next_parent) else {
                stack.pop();
                colors[current] = Color::Complete;
                ordered.push(current);
                continue;
            };
            frame.next_parent += 1;
            if terminals.mask(parent).is_some() {
                continue;
            }
            let parent = *index
                .get(parent.as_str())
                .ok_or(E::Input("missing ancestry parent operation"))?;
            match colors[parent] {
                Color::Active => return Err(E::Input("ancestry operation cycle detected")),
                Color::Complete => continue,
                Color::Unvisited => {
                    if stack.len() >= MAX_JJ_OBSERVATION_OPERATIONS {
                        return Err(E::Input("topology depth limit exceeded"));
                    }
                    colors[parent] = Color::Active;
                    stack.push(Frame {
                        index: parent,
                        next_parent: 0,
                    });
                }
            }
        }
    }

    if ordered.len() != nodes.len() {
        return Err(E::Input("detached ancestry operation"));
    }
    // Shared ancestors contribute once, without repeating native verification
    // or a traversal for every head. Each node retains only one provenance word.
    let mut masks = vec![0u64; nodes.len()];
    for &current in &ordered {
        let mut mask = 0;
        for parent in nodes[current].parents {
            mask |= provenance(parent, &terminals, &index, &masks)?;
        }
        masks[current] = mask;
    }
    let mut reached = 0;
    let mut head_closures = Vec::with_capacity(roots.len());
    for head in roots {
        let mask = provenance(head, &terminals, &index, &masks)?;
        reached |= mask;
        head_closures.push(JjHeadClosure {
            head_id: head.to_owned(),
            reached_baseline_ids: terminals.reached(mask),
            reaches_root: mask & ROOT_MASK != 0,
        });
    }
    Ok(GraphOrder {
        operation_indices: ordered,
        head_closures,
        reached_baseline_ids: terminals.reached(reached),
        reaches_root: reached & ROOT_MASK != 0,
    })
}

fn provenance(
    id: &str,
    terminals: &Terminals<'_>,
    index: &BTreeMap<&str, usize>,
    masks: &[u64],
) -> Result<u64, E> {
    terminals
        .mask(id)
        .or_else(|| index.get(id).and_then(|index| masks.get(*index)).copied())
        .ok_or(E::Input("invalid ancestry ordering"))
}

#[cfg(test)]
mod tests;
