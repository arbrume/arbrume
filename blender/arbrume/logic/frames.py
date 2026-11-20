"""Timeline keys of a growth sequence, without Blender.

Stage ``i`` of ``count`` is shown from frame ``start + i * step`` for ``step`` frames;
the last stage stays shown to the end. Every stage is hidden at the other frames, so one
stage is visible at a time from ``start`` on.
"""

from __future__ import annotations


def visibility_keys(count: int, start: int, step: int) -> list[list[tuple[int, bool]]]:
    """For every stage, its ``(frame, hidden)`` keys in frame order."""
    keys = []
    for index in range(count):
        shown = start + index * step
        stage = [(start, index != 0)]
        if index > 0:
            stage.append((shown, False))
        if index < count - 1:
            stage.append((shown + step, True))
        keys.append(stage)
    return keys


def visible_stage(count: int, start: int, step: int, frame: int) -> int:
    """The stage shown at ``frame``: the first before ``start``, the last at the end."""
    return min(max(frame - start, 0) // step, count - 1)


def last_frame(count: int, start: int, step: int) -> int:
    """The frame where the last stage has been shown for ``step`` frames."""
    return start + count * step - 1
