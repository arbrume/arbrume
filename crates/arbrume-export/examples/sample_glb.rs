//! Writes the two-part sample tree to the `.glb` path given as the only argument.
//!
//! CI runs it and feeds the result to the Khronos glTF validator (`tools/validate_glb.mjs`).

#[path = "../tests/common/mod.rs"]
mod common;

use arbrume_export::{GlbOptions, write_glb_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: sample_glb <out.glb>")?;
    let options = GlbOptions {
        name: "sample".to_owned(),
        extras: Some(serde_json::json!({ "species": "sample", "seed": 1 })),
        ..GlbOptions::default()
    };
    write_glb_file(&common::sample_mesh(), &options, &path)?;
    println!("wrote {path}");
    Ok(())
}
