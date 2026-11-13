//! Typed `u32` identifiers of the plant graph.

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u32);

        impl $name {
            /// The absent id.
            pub const NONE: Self = Self(u32::MAX);

            /// Id for the array index `index`.
            #[inline]
            #[must_use]
            pub const fn new(index: usize) -> Self {
                Self(index as u32)
            }

            /// Array index of this id.
            #[inline]
            #[must_use]
            pub const fn index(self) -> usize {
                self.0 as usize
            }

            /// `true` for [`Self::NONE`].
            #[inline]
            #[must_use]
            pub const fn is_none(self) -> bool {
                self.0 == u32::MAX
            }

            /// `true` for any id other than [`Self::NONE`].
            #[inline]
            #[must_use]
            pub const fn is_some(self) -> bool {
                self.0 != u32::MAX
            }
        }
    };
}

id_type!(
    /// Identifier of a node (metamer). Node ids change in
    /// [`PlantGraph::compact`](super::PlantGraph::compact).
    NodeId
);
id_type!(
    /// Identifier of a chain (axis). Stable for the life of the graph.
    ChainId
);
id_type!(
    /// Identifier of a bud. Stable for the life of the graph.
    BudId
);
