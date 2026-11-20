//! Surface checks: whether a triangle mesh bounds a solid.
//!
//! [`check_surface`] welds vertices that have exactly the same position, which is how
//! texture seams are stored (one vertex per side of the seam), and then reports:
//!
//! * **Boundary edges**, used by one triangle: a hole. A closed surface has none.
//! * **Non-manifold edges**, used by three triangles or more, and **misoriented
//!   edges**, used twice in the same direction: the two triangles disagree on which
//!   side is outside.
//! * **Non-manifold vertices**, whose triangles form more than one fan, as where two
//!   cones touch by their tips.
//! * **Components**, the number of pieces connected through shared vertices.
//! * **Sharp creases**, two triangles sharing an edge whose normals are nearly
//!   opposite: the surface folds back on itself there, by less than about 26 degrees.
//!   A flipped triangle looks like this, and so does the crotch of a narrow fork; it
//!   is reported, not counted against watertightness.
//! * **Self-intersections**, pairs of triangles that share no edge and cross each
//!   other, found with a bounding volume hierarchy over the triangles and the interval
//!   test of Möller (1997) in double precision. Pairs sharing one vertex are tested for
//!   an edge of one crossing the other.
//!
//! A surface is watertight in the sense used for 3D printing and subdivision when it is
//! a closed manifold of one component without self-intersections
//! ([`SurfaceReport::is_watertight`]).

use glam::DVec3;

/// Two triangles sharing an edge make a sharp crease when the cosine between their
/// normals is below this (an angle of about 26 degrees between the two faces).
const CREASE_COSINE: f64 = -0.9;

/// Two triangles are parallel when the squared sine of the angle between their planes is
/// below this.
const PARALLEL: f64 = 1e-10;

/// Distances to a plane below this, relative to the size of the triangle, are taken as 0
/// for parallel triangles.
const COPLANAR: f64 = 1e-6;

/// Distances to a plane, relative to the size of the triangle, below which a vertex is
/// taken to lie on it.
const SIDE_EPSILON: f64 = 1e-9;

/// Most triangles in a leaf of the bounding volume hierarchy.
const LEAF_SIZE: usize = 4;

/// Result of [`check_surface`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SurfaceReport {
    /// Vertices after welding equal positions.
    pub vertices: usize,
    /// Triangles.
    pub triangles: usize,
    /// Triangles with two equal corners or no area.
    pub degenerate: usize,
    /// Edges used by one triangle.
    pub boundary_edges: usize,
    /// Edges used by three triangles or more.
    pub non_manifold_edges: usize,
    /// Edges used by two triangles in the same direction.
    pub misoriented_edges: usize,
    /// Vertices whose triangles form more than one fan.
    pub non_manifold_vertices: usize,
    /// Connected pieces.
    pub components: usize,
    /// Pairs of triangles sharing an edge that make a sharp crease.
    pub creases: usize,
    /// Pairs of triangles sharing no edge that intersect, as triangle numbers, the
    /// smaller first; empty when not computed.
    pub intersections: Vec<(u32, u32)>,
}

impl SurfaceReport {
    /// Every edge used by exactly two triangles in opposite directions, every vertex
    /// with a single fan, and no degenerate triangle.
    #[must_use]
    pub const fn is_closed_manifold(&self) -> bool {
        self.boundary_edges == 0
            && self.non_manifold_edges == 0
            && self.misoriented_edges == 0
            && self.non_manifold_vertices == 0
            && self.degenerate == 0
    }

    /// A closed manifold in one piece without self-intersection.
    #[must_use]
    pub const fn is_watertight(&self) -> bool {
        self.is_closed_manifold() && self.components == 1 && self.intersections.is_empty()
    }
}

/// Union-find over `0..len`.
struct Sets(Vec<u32>);

impl Sets {
    fn new(len: usize) -> Self {
        Self((0..len as u32).collect())
    }

    fn root(&mut self, mut item: u32) -> u32 {
        while self.0[item as usize] != item {
            let parent = self.0[item as usize];
            self.0[item as usize] = self.0[parent as usize];
            item = parent;
        }
        item
    }

    fn join(&mut self, a: u32, b: u32) {
        let (a, b) = (self.root(a), self.root(b));
        if a != b {
            self.0[a.max(b) as usize] = a.min(b);
        }
    }
}

/// Welds equal positions: the welded index of every vertex, and the welded count.
fn weld(positions: &[[f32; 3]]) -> (Vec<u32>, usize) {
    let mut keyed: Vec<([u32; 3], u32)> = positions
        .iter()
        .enumerate()
        .map(|(index, p)| (p.map(|c| (c + 0.0).to_bits()), index as u32))
        .collect();
    keyed.sort_unstable();
    let mut welded = vec![0; positions.len()];
    let mut count = 0;
    for (rank, &(key, index)) in keyed.iter().enumerate() {
        if rank > 0 && keyed[rank - 1].0 != key {
            count += 1;
        }
        welded[index as usize] = count as u32;
    }
    (welded, if positions.is_empty() { 0 } else { count + 1 })
}

/// One use of an edge by a triangle: sorted ends, triangle, and whether the triangle
/// runs from the smaller end to the larger.
type EdgeUse = ((u32, u32), u32, bool);

fn triangle_normal(p: &[DVec3; 3]) -> DVec3 {
    (p[1] - p[0]).cross(p[2] - p[0])
}

/// Checks the triangle list `indices` over `positions`. Self-intersections are searched
/// only when `intersections` is set, as they cost the most.
///
/// # Panics
///
/// Panics when an index is out of range.
#[must_use]
pub fn check_surface(
    positions: &[[f32; 3]],
    indices: &[u32],
    intersections: bool,
) -> SurfaceReport {
    let (welded, vertices) = weld(positions);
    let triangles: Vec<[u32; 3]> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.map(|i| welded[i as usize]))
        .collect();
    let corners: Vec<[DVec3; 3]> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.map(|i| DVec3::from(positions[i as usize].map(f64::from))))
        .collect();
    let mut report = SurfaceReport {
        vertices,
        triangles: triangles.len(),
        ..SurfaceReport::default()
    };
    let flat = |t: &[u32; 3], p: &[DVec3; 3]| {
        t[0] == t[1] || t[1] == t[2] || t[0] == t[2] || triangle_normal(p).length_squared() == 0.0
    };
    report.degenerate = triangles
        .iter()
        .zip(&corners)
        .filter(|(t, p)| flat(t, p))
        .count();
    let mut uses: Vec<EdgeUse> = Vec::with_capacity(triangles.len() * 3);
    for (number, t) in triangles.iter().enumerate() {
        for side in 0..3 {
            let (a, b) = (t[side], t[(side + 1) % 3]);
            if a != b {
                uses.push(((a.min(b), a.max(b)), number as u32, a < b));
            }
        }
    }
    uses.sort_unstable();
    edge_counts(&mut report, &uses, &triangles, &corners);
    report.non_manifold_vertices = fans(&uses, &triangles, vertices);
    let mut sets = Sets::new(vertices);
    for t in &triangles {
        sets.join(t[0], t[1]);
        sets.join(t[1], t[2]);
    }
    let used: Vec<bool> = {
        let mut used = vec![false; vertices];
        triangles
            .iter()
            .flatten()
            .for_each(|&v| used[v as usize] = true);
        used
    };
    report.components = (0..vertices as u32)
        .filter(|&v| used[v as usize] && sets.root(v) == v)
        .count();
    if intersections {
        let keep: Vec<u32> = (0..triangles.len() as u32)
            .filter(|&n| !flat(&triangles[n as usize], &corners[n as usize]))
            .collect();
        find_intersections(&mut report, &triangles, &corners, keep);
    }
    report
}

/// Counts boundary, non-manifold and misoriented edges and sharp creases from the
/// sorted edge uses.
fn edge_counts(
    report: &mut SurfaceReport,
    uses: &[EdgeUse],
    triangles: &[[u32; 3]],
    corners: &[[DVec3; 3]],
) {
    for group in uses.chunk_by(|a, b| a.0 == b.0) {
        match group {
            [_] => report.boundary_edges += 1,
            [a, b] if a.2 == b.2 => report.misoriented_edges += 1,
            [a, b] => {
                let (n1, n2) = (
                    triangle_normal(&corners[a.1 as usize]).normalize_or_zero(),
                    triangle_normal(&corners[b.1 as usize]).normalize_or_zero(),
                );
                let distinct = triangles[a.1 as usize] != triangles[b.1 as usize];
                if distinct && n1.dot(n2) < CREASE_COSINE {
                    report.creases += 1;
                }
            }
            _ => report.non_manifold_edges += 1,
        }
    }
}

/// Number of vertices whose triangles form more than one fan. The corners of two
/// triangles meeting at a vertex belong to one fan when the triangles share an edge
/// through that vertex used by those two triangles only.
fn fans(uses: &[EdgeUse], triangles: &[[u32; 3]], vertices: usize) -> usize {
    let corner = |triangle: u32, vertex: u32| {
        let t = &triangles[triangle as usize];
        let at = t.iter().position(|&v| v == vertex).unwrap_or(0);
        triangle * 3 + at as u32
    };
    let mut sets = Sets::new(triangles.len() * 3);
    for group in uses.chunk_by(|a, b| a.0 == b.0) {
        if let [a, b] = group {
            let (u, w) = a.0;
            sets.join(corner(a.1, u), corner(b.1, u));
            sets.join(corner(a.1, w), corner(b.1, w));
        }
    }
    let mut first_root = vec![u32::MAX; vertices];
    let mut broken = vec![false; vertices];
    for (triangle, t) in triangles.iter().enumerate() {
        for (at, &v) in t.iter().enumerate() {
            let root = sets.root(triangle as u32 * 3 + at as u32);
            let seen = &mut first_root[v as usize];
            if *seen == u32::MAX {
                *seen = root;
            } else if *seen != root {
                broken[v as usize] = true;
            }
        }
    }
    broken.iter().filter(|&&b| b).count()
}

/// Axis-aligned box in double precision.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    min: DVec3,
    max: DVec3,
}

impl Bounds {
    const EMPTY: Self = Self {
        min: DVec3::INFINITY,
        max: DVec3::NEG_INFINITY,
    };

    fn of(points: &[DVec3; 3]) -> Self {
        Self {
            min: points[0].min(points[1]).min(points[2]),
            max: points[0].max(points[1]).max(points[2]),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    fn overlaps(self, other: Self) -> bool {
        self.min.cmple(other.max).all() && other.min.cmple(self.max).all()
    }
}

/// Node of the hierarchy: a box and either two children or a range of triangles.
#[derive(Debug, Clone, Copy)]
struct Node {
    bounds: Bounds,
    /// First child, or first triangle of a leaf.
    first: u32,
    /// Second child of an inner node.
    second: u32,
    /// 0 for an inner node, the number of triangles of a leaf.
    count: u32,
}

/// Builds the hierarchy over `order` (reordered in place), splitting at the median of
/// the centroids along the longest side.
fn build(nodes: &mut Vec<Node>, order: &mut [u32], boxes: &[Bounds], offset: u32) -> u32 {
    let bounds = order
        .iter()
        .fold(Bounds::EMPTY, |b, &t| b.union(boxes[t as usize]));
    let at = nodes.len() as u32;
    nodes.push(Node {
        bounds,
        first: offset,
        second: 0,
        count: order.len() as u32,
    });
    if order.len() <= LEAF_SIZE {
        return at;
    }
    let size = bounds.max - bounds.min;
    let axis = if size.x >= size.y && size.x >= size.z {
        0
    } else if size.y >= size.z {
        1
    } else {
        2
    };
    let middle = order.len() / 2;
    let centroid = |t: u32| boxes[t as usize].min[axis] + boxes[t as usize].max[axis];
    order.select_nth_unstable_by(middle, |&a, &b| centroid(a).total_cmp(&centroid(b)));
    let (left, right) = order.split_at_mut(middle);
    let first = build(nodes, left, boxes, offset);
    let second = build(nodes, right, boxes, offset + middle as u32);
    nodes[at as usize].first = first;
    nodes[at as usize].second = second;
    nodes[at as usize].count = 0;
    at
}

/// Finds the pairs of the triangles `keep` that intersect.
fn find_intersections(
    report: &mut SurfaceReport,
    triangles: &[[u32; 3]],
    corners: &[[DVec3; 3]],
    mut keep: Vec<u32>,
) {
    if keep.is_empty() {
        return;
    }
    let boxes: Vec<Bounds> = corners.iter().map(Bounds::of).collect();
    let mut nodes = Vec::with_capacity(keep.len() / 2);
    build(&mut nodes, &mut keep, &boxes, 0);
    let mut stack = Vec::new();
    for &a in &keep {
        stack.push(0_u32);
        while let Some(node) = stack.pop() {
            let node = nodes[node as usize];
            if !node.bounds.overlaps(boxes[a as usize]) {
                continue;
            }
            if node.count == 0 {
                stack.extend([node.first, node.second]);
                continue;
            }
            let range = node.first as usize..(node.first + node.count) as usize;
            for &b in &keep[range] {
                if b > a
                    && boxes[a as usize].overlaps(boxes[b as usize])
                    && pair_intersects(triangles, corners, a, b)
                {
                    report.intersections.push((a, b));
                }
            }
        }
    }
}

/// Whether triangles `a` and `b` intersect away from what they share.
fn pair_intersects(triangles: &[[u32; 3]], corners: &[[DVec3; 3]], a: u32, b: u32) -> bool {
    let (ta, tb) = (&triangles[a as usize], &triangles[b as usize]);
    let (pa, pb) = (&corners[a as usize], &corners[b as usize]);
    let shared: Vec<(usize, usize)> = (0..3)
        .flat_map(|i| (0..3).map(move |j| (i, j)))
        .filter(|&(i, j)| ta[i] == tb[j])
        .collect();
    match shared.as_slice() {
        [] => triangles_intersect(pa, pb),
        [(i, j)] => {
            let edge_a = [pa[(i + 1) % 3], pa[(i + 2) % 3]];
            let edge_b = [pb[(j + 1) % 3], pb[(j + 2) % 3]];
            segment_crosses(edge_a, pb) || segment_crosses(edge_b, pa)
        }
        _ => false,
    }
}

/// Signed distances of `points` to the plane of `triangle`, scaled to its size, and
/// whether they all lie on one side of it.
fn sides(points: &[DVec3; 3], triangle: &[DVec3; 3]) -> ([f64; 3], bool) {
    let normal = triangle_normal(triangle).normalize_or_zero();
    let scale = (triangle[1] - triangle[0])
        .length()
        .max((triangle[2] - triangle[0]).length());
    let distances = points.map(|p| normal.dot(p - triangle[0]) / scale.max(f64::MIN_POSITIVE));
    let apart =
        distances.iter().all(|&d| d > SIDE_EPSILON) || distances.iter().all(|&d| d < -SIDE_EPSILON);
    (distances, apart)
}

/// Interval of the line of intersection of the two planes covered by a triangle with
/// the given projections onto that line and signed distances to the other plane: the
/// points where its edges cross the plane and its vertices on the plane.
fn interval(projections: [f64; 3], distances: [f64; 3]) -> (f64, f64) {
    let side = |d: f64| {
        if d > SIDE_EPSILON {
            1
        } else if d < -SIDE_EPSILON {
            -1
        } else {
            0
        }
    };
    let signs = distances.map(side);
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for i in 0..3 {
        let j = (i + 1) % 3;
        if signs[i] == 0 {
            (low, high) = (low.min(projections[i]), high.max(projections[i]));
        }
        if signs[i] * signs[j] < 0 {
            let t = distances[i] / (distances[i] - distances[j]);
            let point = projections[i] + (projections[j] - projections[i]) * t;
            (low, high) = (low.min(point), high.max(point));
        }
    }
    (low, high)
}

/// Interval overlap test of Möller (1997) for two triangles sharing no vertex.
fn triangles_intersect(a: &[DVec3; 3], b: &[DVec3; 3]) -> bool {
    let (db, apart_b) = sides(b, a);
    if apart_b {
        return false;
    }
    let (da, apart_a) = sides(a, b);
    if apart_a {
        return false;
    }
    let (na, nb) = (triangle_normal(a), triangle_normal(b));
    let line = na.cross(nb);
    if line.length_squared() < PARALLEL * na.length_squared() * nb.length_squared() {
        let near = |d: &[f64; 3]| d.iter().all(|v| v.abs() < COPLANAR);
        return near(&da) && near(&db) && coplanar_overlap(a, b);
    }
    let line = line.normalize();
    let (ia, ib) = (
        interval(a.map(|p| p.dot(line)), da),
        interval(b.map(|p| p.dot(line)), db),
    );
    let size = (a[1] - a[0]).length().max((b[1] - b[0]).length());
    let tolerance = SIDE_EPSILON * size;
    ia.0 < ib.1 - tolerance && ib.0 < ia.1 - tolerance
}

/// Whether two coplanar triangles overlap: an edge of one properly crosses an edge of
/// the other, or a vertex of one is strictly inside the other.
fn coplanar_overlap(a: &[DVec3; 3], b: &[DVec3; 3]) -> bool {
    let normal = triangle_normal(a).abs();
    let flat = |p: DVec3| {
        if normal.x >= normal.y && normal.x >= normal.z {
            (p.y, p.z)
        } else if normal.y >= normal.z {
            (p.z, p.x)
        } else {
            (p.x, p.y)
        }
    };
    let (fa, fb) = (a.map(flat), b.map(flat));
    let cross = |o: (f64, f64), p: (f64, f64), q: (f64, f64)| {
        (p.0 - o.0) * (q.1 - o.1) - (p.1 - o.1) * (q.0 - o.0)
    };
    let crossing = (0..3).any(|first| {
        (0..3).any(|second| {
            let (start, end) = (fa[first], fa[(first + 1) % 3]);
            let (from, to) = (fb[second], fb[(second + 1) % 3]);
            cross(start, end, from) * cross(start, end, to) < 0.0
                && cross(from, to, start) * cross(from, to, end) < 0.0
        })
    });
    let inside = |triangle: &[(f64, f64); 3], point: (f64, f64)| {
        let signs = [
            cross(triangle[0], triangle[1], point),
            cross(triangle[1], triangle[2], point),
            cross(triangle[2], triangle[0], point),
        ];
        signs.iter().all(|&sign| sign > 0.0) || signs.iter().all(|&sign| sign < 0.0)
    };
    let a_in_b = fa.iter().any(|&point| inside(&fb, point));
    crossing || a_in_b || fb.iter().any(|&point| inside(&fa, point))
}

/// Whether the segment strictly crosses the interior of the triangle.
fn segment_crosses(segment: [DVec3; 2], triangle: &[DVec3; 3]) -> bool {
    let normal = triangle_normal(triangle);
    let (d0, d1) = (
        normal.dot(segment[0] - triangle[0]),
        normal.dot(segment[1] - triangle[0]),
    );
    let scale = normal.length() * (segment[1] - segment[0]).length();
    if d0 * d1 >= 0.0 || (d0 - d1).abs() < 1e-12 * scale {
        return false;
    }
    let point = segment[0] + (segment[1] - segment[0]) * (d0 / (d0 - d1));
    let edge = |i: usize| {
        let (p, q) = (triangle[i], triangle[(i + 1) % 3]);
        (q - p).cross(point - p).dot(normal)
    };
    let margin = 1e-9 * normal.length_squared();
    (0..3).all(|i| edge(i) > margin)
}

#[cfg(test)]
mod tests {
    use super::check_surface;

    /// A closed octahedron.
    fn octahedron() -> (Vec<[f32; 3]>, Vec<u32>) {
        let positions = vec![
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        let indices = vec![
            0, 2, 4, 2, 1, 4, 1, 3, 4, 3, 0, 4, 2, 0, 5, 1, 2, 5, 3, 1, 5, 0, 3, 5,
        ];
        (positions, indices)
    }

    #[test]
    fn octahedron_is_watertight() {
        let (positions, indices) = octahedron();
        let report = check_surface(&positions, &indices, true);
        assert!(report.is_watertight(), "{report:?}");
        assert_eq!((report.vertices, report.triangles), (6, 8));
    }

    #[test]
    fn seams_weld_and_holes_and_flips_are_found() {
        let (mut positions, mut indices) = octahedron();
        positions.push(positions[0]);
        indices[0] = 6;
        assert!(check_surface(&positions, &indices, true).is_watertight());
        let open = check_surface(&positions, &indices[3..], false);
        assert_eq!(open.boundary_edges, 3);
        let mut flipped = indices.clone();
        flipped.swap(0, 1);
        let report = check_surface(&positions, &flipped, false);
        assert_eq!(report.misoriented_edges, 3);
    }

    #[test]
    fn crossing_and_touching_pieces_are_found() {
        let (positions, indices) = octahedron();
        let mut both = positions.clone();
        both.extend(
            positions
                .iter()
                .map(|p| [p[0] + 0.5, p[1] + 0.3, p[2] + 0.2]),
        );
        let mut all = indices.clone();
        all.extend(indices.iter().map(|i| i + 6));
        let report = check_surface(&both, &all, true);
        assert!(report.is_closed_manifold());
        assert_eq!(report.components, 2);
        assert_ne!(report.intersections.len(), 0);
        let mut apart = positions.clone();
        apart.extend(positions.iter().map(|p| [p[0] + 2.0, p[1], p[2]]));
        let report = check_surface(&apart, &all, true);
        assert_eq!(report.intersections.len(), 0);
        assert_eq!(report.non_manifold_vertices, 1);
    }
}
