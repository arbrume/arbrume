"""Snapshot of plants generated through the Python API without surroundings.

One line per case: the species, the seed and a hash of the exact float32 bits of the
vertex positions of every mesh part. With ``ARBRUME_REGOLD=1`` the test writes its
golden file instead of comparing with it.
"""

import hashlib
import os
from pathlib import Path

import arbrume

GOLDEN = Path(__file__).parent / "fixtures" / "surroundings_golden.txt"
SPECIES = ("shrub", "envelope_tree", "hedge_shrub")
SEEDS = (1, 2)
AGE = 6


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
    if os.environ.get("ARBRUME_REGOLD") == "1":
        GOLDEN.parent.mkdir(exist_ok=True)
        GOLDEN.write_text(text, encoding="utf-8")
    assert GOLDEN.read_text(encoding="utf-8") == text
