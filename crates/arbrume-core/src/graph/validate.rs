//! Structural validation of the plant graph.

use super::{BudKind, Chain, ChainId, NodeId, PlantGraph};
use thiserror::Error;

/// First violated invariant found by [`PlantGraph::check_invariants`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum GraphError {
    /// The per-node or per-bud arrays have different lengths.
    #[error("arrays of the graph have inconsistent lengths")]
    ArrayLengths,
    /// A node breaks an invariant.
    #[error("node {node}: {problem}")]
    Node {
        /// Index of the node.
        node: usize,
        /// What is wrong.
        problem: &'static str,
    },
    /// A chain breaks an invariant.
    #[error("chain {chain}: {problem}")]
    Chain {
        /// Index of the chain.
        chain: usize,
        /// What is wrong.
        problem: &'static str,
    },
    /// A bud breaks an invariant.
    #[error("bud {bud}: {problem}")]
    Bud {
        /// Index of the bud.
        bud: usize,
        /// What is wrong.
        problem: &'static str,
    },
    /// An organ site breaks an invariant.
    #[error("organ {organ}: {problem}")]
    Organ {
        /// Index of the organ site.
        organ: usize,
        /// What is wrong.
        problem: &'static str,
    },
}

fn node_error(node: usize, problem: &'static str) -> Result<(), GraphError> {
    Err(GraphError::Node { node, problem })
}

fn chain_error(chain: usize, problem: &'static str) -> Result<(), GraphError> {
    Err(GraphError::Chain { chain, problem })
}

fn bud_error(bud: usize, problem: &'static str) -> Result<(), GraphError> {
    Err(GraphError::Bud { bud, problem })
}

impl PlantGraph {
    /// Verifies every structural invariant of the graph. Linear time; meant for tests and
    /// debug builds.
    ///
    /// Checked: array lengths; each parent stored before its child (hence no cycle);
    /// finite positions and non-negative finite radii; births not decreasing from parent
    /// to child; shed nodes having only shed descendants; each node agreeing with its
    /// chain on order and membership; chains being non-empty parent-linked paths that
    /// partition the nodes; attachment in an earlier chain with an order equal to or one
    /// above it; contiguous ranges when [`PlantGraph::is_contiguous`] is set; buds and
    /// organ sites on valid nodes; terminal buds registered on the tip of their chain.
    ///
    /// # Errors
    ///
    /// Returns the first violation found.
    pub fn check_invariants(&self) -> Result<(), GraphError> {
        self.check_lengths()?;
        let mut members = vec![0_u32; self.chain_count()];
        for node in 0..self.node_count() {
            self.check_node(node)?;
            members[self.node_chain[node].index()] += 1;
        }
        for (id, chain) in self.chains.iter().enumerate() {
            if members[id] != chain.len {
                return chain_error(id, "length differs from the number of member nodes");
            }
            self.check_chain(id, chain)?;
        }
        self.check_buds()?;
        match self
            .organs
            .iter()
            .position(|site| site.node.index() >= self.node_count())
        {
            Some(organ) => Err(GraphError::Organ {
                organ,
                problem: "node out of range",
            }),
            None => Ok(()),
        }
    }

    fn check_lengths(&self) -> Result<(), GraphError> {
        let nodes = self.parent.len();
        let node_arrays = [
            self.position.len(),
            self.radius.len(),
            self.node_chain.len(),
            self.order.len(),
            self.birth.len(),
            self.flags.len(),
        ];
        let buds = self.bud_node.len();
        let bud_arrays = [
            self.bud_kind.len(),
            self.bud_state.len(),
            self.bud_direction.len(),
            self.bud_preferred.len(),
            self.bud_exposure.len(),
            self.bud_resource.len(),
            self.bud_birth.len(),
        ];
        let consistent =
            node_arrays.iter().all(|&n| n == nodes) && bud_arrays.iter().all(|&n| n == buds);
        if consistent {
            Ok(())
        } else {
            Err(GraphError::ArrayLengths)
        }
    }

    fn check_node(&self, node: usize) -> Result<(), GraphError> {
        let chain_id = self.node_chain[node];
        let Some(chain) = self.chains.get(chain_id.index()) else {
            return node_error(node, "chain out of range");
        };
        if self.order[node] != chain.order {
            return node_error(node, "order differs from the order of its chain");
        }
        let radius = self.radius[node];
        if !self.position[node].is_finite() || !radius.is_finite() || radius < 0.0 {
            return node_error(
                node,
                "position or radius is not finite, or radius is negative",
            );
        }
        let parent = self.parent[node];
        let is_first = chain.first.index() == node;
        if parent.is_none() {
            let rooted = is_first && chain.attach.is_none();
            return if rooted {
                Ok(())
            } else {
                node_error(node, "parentless node inside a chain")
            };
        }
        if parent.index() >= node {
            return node_error(node, "parent is not stored before the node");
        }
        if self.birth[parent.index()] > self.birth[node] {
            return node_error(node, "born before its parent");
        }
        if self.flags[parent.index()].is_shed() && !self.flags[node].is_shed() {
            return node_error(node, "live node under a shed parent");
        }
        let continues = self.node_chain[parent.index()] == chain_id;
        if continues == is_first || (is_first && chain.attach != parent) {
            return node_error(node, "parent link disagrees with the chain");
        }
        Ok(())
    }

    fn check_chain(&self, id: usize, chain: &Chain) -> Result<(), GraphError> {
        let count = self.node_count();
        if chain.len == 0 || chain.first.index() >= count || chain.last.index() >= count {
            return chain_error(id, "empty chain or end node out of range");
        }
        let this = ChainId::new(id);
        let mut node = chain.last;
        for _ in 1..chain.len {
            node = self.parent[node.index()];
            if node.is_none() || self.node_chain[node.index()] != this {
                return chain_error(id, "nodes do not form a path from first to last");
            }
        }
        if node != chain.first {
            return chain_error(id, "walking back from last does not reach first");
        }
        if self.contiguous && chain.last.index() - chain.first.index() + 1 != chain.len as usize {
            return chain_error(
                id,
                "not contiguous although the graph is flagged contiguous",
            );
        }
        if chain.attach.is_some() {
            let parent_chain = self.node_chain[chain.attach.index()];
            let parent_order = self.chains[parent_chain.index()].order;
            if parent_chain.index() >= id {
                return chain_error(id, "attached to a chain that is not stored before it");
            }
            if chain.order != parent_order && chain.order != parent_order.saturating_add(1) {
                return chain_error(id, "order is neither that of its parent nor one above");
            }
        }
        let bud = chain.terminal_bud;
        let registered = bud.is_none()
            || (bud.index() < self.bud_count()
                && self.bud_kind[bud.index()] == BudKind::Terminal
                && self.bud_node[bud.index()] == chain.last);
        if registered {
            Ok(())
        } else {
            chain_error(id, "terminal bud is not a bud on its tip")
        }
    }

    fn check_buds(&self) -> Result<(), GraphError> {
        for (bud, &node) in self.bud_node.iter().enumerate() {
            if node.index() >= self.node_count() {
                return bud_error(bud, "node out of range");
            }
            if !self.bud_direction[bud].is_finite() || !self.bud_preferred[bud].is_finite() {
                return bud_error(bud, "direction is not finite");
            }
            let chain = &self.chains[self.node_chain[node.index()].index()];
            let terminal = self.bud_kind[bud] == BudKind::Terminal;
            if terminal && chain.terminal_bud.index() != bud {
                return bud_error(bud, "terminal bud is not registered on its chain");
            }
        }
        Ok(())
    }

    /// `true` when `ancestor` lies on the path from `node` to the base (a node is its own
    /// ancestor). Linear in the depth of `node`.
    #[must_use]
    pub fn is_ancestor(&self, ancestor: NodeId, node: NodeId) -> bool {
        let mut current = node;
        while current.is_some() && current.index() > ancestor.index() {
            current = self.parent[current.index()];
        }
        current == ancestor
    }
}

#[cfg(test)]
mod tests {
    use crate::graph::{ChainKind, GraphError, Metamer, NodeFlags, NodeId, PlantGraph};
    use glam::Vec3;

    fn tree() -> PlantGraph {
        let mut graph = PlantGraph::new();
        let base = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y, 0.1, 0));
        let tip = graph.extend_chain(graph.chain_of(base), Metamer::new(Vec3::Y * 2.0, 0.1, 1));
        graph.add_chain(base, ChainKind::Lateral, Metamer::new(Vec3::X, 0.1, 1));
        assert!(graph.is_ancestor(base, tip) && !graph.is_ancestor(tip, base));
        graph
    }

    #[test]
    fn valid_and_empty_graphs_pass() {
        tree().check_invariants().unwrap();
        PlantGraph::new().check_invariants().unwrap();
    }

    #[test]
    fn bad_values_are_reported() {
        let mut graph = tree();
        graph.radii_mut()[1] = -1.0;
        assert!(matches!(
            graph.check_invariants(),
            Err(GraphError::Node { node: 1, .. })
        ));
        let mut graph = tree();
        graph.positions_mut()[2].x = f32::NAN;
        assert!(matches!(
            graph.check_invariants(),
            Err(GraphError::Node { node: 2, .. })
        ));
    }

    #[test]
    fn shed_flag_must_cover_descendants() {
        let mut graph = tree();
        graph.flags_mut()[0] = NodeFlags::SHED;
        assert!(matches!(
            graph.check_invariants(),
            Err(GraphError::Node { node: 1, .. })
        ));
        graph.shed_subtree(NodeId::new(0));
        graph.check_invariants().unwrap();
    }

    #[test]
    fn dangling_organ_is_reported() {
        let mut graph = tree();
        graph.add_organ(crate::graph::OrganSite {
            node: NodeId::new(99),
            kind: crate::graph::OrganKind::Leaf,
            direction: Vec3::X,
            normal: Vec3::Y,
            scale: 1.0,
            birth: 0,
        });
        assert!(matches!(
            graph.check_invariants(),
            Err(GraphError::Organ { organ: 0, .. })
        ));
    }
}
