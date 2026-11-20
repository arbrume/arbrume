# Species packs

A species pack bundles species definitions, and optionally the images they use, for
others to install and use by name. A pack is content, not code: the species files and
assets carry the licence chosen by their author, named in the pack, which is separate
from the GPL licence of Arbrume. This page is the guide for pack authors; the crate is
`arbrume-pack`.

```sh
arbrume pack new my_trees                      # pack.toml and a placeholder licence file
arbrume pack add-species my_trees oak_like --display-name "Field oak" --category "broadleaf tree"
arbrume pack add-species my_trees path/to/elm.toml --category "broadleaf tree"
arbrume pack validate my_trees                 # parse, grow, check every file
arbrume pack thumbs my_trees                   # render the missing thumbnails
arbrume pack build my_trees -o my_trees-0.1.0.arbpack
arbrume pack install my_trees-0.1.0.arbpack    # into the user pack directory
arbrume gen --species my_trees/field_oak --count 4 --out trees
```

`examples/packs/starter/` is a small pack made of four presets of the built-in library,
with procedural textures only; CI validates it, builds it twice and compares the
archives, installs it and generates plants from it.

## Layout

A pack is a directory, or the same files in a `.arbpack` archive (a zip file):

| File | Content |
|---|---|
| `pack.toml` | what the pack is and which species it holds (below) |
| `LICENCE-CONTENT.txt` | the licence terms of the content, the file named by `licence_file` (`.txt` or `.md` at the root) |
| `README.md` or `README.txt` | optional |
| `species/<name>.toml` | one species file per species ([Species library](species.md)) |
| `textures/...` | optional PNG or JPEG images named by the species (below) |
| `thumbnails/<name>.png` | one thumbnail per species, rendered by `pack thumbs` or when building |
| `previews/<name>.glb` | optional preview models |
| `MANIFEST.sha256` | SHA-256 of every other file; written by `pack build`, required in an archive |

Nothing else may be in a pack: any other file is an error, and so is a file that starts
like an executable (`MZ`, ELF, Mach-O, `#!`) whatever its name, a text file that is not
UTF-8, an image that is not a PNG or JPEG, a preview that is not a binary glTF 2.0 file.
In a pack directory, hidden files and directories (names starting with `.`) are ignored
and symbolic links refused.

Paths are relative, separated by `/`, made of ASCII letters, digits, `.`, `_` and `-`,
with no empty, hidden, `.` or `..` part and no device name of Windows (`con`, `nul`,
`com1`, ...); two paths may not differ only by case. These are names every platform can
extract without leaving the pack directory.

A file may hold at most 64 MiB (`MAX_FILE_BYTES`, an uncompressed RGBA image at the
largest texture resolution, 4096 texels square) and a pack 1 GiB in all
(`MAX_PACK_BYTES`), which bounds what reading an archive may allocate.

## `pack.toml`

```toml
format = 1
id = "starter"
name = "Arbrume starter pack"
version = "0.1.0"
authors = ["Arbrume contributors"]
description = "Four species of the built-in library."
content_licence = "PLACEHOLDER: content licence terms to be written"
licence_file = "LICENCE-CONTENT.txt"
min_schema_version = 0

[[species]]
name = "oak_like"
display_name = "Oak-like tree"
category = "broadleaf tree"
# file = "species/oak_like.toml"         the default
# thumbnail = "thumbnails/oak_like.png"  the default
# preview = "previews/oak_like.glb"      optional
```

| Field | Meaning |
|---|---|
| `format` | version of the format of `pack.toml`, 1 |
| `id` | identifier: a lowercase ASCII letter, then lowercase letters, digits and `_`, at most 64 characters; `builtin` is reserved |
| `name` | display name |
| `version` | `MAJOR.MINOR.PATCH` |
| `authors` | list of names, free text |
| `description` | optional paragraph |
| `content_licence` | the name or identifier of the licence of the content |
| `licence_file` | the file holding its terms |
| `min_schema_version` | the species schema version the species files need (0 today) |
| `species` | the species, each with `name` (same rule as `id`), `display_name`, optional `category` (free text shown to users), and optional `file`, `thumbnail` and `preview` paths |

Unknown keys are errors, as in species files. The species `name` of the pack `id` is
used everywhere as `id/name`: `--species starter/oak_like` on the command line,
`arbrume.generate("starter/oak_like")` in Python, the species menu in Blender. Built-in
presets have no `/`, so they never collide with pack species.

**Content licence.** `pack new` writes `content_licence` and a licence file that say
plainly that the terms are to be written; `pack validate` warns while either still says
`PLACEHOLDER`. Arbrume does not choose or check the terms: write them before publishing.

**Versions.** A `pack.toml` of an older `format` is brought up to date by the migration
steps of `arbrume_pack::manifest::MIGRATIONS`, one format at a time, before it is read;
species files of an older schema go through `SPECIES_MIGRATIONS` the same way. A newer
`format`, a newer `min_schema_version` or a species file of a newer `schema_version` is
rejected with a message asking for a newer Arbrume. Format 1 and schema 0 are the first,
so there is no migration step yet.

## Species and images

A species file of a pack is an ordinary species file. It may name images that replace
the synthesized textures ([Textures](textures.md)); procedural textures stay the default:

| Field | Image |
|---|---|
| `bark.texture` | albedo of the bark over one repeat of `bark.texture_size`, tileable |
| `bark.normal_texture` | its tangent-space normal map (glTF convention), optional |
| `foliage.texture` | one leaf, its alpha cutting the outline: stalk at the bottom centre, tip at the top, the leaf filling the image |
| `foliage.normal_texture` | its normal map, optional |

Paths are relative to the pack and under `textures/`. An image is a PNG (8 or 16 bits,
grey, grey and alpha, RGB, RGBA or palette) or a baseline JPEG (grey or RGB), square,
its side a power of two from 16 to 4096 texels like `texture_resolution`. A normal map
is only read with its albedo. How they are used:

- **Bark.** The image is the bark albedo at its own size (halved until it fits the
  largest resolution of the run). Without a normal map the bark is flat; the roughness is
  the constant `bark.roughness`.
- **Leaves.** The image fills every leaf cell of the atlas of the species, which has the
  shape of the leaf, each texel the mean of the image pixels under it. The needle spray
  of a conifer twig stays synthesized. The texels at the stalk point stay opaque, in the
  colour of young bark where the image is transparent, since twig stems are textured
  with it.
- **Colour.** As for synthesized textures, the colour of an image counts relative to its
  mean: the base colour of a material is `bark.color` or `foliage.color` divided by that
  mean, so these fields still set the mean colour of the plant and its leaves still vary
  in shade. Set them to the mean linear colour of the image to show it as it is.

The images reach every output like synthesized textures: `.glb` files written by
`arbrume gen --textures files|embed`, `textures=True` in Python, the materials in
Blender. Generating needs the images as files: an installed pack or a pack directory is
used in place; the images of an archive opened directly (`arbrume.load_pack("x.arbpack")`)
are extracted once to `<temp dir>/arbrume-packs/<id>-<hash>/`.

`Species::asset_dir` is the directory relative texture paths are resolved against. It
is set by whoever loads the species, is not read from nor written to the file, and is
not part of the metadata of exported plants. A species file given directly
(`arbrume gen --species path/to/elm.toml`, `arbrume.generate("path/to/elm.toml")`)
resolves its images against its own directory.

## Validation

`arbrume pack validate DIR|FILE` (and `arbrume_pack::validate`) checks, and reports every
problem with the file it is in:

- the structure: `pack.toml`, allowed files, contents, paths, sizes, the hashes of
  `MANIFEST.sha256` when present (an archive must have it), the files `pack.toml` names,
  species files it does not list, at least one species;
- every species: it parses (unknown keys are errors) and validates, its images are under
  `textures/`, present, decodable and of a valid size, and are turned into textures; it
  is grown at the youngest, middle and oldest ages of its `variation.age` and meshed at
  the quality of random plants, keeping the invariants of the plant graph, a contiguous
  graph, positive finite radii, finite positions and a non-empty mesh; four individuals
  of its `[variation]` are valid species;
- thumbnails decode like images and have a valid size.

Warnings do not fail a pack: a species without a thumbnail, a placeholder content
licence, an image no species uses. Exit code 2 means the pack is invalid.

## Thumbnails

A thumbnail is the plant of seed 1 at the oldest age of its range, meshed at the quality
of random plants with its textures (images of the pack included), rendered by
`arbrume-raster` with the default thumbnail view and light, 256 pixels square, as PNG.
`arbrume pack thumbs DIR [--size N] [--force]` writes the missing ones (all with
`--force`) to `thumbnails/<name>.png`; `pack build` and `pack install` render those still
missing into the archive or the installed pack. Rendering is deterministic.

## Archives

`arbrume pack build DIR [-o FILE]` validates the pack (errors stop it), renders the
missing thumbnails, writes `MANIFEST.sha256` and the archive (by default
`ID-VERSION.arbpack`). The same pack always gives the same bytes: entries sorted by path
with no directory entries, the timestamp 1980-01-01 00:00 on every entry, the UTF-8
name flag, Unix permissions `rw-r--r--`, no extra field, deflate at level 9 unless it
does not make the file smaller. The archive opens with any zip tool, and
`sha256sum -c MANIFEST.sha256` checks the extracted files.

Reading an archive goes through its central directory and refuses, before extracting
anything: unsafe paths (absolute, `..`, backslashes, drive letters), names that differ
only by case, symbolic links, encrypted entries, methods other than store and deflate,
zip64 and multi-volume archives, sizes over the limits (checked while inflating,
whatever the archive declares), CRC mismatches. Archives made by other zip tools are
read as long as they keep to this.

## Installing and using

`arbrume pack install FILE [--dir DIR]` validates a `.arbpack` (or a pack directory) and
installs it as a directory named by its id, holding the built files, thumbnails and
hashes included; an installed pack of the same id is replaced (the new one is written
beside it under a hidden name, then the directories are swapped). An invalid pack is not
installed and leaves the installed one in place. The hashes of an installed pack are
checked every time it is opened.

The pack directory is, in this order: the environment variable `ARBRUME_PACKS_DIR`, then
`%APPDATA%\arbrume\packs` on Windows, `~/Library/Application Support/arbrume/packs` on
macOS, `$XDG_DATA_HOME/arbrume/packs` or `~/.local/share/arbrume/packs` elsewhere. The
command line, the Python module and the Blender extension share it.

`arbrume pack list` prints one line per pack (id, version, name, content licence) and one
indented line per species (`id/name`, display name, category). `arbrume pack remove ID`
deletes an installed pack.

Pack species are accepted wherever a preset name is: `arbrume gen --species pack/name`
(files are named `pack-name_SEED_INDEX.glb`), the `preset` of a batch config file,
`arbrume sheet` and `arbrume bench`. `arbrume gen --random --packs starter,builtin`
draws random plants from the species of the listed packs, `builtin` standing for the
built-in presets (the default without `--packs`), by their `variation.weight` like the
presets.

From Python ([Python bindings](python.md#species-packs)): `arbrume.load_pack(path)`,
`arbrume.packs()`, `arbrume.install_pack(path)`, `arbrume.remove_pack(id)`,
`arbrume.packs_dir()`, and `arbrume.generate("pack/name")`. In Blender
([Blender extension](blender.md)): the Packs box of the preferences and the species menu.

## Tests

- unit tests of paths, allowed files and content, `pack.toml` rules, migrations, hashes,
  the zip writer and reader (round trip, determinism, traversal and absolute names,
  corruption);
- `crates/arbrume-pack/tests/packs.rs`: authoring a pack with presets and a species with
  images, validation and thumbnails, a deterministic archive, installing it twice,
  generating from the installed pack with its images on the materials, using an archive
  directly, removing; rejected packs (malformed TOML, unknown fields, newer schema and
  format, duplicate species, a disallowed file, executable content, an image of the wrong
  size, missing licence, species and image files, missing or wrong hashes), an archive
  with a `../` entry refused before anything is written, an invalid pack that does not
  replace an installed one;
- `crates/arbrume-cli/tests/pack.rs`: every `arbrume pack` command and its exit codes,
  `gen --species pack/name` and `gen --random --packs`;
- `crates/arbrume-py/tests/test_packs.py`: loading, installing, listing, removing and
  generating from Python, images on the materials, errors;
- the Blender suite installs a pack made by Python's `zipfile` from the preferences
  operator and checks the menu, thumbnails, the Packs box and the images on the
  materials, then removes it.
