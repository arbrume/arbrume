//! Traversals of the plant graph: basipetal and acropetal passes, child lists,
//! depth-first order and compaction.

use super::{ChainId, NodeId, PlantGraph};
use std::cmp::Reverse;

/// Child lists of every node, built on demand by [`PlantGraph::children`].
///
/// Growth never maintains child lists; this index is a compressed snapshot (two arrays)
/// that becomes stale as soon as the graph changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Children {
    offsets: Vec<u32>,
    nodes: Vec<NodeId>,
}

impl Children {
    /// Children of `node` in increasing id order. At most one of them belongs to the
    /// chain of `node` (its continuation); the others start lateral or relay chains.
    #[inline]
    #[must_use]
    pub fn of(&self, node: NodeId) -> &[NodeId] {
        let range = self.offsets[node.index()] as usize..self.offsets[node.index() + 1] as usize;
        &self.nodes[range]
    }
}

impl PlantGraph {
    /// Folds values from the tips to the base.
    ///
    /// `values` holds one value per node. Nodes are visited from the last to the first;
    /// for each node with a parent, `combine(parent_value, child_value)` is called once,
    /// after the child's own value is final. Sums of light, subtree sizes and the pipe
    /// model are all instances of this pass.
    ///
    /// # Panics
    ///
    /// Panics when `values.len()` differs from the node count.
    pub fn accumulate_basipetal<T>(&self, values: &mut [T], mut combine: impl FnMut(&mut T, &T)) {
        assert_eq!(
            values.len(),
            self.node_count(),
            "one value per node is required"
        );
        for id in (0..values.len()).rev() {
            let parent = self.parent[id];
            if parent.is_some() {
                let (before, from_child) = values.split_at_mut(id);
                combine(&mut before[parent.index()], &from_child[0]);
            }
        }
    }

    /// Pushes values from the base to the tips.
    ///
    /// `values` holds one value per node. Nodes are visited from the first to the last;
    /// for each node with a parent, `derive(parent_value, child_value)` is called once,
    /// after the parent's own value is final. Resource allocation and distances from the
    /// root are instances of this pass.
    ///
    /// # Panics
    ///
    /// Panics when `values.len()` differs from the node count.
    pub fn propagate_acropetal<T>(&self, values: &mut [T], mut derive: impl FnMut(&T, &mut T)) {
        assert_eq!(
            values.len(),
            self.node_count(),
            "one value per node is required"
        );
        for (id, parent) in self.parent.iter().enumerate() {
            if parent.is_some() {
                let (before, from_child) = values.split_at_mut(id);
                derive(&before[parent.index()], &mut from_child[0]);
            }
        }
    }

    /// Sets every radius from the pipe model (Shinozaki et al. 1964):
    /// `r^n = sum of r_child^n` with `n = exponent`, and `tip_radius` for nodes without
    /// children.
    ///
    /// Shed nodes count like any other, so a branch keeps the girth it had before it
    /// lost sub-branches. `exponent` and `tip_radius` must be positive.
    pub fn pipe_radii(&mut self, exponent: f32, tip_radius: f32) {
        let tip_area = tip_radius.powf(exponent);
        let mut area = vec![0.0_f32; self.node_count()];
        for id in (0..area.len()).rev() {
            if area[id] <= 0.0 {
                area[id] = tip_area;
            }
            self.radius[id] = area[id].powf(exponent.recip());
            let parent = self.parent[id];
            if parent.is_some() {
                area[parent.index()] += area[id];
            }
        }
    }

    /// Builds the child lists of every node in linear time.
    #[must_use]
    pub fn children(&self) -> Children {
        let count = self.node_count();
        let mut offsets = vec![0_u32; count + 1];
        for parent in self.parent.iter().filter(|p| p.is_some()) {
            offsets[parent.index() + 1] += 1;
        }
        for i in 0..count {
            offsets[i + 1] += offsets[i];
        }
        let mut cursor = offsets.clone();
        let mut nodes = vec![NodeId::NONE; offsets[count] as usize];
        for (id, parent) in self.parent.iter().enumerate().filter(|(_, p)| p.is_some()) {
            let slot = &mut cursor[parent.index()];
            nodes[*slot as usize] = NodeId::new(id);
            *slot += 1;
        }
        Children { offsets, nodes }
    }

    /// Index of every node within its chain (0 for the first node).
    fn ranks(&self) -> Vec<u32> {
        let mut rank = vec![0_u32; self.node_count()];
        for id in 0..rank.len() {
            let parent = self.parent[id];
            if parent.is_some() && self.node_chain[parent.index()] == self.node_chain[id] {
                rank[id] = rank[parent.index()] + 1;
            }
        }
        rank
    }

    /// Child chains of every chain as `(offsets, chains)`, each list sorted from the most
    /// distal attachment to the most proximal, then by chain id. Root chains are listed
    /// under the extra entry at index `chain_count()`.
    fn child_chains(&self, rank: &[u32]) -> (Vec<u32>, Vec<ChainId>) {
        let count = self.chain_count();
        let owner = |attach: NodeId| {
            if attach.is_some() {
                self.node_chain[attach.index()].index()
            } else {
                count
            }
        };
        let mut offsets = vec![0_u32; count + 2];
        for chain in &self.chains {
            offsets[owner(chain.attach) + 1] += 1;
        }
        for i in 0..=count {
            offsets[i + 1] += offsets[i];
        }
        let mut cursor = offsets.clone();
        let mut lists = vec![ChainId::NONE; count];
        for (id, chain) in self.chains.iter().enumerate() {
            let slot = &mut cursor[owner(chain.attach)];
            lists[*slot as usize] = ChainId::new(id);
            *slot += 1;
        }
        for parent in 0..count {
            let list = &mut lists[offsets[parent] as usize..offsets[parent + 1] as usize];
            list.sort_by_key(|c| Reverse(rank[self.chains[c.index()].attach.index()]));
        }
        (offsets, lists)
    }

    /// Nodes in depth-first pre-order, the order used to serialize a plant.
    ///
    /// From a node, the traversal first follows the continuation of its chain to the
    /// tip, then visits the chains attached to it in chain id order. As a result:
    ///
    /// * each chain is contiguous, base to tip;
    /// * the subtree of any node is one contiguous run starting at that node;
    /// * the chains attached to an axis appear from its tip down to its base;
    /// * a parent always precedes its children.
    ///
    /// Root chains are visited in chain id order. The result lists old node ids in their
    /// new order; after [`PlantGraph::compact`] it is the identity.
    #[must_use]
    pub fn depth_first_order(&self) -> Vec<NodeId> {
        let rank = self.ranks();
        let (offsets, lists) = self.child_chains(&rank);
        let children = |c: usize| &lists[offsets[c] as usize..offsets[c + 1] as usize];
        let mut order = vec![NodeId::NONE; self.node_count()];
        let mut filled = 0_usize;
        let mut stack: Vec<ChainId> = children(self.chain_count()).iter().rev().copied().collect();
        while let Some(chain) = stack.pop() {
            for node in self.chain_nodes_rev(chain) {
                order[filled + rank[node.index()] as usize] = node;
            }
            filled += self.chains[chain.index()].len as usize;
            stack.extend(children(chain.index()).iter().rev());
        }
        order
    }

    /// Reorders the arena into [depth-first order](Self::depth_first_order), which makes
    /// every chain contiguous.
    ///
    /// Returns `None` when the nodes were already in that order and no id changed.
    /// Otherwise returns the mapping from old to new node id, indexed by old id; node ids
    /// held outside the graph must be translated through it. Chain ids, bud ids and organ
    /// indices do not change. The pass is linear apart from sorting sibling chains, and
    /// deterministic: it depends only on the graph.
    pub fn compact(&mut self) -> Option<Vec<NodeId>> {
        let order = self.depth_first_order();
        self.contiguous = true;
        if order
            .iter()
            .enumerate()
            .all(|(new, old)| old.index() == new)
        {
            return None;
        }
        let mut remap = vec![NodeId::NONE; order.len()];
        for (new, old) in order.iter().enumerate() {
            remap[old.index()] = NodeId::new(new);
        }
        let translate = |id: NodeId| if id.is_some() { remap[id.index()] } else { id };
        self.parent = order
            .iter()
            .map(|old| translate(self.parent[old.index()]))
            .collect();
        self.position = permute(&self.position, &order);
        self.radius = permute(&self.radius, &order);
        self.node_chain = permute(&self.node_chain, &order);
        self.order = permute(&self.order, &order);
        self.birth = permute(&self.birth, &order);
        self.flags = permute(&self.flags, &order);
        for chain in &mut self.chains {
            chain.first = translate(chain.first);
            chain.last = translate(chain.last);
            chain.attach = translate(chain.attach);
        }
        for node in &mut self.bud_node {
            *node = translate(*node);
        }
        for organ in &mut self.organs {
            organ.node = translate(organ.node);
        }
        Some(remap)
    }

    /// Calls [`PlantGraph::compact`] only when the layout is not contiguous. Use this
    /// before meshing; use `compact` when the canonical depth-first order matters.
    pub fn ensure_contiguous(&mut self) -> Option<Vec<NodeId>> {
        if self.contiguous {
            None
        } else {
            self.compact()
        }
    }
}

fn permute<T: Copy>(values: &[T], order: &[NodeId]) -> Vec<T> {
    order.iter().map(|old| values[old.index()]).collect()
}

#[cfg(test)]
mod tests {
    use crate::graph::{ChainKind, Metamer, NodeId, PlantGraph};
    use glam::Vec3;

    fn at(x: f32) -> Metamer {
        Metamer::new(Vec3::new(x, 0.0, 0.0), 1.0, 0)
    }

    /// Trunk 0-1-2 grown one node per step, with laterals started in between:
    /// node 3 on node 0 and node 5 on node 1; trunk nodes are 0, 1, 2, 4.
    fn interleaved() -> PlantGraph {
        let mut graph = PlantGraph::new();
        let n0 = graph.add_root_chain(Vec3::ZERO, at(0.0));
        let trunk = graph.chain_of(n0);
        let n1 = graph.extend_chain(trunk, at(1.0));
        graph.extend_chain(trunk, at(2.0));
        graph.add_chain(n0, ChainKind::Lateral, at(3.0));
        graph.extend_chain(trunk, at(4.0));
        graph.add_chain(n1, ChainKind::Lateral, at(5.0));
        graph
    }

    #[test]
    fn depth_first_order_follows_the_axis_then_laterals_from_the_tip_down() {
        let graph = interleaved();
        let order: Vec<u32> = graph.depth_first_order().iter().map(|n| n.0).collect();
        assert_eq!(order, [0, 1, 2, 4, 5, 3]);
    }

    #[test]
    fn subtree_sizes_by_basipetal_accumulation() {
        let graph = interleaved();
        let mut sizes = vec![1_u32; graph.node_count()];
        graph.accumulate_basipetal(&mut sizes, |parent, child| *parent += *child);
        assert_eq!(sizes, [6, 4, 2, 1, 1, 1]);
    }

    #[test]
    fn depth_by_acropetal_propagation() {
        let graph = interleaved();
        let mut depth = vec![0_u32; graph.node_count()];
        graph.propagate_acropetal(&mut depth, |parent, child| *child = *parent + 1);
        assert_eq!(depth, [0, 1, 2, 1, 3, 2]);
    }

    #[test]
    fn pipe_model_sums_cross_sections() {
        let mut graph = interleaved();
        graph.pipe_radii(2.0, 1.0);
        let expected = [3.0_f32, 2.0, 1.0, 1.0, 1.0, 1.0].map(f32::sqrt);
        for (radius, want) in graph.radii().iter().zip(expected) {
            assert!((radius - want).abs() < 1e-5);
        }
    }

    #[test]
    fn children_lists_match_parents() {
        let graph = interleaved();
        let children = graph.children();
        assert_eq!(
            children.of(NodeId::new(0)),
            [NodeId::new(1), NodeId::new(3)]
        );
        assert_eq!(children.of(NodeId::new(2)), [NodeId::new(4)]);
        assert_eq!(children.of(NodeId::new(5)), []);
    }

    #[test]
    fn ensure_contiguous_is_a_no_op_on_contiguous_graphs() {
        let mut graph = PlantGraph::new();
        let n0 = graph.add_root_chain(Vec3::ZERO, at(0.0));
        graph.add_chain(n0, ChainKind::Lateral, at(1.0));
        assert_eq!(graph.ensure_contiguous(), None);
        let mut broken = interleaved();
        assert!(broken.ensure_contiguous().is_some());
        assert!(broken.is_contiguous());
    }
}
