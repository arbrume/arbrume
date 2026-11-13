"""Shared fixtures of the arbrume binding tests."""

import arbrume
import pytest

SMALL = {"species": "shrub", "seed": 7, "age": 4}


@pytest.fixture(scope="session")
def tree() -> arbrume.Tree:
    """A small shrub shared by the read-only tests."""
    return arbrume.generate(SMALL["species"], seed=SMALL["seed"], age=SMALL["age"])
