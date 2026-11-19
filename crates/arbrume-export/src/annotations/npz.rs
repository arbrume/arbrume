//! Uncompressed `.npz` archives: a ZIP file of `.npy` arrays, loadable with `numpy.load`.
//!
//! Each array is written as a NPY format 1.0 entry (a magic string, a Python dict
//! literal header padded to 64 bytes, then the little-endian data in C order) and
//! stored without compression, so that loading is a copy. Timestamps are fixed, so the
//! bytes of an archive depend on its arrays only. ZIP64 is not written: an archive is
//! limited to 4 GiB, far above the annotations of one plant.
//!
//! The reader accepts the archives of this writer (stored entries, sizes in the local
//! headers); it is used to load sidecars back and by the tests, not as a general ZIP
//! reader.

use std::borrow::Cow;
use std::io::{self, Write};

/// Element type of an array, with its NPY `descr` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dtype {
    /// `u8`, `|u1`.
    U8,
    /// `u16`, `<u2`.
    U16,
    /// `u32`, `<u4`.
    U32,
    /// `i32`, `<i4`.
    I32,
    /// `f32`, `<f4`.
    F32,
}

impl Dtype {
    /// NPY `descr` of the type.
    #[must_use]
    pub const fn descr(self) -> &'static str {
        match self {
            Self::U8 => "|u1",
            Self::U16 => "<u2",
            Self::U32 => "<u4",
            Self::I32 => "<i4",
            Self::F32 => "<f4",
        }
    }

    /// Size of one element in bytes.
    #[must_use]
    pub const fn size(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
        }
    }

    fn from_descr(descr: &str) -> Option<Self> {
        [Self::U8, Self::U16, Self::U32, Self::I32, Self::F32]
            .into_iter()
            .find(|dtype| dtype.descr() == descr || (descr == "<u1" && *dtype == Self::U8))
    }
}

/// One array of an archive: name, type, shape and little-endian bytes, borrowed from
/// the values written or owned after reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Array<'a> {
    /// Name, without the `.npy` suffix.
    pub name: String,
    /// Element type.
    pub dtype: Dtype,
    /// Shape, C order; empty for a scalar.
    pub shape: Vec<usize>,
    /// Data, `shape.product() * dtype.size()` bytes.
    pub data: Cow<'a, [u8]>,
}

impl<'a> Array<'a> {
    /// An array of `values` with `shape`, borrowing them.
    ///
    /// # Panics
    ///
    /// Panics when the shape does not match the number of values.
    #[must_use]
    pub fn new<T: bytemuck::Pod>(
        name: &str,
        dtype: Dtype,
        shape: &[usize],
        values: &'a [T],
    ) -> Self {
        Self::with_data(
            name,
            dtype,
            shape,
            Cow::Borrowed(bytemuck::cast_slice(values)),
        )
    }

    /// An array of `values` with `shape`, owning them.
    ///
    /// # Panics
    ///
    /// Panics when the shape does not match the number of values.
    #[must_use]
    pub fn owned<T: bytemuck::Pod>(
        name: &str,
        dtype: Dtype,
        shape: &[usize],
        values: &[T],
    ) -> Self {
        let data = bytemuck::cast_slice(values).to_vec();
        Self::with_data(name, dtype, shape, Cow::Owned(data))
    }

    fn with_data(name: &str, dtype: Dtype, shape: &[usize], data: Cow<'a, [u8]>) -> Self {
        assert_eq!(
            data.len(),
            shape.iter().product::<usize>() * dtype.size(),
            "shape of `{name}` does not match its data"
        );
        Self {
            name: name.to_owned(),
            dtype,
            shape: shape.to_vec(),
            data,
        }
    }

    /// The data as values of type `T`, which must have the size of the element type.
    ///
    /// # Errors
    ///
    /// Returns a message when the size of `T` differs from that of the element type.
    pub fn values<T: bytemuck::Pod>(&self) -> Result<Vec<T>, String> {
        if std::mem::size_of::<T>() != self.dtype.size() {
            return Err(format!("`{}` has elements of another size", self.name));
        }
        let mut values = vec![T::zeroed(); self.data.len() / self.dtype.size()];
        bytemuck::cast_slice_mut::<T, u8>(&mut values).copy_from_slice(&self.data);
        Ok(values)
    }

    /// NPY format 1.0 header, padded so that the data starts on a 64-byte boundary.
    fn npy_header(&self) -> Vec<u8> {
        let shape = match self.shape.as_slice() {
            [] => "()".to_owned(),
            [one] => format!("({one},)"),
            many => {
                let parts: Vec<String> = many.iter().map(usize::to_string).collect();
                format!("({})", parts.join(", "))
            }
        };
        let dict = format!(
            "{{'descr': '{}', 'fortran_order': False, 'shape': {shape}, }}",
            self.dtype.descr()
        );
        let unpadded = 10 + dict.len() + 1;
        let total = unpadded.div_ceil(64) * 64;
        let mut header = Vec::with_capacity(total);
        header.extend_from_slice(b"\x93NUMPY\x01\x00");
        header.extend_from_slice(&((total - 10) as u16).to_le_bytes());
        header.extend_from_slice(dict.as_bytes());
        header.resize(total - 1, b' ');
        header.push(b'\n');
        header
    }
}

/// CRC-32 (IEEE 802.3, reflected, as ZIP uses) of the concatenation of `parts`.
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize()
}

/// DOS date of 1980-01-01, the earliest a ZIP entry can carry.
const DOS_DATE: u16 = (1 << 5) | 1;

/// Writes `arrays` as an uncompressed `.npz` archive to `out`.
///
/// # Errors
///
/// Returns the error of `out`, or [`io::ErrorKind::InvalidInput`] when the archive would
/// exceed the 4 GiB of a ZIP file without ZIP64.
pub fn write_npz(arrays: &[Array<'_>], mut out: impl Write) -> io::Result<()> {
    let too_large = || io::Error::new(io::ErrorKind::InvalidInput, "npz over 4 GiB");
    let mut central = Vec::new();
    let mut offset = 0_usize;
    for array in arrays {
        let name = format!("{}.npy", array.name);
        let header = array.npy_header();
        let size = u32::try_from(header.len() + array.data.len()).map_err(|_| too_large())?;
        let crc = crc32(&[&header, &array.data]);
        let fields = EntryFields {
            crc,
            size,
            name: name.as_bytes(),
        };
        let local = fields.local_header();
        out.write_all(&local)?;
        out.write_all(&header)?;
        out.write_all(&array.data)?;
        let at = u32::try_from(offset).map_err(|_| too_large())?;
        central.extend_from_slice(&fields.central_header(at));
        offset += local.len() + size as usize;
    }
    let at = u32::try_from(offset).map_err(|_| too_large())?;
    let count = u16::try_from(arrays.len()).map_err(|_| too_large())?;
    let mut end = Vec::with_capacity(22);
    end.extend_from_slice(&0x0605_4B50_u32.to_le_bytes());
    end.extend_from_slice(&[0; 4]);
    end.extend_from_slice(&count.to_le_bytes());
    end.extend_from_slice(&count.to_le_bytes());
    end.extend_from_slice(&(central.len() as u32).to_le_bytes());
    end.extend_from_slice(&at.to_le_bytes());
    end.extend_from_slice(&[0; 2]);
    out.write_all(&central)?;
    out.write_all(&end)?;
    out.flush()
}

/// The fields shared by the local and the central header of one stored entry.
struct EntryFields<'a> {
    crc: u32,
    size: u32,
    name: &'a [u8],
}

impl EntryFields<'_> {
    /// Version, flags, method, time, date, CRC, both sizes and the name length.
    fn common(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&20_u16.to_le_bytes());
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&self.crc.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&(self.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0; 2]);
    }

    fn local_header(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(30 + self.name.len());
        out.extend_from_slice(&0x0403_4B50_u32.to_le_bytes());
        self.common(&mut out);
        out.extend_from_slice(self.name);
        out
    }

    fn central_header(&self, offset: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(46 + self.name.len());
        out.extend_from_slice(&0x0201_4B50_u32.to_le_bytes());
        out.extend_from_slice(&20_u16.to_le_bytes());
        self.common(&mut out);
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(self.name);
        out
    }
}

/// Little-endian `u16` at `at`.
fn le16(bytes: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(
        bytes.get(at..at + 2)?.try_into().ok()?,
    )))
}

/// Little-endian `u32` at `at`.
fn le32(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Reads the arrays of an archive written by [`write_npz`].
///
/// # Errors
///
/// Returns a message for a truncated archive, a compressed entry, a CRC mismatch or a
/// `.npy` entry this reader does not understand.
pub fn read_npz(bytes: &[u8]) -> Result<Vec<Array<'static>>, String> {
    let truncated = || "truncated npz archive".to_owned();
    let mut arrays = Vec::new();
    let mut at = 0;
    while le32(bytes, at) == Some(0x0403_4B50) {
        if le16(bytes, at + 8) != Some(0) {
            return Err("compressed npz entries are not supported".to_owned());
        }
        let crc = le32(bytes, at + 14).ok_or_else(truncated)?;
        let size = le32(bytes, at + 18).ok_or_else(truncated)?;
        let name_len = le16(bytes, at + 26).ok_or_else(truncated)?;
        let extra_len = le16(bytes, at + 28).ok_or_else(truncated)?;
        let name_at = at + 30;
        let data_at = name_at + name_len + extra_len;
        let name = bytes
            .get(name_at..name_at + name_len)
            .ok_or_else(truncated)?;
        let data = bytes.get(data_at..data_at + size).ok_or_else(truncated)?;
        if crc32(&[data]) as usize != crc {
            return Err("CRC mismatch in npz archive".to_owned());
        }
        let name = String::from_utf8_lossy(name);
        let name = name.strip_suffix(".npy").unwrap_or(&name);
        arrays.push(parse_npy(name, data)?);
        at = data_at + size;
    }
    Ok(arrays)
}

/// Parses one `.npy` entry of format 1.0 or 2.0, C order.
fn parse_npy(name: &str, bytes: &[u8]) -> Result<Array<'static>, String> {
    let fail = |what: &str| format!("`{name}.npy`: {what}");
    if !bytes.starts_with(b"\x93NUMPY") {
        return Err(fail("not a npy entry"));
    }
    let (header_len, start) = match bytes.get(6) {
        Some(1) => (le16(bytes, 8), 10),
        Some(2 | 3) => (le32(bytes, 8), 12),
        _ => (None, 0),
    };
    let header_len = header_len.ok_or_else(|| fail("unknown npy version"))?;
    let header = bytes
        .get(start..start + header_len)
        .ok_or_else(|| fail("truncated header"))?;
    let header = String::from_utf8_lossy(header);
    let field = |key: &str| {
        let after = header.split_once(key)?.1;
        Some(after.trim_start_matches([':', ' ', '\'']).to_owned())
    };
    if field("'fortran_order'").is_some_and(|value| value.starts_with("True")) {
        return Err(fail("Fortran order is not supported"));
    }
    let descr = field("'descr'").ok_or_else(|| fail("no descr"))?;
    let descr = descr.split('\'').next().unwrap_or_default();
    let dtype = Dtype::from_descr(descr).ok_or_else(|| fail("unsupported dtype"))?;
    let shape = field("'shape'").ok_or_else(|| fail("no shape"))?;
    let inside = shape
        .trim_start_matches('(')
        .split(')')
        .next()
        .unwrap_or_default();
    let shape = inside
        .split(',')
        .map(str::trim)
        .filter(|dim| !dim.is_empty())
        .map(str::parse)
        .collect::<Result<Vec<usize>, _>>()
        .map_err(|_| fail("bad shape"))?;
    let data: Cow<'static, [u8]> = Cow::Owned(bytes[start + header_len..].to_vec());
    if data.len() != shape.iter().product::<usize>() * dtype.size() {
        return Err(fail("data size does not match the shape"));
    }
    Ok(Array {
        name: name.to_owned(),
        dtype,
        shape,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::{Array, Dtype, crc32, read_npz, write_npz};

    #[test]
    fn crc_matches_the_reference_value() {
        assert_eq!(crc32(&[b"123456789"]), 0xCBF4_3926);
        assert_eq!(crc32(&[b"1234", b"56789"]), 0xCBF4_3926);
    }

    #[test]
    fn arrays_round_trip() {
        let arrays = vec![
            Array::new(
                "a",
                Dtype::F32,
                &[2, 3],
                &[1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0],
            ),
            Array::new("b", Dtype::I32, &[2], &[-1_i32, 7]),
            Array::new("c", Dtype::U8, &[0], &[0_u8; 0]),
            Array::new("d", Dtype::U16, &[], &[9_u16]),
        ];
        let mut bytes = Vec::new();
        write_npz(&arrays, &mut bytes).unwrap();
        assert_eq!(read_npz(&bytes).unwrap(), arrays);
        let headers = arrays.iter().map(Array::npy_header);
        assert!(headers.into_iter().all(|h| h.len() % 64 == 0));
    }
}
