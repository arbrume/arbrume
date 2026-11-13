# Arbrume

Open-source procedural tree and plant generator. Licence: GPL-3.0-or-later.

Early development: the growth engine, the mesher, the `.glb` exporter and the batch
command line work; `docs/` holds the developer documentation.

## Quick start

```
cargo build --release -p arbrume
target/release/arbrume species list
target/release/arbrume gen --species broadleaf --count 100 --seed 1 --age-range 10..30 --out out --manifest
target/release/arbrume gen --species conifer --count 10 --lods 3 --out out
target/release/arbrume gen --species my_species.toml --count 10 --out out
target/release/arbrume gen --config batch.toml
target/release/arbrume gen --random --count 256 --seed 1 --thumb --out plants
target/release/arbrume info my_species.toml
target/release/arbrume bench
```

Each plant is written to `out/<species>_<seed>_<index>.glb` with its full parameters in
the file metadata; the output does not depend on the number of threads. See
`docs/cli.md` for every option, batch config files and the `arbrume-gen` library,
and `docs/species.md` for the preset library.

## Docs

```
uv run sphinx-build -W --keep-going -b html docs docs/_build/html
```

Inside Vlam3d, the docs build with the shared archilab look when the sibling
`vlam3d-doc-utils` clone is available:

```
uv run --with-editable ../vlam3d-doc-utils sphinx-build -W --keep-going -b html docs docs/_build/html
```
