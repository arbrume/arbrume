# Wind

Arbrume bakes wind data, it does not simulate wind. When a plant is meshed with
`MeshOptions::wind` (`--wind` on the command line, `LodSettings::wind` for a chain of
[levels of detail](lod.md)), every vertex gets the few numbers a vertex shader needs to
bend the plant, and the mesh gets a table of its branches. This page describes the data,
how it is carried in `.glb` files and why, and a small three.js shader that uses it.

## The data

The scheme is the hierarchical vertex displacement of Habel et al. (2009): every meshed
axis is a branch, a tapered cantilever bending about its base, and a vertex moves with
every branch of its lineage, from the trunk down to its own branch.

**Levels.** A root axis has level 0, an axis its parent's level plus one, at most 3. An
axis deeper than level 3 gets level 3 and takes its own base as pivot, so the levels
between move with the level-3 ancestor without bending of their own. A relay axis, which
continues its parent in a sympodial plant, is a branch of its own one level down.

**Per vertex** (`MeshBuffers::wind`, six floats):

| Value | Meaning |
|---|---|
| `x0`, `x1`, `x2`, `x3` | normalized position, 0 at the base and 1 at the tip by arc length along the axis polyline, along the branch of each level that carries the vertex: below its own level, where its lineage leaves that branch; at its own level, where the vertex is; above, 0 |
| `branch` | index of its own branch in the table, as a float (exact below 2^24) |
| `flutter` | 0 on bark; on a leaf or twig, its distance from the attachment point over the largest such distance of its template, from 0 to 1 |

A leaf or twig takes the values of the point of the axis it is attached to, so it follows
its branch; a child where it leaves its parent carries the values of the parent there.
The tests check both, and that every value is in range.

**Per branch** (`TreeMesh::wind`, a `WindTable` of `WindBranch`):

| Field | Meaning |
|---|---|
| `chain` | chain id of the axis in the plant graph |
| `parent`, `level` | parent branch (none for a root) and hierarchy level |
| `lineage` | the branch of each level `0..=level` of its lineage, itself at `level` |
| `lineage_x` | `x` of the lineage on the branches below its level, the `x0..x3` every vertex of the branch carries below its own level |
| `pivot` | base of the branch |
| `tangent`, `right` | direction at the base (`t` of Habel et al.) and a unit vector orthogonal to it, as horizontal as possible (`r`); the third axis is `tangent x right` (`s`) |
| `length`, `root_radius`, `tip_radius` | length along the axis and radii at both ends, in meters |
| `taper` | `tip_radius / root_radius`, the taper ratio `alpha` |
| `mass` | volume of the frusta of the branch and of every branch it bears (Habel et al., eq. 16) with a unit density: multiply by the density of the wood |
| `shape` | `c2`, `c4` of the deflection `u(x) = c2 x^2 + c4 x^4`, normalized so that `u(1) = 1` |

**Deflection shape.** Habel et al. fit `c2 x^2 + c4 x^4` to the static deflection of a
cantilever of linearly tapering radius under a uniform load, `I(x)` proportional to
`((alpha - 1) x + 1)^4`. The closed form of their appendix, as transcribed in our copy,
does not reproduce their table, so the deflection is integrated numerically instead
(`u'' = (1 - x)^2 / (2 I(x))`, clamped root, free tip) and fitted by least squares: at
`alpha = 0.1` this gives 0.3328 and 0.3988 before normalization, against 0.3326 and
0.3989 in their table 1. A table of 65 taper ratios is computed once and interpolated.
The amplitude, which depends on the stiffness of the wood and on the radius to the fourth
power, is left to the shader.

**Not baked.** The frequency rule Habel et al. cite, `f = 2.55 L^-0.59`, gives no units
for `L` and `f` in the paper, and we could not check it on a page image: it is not baked
as a physical default. A shader can derive frequencies from `length` and `level` as it
sees fit. Motion textures, damping and the wind spectrum belong to the runtime.

## Skeleton

For engines without custom vertex shaders, `ExportOptions::skin` (`--skin`) adds a
skeleton built from the same data: every branch of level at most `max_level` (default 1:
the trunk and its main branches) gets `segments` joints (default 4), the first at its
pivot and the others at the centroid of its bark at `x = k / segments`. The joints of a
branch form a chain; the first joint of a branch is a child of the joint of its parent
below its attachment. A vertex is bound to the branch of its lineage at level
`min(level, max_level)`, to the two joints around its `x` with linear weights. Animate
the joints and the plant bends; leaves follow their branch but do not flutter.

## glTF carriage trial

glTF has standard slots for texture coordinates (`TEXCOORD_n`), colours (`COLOR_n`),
skins (`JOINTS_0`, `WEIGHTS_0`) and instancing (`EXT_mesh_gpu_instancing`), and allows
application-specific attributes whose names start with an underscore. Which of them
survive an import decides how the wind data is written. We wrote small files with each
(all valid for the Khronos validator), imported them in Blender 5.3 (background script)
and three.js r170 (`GLTFLoader` under Node), and read the documentation of Unity's
glTFast (features page of version 6.10) and of Unreal's glTF support. The files written
by `arbrume gen` were then checked the same way.

| Carriage | Blender 5.3 | three.js r170 | Unity glTFast (documented) | Unreal Interchange |
|---|---|---|---|---|
| Custom float attribute `_WIND_X` (VEC4) | kept as a float colour attribute `_WIND_X`, full precision | kept as `_wind_x` (lower case), full precision | not documented; the instancing footnote says custom attributes are not supported there | not verified |
| Custom integer attribute (VEC4 `u16`) | damaged: becomes an 8-bit colour, values clamped to 1 | kept, `Uint16Array` | not documented | not verified |
| `TEXCOORD_1` to `TEXCOORD_3` | kept as `UVMap.001` to `.003`, full precision, `v` flipped to `1 - v` (Blender's convention, flipped back on export) | kept as `uv1` to `uv3`, full precision | up to 8 sets imported; its own shaders use 2 | not verified |
| `COLOR_1` | kept as `Color.001`, quantized to 8 bits per channel | kept as `color_1`, full precision | vertex colours supported, `COLOR_1` not mentioned | not verified |
| `EXT_mesh_gpu_instancing` | expanded into one object per instance (1 721 objects for one conifer), custom instance attributes dropped | `InstancedMesh`, custom instance attributes kept | import supported, without custom attributes | not verified |
| Skin (`JOINTS_0`, `WEIGHTS_0`) | armature, vertex groups and an armature modifier | `SkinnedMesh` with its bones | supported, up to 4 joints per vertex | not verified |
| `MSFT_lod` | nodes imported unlinked, coverage kept as a custom property | level 0 only, the other levels dropped | "not investigated yet" | not verified |
| Separate nodes `_LOD0`, `_LOD1`, ... | kept, children of an empty | kept, children of a group | kept as nodes | not verified |
| Mesh and node `extras` | custom properties | `userData` | through an add-on | not verified |

Unreal: the pages of the Unreal Engine documentation we reached describe the glTF
exporter and the import workflow, not which attributes and extensions the Interchange
importer keeps, so every Unreal cell is unverified.

**Chosen layout.** The default, `WindLayout::TexCoord` (`--wind` or `--wind texcoord`):

| Attribute | Content |
|---|---|
| `TEXCOORD_1` | `x0`, `x1` |
| `TEXCOORD_2` | `x2`, `x3` |
| `TEXCOORD_3` | `branch`, `flutter` |

Texture coordinates are the only channel every importer we tried keeps at full precision,
and the one engines expose to materials without code. The cost is three texture
coordinate sets that a material could otherwise use, and Blender's `v` flip (its
`UVMap.00n` hold `1 - x1`, `1 - x3` and `1 - flutter` until exported back).
`WindLayout::Custom` (`--wind custom`) writes `_WIND_X` (`x0..x3`) and `_WIND_DATA`
(`branch`, `flutter`) instead: self-describing names, kept by three.js and Blender, but
not documented by glTFast. `COLOR_1` was rejected for the 8-bit quantization in Blender.
Both layouts store the six floats once, interleaved with a stride of 24 bytes, and point
several accessors into them. A part without `TEXCOORD_0` gets the custom names in either
layout, since texture coordinate sets must be numbered without gaps.

The branch table is written to the `extras` of each mesh as `arbrume_wind`:
`{"layout": "texcoord", "branches": {"count": n, "chain": [...], "parent": [...], ...}}`,
one array per field of `WindBranch` (`-1` for no parent), vectors flattened. Blender keeps
it as a custom property and three.js as `userData`.

Levels of detail in one file are separate child nodes `<name>_LOD<k>`, since `MSFT_lod`
was dropped or ignored everywhere. Instancing (`--instance-leaves`) is an option, not the
default: Blender expands it into thousands of objects, and the per-leaf colour and wind
data have no portable carriage.

## A shader

A three.js page that reads a file written with `--wind` and sways it takes, per vertex,
the branch of each level from the table (a float texture with one column per branch),
evaluates the shape of each branch at the `x` of the vertex, and adds an offset across the
branch along the wind:

```glsl
attribute vec2 uv1; attribute vec2 uv2; attribute vec2 uv3;
uniform float uTime; uniform vec3 uWind; uniform sampler2D uBranches;

vec3 windOffset(vec3 p) {
  float xs[4] = float[4](uv1.x, uv1.y, uv2.x, uv2.y);
  int own = int(uv3.x + 0.5);
  vec4 lineage = texelFetch(uBranches, ivec2(own, 2), 0);
  int level = int(texelFetch(uBranches, ivec2(own, 1), 0).z + 0.5);
  vec3 offset = vec3(0.0);
  for (int k = 0; k < 4; k++) {
    if (k > level) break;
    int b = int(lineage[k] + 0.5);
    vec4 tangentLength = texelFetch(uBranches, ivec2(b, 0), 0);
    vec4 shapePhase = texelFetch(uBranches, ivec2(b, 1), 0);
    float x = xs[k];
    float u = shapePhase.x * x * x + shapePhase.y * x * x * x * x;
    vec3 across = normalize(uWind - tangentLength.xyz * dot(uWind, tangentLength.xyz));
    float sway = 0.6 + 0.4 * sin(uTime * (0.35 + 0.45 * float(k)) * 6.2831 + shapePhase.w);
    offset += across * 0.03 * tangentLength.w / (1.0 + float(k)) * u * sway;
  }
  return offset + uv3.y * 0.02 * vec3(sin(uTime * 9.0 + p.x * 7.0), 0.0, cos(uTime * 8.0 + p.y * 6.0));
}
```

The amplitudes and frequencies are those of a demonstration, not physical values. Bending
a branch rotates the branches it bears only through their offset, not their direction:
the full scheme of Habel et al. also transforms the frames of the child branches and the
normals.

## Limits

- Deep axes (level above 3) bend about their own base only; the levels between move
  rigidly.
- A billboard carries `x0` along the first root branch only, so it sways as one piece.
- Instanced leaves carry no wind data and no flutter.
- The skeleton joints follow the centroid of the bark of a branch, which is close to its
  axis but not on it where the bark is sparse.
