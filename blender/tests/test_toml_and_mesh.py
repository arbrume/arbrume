"""TOML export round trip through the generator, and mesh array preparation."""

import tomllib

import arbrume
import numpy as np
import pytest
from logic import meshdata, toml_out
from logic.species import Source, full_species


@pytest.mark.parametrize("preset", arbrume.presets())
def test_export_round_trip_gives_the_same_tree(preset):
    overrides = {"growth.gnarl": 7.5, "foliage.color": [0.1, 0.3, 0.05]}
    species = full_species(Source(preset=preset), overrides)
    text = toml_out.export_text(species, "My plant", f"Exported from {preset}")
    assert text.startswith("# Exported from")
    parsed = tomllib.loads(text)
    assert parsed["name"] == "My plant"
    original = arbrume.generate(preset, seed=3, age=6, quality=0.3, overrides=overrides)
    exported = arbrume.generate(text, seed=3, age=6, quality=0.3)
    assert exported.stats == original.stats
    assert exported.species_name == "My plant"
    for a, b in zip(original.parts, exported.parts, strict=True):
        assert np.array_equal(a.vertices, b.vertices)


def test_value_text():
    assert toml_out.value_text({"by_order": [1.0, [[0.0, 0.5]]]}) == (
        "{ by_order = [1.0, [[0.0, 0.5]]] }"
    )
    assert toml_out.key_text("growth.gnarl") == '"growth.gnarl"'
    assert toml_out.value_text([True, 'a"b']) == '[true, "a\\"b"]'
    with pytest.raises(TypeError):
        toml_out.value_text(object())


def test_nested_tables_are_written_after_scalars():
    text = toml_out.dumps({"a": {"x": 1, "sub": {"y": 2.0}, "z": "s"}, "name": "n"})
    assert tomllib.loads(text) == {"a": {"x": 1, "sub": {"y": 2.0}, "z": "s"}, "name": "n"}
    assert text.index("[a]") < text.index("[a.sub]")


def test_mesh_arrays_match_the_tree():
    tree = arbrume.generate("broadleaf", seed=2, age=8, quality=0.4)
    arrays = meshdata.build_arrays(tree.parts)
    assert arrays.vertex_count == tree.stats["vertices"]
    assert arrays.face_count == tree.stats["triangles"]
    assert arrays.kinds == ["bark", "leaf"]
    assert arrays.positions.dtype == np.float32
    assert arrays.corner_verts.dtype == np.int32
    assert arrays.corner_verts.max() < arrays.vertex_count
    bark = tree.parts[0]
    x, y, z = bark.vertices[0]
    assert arrays.positions[:3].tolist() == [x, -z, y]
    nb = bark.indices.shape[0]
    assert (arrays.material_indices[:nb] == 0).all()
    assert (arrays.material_indices[nb:] == 1).all()
    leaf_first = tree.parts[1].indices[0] + bark.vertex_count
    assert np.array_equal(arrays.corner_verts[3 * nb : 3 * nb + 3], leaf_first)
    assert arrays.corner_uvs.shape == (2 * 3 * arrays.face_count,)
    assert arrays.colors.shape == (4 * arrays.vertex_count,)
    assert np.allclose(np.linalg.norm(arrays.normals, axis=1), 1.0, atol=1e-3)


def test_axis_swap_is_a_rotation():
    a = np.array([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], dtype=np.float32)
    b = meshdata.y_up_to_z_up(a)
    assert np.linalg.det(b) == pytest.approx(1.0)
    assert b[1].tolist() == [0.0, 0.0, 1.0]


def test_material_key_is_stable():
    mat = {"base_color": [0.1, 0.2, 0.3, 1.0], "roughness": 0.5, "double_sided": True}
    key = meshdata.material_key("Generic broadleaf", "leaf", mat)
    assert key == meshdata.material_key("Generic broadleaf", "leaf", dict(mat))
    assert key != meshdata.material_key("Generic broadleaf", "bark", mat)
    assert meshdata.slot_order(["leaf", "bark", "flower"]) == ["bark", "leaf", "flower"]
