//! Instance tables: the placements of a forest for engines that instance the variant
//! files themselves.
//!
//! * CSV: a header line, then one line per tree:
//!   `species,variant,x,y,z,qx,qy,qz,qw,scale,r,g,b` with the species as its index in
//!   the manifest, the rotation as a unit quaternion and the colour factor as floats.
//! * Binary (`.bin`): a 32-byte header, then one 40-byte record per tree, little-endian.
//!   The header is the magic `ARBINST1`, the format version (`u32`, 1), the record size
//!   (`u32`, 40), the record count (`u64`) and 8 reserved zero bytes. A record is the
//!   position (3 `f32`), the rotation (4 `f32`, `x y z w`), the scale (`f32`), the
//!   species (`u16`), the variant (`u16`) and the colour factor (4 `u8`, `value / 128`).
//!   In Python it is the structured `NumPy` type `arbrume.INSTANCE_DTYPE`, read from
//!   byte 32.
//! * JSON: `{"instances": [[species, variant, x, y, z, qx, qy, qz, qw, scale, r, g, b],
//!   ...]}`, for small forests.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;

use crate::place::Instance;

/// Magic bytes of the binary table.
pub const MAGIC: &[u8; 8] = b"ARBINST1";
/// Bytes of the header of the binary table.
pub const HEADER_BYTES: usize = 32;
/// Bytes of one record of the binary table.
pub const RECORD_BYTES: usize = 40;

/// The bytes of one record.
fn record(tree: &Instance) -> [u8; RECORD_BYTES] {
    let mut bytes = [0u8; RECORD_BYTES];
    let floats = tree
        .position
        .iter()
        .chain(&tree.rotation)
        .chain(std::iter::once(&tree.scale));
    for (index, value) in floats.enumerate() {
        bytes[4 * index..4 * index + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[32..34].copy_from_slice(&tree.species.to_le_bytes());
    bytes[34..36].copy_from_slice(&tree.variant.to_le_bytes());
    bytes[36..40].copy_from_slice(&tree.tint);
    bytes
}

/// Writes the binary table of `trees` to `out`.
///
/// # Errors
/// When writing fails.
pub fn write_binary(trees: &[Instance], mut out: impl Write) -> io::Result<()> {
    let mut header = [0u8; HEADER_BYTES];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&1u32.to_le_bytes());
    header[12..16].copy_from_slice(&(RECORD_BYTES as u32).to_le_bytes());
    header[16..24].copy_from_slice(&(trees.len() as u64).to_le_bytes());
    out.write_all(&header)?;
    for tree in trees {
        out.write_all(&record(tree))?;
    }
    out.flush()
}

/// Reads a binary table written by [`write_binary`].
///
/// # Errors
/// When reading fails or the header is not that of a table.
pub fn read_binary(mut input: impl Read) -> io::Result<Vec<Instance>> {
    let mut header = [0u8; HEADER_BYTES];
    input.read_exact(&mut header)?;
    let size = u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;
    if &header[..8] != MAGIC || size != RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not an Arbrume instance table",
        ));
    }
    let mut count_bytes = [0u8; 8];
    count_bytes.copy_from_slice(&header[16..24]);
    let count = u64::from_le_bytes(count_bytes) as usize;
    let mut trees = Vec::with_capacity(count);
    let mut bytes = [0u8; RECORD_BYTES];
    let float = |b: &[u8], i: usize| {
        f32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
    };
    for _ in 0..count {
        input.read_exact(&mut bytes)?;
        trees.push(Instance {
            position: [float(&bytes, 0), float(&bytes, 1), float(&bytes, 2)],
            rotation: [
                float(&bytes, 3),
                float(&bytes, 4),
                float(&bytes, 5),
                float(&bytes, 6),
            ],
            scale: float(&bytes, 7),
            species: u16::from_le_bytes([bytes[32], bytes[33]]),
            variant: u16::from_le_bytes([bytes[34], bytes[35]]),
            tint: [bytes[36], bytes[37], bytes[38], bytes[39]],
        });
    }
    Ok(trees)
}

/// The values of one CSV or JSON row.
fn row(tree: &Instance) -> [String; 13] {
    let tint = tree.tint_factor();
    let floats = tree
        .position
        .iter()
        .chain(&tree.rotation)
        .chain(std::iter::once(&tree.scale))
        .chain(&tint[..3]);
    let mut out: [String; 13] = Default::default();
    out[0] = tree.species.to_string();
    out[1] = tree.variant.to_string();
    for (slot, value) in out[2..].iter_mut().zip(floats) {
        *slot = value.to_string();
    }
    out
}

/// Writes the CSV table of `trees` to `out`.
///
/// # Errors
/// When writing fails.
pub fn write_csv(trees: &[Instance], mut out: impl Write) -> io::Result<()> {
    writeln!(out, "species,variant,x,y,z,qx,qy,qz,qw,scale,r,g,b")?;
    for tree in trees {
        writeln!(out, "{}", row(tree).join(","))?;
    }
    out.flush()
}

/// Writes the JSON table of `trees` to `out`.
///
/// # Errors
/// When writing fails.
pub fn write_json(trees: &[Instance], mut out: impl Write) -> io::Result<()> {
    write!(out, "{{\"instances\": [")?;
    for (index, tree) in trees.iter().enumerate() {
        let separator = if index == 0 { "" } else { "," };
        write!(out, "{separator}\n[{}]", row(tree).join(","))?;
    }
    writeln!(out, "\n]}}")?;
    out.flush()
}

/// Writes `trees` to `path` in the format of its extension (`csv`, `bin`, `json`).
///
/// # Errors
/// When the extension is unknown or writing fails.
pub fn write_table(trees: &[Instance], path: &Path) -> io::Result<()> {
    let out = BufWriter::with_capacity(1 << 20, File::create(path)?);
    match path.extension().and_then(|e| e.to_str()) {
        Some("csv") => write_csv(trees, out),
        Some("bin") => write_binary(trees, out),
        Some("json") => write_json(trees, out),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown table format: {}", path.display()),
        )),
    }
}
