"""Array shapes, dtypes, zero-copy access and lifetime of the NumPy views."""

import gc

import arbrume
import numpy as np
import pytest

PART_ARRAYS = {
    "vertices": (3, np.float32),
    "normals": (3, np.float32),
    "uvs": (2, np.float32),
    "colors": (4, np.float32),
    "indices": (3, np.uint32),
}


def test_part_shapes_and_dtypes(tree: arbrume.Tree) -> None:
    kinds = [part.kind for part in tree.parts]
    assert "bark" in kinds
    for part in tree.parts:
        for name, (columns, dtype) in PART_ARRAYS.items():
            array = getattr(part, name)
            assert array.dtype == dtype, name
            assert array.ndim == 2
            assert array.shape[1] == columns, name
        rows = part.vertices.shape[0]
        assert rows == part.vertex_count
        assert part.indices.shape[0] == part.triangle_count
        assert int(part.indices.max()) < rows
        for name in ("normals", "uvs", "colors"):
            assert getattr(part, name).shape[0] in (0, rows)


def test_skeleton_and_organs(tree: arbrume.Tree) -> None:
    skeleton = tree.skeleton
    nodes = len(skeleton)
    assert nodes == tree.stats["nodes"]
    assert skeleton.positions.shape == (nodes, 3)
    assert skeleton.positions.dtype == np.float32
    expected = {
        "radii": np.float32,
        "parents": np.int32,
        "orders": np.uint8,
        "births": np.uint16,
        "chains": np.int32,
        "shed": np.bool_,
    }
    for name, dtype in expected.items():
        array = getattr(skeleton, name)
        assert array.shape == (nodes,), name
        assert array.dtype == dtype, name
    parents = skeleton.parents
    assert parents[0] == -1
    assert np.all(parents < np.arange(nodes))
    organs = tree.organs
    assert (
        int(np.sum(organs.kinds == arbrume.ORGAN_KINDS.index("leaf"))) == tree.stats["leaf_sites"]
    )
    assert organs.directions.shape == (len(organs), 3)
    assert np.all(organs.nodes < nodes)
    assert set(np.unique(organs.kinds)) <= set(range(len(arbrume.ORGAN_KINDS)))


def test_arrays_are_views_without_copy(tree: arbrume.Tree) -> None:
    part = tree.parts[0]
    first, second = part.vertices, part.vertices
    assert first is not second
    assert np.shares_memory(first, second)
    assert first.__array_interface__["data"][0] == second.__array_interface__["data"][0]
    assert not first.flags.owndata
    assert isinstance(first.base, memoryview)
    assert isinstance(first.base.obj, arbrume.Buffer)
    assert np.shares_memory(tree.skeleton.radii, tree.skeleton.radii)


def test_arrays_are_read_only(tree: arbrume.Tree) -> None:
    vertices = tree.parts[0].vertices
    assert not vertices.flags.writeable
    with pytest.raises(ValueError, match="read-only"):
        vertices[0, 0] = 1.0
    copy = vertices.copy()
    copy[0, 0] = 1.0
    assert copy.flags.writeable


def test_buffer_protocol_without_numpy(tree: arbrume.Tree) -> None:
    part = tree.parts[0]
    view = memoryview(part.buffer("vertices"))
    assert view.readonly
    assert view.format == "f"
    assert view.shape == (part.vertex_count, 3)
    assert view.c_contiguous
    assert view.tolist()[0] == part.vertices[0].tolist()
    with pytest.raises(ValueError, match="no array"):
        part.buffer("nope")


def test_arrays_outlive_the_tree() -> None:
    tree = arbrume.generate("shrub", seed=3, age=3)
    vertices = tree.parts[-1].vertices
    parents = tree.skeleton.parents
    expected = vertices.copy(), parents.copy()
    del tree
    gc.collect()
    filler = [np.ones(len(expected[0]) * 3, np.float32) for _ in range(20)]
    assert np.array_equal(vertices, expected[0])
    assert np.array_equal(parents, expected[1])
    assert filler[0].sum() > 0
