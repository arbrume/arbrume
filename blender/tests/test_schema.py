"""Species introspection into a user-interface description."""

import arbrume
import pytest
from logic import schema, species


@pytest.fixture(scope="module")
def sections():
    return species.preset_schema()


def by_path(sections):
    return {spec.path: spec for section in sections for spec in section.params()}


def test_sections_in_order(sections):
    names = [s.name for s in sections]
    assert names[:5] == ["architecture", "growth", "geometry", "foliage", "bark"]
    assert "variation" not in names


def test_kinds_found_from_the_data(sections):
    specs = by_path(sections)
    assert specs["growth.axis_length"].kind == schema.CURVE
    assert specs["growth.axis_length"].by_order
    assert specs["growth.aging_angle"].kind == schema.CURVE
    assert not specs["growth.aging_angle"].by_order
    assert specs["growth.fork_angle"].kind == schema.CURVE
    assert specs["foliage.size_along_axis"].kind == schema.CURVE
    assert specs["growth.stems"].kind == schema.INT
    assert specs["foliage.length"].kind == schema.FLOAT
    assert specs["foliage.enabled"].kind == schema.BOOL
    assert specs["bark.color"].kind == schema.COLOR
    assert specs["bark.young_color"].kind == schema.COLOR
    assert specs["geometry.radial_segments"].kind == schema.VECTOR
    assert specs["geometry.radial_segments"].integer
    assert specs["foliage.shape"].kind == schema.ENUM
    assert set(specs["foliage.shape"].options) >= {"ovate", "needle", "lanceolate"}
    assert specs["architecture.branching"].options == ("monopodial", "sympodial")
    assert "architecture.axes" not in specs
    assert specs["foliage.twig.length"].group == "Twig"


def test_focus_curves_come_first(sections):
    growth = next(s for s in sections if s.name == "growth")
    first = growth.groups["Lengths and extension"][0]
    assert first.path == "growth.axis_length"
    assert all(spec.ident == spec.path.replace(".", "__") for spec in growth.params())


def test_unknown_parameters_are_shown_too():
    def probe(path, value):
        if path == "growth.new_curve" and not isinstance(value, str):
            return None
        return "invalid type"

    data = [{"growth": {"new_curve": 0.5, "new_count": 3, "new_mode": "a"}}]
    specs = by_path(schema.build_schema(data, probe))
    assert specs["growth.new_curve"].kind == schema.CURVE
    assert specs["growth.new_curve"].by_order
    assert specs["growth.new_curve"].group == "Other"
    assert specs["growth.new_count"].kind == schema.INT
    assert specs["growth.new_mode"].options == ("a",)
    assert specs["growth.new_count"].label == "New count"


def test_enum_options_from_messages():
    msg = "unknown variant `zz`, expected one of `elliptic`, `ovate`, `needle`"
    assert schema.enum_options(msg) == ("elliptic", "ovate", "needle")
    assert schema.enum_options("unknown variant `zz`, expected `a` or `b`") == ("a", "b")


def test_cache_and_effective_species():
    cache = species.SpeciesCache(capacity=2)
    source = species.Source(preset="shrub")
    base = cache.base(source)
    assert base["name"]
    eff = cache.effective(source, {"growth.gnarl": 7.0})
    assert eff["growth"]["gnarl"] == 7.0
    generated = species.full_species(source, {"growth.gnarl": 7.0})
    assert generated["growth"]["gnarl"] == 7.0
    bad = species.Source(toml="not = [valid")
    assert cache.base(bad) == {}
    assert bad.key in cache.errors
    assert schema.order_count(base) == base["architecture"]["max_order"] + 1
    assert species.readable_name("oak_like") == "Oak-like"
    assert species.readable_name("rosette_herb") == "Rosette herb"
    assert len(arbrume.presets()) > 3
