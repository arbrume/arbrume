"""Species packs from Python: load, install, list, remove, and generate their species."""

import struct
import zlib
from pathlib import Path

import arbrume
import pytest


def png(side: int, rgba: tuple[int, int, int, int]) -> bytes:
    """A square 8-bit RGBA PNG of one colour."""

    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    row = b"\x00" + bytes(rgba) * side
    header = struct.pack(">IIBBBBB", side, side, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(row * side))
        + chunk(b"IEND", b"")
    )


def make_pack(root: Path, pack_id: str = "pytest_pack") -> Path:
    """A pack directory with a plain herb and a herb whose bark is an image."""
    pack = root / pack_id
    (pack / "species").mkdir(parents=True)
    (pack / "textures").mkdir()
    (pack / "pack.toml").write_text(
        f'format = 1\nid = "{pack_id}"\nname = "Pytest pack"\nversion = "1.2.3"\n'
        'content_licence = "PLACEHOLDER"\nlicence_file = "LICENCE-CONTENT.txt"\n'
        '[[species]]\nname = "herb"\ndisplay_name = "Herb"\ncategory = "herb"\n'
        '[[species]]\nname = "barked"\ndisplay_name = "Barked herb"\n',
        encoding="utf-8",
    )
    (pack / "LICENCE-CONTENT.txt").write_text("PLACEHOLDER\n", encoding="utf-8")
    herb = arbrume.preset_toml("rosette_herb")
    (pack / "species" / "herb.toml").write_text(herb, encoding="utf-8")
    barked = herb.replace("[bark]", '[bark]\ntexture = "textures/bark.png"')
    assert "textures/bark.png" in barked
    (pack / "species" / "barked.toml").write_text(barked, encoding="utf-8")
    (pack / "textures" / "bark.png").write_bytes(png(16, (200, 30, 30, 255)))
    return pack


def test_loaded_pack_species_generate_like_their_text(tmp_path: Path) -> None:
    pack = arbrume.load_pack(make_pack(tmp_path))
    assert pack["id"] == "pytest_pack"
    assert pack["version"] == "1.2.3"
    assert not pack["archive"]
    ids = [species["id"] for species in pack["species"]]
    assert ids == ["pytest_pack/herb", "pytest_pack/barked"]
    assert pack["species"][0]["display_name"] == "Herb"
    assert pack["species"][0]["thumbnail"] is None
    text = arbrume.preset_toml("rosette_herb")
    tree = arbrume.generate("pytest_pack/herb", seed=3, age=2)
    assert tree.to_glb() == arbrume.generate(text, seed=3, age=2).to_glb()
    assert any(p["id"] == "pytest_pack" for p in arbrume.packs(tmp_path / "none"))


def test_pack_images_reach_the_materials(tmp_path: Path) -> None:
    arbrume.load_pack(make_pack(tmp_path))
    tree = arbrume.generate("pytest_pack/barked", seed=1, age=2, textures=True)
    bark = next(part for part in tree.parts if part.kind == "bark")
    texture = bark.material["base_color_texture"]
    assert texture is not None and texture.startswith(b"\x89PNG")
    plain = arbrume.generate("pytest_pack/herb", seed=1, age=2, textures=True)
    plain_bark = next(part for part in plain.parts if part.kind == "bark")
    assert plain_bark.material["base_color_texture"] != texture


def test_install_list_remove(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    store = tmp_path / "store"
    monkeypatch.setenv("ARBRUME_PACKS_DIR", str(store))
    assert Path(arbrume.packs_dir()) == store
    installed = arbrume.install_pack(make_pack(tmp_path, "installed_pack"))
    assert installed["id"] == "installed_pack"
    assert (store / "installed_pack" / "MANIFEST.sha256").is_file()
    thumbnail = installed["species"][0]["thumbnail"]
    assert thumbnail is not None and Path(thumbnail).is_file()
    assert "installed_pack" in [p["id"] for p in arbrume.packs()]
    tree = arbrume.generate("installed_pack/herb", seed=2, age=1)
    assert tree.stats["triangles"] > 0
    arbrume.remove_pack("installed_pack")
    assert "installed_pack" not in [p["id"] for p in arbrume.packs()]
    with pytest.raises(FileNotFoundError):
        arbrume.remove_pack("installed_pack")
    with pytest.raises(FileNotFoundError, match="no pack"):
        arbrume.generate("installed_pack/herb")


def test_invalid_packs_raise(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("ARBRUME_PACKS_DIR", str(tmp_path / "store"))
    pack = make_pack(tmp_path, "broken_pack")
    (pack / "species" / "herb.toml").write_text("[growth\n", encoding="utf-8")
    with pytest.raises(ValueError, match=r"species/herb.toml"):
        arbrume.install_pack(pack)
    (pack / "run.sh").write_text("#!/bin/sh\n", encoding="utf-8")
    with pytest.raises(ValueError, match=r"run.sh"):
        arbrume.load_pack(pack)
    with pytest.raises(FileNotFoundError):
        arbrume.load_pack(tmp_path / "missing")
    arbrume.load_pack(make_pack(tmp_path / "again"))
    with pytest.raises(FileNotFoundError, match="no species `nope`"):
        arbrume.generate("pytest_pack/nope")
