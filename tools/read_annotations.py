"""Load the ML annotation sidecars written by `arbrume gen --annotations`.

Pure Python: the standard library and NumPy only. The formats are described in
`docs/annotations.md`; in short, for a plant `<stem>.glb`:

* `<stem>.ann.npz`: uncompressed NumPy archive of the skeleton, the per-vertex labels and
  the branch table, plus `meta`, the UTF-8 bytes of a JSON object (format, version, class
  names, description of every array). `numpy.load` opens it directly.
* `<stem>.ann.json`: the same arrays as nested lists, under the same keys, with `meta` as
  an object.
* `<stem>.points.ply`: binary little-endian PLY point cloud with normals and labels.

Library use::

    import read_annotations as ra
    ann = ra.load_sidecar("out/broadleaf_1_0.ann.npz")
    ann["node_position"].shape, ann["meta"]["semantic_classes"]
    points = ra.load_points("out/broadleaf_1_0.points.ply")
    points["xyz"], points["semantic"], points["instance"]

Command line: `python tools/read_annotations.py FILE_OR_DIR ...` prints a summary of each
sidecar and point cloud and checks the invariants of `check_sidecar`; `--time` also
prints load times.
"""

import argparse
import json
import sys
import time
from pathlib import Path

import numpy as np

FORMAT = "arbrume-annotations"
VERSION = 1
SEMANTIC_CLASSES = ("trunk", "branch", "twig", "leaf")
PLY_TYPES = {"float": "<f4", "uchar": "u1", "uint": "<u4", "int": "<i4", "ushort": "<u2"}


def _check_meta(meta, source):
    """Raise ValueError unless `meta` names a sidecar of the supported version."""
    if meta.get("format") != FORMAT or meta.get("version") != VERSION:
        raise ValueError(f"{source}: not a version {VERSION} {FORMAT} file")
    return meta


def load_sidecar(path):
    """Return the arrays of a `.ann.npz` sidecar as a dict, with `meta` decoded to a dict.

    Arrays keep the dtypes and shapes given in `meta["arrays"]`.
    """
    with np.load(path, allow_pickle=False) as archive:
        arrays = {name: archive[name] for name in archive.files}
    meta = json.loads(arrays.pop("meta").tobytes().decode("utf-8"))
    arrays["meta"] = _check_meta(meta, path)
    return arrays


def load_sidecar_json(path):
    """Return the arrays of a `.ann.json` sidecar as NumPy arrays, like `load_sidecar`.

    Dtypes come from `meta["arrays"]`; empty arrays get their trailing dimensions back from
    the symbolic shape, so the result equals that of the `.npz` file.
    """
    document = json.loads(Path(path).read_text(encoding="utf-8"))
    meta = _check_meta(document.pop("meta"), path)
    arrays = {}
    for name, values in document.items():
        described = meta["arrays"][name]
        array = np.asarray(values, dtype=np.dtype(described["dtype"]))
        if array.size == 0:
            trailing = described["shape"].split(",")[1:]
            fixed = [int(d) if d.isdigit() else meta.get("branch_samples") or 0 for d in trailing]
            array = array.reshape([0, *fixed])
        arrays[name] = array
    arrays["meta"] = meta
    return arrays


def load_points(path):
    """Return a binary little-endian PLY point cloud as a dict of arrays.

    Keys: `xyz` (N, 3) float32, `normal` (N, 3) float32, `semantic` (N,) uint8,
    `instance` (N,) uint32, and `comments`, the list of header comment lines.
    """
    data = Path(path).read_bytes()
    end = data.index(b"end_header\n") + len(b"end_header\n")
    header = data[:end].decode("ascii").splitlines()
    if header[0] != "ply" or header[1] != "format binary_little_endian 1.0":
        raise ValueError(f"{path}: not a binary little-endian PLY file")
    fields, count, comments = [], 0, []
    for line in header[2:]:
        words = line.split()
        if words[0] == "comment":
            comments.append(line[len("comment ") :])
        elif words[0] == "element":
            count = int(words[2])
        elif words[0] == "property":
            fields.append((words[2], PLY_TYPES[words[1]]))
    rows = np.frombuffer(data, dtype=np.dtype(fields), count=count, offset=end)
    return {
        "xyz": np.stack([rows["x"], rows["y"], rows["z"]], axis=1),
        "normal": np.stack([rows["nx"], rows["ny"], rows["nz"]], axis=1),
        "semantic": rows["semantic"].copy(),
        "instance": rows["instance"].copy(),
        "comments": comments,
    }


def chain_rows(sidecar):
    """Return a dict from chain id (the bark instance id) to its chain row."""
    return {int(chain): row for row, chain in enumerate(sidecar["chain_id"])}


def subtree_end(parents):
    """Return, for each row of a parent array in depth-first order, the end of its subtree.

    The subtree of row `i` is the rows `i` to `end[i] - 1`.
    """
    sizes = np.ones(len(parents), dtype=np.int64)
    for row in range(len(parents) - 1, -1, -1):
        if parents[row] >= 0:
            sizes[parents[row]] += sizes[row]
    return np.arange(len(parents)) + sizes


def parameter_vector(species_params, prefix=""):
    """Flatten the `species_params` object of a manifest line into (names, values).

    Numbers and booleans become floats under their dotted path, list items get their
    index (`growth.axis_length.by_order.0`); strings are left out. Names are sorted, so
    vectors of plants of the same species line up.
    """
    flat = {}

    def walk(value, path):
        if isinstance(value, bool | int | float):
            flat[path] = float(value)
        elif isinstance(value, dict):
            for key, item in value.items():
                walk(item, f"{path}.{key}" if path else key)
        elif isinstance(value, list):
            for index, item in enumerate(value):
                walk(item, f"{path}.{index}")

    walk(species_params, prefix)
    names = sorted(flat)
    return names, np.array([flat[name] for name in names], dtype=np.float64)


def read_manifest(directory):
    """Return the lines of `manifest.jsonl` in `directory` as a list of dicts."""
    text = (Path(directory) / "manifest.jsonl").read_text(encoding="utf-8")
    return [json.loads(line) for line in text.splitlines() if line]


def check_sidecar(sidecar):
    """Raise AssertionError when a sidecar breaks one of its documented invariants."""
    if "node_parent" in sidecar:
        parents = sidecar["node_parent"]
        rows = np.arange(len(parents))
        assert np.all(parents < rows), "a parent row comes after its child"
        end = subtree_end(parents)
        has_parent = parents >= 0
        assert np.all(end[rows[has_parent]] <= end[parents[has_parent]]), "subtree not contiguous"
        first, last = sidecar["chain_first"], sidecar["chain_last"]
        assert np.all(np.diff(first) > 0), "chain rows not in depth-first order"
        for row, chain in enumerate(sidecar["chain_id"]):
            members = sidecar["node_chain"][first[row] : last[row] + 1]
            assert np.all(members == chain), f"chain {chain} is not contiguous"
    if "vertex_semantic" in sidecar:
        semantic = sidecar["vertex_semantic"]
        assert semantic.max(initial=0) < len(SEMANTIC_CLASSES), "unknown semantic class"
        assert sidecar["part_offsets"][-1] == len(semantic), "labels do not cover the mesh"
    if "branch_parent" in sidecar:
        parents = sidecar["branch_parent"]
        assert np.all(parents < np.arange(len(parents))), "branch parent after its child"


def summary(path, timed):
    """Return a one-line description of the sidecar or point cloud at `path`."""
    start = time.perf_counter()
    if path.name.endswith(".ply"):
        points = load_points(path)
        took = time.perf_counter() - start
        classes = np.bincount(points["semantic"], minlength=len(SEMANTIC_CLASSES))
        text = f"{path.name}: {len(points['xyz'])} points, per class {classes.tolist()}"
    else:
        loader = load_sidecar_json if path.name.endswith(".json") else load_sidecar
        sidecar = loader(path)
        took = time.perf_counter() - start
        check_sidecar(sidecar)
        sizes = {
            "nodes": len(sidecar.get("node_parent", [])),
            "chains": len(sidecar.get("chain_id", [])),
            "organs": len(sidecar.get("organ_node", [])),
            "vertices": len(sidecar.get("vertex_semantic", [])),
            "branches": len(sidecar.get("branch_chain", [])),
        }
        text = f"{path.name}: " + ", ".join(f"{v} {k}" for k, v in sizes.items())
    if timed:
        text += f", loaded in {took * 1e3:.2f} ms"
    return text


def main(argv=None):
    """Print a checked summary of every annotation file given or found in a directory."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument("--time", action="store_true", help="print load times")
    args = parser.parse_args(argv)
    suffixes = (".ann.npz", ".ann.json", ".points.ply")
    for path in args.paths:
        files = sorted(path.iterdir()) if path.is_dir() else [path]
        for file in files:
            if file.name.endswith(suffixes):
                print(summary(file, args.time))
    return 0


if __name__ == "__main__":
    sys.exit(main())
