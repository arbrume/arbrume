"""Species of installed packs as the species menu shows them, without ``bpy``.

``arbrume.packs()`` describes each pack as a dict (``id``, ``name``, ``version``,
``species``: dicts with ``id``, ``display_name``, ``category``, ``thumbnail``). The menu
lists the built-in presets, then one group per pack, sorted by pack name, with its species
in the order of its ``pack.toml``.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class PackSpecies:
    """One species of a pack, as a menu item."""

    id: str
    name: str
    description: str
    thumbnail: str | None


@dataclass(frozen=True)
class PackGroup:
    """The species of one pack."""

    pack_id: str
    title: str
    species: tuple[PackSpecies, ...]


def groups(packs: list[dict]) -> list[PackGroup]:
    """The menu groups of ``packs``, sorted by pack name, then id."""
    result = []
    for pack in sorted(packs, key=lambda p: (p["name"].lower(), p["id"])):
        species = tuple(
            PackSpecies(
                id=entry["id"],
                name=entry["display_name"],
                description=" - ".join(
                    part for part in (entry.get("category", ""), pack["name"]) if part
                ),
                thumbnail=str(entry["thumbnail"]) if entry.get("thumbnail") else None,
            )
            for entry in pack["species"]
        )
        title = f"{pack['name']} {pack['version']}"
        result.append(PackGroup(pack_id=pack["id"], title=title, species=species))
    return result


def species_ids(presets: list[str], pack_groups: list[PackGroup]) -> list[str]:
    """Every species of the menu in order: presets, then the species of each pack."""
    return [*presets, *(entry.id for group in pack_groups for entry in group.species)]


def display_name(species_id: str, pack_groups: list[PackGroup]) -> str | None:
    """Display name of a pack species, or ``None`` when no pack has it."""
    for group in pack_groups:
        for entry in group.species:
            if entry.id == species_id:
                return entry.name
    return None
