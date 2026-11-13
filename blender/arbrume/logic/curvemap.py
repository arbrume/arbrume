"""Conversion between species curves and the points of a Blender curve widget.

Blender's curve widget limits its range to [-100, 100], too small for angles in degrees,
so the widget shows each curve normalized to [0, 1] over a value range fixed when the
widget opens: ``low`` at the bottom, ``high`` at the top. The range is the slider range of
the parameter, widened to hold the current points. Linear species curves use vector
handles, smooth ones automatic clamped handles (Blender's closest match to the monotone
cubic of the generator).
"""

from __future__ import annotations

from .values import CurvePoints, clean_float

LINEAR_HANDLE = "VECTOR"
SMOOTH_HANDLE = "AUTO_CLAMPED"


def value_range(curve: CurvePoints, soft_min: float | None, soft_max: float | None):
    """``(low, high)`` value range of a widget showing ``curve``."""
    ys = [y for _, y in curve.points]
    low = min(ys) if soft_min is None else min(*ys, soft_min)
    if soft_max is not None and max(ys) <= soft_max and soft_max > low:
        high = soft_max
    elif max(ys) > low:
        high = low + 1.25 * (max(ys) - low)
    else:
        high = low + max(1.0, abs(low))
    return clean_float(low), clean_float(high)


def to_widget(curve: CurvePoints, low: float, high: float) -> list[tuple[float, float, str]]:
    """Widget points ``(x, y_normalized, handle_type)`` of a curve."""
    span = high - low
    handle = SMOOTH_HANDLE if curve.smooth else LINEAR_HANDLE
    return [(x, (y - low) / span, handle) for x, y in curve.normalized().points]


def from_widget(points: list[tuple[float, float, str]], low: float, high: float) -> CurvePoints:
    """Species curve of widget points ``(x, y_normalized, handle_type)``."""
    span = high - low
    smooth = any(handle != LINEAR_HANDLE for _, _, handle in points)
    curve = CurvePoints(tuple((x, low + y * span) for x, y, _ in points), smooth)
    return curve.normalized()
