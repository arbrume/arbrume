"""Snapshot of plants generated through the Python API without surroundings.

One line per case: the species, the seed and a hash of the exact float32 bits of the
vertex positions of every mesh part. Those bits are the same for every thread count on
one platform, but not across platforms (the math library of each rounds functions such
as ``powf`` and ``sin`` its own way), so the golden file is
``surroundings_golden.<os>.txt`` with ``<os>`` one of ``windows``, ``linux`` and
``macos``. With ``ARBRUME_REGOLD=1`` the test writes the golden file of the current
platform instead of comparing with it; without it, a missing golden file fails the test
with a message that says how to make it.
"""

import hashlib
import os
import sys
from pathlib import Path

import arbrume
import pytest

SPECIES = ("shrub", "envelope_tree", "hedge_shrub")
SEEDS = (1, 2)
AGE = 6
PLATFORMS = {"win32": "windows", "linux": "linux", "darwin": "macos"}


def _golden() -> Path:
    """The golden file of the current platform."""
    system = PLATFORMS.get(sys.platform, sys.platform)
    return Path(__file__).parent / "fixtures" / f"surroundings_golden.{system}.txt"


def _line(species: str, seed: int) -> str:
    """The golden line of one plant."""
    tree = arbrume.generate(species, seed=seed, age=AGE, quality=0.5)
    digest = hashlib.sha256()
    for part in tree.parts:
        digest.update(part.kind.encode())
        digest.update(part.vertices.astype("<f4").tobytes())
    return f"{species} seed={seed} {digest.hexdigest()}"


def test_plants_match_the_snapshot() -> None:
    lines = [_line(species, seed) for species in SPECIES for seed in SEEDS]
    text = "\n".join(lines) + "\n"
    golden = _golden()
    if os.environ.get("ARBRUME_REGOLD") == "1":
        golden.parent.mkdir(exist_ok=True)
        golden.write_text(text, encoding="utf-8")
    if not golden.exists():
        pytest.fail(
            f"no golden file {golden}: run this test on this platform with ARBRUME_REGOLD=1 "
            "to write it, then commit it. For Linux and macOS the manual workflow "
            ".github/workflows/regold.yml writes the files and uploads them as the "
            "artifacts goldens-linux and goldens-macos."
        )
    assert golden.read_text(encoding="utf-8") == text
