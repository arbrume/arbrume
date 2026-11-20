"""Pack species in the species menu: groups, order and names."""

from logic import packs

PACKS = [
    {
        "id": "zeta",
        "name": "Zeta trees",
        "version": "1.0.0",
        "species": [
            {"id": "zeta/elm", "display_name": "Elm", "category": "tree", "thumbnail": "/t/e.png"},
        ],
    },
    {
        "id": "alpha",
        "name": "Alpha shrubs",
        "version": "0.2.0",
        "species": [
            {"id": "alpha/box", "display_name": "Box", "category": "", "thumbnail": None},
            {"id": "alpha/yew", "display_name": "Yew", "category": "shrub", "thumbnail": None},
        ],
    },
]


def test_groups_are_sorted_by_pack_name_and_keep_species_order() -> None:
    groups = packs.groups(PACKS)
    assert [g.pack_id for g in groups] == ["alpha", "zeta"]
    assert groups[0].title == "Alpha shrubs 0.2.0"
    assert [s.id for s in groups[0].species] == ["alpha/box", "alpha/yew"]
    assert groups[0].species[1].description == "shrub - Alpha shrubs"
    assert groups[0].species[0].description == "Alpha shrubs"
    assert groups[1].species[0].thumbnail == "/t/e.png"


def test_species_ids_and_display_names() -> None:
    groups = packs.groups(PACKS)
    ids = packs.species_ids(["broadleaf", "shrub"], groups)
    assert ids == ["broadleaf", "shrub", "alpha/box", "alpha/yew", "zeta/elm"]
    assert packs.display_name("zeta/elm", groups) == "Elm"
    assert packs.display_name("broadleaf", groups) is None
    assert packs.groups([]) == []
