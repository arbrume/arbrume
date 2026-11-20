# Textures

Arbrume synthesizes the textures of a species from its parameters: tileable bark maps and
a leaf atlas, on the CPU, with no photographed or downloaded image. A species pack is
therefore only TOML files. This page describes what is synthesized, the species fields
that drive it, how the textures reach the materials, files and Blender, and what they
cost. The crate is `arbrume-texture`.

```rust
use arbrume_gen::{GenParams, Generator, TextureCache, TextureOptions, preset};
use std::sync::Arc;

let cache = Arc::new(TextureCache::new(TextureOptions::default()));
let mut generator = Generator::new();
generator.set_textures(Some(cache));
let plant = generator.generate(&preset("oak_like").unwrap(), &GenParams::new(3, 20))?;
// plant.mesh: bark with albedo, normal and roughness maps; leaves alpha-masked
```

From the command line, `arbrume gen --textures files` (the default) writes the images
once next to the `.glb` files, `--textures embed` stores them in every file and
`--textures none` leaves them out. From Python, `arbrume.generate(..., textures=True)`.

## What is synthesized

| Material | Images | Format |
|---|---|---|
| bark | albedo, normal, roughness | sRGB RGB; linear RGB; linear grey |
| leaf | albedo with coverage, normal | sRGB RGBA, alpha cut at 0.5; linear RGB |

Normal maps are in tangent space with the glTF convention: `+x` towards increasing `u`,
`+y` towards decreasing `v` (up the image), the frame the tangents of `arbrume-export`
and the normal mapping of `arbrume-raster` use. The roughness image is the glTF
metallic-roughness texture: roughness in the green channel, and since it is grey the
blue (metallic) channel is multiplied by the metallic factor 0. PNG encoding uses the
fast deflate mode of the `png` crate (about 20 times faster than the default for
5 to 12 % larger files on the presets).

### Bark

The bark texture covers one repeat of `bark.texture_size`, `[around, along]` the axis in
meters, with `u` around and `v` along, as the bark texture coordinates of the mesher
([Meshing](mesh.md#bark-texture-coordinates)): `u` takes a whole number of repeats
around an axis, so a texture that tiles in `u` closes without a seam, and `v` follows the
arc length, which Bloomenthal (1985) already used to map bark. Features sit on a lattice
of `n_u x n_v` cells, `n_u = round(around / feature_size)` and
`n_v = round(along / (feature_size * stretch))`; every noise (gradient noise, its
fractal sums, cellular noise) wraps around that lattice, and so do their compositions
(domain warping), so the texture tiles in both directions.

| `pattern.kind` | How | Like |
|---|---|---|
| `smooth` | mottled surface; lenticels, short dark marks stretched around the axis, on a jittered lattice; sparse wide scars | birch, cherry, beech, young bark |
| `furrowed` | furrows along the zero lines of gradient noise stretched along the axis and warped across it, V-shaped in profile; flat-topped ridges with short cross checks | oak, ash, poplar, willow |
| `plated` | Voronoi plates with their own height and tint, flaky noise on top, narrow deep cracks | pine, larch, spruce |
| `fibrous` | ridged noise in two octaves, stretched along the axis and warped across it: strands that part and merge | cedar, cypress, redwood |
| `peeling` | strongly warped Voronoi patches; a fraction has shed its outer layer and shows a younger layer of another colour, with a raised rim | plane, eucalyptus |

The V-shaped furrow is how Kratt et al. (2015) open the cracks of their simulated bark;
their cambial simulation itself is out of scope.

### Leaves

The leaf atlas is a grid of cells: `foliage.texture_variants` variants of the leaf, then,
for a twig with fins, the needle spray of the fins (`LeafAtlas`). Each leaf cell holds
the rectangle of the leaf template of the mesher, `[0, petiole + 1] x [-half_width,
half_width]` in leaf lengths (`LeafLayout`), so texture coordinates `[0, 1]^2` over the
rectangle put every blade, leaflet and petiole of the texture on the geometry, and pixels
outside the leaf are transparent (Mündermann et al. 2003 texture lobed leaves the same
way). The grid is chosen so that cells have about the shape of the leaf; cells are inset
by 1/32 of their smaller side so that filtering does not bleed between them. Every leaf
cell is opaque at `LeafAtlas::STEM_UV`, on the stalk, which the twig stems sample.

* **Outline and margin.** A texel is inside a blade when its distance to the axis of
  the blade is below the smooth half-width `0.5 width_ratio blade_width(t)`, cut by the
  margin; the edge is antialiased over one texel. Teeth (`serrate`, `dentate`,
  `crenate`) are evenly spaced along the blade with jittered depth. Lobes sit at the ends
  of the secondary veins (pinnate) or, for palmate venation, around the primary veins
  in polar coordinates about the base, with parabolic sinuses between them: Runions et
  al. (2017) show that the tips of teeth and lobes and the ends of veins are the same
  points. This is a parametric stand-in for their margin model, not a simulation.
* **Veins.** `pinnate`: a midrib tapering to the tip and secondary veins that run from
  evenly spaced margin points to the midrib, which they meet at about `angle_deg`, the
  constant angle of the attachment rule of Runions et al. (2017); each is a quadratic arc
  that leaves the midrib steeper than its chord and nears the margin flatter. `palmate`:
  `2 veins + 1` primary veins fanning from the base. `parallel`: veins following the
  outline from base to tip. `open`: the open venation of Runions et al. (2005) on a
  static blade (dart-thrown sources, nearest-node attraction, a step towards the mean
  direction, kill distance), widths by Murray's law with exponent 2. A Voronoi network of
  fine veins (`tertiary`) fills the areoles of every kind. Veins are rasterized as
  segments into an intensity buffer; one thinner than a texel fades instead of aliasing.
* **Colour.** Each variant shifts hue and brightness, the blade is mottled and its
  margin paler (`texture_variation`), and veins take the vein colour (`contrast`).
  Heights (veins raised by `relief`, a faint puckering) become the normal map.
* **Needles.** A needle (`shape = "needle"`) keeps its single triangle; its cell is
  opaque, rounded across with a pale band of stomata.
* **Needle spray.** The fins of a conifer twig (`twig.fins`) show a spray: the stem along
  the middle and needles on both sides at the inclination of the twig leaves, about five
  needle widths apart, jittered, about one in seven left out, as long as the needles of
  the species but no longer than the fin is wide, darker near the stem.

**Which venation.** The open venation model is fast enough to use: a 512 texel atlas of
four elliptic leaves takes 50 to 65 ms with it against 28 ms with the pinnate model (two
runs, one thread, release build, `swatches` example), so the choice is one of look, not
cost: the pinnate model gives the regular secondary veins of most
broadleaves, the open model the irregular network of leaves such as *Ginkgo* or
*Alchemilla* that Runions et al. (2005) grow.

## Colour and sharing

A texture holds the variation of the colour around its mean, scaled into `[0, 1]` by the
largest level per channel that leaves at most 0.2 % of its texels clipped, and records
that mean. `SpeciesTextures::apply` sets the base colour factor of each material to its
colour (the one the mesher gave it) divided by that mean, at most 1. The vertex colours
of the mesh (young bark, the shade and hue of each leaf, twig stems) keep their meaning,
and a textured plant has the mean colour of an untextured one.

Individuals of a species drawn by `Species::varied` (`--random`, `RandomSampler`) differ
in colour, leaf width and twig length. They share the textures of their species
(`Generator::set_texture_base`): the factor brings each to its own colour, and `apply`
moves the texture coordinates of their leaves from their own atlas, whose cells depend
on the leaf width, to the atlas of the textures. A batch therefore synthesizes one
texture set per species, which `TextureCache` keeps and shares between threads (one
thread synthesizes, the others wait). Species are told apart by their name and their
bark and foliage tables.

**Geometry.** Outline leaves are meshed as cards when textured
(`arbrume_texture::textured_species`): a rectangle over the leaf, folded and bent like
the blade in one segment fewer than the outline would have, which keeps its triangle
count; the alpha of the texture cuts it to the outline and margin. Needles keep their
triangle. Bark geometry is unchanged.

## Species fields

### `[bark]`

| Field | Default | Meaning |
|---|---|---|
| `texture_resolution` | 512 | width and height of the bark texture in texels, a power of two from 16 to 4096 |
| `pattern.kind` | `"furrowed"` | `smooth`, `furrowed`, `plated`, `fibrous`, `peeling` |
| `pattern.feature_size` | 0.04 m | spacing of the features around the axis |
| `pattern.stretch` | 3.0 | length of a feature along the axis over its width (0.1 to 100) |
| `pattern.relief` | 0.5 | depth of the relief relative to `feature_size`, scales the normal map |
| `pattern.coverage` | 0.35 | share of furrows, cracks, lenticels or shed patches, in `[0, 1]` |
| `pattern.irregularity` | 0.5 | domain warping and size jitter, in `[0, 1]` |
| `pattern.contrast` | 0.3 | colour variation between plates, patches, ridges, and mottling |
| `pattern.color` | `color` x 0.35 | colour of furrow bottoms, cracks, lenticels, younger layers |
| `pattern.roughness_variation` | 0.3 | how much rougher furrows and cracks are, in `[0, 1]` |

`texture_size` (the repeat in meters, default `[0.5, 0.5]`) sets the scale of the
pattern, `color` its main colour and `roughness` its mean roughness.

### `[foliage]`

| Field | Default | Meaning |
|---|---|---|
| `texture_resolution` | 512 | width and height of the leaf atlas in texels, a power of two from 16 to 4096 |
| `texture_variants` | 4 | leaf variants in the atlas (1 to 16) |
| `texture_variation` | 0.3 | mottling, paler margins and variation between variants, in `[0, 1]` |
| `venation.kind` | `"pinnate"` | `none`, `pinnate`, `palmate`, `parallel`, `open` |
| `venation.veins` | 7 | secondary veins per side, primaries per side (palmate) or veins per side (parallel), at most 64 |
| `venation.angle_deg` | 45 | angle between a secondary vein and the midrib (5 to 85); spread of palmate veins |
| `venation.width` | 0.04 | midrib width at the base over the blade width (0 to 0.25) |
| `venation.contrast` | 0.4 | how visible the veins are in the colour |
| `venation.relief` | 0.4 | height of the veins in the normal map |
| `venation.tertiary` | 0.3 | strength of the network of fine veins |
| `venation.color` | leaf colour lightened | colour of the veins |
| `margin.kind` | `"entire"` | `entire`, `serrate`, `dentate`, `crenate`, `lobed` |
| `margin.teeth` | 16 | teeth per side (lobes follow the veins) |
| `margin.depth` | 0.06 | depth of a tooth or sinus over the local half-width (0 to 0.9) |
| `margin.irregularity` | 0.3 | jitter of the size and place of teeth and lobes |

Every table rejects unknown keys and every value is validated, like the rest of the
schema ([Species library](species.md)).

## Files

`arbrume gen --textures MODE` (also `textures = "MODE"` in a batch config file):

* `files` (default): each image is written once to the output directory as
  `arbrume_<hash>.png`, named by a 64-bit FNV-1a hash of its bytes
  (`arbrume_export::image_file_name`), and the `.glb` files reference it by relative URI.
  Plants of a species share the files. An image is written under a temporary name and
  renamed, so concurrent workers never leave a partial file.
* `embed`: the images are in the binary chunk of each `.glb`. A file holds each distinct
  image once, whatever the number of levels of detail that use it.
* `none`: no texture, outline leaves, as before textures existed.

With `--billboard`, the billboard is baked from the textured leaves.

**Measured on the 256-plant random set** (`arbrume gen --random --count 256 --seed 1`,
16 threads, release build):

| Mode | Total size | of which `.glb` | images | Wall time |
|---|---|---|---|---|
| `none` | 512.3 MB | 512.2 MB | | 0.30 s |
| `embed` | 820.6 MB (+60 %) | 820.4 MB | | 0.65 s |
| `files` | 675.9 MB (+32 %) | 659.1 MB | 16.7 MB (135 files) | 0.81 s |

`files` is the default because it costs half as much space as `embed`. Of the 147 MB the
`.glb` files gain, 154 MB are the `TANGENT` attribute that the writer adds to every part
with a normal map (16 bytes a vertex); the card leaves have slightly fewer vertices than
the outline leaves. The images themselves are 3 % of the set.

## Cost

Synthesis time of the textures of each species, one thread, release build
(`RAYON_NUM_THREADS=1 cargo run --release -p arbrume-gen --example texture_bench`):

| Textures | Bark | Leaf atlas | Encoding | Total |
|---|---|---|---|---|
| tree presets, bark 512, leaves 512 | 48 to 84 ms | 27 to 46 ms | 4 to 5 ms | 83 to 130 ms |
| conifer presets, bark 512, needles 256 | 70 to 72 ms | 4 to 5 ms | 3 to 4 ms | 78 to 81 ms |
| shrub presets, 256 and 256 | 12 to 21 ms | 7 to 10 ms | 1 ms | 20 to 32 ms |
| fern, compound leaf of 11 blades, 512 | 6 ms (128) | 111 ms | 1 ms | 118 ms |
| all 27 presets | | | | 2.1 s, 16 MB |

The synthesis runs in parallel over rows and cells, so in a batch it costs about this
divided by the number of idle threads, once per species. On the random set above (27
species, 256 small plants of about 1 ms of growing and meshing each) it is the larger
part of the run, 0.35 to 0.5 s of wall time; a batch of many plants of a few species
pays it once per species. The presets use 512 texel bark and leaves for trees, 256 for
shrubs and needles, smaller for herbs; `TextureOptions::max_resolution` caps all of
them.

## Looking at the textures

```
cargo run --release -p arbrume-texture --example swatches -- OUT_DIR [species.toml...]
cargo run --release -p arbrume-gen --example texture_look -- OUT_DIR [preset...]
```

`swatches` writes `bark_sheet.png` (each bark kind as a 2 x 2 repetition, lit through its
normal map, and its albedo) and `leaf_sheet.png` (each leaf shape and venation);
`texture_look` renders presets with `arbrume-raster` without and with textures: the
whole plant, the base of the trunk and a twig at the edge of the crown.

## Tests

* every bark kind tiles: the mean step across the seam (last column next to the first,
  last row next to the first) may not exceed the largest step between neighbouring
  columns or rows inside the texture, so the seam is indistinguishable from an interior
  boundary; a texture that does not tile has a seam step of the order of its full
  contrast;
* textures are byte-identical on 1 and 4 threads and from run to run, and textured
  batches are identical on 1 and 3 threads;
* sizes and PNG formats of every map, the recorded mean against the decoded albedo,
  transparent cell padding and opaque stem texels;
* the cache synthesizes a species once across threads; individuals share the textures
  of their species and their leaves land in its atlas cells;
* `.glb` files with embedded and external images pass the Khronos validator with no error
  or warning (`tools/validate_glb.mjs` reads external images relative to the file);
* the species fields are validated and unknown keys rejected.

## Limits

* The textures do not depend on the age or size of an axis: a thin twig shows the same
  pattern as the trunk, scaled by its texture coordinates (`bark.young_color` still
  changes its colour).
* The bark repeat is visible on long straight trunks at close range; the random `v`
  offset of each branch hides it between branches only.
* The `bark.texture` and `foliage.texture` paths of the schema are not read; textures are
  always synthesized.
* Card leaves are cut by alpha: they need alpha testing in the renderer, and their
  rectangle draws more fragments than the outline polygon.
* The margin is parametric: it does not grow teeth and lobes as Runions et al. (2017) do,
  and the open venation runs on a static blade, without the marginal growth that gives
  their leaves their hierarchy of veins.
