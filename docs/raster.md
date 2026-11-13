# Rasterizer

`arbrume-raster` draws a `TreeMesh` on the CPU, on one thread, without a GPU. Its first
use is thumbnails, so that engineers and the web gallery can look at hundreds of plants;
its buffers (colour, albedo, normal, depth, coverage) are also what billboard atlas
baking needs. The rustdoc has the signatures (`cargo doc -p arbrume-raster --open`).

```rust
use arbrume_raster::{ThumbnailOptions, render_thumbnail};

let image = render_thumbnail(&mesh, &ThumbnailOptions::default());
image.write_png("plant.png")?;
let jpeg = image.to_jpeg(85, [255, 255, 255])?;
```

| Entry point | Use |
|---|---|
| `render_thumbnail(mesh, options)` | one thumbnail as an 8-bit sRGB `Image` |
| `Renderer::new()`, then `renderer.thumbnail(...)` or `renderer.thumbnail_target(...)` | many thumbnails; the renderer keeps its buffers |
| `renderer.render(mesh, &scene)` | any camera, lights, ground and background, into a `RenderTarget` |
| `Camera::frame(bounds, azimuth, elevation, fov)`, `Camera::frame_points(...)` | a camera that shows a box or a point set with a margin |
| `raster::rasterize(buffer, call)` | the bare triangle rasterizer, into a depth and id `VisibilityBuffer` |
| `Image::to_png()`, `Image::write_png(path)`, `Image::to_jpeg(quality, matte)` | encoding |

A renderer is single-threaded: to render many plants, run one renderer per thread. The
output is deterministic: the same mesh and options give byte-identical images on the
same platform, whether or not the renderer is reused.

## Pipeline

1. **Rasterize.** Every vertex is transformed to clip space. Each triangle is tested
   against the frustum; one crossing the near plane or a guard band of twice the
   viewport is clipped (Sutherland-Hodgman) and fanned. Vertices are snapped to 1/256
   pixel and the three edge functions are evaluated exactly in 64-bit integers, stepped
   incrementally over the bounding box (for wide boxes the covered run of each row is
   solved directly). A tie rule gives every pixel on a shared edge to exactly one
   triangle. Back faces of single-sided parts (bark) are culled; double-sided parts
   (leaves) are drawn from both sides. The pass writes only depth and triangle id, at
   `supersampling` times the output resolution in each direction.
2. **Light maps.** The sun shadow map is the plant rasterized from the sun with an
   orthographic camera that frames it. The sky occlusion grid is a coarse density field
   of the plant (see below).
3. **Shade.** For each visible sample the triangle is rebuilt from its id, and the
   perspective-correct barycentric coordinates come from the clip-space vertices
   (`M^-1 (u, v, 1)`, `M` having the columns `(x, y, w)` of the three vertices), which
   also tells whether the front face is seen. Position, normal and vertex colour times
   base colour are interpolated. Samples of a pixel that see the same triangle share one
   lighting computation, as in multisampling. The ground disc is intersected
   analytically along the pixel ray.
4. **Resolve.** The samples of each pixel are box-filtered into the `RenderTarget`.

## Shading

Shading is Lambertian, in linear colour:

- a sun term, attenuated by the shadow map with a percentage-closer filter (8 taps on the
  plant, 16 on the ground, rotated per pixel), a normal offset towards the light and a
  depth bias, so thin leaves do not shadow themselves;
- light through leaves lit from behind, a fraction `leaf_translucency` of the sun term
  of the back face;
- a hemisphere ambient, sky colour above and bounce colour below, blended by the
  normal's height and attenuated by sky occlusion.

Sky occlusion treats the crown as a participating medium. Each triangle adds its surface
area (both faces for leaves) to the cell of a grid, 32 cells along the plant's longest
side by default, holding its centroid. By Cauchy's formula the mean projected area of
randomly oriented surfaces is a quarter of their area, so a cell's extinction
coefficient is its area over four times its volume. Optical depths towards nine sky
directions, each one cell up and zero or one cell sideways, are accumulated in one sweep
from the top layer down; the visibility of a cell is the transmittance `exp(-depth)`
averaged over the directions with cosine weights. Shading points read it by trilinear
interpolation. Inner leaves and the ground under the crown come out darker than the
outer leaves, at a cost independent of the image size.

The shaded colour is multiplied by `exposure`, compressed above 0.8 by a soft shoulder
that reaches 1 asymptotically, and encoded to sRGB.

## Thumbnail options

`ThumbnailOptions` frames the whole plant (all vertices) from a view azimuth and
elevation, puts a ground disc under it, and places the sun relative to the view, so that
every thumbnail of a gallery is lit the same way whatever its view angle. Angles are in
degrees; azimuth 0 looks from +Z, and elevation 90 from straight above.

| Field | Default | Meaning |
|---|---|---|
| `width`, `height` | 256 | image size in pixels |
| `azimuth`, `elevation` | 0, 12 | view direction |
| `fov` | 30 | field of view across the smaller side; 0 gives an orthographic view |
| `margin` | 0.06 | fraction of the half image size left free around the plant |
| `supersampling` | 3 | samples per pixel along each axis (3 means 3x3) |
| `background` | pale sky gradient | `Solid`, vertical `Gradient`, or `Transparent` |
| `ground` | true | a disc of 1.6 times the plant's reach, fading out over its outer half |
| `sun_azimuth`, `sun_elevation` | -45, 45 | sun relative to the view: negative azimuths light from the left |
| `lighting` | `Lighting::default()` | colours, shadows, occlusion, translucency, exposure |

The `Lighting` defaults (sun, sky and bounce colours, translucency, filter radius) were
chosen by eye on the starter species and are not measured values. The shadow map size
defaults to twice the larger image side, clamped to 256 to 4096 texels, so the penumbra
keeps its size in pixels at every image size.

With a transparent background the ground fades to transparent at its rim. JPEG has no
alpha, so `to_jpeg` composites over a matte colour.

## Render target

`RenderTarget` holds, per output pixel, in linear floating point:

| Buffer | Content |
|---|---|
| `color` | shaded RGBA, alpha-premultiplied |
| `albedo` | plant albedo, averaged over the covered samples |
| `normal` | world normal of the plant, facing the camera, averaged and renormalized |
| `depth` | clip-space depth `z / w` of the nearest plant sample, 1 where uncovered |
| `coverage` | fraction of the samples covered by the plant (ground and background excluded) |

`to_image()` converts the colour buffer to 8-bit sRGB with straight alpha.

## Performance

Measured with `cargo bench -p arbrume-raster` on one thread, with the default options
(3x3 supersampling, shadows, sky occlusion, ground), on starter broadleaf plants:

| Plant | 256 x 256 | 512 x 512 |
|---|---|---|
| age 14, 35k triangles | 8.2 ms | 21.8 ms |
| age 30, quality 1.5, 285k triangles | 30.1 ms | 52.6 ms |

Time grows with the number of samples (9 per pixel at 3x3) and with the number of
triangles, which are rasterized twice (camera and sun). Figures are from one Windows
desktop machine and are only indicative.

## Limits

- Base-colour textures and alpha cutoff are ignored; leaves are drawn as their geometry.
- Geometry thinner than a sample (fine twigs, needles) aliases or vanishes; more
  supersampling helps.
- Only the plant casts shadows, on itself and on the ground disc.
