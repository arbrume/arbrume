"""`generate_many`: determinism, job lists, threads and release of the GIL."""

import threading
import time

import arbrume
import pytest

SEEDS = list(range(6))


def glbs(trees: list[arbrume.Tree]) -> list[bytes]:
    """The glb bytes of every tree, which cover mesh, parameters and metadata."""
    return [tree.to_glb() for tree in trees]


def test_batch_equals_a_loop_whatever_the_threads() -> None:
    expected = glbs([arbrume.generate("shrub", seed=s, age=3, quality=0.5) for s in SEEDS])
    for threads in (None, 1, 2, 5):
        trees = arbrume.generate_many("shrub", SEEDS, 3, quality=0.5, threads=threads)
        assert [tree.seed for tree in trees] == SEEDS
        assert glbs(trees) == expected, threads


def test_job_dicts_with_defaults() -> None:
    jobs = [
        {"species": "shrub", "seed": 1, "age": 3},
        {"species": "conifer", "seed": 2, "age": 2, "quality": 1.0},
        {"species": "shrub", "seed": 3, "overrides": {"foliage.length": 0.2}},
    ]
    trees = arbrume.generate_many(jobs, age=2, quality=0.5, threads=3)
    expected = [
        arbrume.generate("shrub", seed=1, age=3, quality=0.5),
        arbrume.generate("conifer", seed=2, age=2, quality=1.0),
        arbrume.generate("shrub", seed=3, age=2, quality=0.5, **{"foliage.length": 0.2}),
    ]
    assert glbs(trees) == glbs(expected)


def test_batch_errors() -> None:
    with pytest.raises(ValueError, match="job 1"):
        arbrume.generate_many([{"species": "shrub"}, {"species": "shrub", "scale": 0.0}])
    with pytest.raises(ValueError, match="no `species`"):
        arbrume.generate_many([{"seed": 1}])
    with pytest.raises(TypeError, match="seeds"):
        arbrume.generate_many("shrub")
    with pytest.raises(ValueError, match="threads"):
        arbrume.generate_many("shrub", [1], threads=0)
    assert arbrume.generate_many("shrub", []) == []


def timed_batch(progress: dict, count: int) -> tuple[float, int]:
    """Seconds taken by a batch of `count` trees and the counter increments meanwhile."""
    before = progress["count"]
    start = time.perf_counter()
    arbrume.generate_many("broadleaf", range(count), 30, threads=2)
    return time.perf_counter() - start, progress["count"] - before


def test_the_gil_is_released_during_a_batch() -> None:
    progress = {"count": 0, "running": True}

    def count() -> None:
        while progress["running"]:
            progress["count"] += 1

    worker = threading.Thread(target=count)
    worker.start()
    time.sleep(0.05)
    try:
        trees = 16
        elapsed, during = timed_batch(progress, trees)
        while elapsed < 0.2 and trees < 1024:
            trees *= 2
            elapsed, during = timed_batch(progress, trees)
    finally:
        progress["running"] = False
        worker.join()
    assert elapsed >= 0.2, "batch too short to observe the GIL"
    assert during > 10_000, f"{during} increments in {elapsed:.2f} s"
