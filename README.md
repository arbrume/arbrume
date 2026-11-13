# Arbrume

Open-source procedural tree and plant generator. Licence: GPL-3.0-or-later.

Early development. The code is not here yet; `docs/` holds the developer documentation.

## Docs

```
uv run sphinx-build -W --keep-going -b html docs docs/_build/html
```

Inside Vlam3d, the docs build with the shared archilab look when the sibling
`vlam3d-doc-utils` clone is available:

```
uv run --with-editable ../vlam3d-doc-utils sphinx-build -W --keep-going -b html docs docs/_build/html
```
