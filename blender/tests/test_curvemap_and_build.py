"""Curve widget conversion and the packaging helpers of ``build_extension.py``."""

import importlib.util
from pathlib import Path

import pytest
from logic import curvemap
from logic.values import CurvePoints

BUILD = Path(__file__).resolve().parents[1] / "build_extension.py"


@pytest.fixture(scope="module")
def build():
    spec = importlib.util.spec_from_file_location("build_extension", BUILD)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_widget_round_trip():
    curve = CurvePoints(((0.0, 85.0), (0.5, 60.0), (1.0, 35.0)))
    low, high = curvemap.value_range(curve, 0.0, 180.0)
    assert (low, high) == (0.0, 180.0)
    widget = curvemap.to_widget(curve, low, high)
    assert widget[0] == (0.0, 85.0 / 180.0, curvemap.LINEAR_HANDLE)
    assert curvemap.from_widget(widget, low, high).close_to(curve)
    smooth = CurvePoints(curve.points, smooth=True)
    widget = curvemap.to_widget(smooth, low, high)
    assert {h for _, _, h in widget} == {curvemap.SMOOTH_HANDLE}
    assert curvemap.from_widget(widget, low, high).smooth


def test_value_range_holds_the_points():
    assert curvemap.value_range(CurvePoints.flat(1.0), 0.0, 1.0) == (0.0, 1.0)
    assert curvemap.value_range(CurvePoints(((0, 0.0), (1, 30.0))), 0.0, 20.0) == (0.0, 37.5)
    assert curvemap.value_range(CurvePoints(((0, -5.0), (1, 5.0))), 0.0, None) == (-5.0, 7.5)
    low, high = curvemap.value_range(CurvePoints.flat(3.0), None, None)
    assert low == 3.0 and high > low


def test_wheel_platforms(build):
    assert build.wheel_platforms("arbrume-0.0.1-cp311-abi3-win_amd64.whl") == ["windows-x64"]
    linux = "arbrume-0.0.1-cp311-abi3-manylinux_2_17_aarch64.manylinux2014_aarch64.whl"
    assert build.wheel_platforms(linux) == ["linux-arm64"]
    assert build.wheel_platforms("a-1-cp311-abi3-manylinux_2_28_x86_64.whl") == ["linux-x64"]
    mac = "a-1-cp311-abi3-macosx_10_12_x86_64.macosx_11_0_arm64.macosx_10_12_universal2.whl"
    assert build.wheel_platforms(mac) == ["macos-arm64", "macos-x64"]
    assert build.wheel_platforms("a-1-cp311-abi3-macosx_11_0_arm64.whl") == ["macos-arm64"]


def test_manifest_lists_are_replaced(build):
    text = (Path(build.PACKAGE) / "blender_manifest.toml").read_text("utf-8")
    out = build.manifest_for(text, ["linux-x64"], ["./wheels/a.whl"])
    assert 'platforms = ["linux-x64"]' in out
    assert 'wheels = ["./wheels/a.whl"]' in out
    assert 'id = "arbrume"' in out
    assert 'blender_version_min = "4.2.0"' in out
