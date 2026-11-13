"""Writing a species as a standalone TOML file.

The standard library reads TOML but does not write it, and the species only uses a small
part of the format, so this module writes it directly: tables as ``[section]`` headers,
curves and short tables as inline tables, numbers as the shortest text of their ``f32``
value so that a file reads back as exactly the same species.
"""

from __future__ import annotations

import json
import re
from typing import Any

from .values import f32_text, is_curve_value

_BARE_KEY = re.compile(r"^[A-Za-z0-9_-]+$")

TOP_ORDER = ("schema_version", "name")


def key_text(key: str) -> str:
    """A TOML key: bare when it can be, quoted otherwise."""
    return key if _BARE_KEY.match(key) else json.dumps(key)


def value_text(value: Any) -> str:
    """Inline TOML text of a value (not None)."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return f32_text(value)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, list | tuple):
        return "[" + ", ".join(value_text(item) for item in value) + "]"
    if isinstance(value, dict):
        items = [f"{key_text(k)} = {value_text(v)}" for k, v in value.items() if v is not None]
        return "{ " + ", ".join(items) + " }" if items else "{}"
    raise TypeError(f"cannot write {type(value).__name__} as TOML")


def _is_table(value: Any) -> bool:
    return isinstance(value, dict) and not is_curve_value(value)


def _write_table(lines: list[str], table: dict, path: str) -> None:
    scalars = [(k, v) for k, v in table.items() if v is not None and not _is_table(v)]
    tables = [(k, v) for k, v in table.items() if _is_table(v)]
    if path and (scalars or not tables):
        lines.extend(["", f"[{path}]"])
    lines.extend(f"{key_text(k)} = {value_text(v)}" for k, v in scalars)
    for key, value in tables:
        _write_table(lines, value, f"{path}.{key_text(key)}" if path else key_text(key))


def dumps(species: dict, header: str = "") -> str:
    """TOML text of a species dict; ``header`` lines are written as comments first."""
    lines = [f"# {line}".rstrip() for line in header.splitlines()]
    ordered = {k: species[k] for k in TOP_ORDER if k in species}
    ordered.update({k: v for k, v in species.items() if k not in ordered})
    _write_table(lines, ordered, "")
    return "\n".join(lines).lstrip("\n") + "\n"


def export_text(species: dict, name: str, description: str = "") -> str:
    """A standalone species file for ``species`` (full species dict), renamed ``name``."""
    document = dict(species)
    document["name"] = name
    return dumps(document, description)
