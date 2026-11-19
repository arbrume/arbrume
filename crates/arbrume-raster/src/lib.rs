//! CPU rasterizer for plant thumbnails and billboard atlas baking.
//!
//! The crate renders an [`arbrume_core::mesh::TreeMesh`] on one thread, without a GPU:
//!
//! * [`camera`]: perspective and orthographic [`Camera`]s and automatic framing;
//! * [`raster`]: exact fixed-point edge-function rasterization of clip-space triangles
//!   into a depth and triangle-id [`VisibilityBuffer`], with near-plane and guard-band
//!   clipping and back-face culling;
//! * [`shadow`]: the sun shadow map, with filtered soft shadows;
//! * [`occlusion`]: sky occlusion from a coarse foliage density grid;
//! * [`shade`]: deferred, per-sample shading and the supersampling box filter;
//! * [`scene`]: the [`Scene`] description and the reusable [`Renderer`];
//! * [`target`]: the resolved [`RenderTarget`] (colour, albedo, normal, depth, coverage)
//!   and 8-bit sRGB [`Image`]s;
//! * [`encode`]: PNG and JPEG encoding;
//! * [`texture`]: PNG textures of parts, alpha masking and normal mapping;
//! * [`thumbnail`]: [`render_thumbnail`], one call from a mesh to an image.
//!
//! Output is deterministic: the same mesh and options give byte-identical images on the
//! same platform (no threads, no time- or address-dependent state).
//!
//! ```
//! use arbrume_core::mesh::TreeMesh;
//! use arbrume_raster::{ThumbnailOptions, render_thumbnail};
//!
//! let image = render_thumbnail(&TreeMesh::default(), &ThumbnailOptions::default());
//! assert_eq!((image.width, image.height), (256, 256));
//! ```

pub mod camera;
pub mod encode;
pub mod occlusion;
pub mod raster;
pub mod scene;
pub mod shade;
pub mod shadow;
pub mod target;
pub mod texture;
pub mod thumbnail;

pub use camera::{Bounds, Camera, Projection};
pub use encode::EncodeError;
pub use raster::VisibilityBuffer;
pub use scene::{Background, Ground, Lighting, Renderer, Scene};
pub use target::{Image, RenderTarget};
pub use texture::{AlphaTest, Texture};
pub use thumbnail::{ThumbnailOptions, render_thumbnail};
