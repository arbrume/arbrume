"""Build the Blender extension zips of Arbrume, one per platform.

Usage, from the repository root::

    uv run blender/build_extension.py                      # build the wheel of this machine
    uv run blender/build_extension.py --wheel dist/*.whl   # package wheels built elsewhere
    uv run blender/build_extension.py --blender "D:/Blender/blender.exe"

Steps:

1. Wheels go to ``blender/arbrume/wheels/`` (git-ignored): the ``--wheel`` files, or the
   wheel of this machine built with maturin (``target/blender``), or with ``--no-build``
   the wheels already there.
2. Wheels are grouped by Blender platform (``windows-x64``, ``linux-arm64``, ...). The
   wheels are abi3, so one wheel per platform serves every Python of Blender 4.2 and later.
3. For each platform the package is copied to ``target/blender/extension/<platform>/``
   with only that platform's wheel, and its manifest gets ``platforms`` and ``wheels``.
4. The zip is made by ``blender --command extension build`` and checked by
   ``blender --command extension validate`` when Blender is given (``--blender`` or the
   ``BLENDER`` environment variable), otherwise written directly with the same layout:
   the manifest at the root of the zip. Zips land in ``blender/dist/``.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tomllib
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
PACKAGE = HERE / "arbrume"
WHEELS = PACKAGE / "wheels"
DIST = HERE / "dist"
STAGING = REPO / "target" / "blender" / "extension"
SKIPPED = {"wheels", "__pycache__"}


def wheel_platforms(wheel: str) -> list[str]:
    """Blender platforms served by a wheel, from the platform tag of its file name."""
    tag = Path(wheel).stem.split("-")[-1]
    platforms: list[str] = []
    for part in tag.split("."):
        if part.startswith("macosx_"):
            if part.endswith("universal2"):
                platforms += ["macos-x64", "macos-arm64"]
            elif part.endswith("_x86_64"):
                platforms.append("macos-x64")
            elif part.endswith("_arm64"):
                platforms.append("macos-arm64")
        elif part in ("win_amd64", "win_arm64"):
            platforms.append("windows-x64" if part == "win_amd64" else "windows-arm64")
        elif part.startswith(("manylinux", "musllinux", "linux")):
            platforms.append("linux-arm64" if part.endswith("aarch64") else "linux-x64")
    return sorted(set(platforms))


def manifest_for(text: str, platforms: list[str], wheels: list[str]) -> str:
    """Manifest text with its ``platforms`` and ``wheels`` lists replaced."""

    def listing(values: list[str]) -> str:
        return "[" + ", ".join(f'"{v}"' for v in values) + "]"

    text = re.sub(r"(?m)^platforms = .*$", f"platforms = {listing(platforms)}", text)
    return re.sub(r"(?m)^wheels = .*$", f"wheels = {listing(wheels)}", text)


def build_host_wheel() -> None:
    """Builds the wheel of this machine with maturin into ``blender/arbrume/wheels``."""
    env = dict(os.environ, CARGO_TARGET_DIR=str(REPO / "target" / "blender"))
    cargo_bin = Path.home() / ".cargo" / "bin"
    if cargo_bin.is_dir():
        env["PATH"] = f"{cargo_bin}{os.pathsep}{env.get('PATH', '')}"
    command = ["uvx", "maturin", "build", "--release", "--out", str(WHEELS)]
    subprocess.run(command, cwd=REPO / "crates" / "arbrume-py", env=env, check=True)


def collect_wheels(given: list[str], build: bool) -> list[Path]:
    """Wheels to package, copied into ``blender/arbrume/wheels``."""
    WHEELS.mkdir(parents=True, exist_ok=True)
    if given:
        for old in WHEELS.glob("*.whl"):
            old.unlink()
        for wheel in given:
            shutil.copy2(wheel, WHEELS)
    elif build:
        for old in WHEELS.glob("*.whl"):
            old.unlink()
        build_host_wheel()
    return sorted(WHEELS.glob("*.whl"))


def stage(platform: str, wheels: list[Path]) -> Path:
    """Package directory of one platform, holding only that platform's wheels."""
    target = STAGING / platform / "arbrume"
    shutil.rmtree(target, ignore_errors=True)
    shutil.copytree(PACKAGE, target, ignore=shutil.ignore_patterns(*SKIPPED, "*.pyc"))
    (target / "wheels").mkdir()
    for wheel in wheels:
        shutil.copy2(wheel, target / "wheels")
    manifest = target / "blender_manifest.toml"
    names = [f"./wheels/{w.name}" for w in wheels]
    manifest.write_text(manifest_for(manifest.read_text("utf-8"), [platform], names), "utf-8")
    return target


def zip_directly(source: Path, output: Path) -> None:
    """Zips a package directory with its manifest at the root of the archive."""
    with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(source.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(source).as_posix())


def zip_with_blender(blender: str, source: Path, output: Path) -> None:
    """Builds and validates the zip with Blender's extension command."""
    command = [blender, "--factory-startup", "--command", "extension"]
    subprocess.run(
        [*command, "build", "--source-dir", str(source), "--output-filepath", str(output)],
        check=True,
    )
    subprocess.run([*command, "validate", str(output)], check=True)


def main(argv: list[str] | None = None) -> int:
    """Command line entry point."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--wheel", action="append", default=[], help="wheel to package")
    parser.add_argument("--no-build", action="store_true", help="use the wheels already there")
    parser.add_argument("--blender", default=os.environ.get("BLENDER"), help="Blender binary")
    args = parser.parse_args(argv)
    wheels = collect_wheels(args.wheel, build=not args.no_build)
    by_platform: dict[str, list[Path]] = {}
    for wheel in wheels:
        for platform in wheel_platforms(wheel.name):
            by_platform.setdefault(platform, []).append(wheel)
    if not by_platform:
        print("no wheel to package in", WHEELS, file=sys.stderr)
        return 1
    version = tomllib.loads((PACKAGE / "blender_manifest.toml").read_text("utf-8"))["version"]
    DIST.mkdir(exist_ok=True)
    for platform, platform_wheels in sorted(by_platform.items()):
        source = stage(platform, platform_wheels)
        output = DIST / f"arbrume-{version}-{platform.replace('-', '_')}.zip"
        output.unlink(missing_ok=True)
        if args.blender:
            zip_with_blender(args.blender, source, output)
        else:
            zip_directly(source, output)
        print(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
