//! Pack archives: a small zip writer that gives the same bytes for the same files, and
//! a strict zip reader.
//!
//! **Writing** ([`write_archive`]). Entries are the files in the order of their paths,
//! with no directory entries; every entry has the timestamp 1980-01-01 00:00 (the
//! earliest a zip can hold), the UTF-8 name flag, Unix permissions `rw-r--r--` and no
//! extra field. A file is deflated (level 9) unless that does not make it smaller, in
//! which case it is stored. The same files therefore always give the same archive.
//!
//! **Reading** ([`read_archive`]) goes through the central directory and rejects what a
//! pack may not hold: a name that is not a safe path (`paths::check_path`: absolute,
//! `..`, backslashes, drive letters), two names that differ only by case, symbolic
//! links, encrypted entries, methods other than store and deflate, zip64 archives
//! (packs are smaller than 4 GiB), a file larger than `MAX_FILE_BYTES` or a total
//! larger than `MAX_PACK_BYTES` (checked while inflating, whatever the sizes the
//! archive declares), and a CRC that does not match.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use crate::paths::check_path;
use crate::{MAX_FILE_BYTES, MAX_PACK_BYTES};

/// Signature of a local file header.
const LOCAL: u32 = 0x0403_4B50;
/// Signature of a central directory header.
const CENTRAL: u32 = 0x0201_4B50;
/// Signature of the end of central directory record.
const END: u32 = 0x0605_4B50;
/// MS-DOS date of 1980-01-01.
const DOS_DATE: u16 = (1 << 5) | 1;
/// General purpose flag: names are UTF-8.
const UTF8: u16 = 1 << 11;
/// Unix mode of a regular file `rw-r--r--`, in the high half of the external attributes.
const MODE: u32 = 0o100_644 << 16;

/// One entry as written.
struct Written {
    name: String,
    method: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
}

/// Appends little-endian fields.
fn put(out: &mut Vec<u8>, fields: &[u32], widths: &[usize]) {
    for (&value, &width) in fields.iter().zip(widths) {
        out.extend_from_slice(&value.to_le_bytes()[..width]);
    }
}

/// The deflated bytes of `data` and the method, or `data` itself stored when deflating
/// does not shrink it.
fn compress(data: &[u8]) -> (u16, Vec<u8>) {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(9));
    let deflated = encoder
        .write_all(data)
        .and_then(|()| encoder.finish())
        .ok()
        .filter(|deflated| deflated.len() < data.len());
    deflated.map_or_else(|| (0, data.to_vec()), |deflated| (8, deflated))
}

/// Writes `files` (path to bytes) as a zip archive, the same bytes for the same files
/// (see the module documentation).
///
/// # Errors
///
/// Returns why the files cannot form a pack archive: an unsafe path, or more than
/// 65 535 files or 4 GiB.
pub fn write_archive(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let count = u16::try_from(files.len()).map_err(|_| "more than 65 535 files")?;
    let too_large = || "the archive would exceed 4 GiB".to_owned();
    let mut out = Vec::new();
    let mut written = Vec::with_capacity(files.len());
    for (name, data) in files {
        check_path(name)?;
        let (method, body) = compress(data);
        let entry = Written {
            name: name.clone(),
            method,
            crc: crc32fast::hash(data),
            compressed: u32::try_from(body.len()).map_err(|_| too_large())?,
            size: u32::try_from(data.len()).map_err(|_| too_large())?,
            offset: u32::try_from(out.len()).map_err(|_| too_large())?,
        };
        let header = [
            LOCAL,
            20,
            u32::from(UTF8),
            u32::from(method),
            0,
            u32::from(DOS_DATE),
            entry.crc,
            entry.compressed,
            entry.size,
            name.len() as u32,
            0,
        ];
        put(&mut out, &header, &[4, 2, 2, 2, 2, 2, 4, 4, 4, 2, 2]);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&body);
        written.push(entry);
    }
    let directory = u32::try_from(out.len()).map_err(|_| too_large())?;
    for entry in &written {
        let header = [
            CENTRAL,
            (3 << 8) | 0x14,
            20,
            u32::from(UTF8),
            u32::from(entry.method),
            0,
            u32::from(DOS_DATE),
            entry.crc,
            entry.compressed,
            entry.size,
            entry.name.len() as u32,
            0,
            0,
            0,
            0,
            MODE,
            entry.offset,
        ];
        put(
            &mut out,
            &header,
            &[4, 2, 2, 2, 2, 2, 2, 4, 4, 4, 2, 2, 2, 2, 2, 4, 4],
        );
        out.extend_from_slice(entry.name.as_bytes());
    }
    let size = u32::try_from(out.len()).map_err(|_| too_large())? - directory;
    let end = [
        END,
        0,
        0,
        u32::from(count),
        u32::from(count),
        size,
        directory,
        0,
    ];
    put(&mut out, &end, &[4, 2, 2, 2, 2, 4, 4, 2]);
    Ok(out)
}

/// Little-endian reader over a byte slice with bounds checks.
struct Fields<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Fields<'_> {
    fn take(&mut self, width: usize) -> Result<u32, String> {
        let end = self
            .at
            .checked_add(width)
            .filter(|&end| end <= self.bytes.len());
        let slice = end
            .map(|end| &self.bytes[self.at..end])
            .ok_or("truncated archive")?;
        self.at += width;
        let mut word = [0; 4];
        word[..width].copy_from_slice(slice);
        Ok(u32::from_le_bytes(word))
    }

    fn slice(&mut self, length: usize) -> Result<&[u8], String> {
        let end = self
            .at
            .checked_add(length)
            .filter(|&end| end <= self.bytes.len());
        let end = end.ok_or("truncated archive")?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
}

/// One entry of the central directory.
struct Entry {
    name: String,
    flags: u32,
    method: u32,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
    unix_mode: Option<u32>,
}

/// Position of the end of central directory record.
fn find_end(bytes: &[u8]) -> Result<usize, String> {
    let last = bytes.len().checked_sub(22).ok_or("not a zip archive")?;
    let first = last.saturating_sub(u16::MAX as usize);
    (first..=last)
        .rev()
        .find(|&at| bytes[at..at + 4] == END.to_le_bytes())
        .ok_or_else(|| "not a zip archive (no end of central directory)".to_owned())
}

/// Reads one central directory entry at the position of `fields`.
fn central_entry(fields: &mut Fields<'_>) -> Result<Entry, String> {
    if fields.take(4)? != CENTRAL {
        return Err("corrupt central directory".to_owned());
    }
    let made_by = fields.take(2)?;
    fields.take(2)?;
    let flags = fields.take(2)?;
    let method = fields.take(2)?;
    fields.take(4)?;
    let (crc, compressed, size) = (fields.take(4)?, fields.take(4)?, fields.take(4)?);
    let (name_len, extra_len, comment_len) = (fields.take(2)?, fields.take(2)?, fields.take(2)?);
    fields.take(4)?;
    let external = fields.take(4)?;
    let offset = fields.take(4)?;
    let name = fields.slice(name_len as usize)?;
    let name = String::from_utf8(name.to_vec()).map_err(|_| "an entry name is not UTF-8")?;
    fields.slice(extra_len as usize + comment_len as usize)?;
    if [compressed, size, offset].contains(&u32::MAX) {
        return Err("zip64 archives are not supported".to_owned());
    }
    Ok(Entry {
        name,
        flags,
        method,
        crc,
        compressed,
        size,
        offset,
        unix_mode: (made_by >> 8 == 3).then_some(external >> 16),
    })
}

/// Checks an entry before its data is read.
fn check_entry(entry: &Entry, seen: &mut BTreeMap<String, String>) -> Result<(), String> {
    let name = entry.name.trim_end_matches('/');
    check_path(name).map_err(|reason| format!("entry `{}`: {reason}", entry.name))?;
    if let Some(other) = seen.insert(name.to_ascii_lowercase(), entry.name.clone()) {
        return Err(format!("entries `{other}` and `{}` collide", entry.name));
    }
    if entry
        .unix_mode
        .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
    {
        return Err(format!("entry `{}` is a symbolic link", entry.name));
    }
    if entry.flags & 1 != 0 {
        return Err(format!("entry `{}` is encrypted", entry.name));
    }
    if entry.method != 0 && entry.method != 8 {
        let method = entry.method;
        return Err(format!(
            "entry `{}` uses compression method {method}",
            entry.name
        ));
    }
    if u64::from(entry.size) > MAX_FILE_BYTES {
        return Err(format!(
            "entry `{}` is larger than {MAX_FILE_BYTES} bytes",
            entry.name
        ));
    }
    Ok(())
}

/// The bytes of `entry`, inflated and checked against its size and CRC.
fn entry_data(bytes: &[u8], entry: &Entry) -> Result<Vec<u8>, String> {
    let mut local = Fields {
        bytes,
        at: entry.offset as usize,
    };
    if local.take(4)? != LOCAL {
        return Err(format!("entry `{}`: corrupt local header", entry.name));
    }
    local.at += 22;
    let (name_len, extra_len) = (local.take(2)?, local.take(2)?);
    if local.slice(name_len as usize)? != entry.name.as_bytes() {
        return Err(format!(
            "entry `{}`: local and central names differ",
            entry.name
        ));
    }
    local.slice(extra_len as usize)?;
    let body = local.slice(entry.compressed as usize)?;
    let limit = u64::from(entry.size) + 1;
    let mut data = Vec::new();
    let read = if entry.method == 0 {
        body.take(limit).read_to_end(&mut data)
    } else {
        DeflateDecoder::new(body).take(limit).read_to_end(&mut data)
    };
    read.map_err(|e| format!("entry `{}`: {e}", entry.name))?;
    if data.len() != entry.size as usize || crc32fast::hash(&data) != entry.crc {
        return Err(format!(
            "entry `{}`: size or CRC does not match",
            entry.name
        ));
    }
    Ok(data)
}

/// Reads the files of a zip archive, checked as the module documentation describes;
/// directory entries are skipped.
///
/// # Errors
///
/// Returns the first problem found, naming the entry.
pub fn read_archive(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let end = find_end(bytes)?;
    let mut fields = Fields { bytes, at: end + 4 };
    let (disk, start_disk) = (fields.take(2)?, fields.take(2)?);
    let (on_disk, total) = (fields.take(2)?, fields.take(2)?);
    let (_size, offset) = (fields.take(4)?, fields.take(4)?);
    if disk != 0 || start_disk != 0 || on_disk != total {
        return Err("multi-volume archives are not supported".to_owned());
    }
    if total == u32::from(u16::MAX) || offset == u32::MAX {
        return Err("zip64 archives are not supported".to_owned());
    }
    let mut fields = Fields {
        bytes,
        at: offset as usize,
    };
    let mut seen = BTreeMap::new();
    let mut files = BTreeMap::new();
    let mut total_bytes = 0_u64;
    for _ in 0..total {
        let entry = central_entry(&mut fields)?;
        check_entry(&entry, &mut seen)?;
        if entry.name.ends_with('/') {
            continue;
        }
        total_bytes += u64::from(entry.size);
        if total_bytes > MAX_PACK_BYTES {
            return Err(format!("the files exceed {MAX_PACK_BYTES} bytes in total"));
        }
        let data = entry_data(bytes, &entry)?;
        files.insert(entry.name, data);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::{read_archive, write_archive};
    use std::collections::BTreeMap;

    fn files() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            ("pack.toml".to_owned(), b"format = 1\n".repeat(40)),
            ("textures/a.png".to_owned(), vec![7, 1, 200]),
        ])
    }

    /// An archive with one stored entry named `name`.
    fn with_name(name: &str) -> Vec<u8> {
        let placeholder = "x".repeat(name.len());
        let bytes = write_archive(&BTreeMap::from([(placeholder.clone(), b"abc".to_vec())]));
        let mut bytes = bytes.unwrap();
        let mut at = 0;
        while let Some(found) = bytes[at..]
            .windows(name.len())
            .position(|w| w == placeholder.as_bytes())
        {
            bytes[at + found..at + found + name.len()].copy_from_slice(name.as_bytes());
            at += found + name.len();
        }
        bytes
    }

    #[test]
    fn archives_round_trip_and_are_deterministic() {
        let bytes = write_archive(&files()).unwrap();
        assert_eq!(bytes, write_archive(&files()).unwrap());
        assert_eq!(read_archive(&bytes).unwrap(), files());
        assert!(bytes.len() < 300, "the text is deflated");
    }

    #[test]
    fn traversal_and_absolute_names_are_rejected() {
        for name in ["../evil.toml", "/etc/passwdx", "a\\..\\..\\b", "C:/x/y.png"] {
            let error = read_archive(&with_name(name)).unwrap_err();
            assert!(error.contains("entry"), "{name}: {error}");
        }
        assert!(read_archive(&with_name("species/a.toml")).is_ok());
    }

    #[test]
    fn corrupt_archives_are_rejected() {
        let mut bytes = write_archive(&files()).unwrap();
        assert!(read_archive(&bytes[..bytes.len() - 30]).is_err());
        assert!(read_archive(b"not a zip at all, not at all, no").is_err());
        let data = bytes.windows(3).position(|w| w == [7, 1, 200]).unwrap();
        bytes[data] ^= 0xFF;
        assert!(read_archive(&bytes).is_err());
    }
}
