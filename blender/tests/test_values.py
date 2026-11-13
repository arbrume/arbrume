"""Overrides, value comparison and curve conversions."""

import numpy as np
import pytest
from logic import values
from logic.values import CurvePoints


def test_f32_text_is_short_and_exact():
    assert values.f32_text(0.12999999523162842) == "0.13"
    assert values.f32_text(15.0) == "15.0"
    assert values.f32_text(1e-5) == "1e-05"
    for x in (0.1, 1 / 3, 123456.789, 2.5e-7):
        assert np.float32(float(values.f32_text(x))) == np.float32(x)
    with pytest.raises(ValueError):
        values.f32_text(float("nan"))


def test_apply_overrides_matches_generator_rules():
    base = {"growth": {"gnarl": 1.0, "stems": 1}, "foliage": {"twig": {"length": 0.1}}}
    out = values.apply_overrides(
        base, {"growth.gnarl": 5.0, "foliage": {"length": 0.2}, "foliage.twig.leaves": 3}
    )
    assert out["growth"] == {"gnarl": 5.0, "stems": 1}
    assert out["foliage"] == {"twig": {"length": 0.1, "leaves": 3}, "length": 0.2}
    assert base["growth"]["gnarl"] == 1.0


def test_with_override_drops_base_values():
    base = {"growth": {"gnarl": 0.12999999523162842}}
    assert values.with_override({}, base, "growth.gnarl", 0.13) == {}
    assert values.with_override({}, base, "growth.gnarl", 0.5) == {"growth.gnarl": 0.5}
    kept = {"growth.gnarl": 2.0, "foliage.twig.length": 1.0}
    assert values.without_override(kept, "foliage.twig") == {"growth.gnarl": 2.0}
    assert values.without_override(kept, "growth.gnarl") == {"foliage.twig.length": 1.0}


def test_overrides_text_round_trip_and_bad_text():
    data = {"b": [0.12999999523162842, 1], "a": {"by_order": [1.0]}}
    text = values.dump_overrides(data)
    assert values.load_overrides(text) == {"a": {"by_order": [1.0]}, "b": [0.13, 1]}
    assert values.load_overrides("") == {}
    assert values.load_overrides("{not json") == {}
    assert values.load_overrides("[1, 2]") == {}


def test_entries_and_orders():
    value = {"by_order": [15.0, [[0.0, 1.0], [1.0, 2.0]], 0.4]}
    assert values.entry_at(value, 0) == 15.0
    assert values.entry_at(value, 7) == 0.4
    assert values.entry_points(values.entry_at(value, 1)) == CurvePoints(((0, 1), (1, 2)))
    smooth = {"points": [[0.0, 1.0], [1.0, 0.0]], "interp": "smooth"}
    assert values.entry_points(smooth).smooth
    assert values.entry_constant(smooth) is None
    assert values.entry_at(3.0, 2) == 3.0


def test_curve_points_both_ways():
    curve = CurvePoints(((1.2, 2.0), (0.5, 1.0), (0.5, 3.0), (-1, 0.12999999523162842)))
    entry = values.points_entry(curve)
    assert entry == [[0.0, 0.13], [0.5, 3.0], [1.0, 2.0]]
    back = values.entry_points(entry)
    assert back.close_to(curve.normalized())
    smooth = values.points_entry(CurvePoints(((0, 1), (1, 2)), smooth=True))
    assert smooth == {"points": [[0.0, 1.0], [1.0, 2.0]], "interp": "smooth"}
    assert values.entry_points(smooth).smooth
    assert CurvePoints.flat(2.0).mean() == 2.0
    assert CurvePoints(((0.0, 0.0), (1.0, 1.0))).mean() == 0.5
    assert CurvePoints(((0.5, 4.0),)).normalized().points == ((0.0, 4.0), (1.0, 4.0))


def test_with_entry_keeps_other_orders():
    value = 3.0
    out = values.with_entry(value, 1, [[0.0, 1.0], [1.0, 2.0]], order_count=4)
    assert out == {"by_order": [3.0, [[0.0, 1.0], [1.0, 2.0]], 3.0]}
    assert values.with_entry(out, 1, 3.0, order_count=4) == 3.0
    tail = values.with_entry({"by_order": [1.0, 2.0]}, 3, 9.0, order_count=4)
    assert tail == {"by_order": [1.0, 2.0, 2.0, 9.0]}
    assert values.with_entry([[0.0, 1.0], [1.0, 1.0]], 0, 5.0, order_count=1) == 5.0
