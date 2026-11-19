"""Round trip of the annotation sidecars: written by `arbrume gen`, read by NumPy.

Run with `uv run --no-project --with numpy --with pytest pytest tools/test_read_annotations.py`
after building the binary (`cargo build --release -p arbrume`). The binary is taken from
`ARBRUME_BIN`, else from `target/release`; the tests are skipped when there is none.
"""

import json
import os
import shutil
import struct
import subprocess
from pathlib import Path

import numpy as np
import pytest
import read_annotations as ra

ROOT = Path(__file__).resolve().parent.parent
GLTF_TYPES = {5121: "u1", 5123: "<u2", 5125: "<u4", 5126: "<f4"}
GLTF_WIDTH = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


def binary():
    """Path of the `arbrume` binary, or None."""
    given = os.environ.get("ARBRUME_BIN")
    if given:
        return Path(given)
    for name in ("arbrume", "arbrume.exe"):
        path = ROOT / "target" / "release" / name
        if path.is_file():
            return path
    return shutil.which("arbrume") and Path(shutil.which("arbrume"))


@pytest.fixture(scope="module")
def dataset(tmp_path_factory):
    """A directory of three annotated conifers (twig stems, fins and needles)."""
    exe = binary()
    if exe is None or not exe.is_file():
        pytest.skip("no arbrume binary: build it or set ARBRUME_BIN")
    out = tmp_path_factory.mktemp("annotated")
    command = [str(exe), "gen", "--species", "conifer", "--count", "3", "--age", "12"]
    command += ["--seed", "4", "--quality", "0.5", "--manifest", "--out", str(out)]
    command += ["--annotations", "skeleton,labels,points=1000,branches=5,metrics,json"]
    subprocess.run(command, check=True, capture_output=True)
    return out


def glb_attribute(path, primitive, name):
    """Values of attribute `name` of primitive `primitive` of a `.glb` file."""
    data = Path(path).read_bytes()
    json_length = struct.unpack_from("<I", data, 12)[0]
    document = json.loads(data[20 : 20 + json_length])
    binary_start = 20 + json_length + 8
    attributes = document["meshes"][0]["primitives"][primitive]["attributes"]
    accessor = document["accessors"][attributes[name]]
    view = document["bufferViews"][accessor["bufferView"]]
    count = accessor["count"] * GLTF_WIDTH[accessor["type"]]
    offset = binary_start + view["byteOffset"] + accessor.get("byteOffset", 0)
    dtype = np.dtype(GLTF_TYPES[accessor["componentType"]])
    return np.frombuffer(data, dtype=dtype, count=count, offset=offset)


def test_npz_and_json_hold_equal_arrays(dataset):
    for npz in sorted(dataset.glob("*.ann.npz")):
        binary_form = ra.load_sidecar(npz)
        text_form = ra.load_sidecar_json(npz.with_name(npz.name.replace(".npz", ".json")))
        assert binary_form.keys() == text_form.keys()
        assert binary_form["meta"] == text_form["meta"]
        for name, array in binary_form.items():
            if name != "meta":
                assert array.dtype == text_form[name].dtype, name
                assert np.array_equal(array, text_form[name]), name
        ra.check_sidecar(binary_form)


def test_labels_match_the_glb_attributes(dataset):
    for npz in sorted(dataset.glob("*.ann.npz")):
        sidecar = ra.load_sidecar(npz)
        offsets = sidecar["part_offsets"]
        glb = npz.with_name(npz.name.replace(".ann.npz", ".glb"))
        for part in range(len(sidecar["part_kind"])):
            rows = slice(offsets[part], offsets[part + 1])
            semantic = glb_attribute(glb, part, "_SEMANTIC")
            instance = glb_attribute(glb, part, "_INSTANCE")
            assert np.array_equal(semantic, sidecar["vertex_semantic"][rows])
            assert np.array_equal(instance, sidecar["vertex_instance"][rows])
        semantic = sidecar["vertex_semantic"]
        assert set(np.unique(semantic)) <= {0, 1, 2, 3}
        assert (semantic == 2).any(), "conifer twig stems are labelled twig"
        bark = semantic <= 1
        chains = set(sidecar["chain_id"].tolist())
        assert set(sidecar["vertex_instance"][bark].tolist()) <= chains
        assert sidecar["vertex_instance"][~bark].max() < len(sidecar["organ_node"])


def test_points_and_manifest(dataset):
    lines = ra.read_manifest(dataset)
    assert len(lines) == 3
    for line in lines:
        files = line["annotations"]
        points = ra.load_points(dataset / files["points"])
        assert points["xyz"].shape == (1000, 3)
        assert np.allclose(np.linalg.norm(points["normal"], axis=1), 1.0, atol=1e-4)
        assert points["semantic"].max() <= 3
        names, values = ra.parameter_vector(line["species_params"])
        assert len(names) == len(values) > 50
        assert "growth" in {name.split(".")[0] for name in names}
        assert line["metrics"]["global"]["tip_count"] > 0
        sidecar = ra.load_sidecar(dataset / files["sidecar"])
        assert sidecar["branch_polyline"].shape[1:] == (5, 4)
        assert sidecar["meta"]["extras"]["seed"] == line["plant_seed"]
