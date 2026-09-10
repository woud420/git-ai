use super::*;

pub(super) struct Fixture {
    pub heads: Vec<String>,
    pub nodes: Vec<(String, Vec<String>)>,
    pub baseline: Vec<String>,
}

impl Fixture {
    pub fn new(heads: &[&str], nodes: &[(&str, &[&str])], baseline: &[&str]) -> Self {
        Self {
            heads: heads.iter().map(|id| (*id).to_owned()).collect(),
            nodes: nodes
                .iter()
                .map(|(id, parents)| {
                    (
                        (*id).to_owned(),
                        parents.iter().map(|id| (*id).to_owned()).collect(),
                    )
                })
                .collect(),
            baseline: baseline.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    fn topology(&self) -> Vec<TopologyNode<'_>> {
        self.nodes
            .iter()
            .map(|(id, parents)| TopologyNode { id, parents })
            .collect()
    }

    pub fn success(&self, expected: &[&str], reached: &[&str], reaches_root: bool) -> GraphOrder {
        let order = order_to_baseline(&self.heads, &self.topology(), &self.baseline).unwrap();
        let ids: Vec<_> = order
            .operation_indices
            .iter()
            .map(|index| self.nodes[*index].0.as_str())
            .collect();
        assert_eq!(ids, expected);
        assert_eq!(order.reached_baseline_ids, reached);
        assert_eq!(order.reaches_root, reaches_root);
        order
    }

    pub fn rejects(&self, category: &str) {
        let error = match order_to_baseline(&self.heads, &self.topology(), &self.baseline) {
            Ok(_) => panic!("invalid topology produced an order"),
            Err(error) => error,
        };
        assert!(
            error.to_string().to_ascii_lowercase().contains(category),
            "{error}"
        );
    }
}

pub(super) fn root() -> String {
    "00".repeat(64)
}
