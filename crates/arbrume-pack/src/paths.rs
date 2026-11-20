//! Paths inside a pack, the files a pack may hold and the content it may not.
//!
//! A path is relative, its parts separated by `/`, each part made of ASCII letters,
//! digits, `.`, `_` and `-`, not `.` nor `..`, not starting with `.`, and not a device
//! name of Windows (`con`, `nul`, `com1`, ...). These are names every platform can
//! extract and that never leave the pack directory. Two paths may not differ only by
//! case, since they would be the same file on Windows and macOS.
//!
//! Allowed files ([`kind`]): `pack.toml`, `MANIFEST.sha256`, a `README.md` or
//! `README.txt`, the licence file named by `pack.toml` (`.txt` or `.md` at the root),
//! `species/*.toml`, images under `textures/` (`.png`, `.jpg`, `.jpeg`),
//! `thumbnails/*.png` and `previews/*.glb`. Text files must be UTF-8; a file starting
//! like an executable (`MZ`, ELF, Mach-O, `#!`) is rejected whatever its name.

use crate::{HASHES_FILE, PACK_FILE};

/// Device names that Windows reserves in every directory, whatever the extension.
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Checks that `path` is a safe relative path inside a pack (see the module
/// documentation).
///
/// # Errors
///
/// Returns why the path is rejected.
pub fn check_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("empty path".to_owned());
    }
    if path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return Err(format!(
            "`{path}` is not a relative path with `/` separators"
        ));
    }
    for part in path.split('/') {
        if part.is_empty() || part.starts_with('.') {
            return Err(format!("`{path}` has an empty, hidden, `.` or `..` part"));
        }
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
        if !part.chars().all(allowed) {
            return Err(format!(
                "`{path}`: a path is made of ASCII letters, digits, `.`, `_` and `-`"
            ));
        }
        let stem = part.split('.').next().unwrap_or(part).to_ascii_lowercase();
        if RESERVED.contains(&stem.as_str()) {
            return Err(format!("`{path}` uses the reserved name `{stem}`"));
        }
    }
    Ok(())
}

/// What a file of a pack is, from its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    /// `pack.toml`.
    Pack,
    /// `MANIFEST.sha256`.
    Hashes,
    /// The licence file or a readme.
    Text,
    /// A species file.
    Species,
    /// A PNG or JPEG image under `textures/`.
    Texture,
    /// A PNG thumbnail.
    Thumbnail,
    /// A binary glTF preview.
    Preview,
}

/// The kind of the file at `path`, or `None` when a pack may not hold it;
/// `licence_file` is the licence file named by `pack.toml`.
pub(crate) fn kind(path: &str, licence_file: &str) -> Option<FileKind> {
    let extension = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let extension = extension.as_deref().unwrap_or("");
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let text = matches!(extension, "txt" | "md");
    match (dir, extension) {
        _ if path == PACK_FILE => Some(FileKind::Pack),
        _ if path == HASHES_FILE => Some(FileKind::Hashes),
        ("", _) if text && (path == licence_file || name.starts_with("README.")) => {
            Some(FileKind::Text)
        }
        ("species", "toml") => Some(FileKind::Species),
        (_, "png" | "jpg" | "jpeg") if dir == "textures" || dir.starts_with("textures/") => {
            Some(FileKind::Texture)
        }
        ("thumbnails", "png") => Some(FileKind::Thumbnail),
        ("previews", "glb") => Some(FileKind::Preview),
        _ => None,
    }
}

/// Checks the bytes of a file of kind `kind`: no executable header, UTF-8 text, the
/// signature of its format.
pub(crate) fn check_content(kind: FileKind, bytes: &[u8]) -> Result<(), String> {
    let executable: [&[u8]; 6] = [
        b"MZ",
        b"\x7fELF",
        b"#!",
        &[0xCF, 0xFA, 0xED, 0xFE],
        &[0xCE, 0xFA, 0xED, 0xFE],
        &[0xCA, 0xFE, 0xBA, 0xBE],
    ];
    if executable.iter().any(|magic| bytes.starts_with(magic)) {
        return Err("executable content is not allowed in a pack".to_owned());
    }
    match kind {
        FileKind::Pack | FileKind::Hashes | FileKind::Text | FileKind::Species => {
            std::str::from_utf8(bytes)
                .map(|_| ())
                .map_err(|_| "a text file must be UTF-8".to_owned())
        }
        FileKind::Texture => {
            let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
            let jpeg = bytes.starts_with(&[0xFF, 0xD8, 0xFF]);
            (png || jpeg)
                .then_some(())
                .ok_or_else(|| "not a PNG or JPEG image".to_owned())
        }
        FileKind::Thumbnail => bytes
            .starts_with(b"\x89PNG\r\n\x1a\n")
            .then_some(())
            .ok_or_else(|| "a thumbnail is a PNG image".to_owned()),
        FileKind::Preview => check_glb(bytes),
    }
}

/// Checks the header of a binary glTF file: magic, version 2, declared length.
fn check_glb(bytes: &[u8]) -> Result<(), String> {
    let word = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if !bytes.starts_with(b"glTF") || word(4) != Some(2) {
        return Err("a preview is a binary glTF 2.0 file (.glb)".to_owned());
    }
    if word(8).map(|n| n as usize) != Some(bytes.len()) {
        return Err("the length in the .glb header is not the file size".to_owned());
    }
    Ok(())
}

/// Checks a pack or species name: an ASCII lowercase letter, then lowercase letters,
/// digits and `_`, at most 64 characters.
///
/// # Errors
///
/// Returns why the name is rejected.
pub(crate) fn check_name(what: &str, name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let first = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    let rest = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if first && rest && name.len() <= 64 {
        return Ok(());
    }
    Err(format!(
        "{what} `{name}` must start with a lowercase ASCII letter and hold only lowercase \
         letters, digits and `_`, at most 64 characters"
    ))
}

#[cfg(test)]
mod tests {
    use super::{FileKind, check_content, check_name, check_path, kind};

    #[test]
    fn unsafe_paths_are_rejected() {
        for path in [
            "species/oak.toml",
            "textures/bark/oak-1.png",
            "LICENCE-CONTENT.txt",
        ] {
            assert!(check_path(path).is_ok(), "{path}");
        }
        for path in [
            "",
            "/etc/passwd",
            "../escape.toml",
            "species/../../x",
            "species//a.toml",
            "C:/a.png",
            "textures\\a.png",
            "species/.hidden.toml",
            "species/my oak.toml",
            "textures/nul.png",
            "textures/caf\u{e9}.png",
        ] {
            assert!(check_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn only_known_files_are_allowed() {
        let licence = "LICENCE-CONTENT.txt";
        assert_eq!(kind("pack.toml", licence), Some(FileKind::Pack));
        assert_eq!(kind("LICENCE-CONTENT.txt", licence), Some(FileKind::Text));
        assert_eq!(kind("README.md", licence), Some(FileKind::Text));
        assert_eq!(kind("species/oak.toml", licence), Some(FileKind::Species));
        assert_eq!(kind("textures/a/b.JPG", licence), Some(FileKind::Texture));
        assert_eq!(
            kind("thumbnails/oak.png", licence),
            Some(FileKind::Thumbnail)
        );
        assert_eq!(kind("previews/oak.glb", licence), Some(FileKind::Preview));
        for path in [
            "run.exe",
            "species/oak.py",
            "species/sub/oak.toml",
            "textures/a.gif",
            "other.txt",
            "thumbnails/a.jpg",
            "scripts/install.sh",
        ] {
            assert_eq!(kind(path, licence), None, "{path}");
        }
    }

    #[test]
    fn executables_and_wrong_formats_are_rejected() {
        assert!(check_content(FileKind::Texture, b"MZ\x90\x00").is_err());
        assert!(check_content(FileKind::Species, b"#!/bin/sh\nrm -rf /").is_err());
        assert!(check_content(FileKind::Species, b"name = \"a\"").is_ok());
        assert!(check_content(FileKind::Species, &[0xFF, 0xFE, 0x00]).is_err());
        assert!(check_content(FileKind::Texture, b"GIF89a").is_err());
        assert!(check_content(FileKind::Preview, b"glTF\x02\x00\x00\x00\x0c\x00\x00\x00").is_ok());
        assert!(check_content(FileKind::Preview, b"glTF\x02\x00\x00\x00\x20\x00\x00\x00").is_err());
    }

    #[test]
    fn names_are_lowercase_identifiers() {
        assert!(check_name("pack id", "starter_2").is_ok());
        for name in ["", "Starter", "2pack", "a-b", "a/b", &"a".repeat(65)] {
            assert!(check_name("pack id", name).is_err(), "{name}");
        }
    }
}
