"""Root systems through the Python API: the ``roots`` and ``soil`` keywords."""

import arbrume
import pytest


def _vertices(tree: arbrume.Tree) -> int:
    return sum(len(part.vertices) for part in tree.parts)


def test_a_habit_adds_roots_and_false_removes_them() -> None:
    plain = arbrume.generate("broadleaf", seed=1, age=8, quality=0.5)
    rooted = arbrume.generate("broadleaf", seed=1, age=8, quality=0.5, roots="plate")
    assert _vertices(rooted) > _vertices(plain)
    assert rooted.metadata["species"]["roots"]["habit"] == "plate"
    removed = arbrume.generate("oak_like_rooted", seed=1, age=8, quality=0.5, roots=False)
    assert "roots" not in removed.metadata["species"]
    kept = arbrume.generate("oak_like_rooted", seed=1, age=8, quality=0.5, roots=None)
    assert kept.metadata["species"]["roots"]["habit"] == "tap"


def test_dotted_overrides_and_dicts_change_one_field() -> None:
    tree = arbrume.generate("maple_like_rooted", seed=2, age=6, **{"roots.exposed": 0.5})
    assert tree.metadata["species"]["roots"]["exposed"] == 0.5
    tree = arbrume.generate("broadleaf", seed=2, age=6, roots={"habit": "tap", "primaries": 3})
    roots = tree.metadata["species"]["roots"]
    assert roots["primaries"] == 3
    assert roots["tap_root"] is True


def test_soil_shapes_the_roots() -> None:
    flat = arbrume.generate("maple_like_rooted", seed=3, age=8, quality=0.5)
    soil = {"ground": {"kind": "slope", "rise": [0.4, 0.0]}}
    sloped = arbrume.generate("maple_like_rooted", seed=3, age=8, quality=0.5, soil=soil)
    assert _vertices(flat) != _vertices(sloped) or any(
        (a.vertices != b.vertices).any() for a, b in zip(flat.parts, sloped.parts, strict=True)
    )


def test_bad_roots_and_soil_are_rejected() -> None:
    with pytest.raises(ValueError):
        arbrume.generate("broadleaf", roots="taproot")
    with pytest.raises(TypeError):
        arbrume.generate("broadleaf", roots=3)
    with pytest.raises(ValueError):
        arbrume.generate("broadleaf", roots=True, soil={"grund": 1})
