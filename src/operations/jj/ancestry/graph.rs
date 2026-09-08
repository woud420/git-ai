use super::JjAncestryError as E;
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

struct Terminals<'a> {
    baseline: BTreeSet<&'a str>,
    reached: BTreeSet<&'a str>,
    root: bool,
}

impl Terminals<'_> {
    fn stop(&mut self, id: &str) -> bool {
        if is_root(id) {
            self.root = true;
            true
        } else if let Some(boundary) = self.baseline.get(id).copied() {
            self.reached.insert(boundary);
            true
        } else {
            false
        }
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
    let mut terminals = Terminals {
        baseline: baseline_heads.iter().map(String::as_str).collect(),
        reached: BTreeSet::new(),
        root: false,
    };
    let mut roots: Vec<_> = heads.iter().map(String::as_str).collect();
    roots.sort_unstable();
    let mut colors = vec![Color::Unvisited; nodes.len()];
    let mut stack: Vec<Frame> = Vec::with_capacity(nodes.len());
    let mut ordered = Vec::with_capacity(nodes.len());

    for root in roots {
        if terminals.stop(root) {
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
            if terminals.stop(parent) {
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
    Ok(GraphOrder {
        operation_indices: ordered,
        reached_baseline_ids: terminals.reached.into_iter().map(str::to_owned).collect(),
        reaches_root: terminals.root,
    })
}

#[cfg(test)]
mod tests;
