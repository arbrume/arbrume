//! The hash manifest `MANIFEST.sha256`: one line `<sha256 in hex>  <path>` per file of
//! the pack other than itself, sorted by path, the format of `sha256sum` (so
//! `sha256sum -c MANIFEST.sha256` checks an extracted pack).

use std::collections::BTreeMap;
use std::fmt::Write;

use sha2::{Digest, Sha256};

use crate::HASHES_FILE;

/// SHA-256 of `bytes` in lowercase hexadecimal.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// The text of the hash manifest of `files` (path to bytes), which skips
/// `MANIFEST.sha256` itself.
#[must_use]
pub fn hash_manifest(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut text = String::new();
    for (path, bytes) in files.iter().filter(|(path, _)| *path != HASHES_FILE) {
        let _ = writeln!(text, "{}  {path}", sha256_hex(bytes));
    }
    text
}

/// Checks `files` against their hash manifest `manifest`.
///
/// # Errors
///
/// Returns every mismatch: a malformed line, a file listed but missing, a file not
/// listed, a hash that differs.
pub fn verify_hashes(files: &BTreeMap<String, Vec<u8>>, manifest: &str) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let mut listed = BTreeMap::new();
    for line in manifest.lines().filter(|line| !line.trim().is_empty()) {
        match line.split_once("  ") {
            Some((hash, path)) if hash.len() == 64 => {
                listed.insert(path.to_owned(), hash.to_owned());
            }
            _ => problems.push(format!("malformed line `{line}`")),
        }
    }
    for (path, bytes) in files.iter().filter(|(path, _)| *path != HASHES_FILE) {
        match listed.remove(path) {
            None => problems.push(format!("`{path}` is not listed")),
            Some(hash) if hash != sha256_hex(bytes) => {
                problems.push(format!("`{path}` does not match its hash"));
            }
            Some(_) => {}
        }
    }
    problems.extend(
        listed
            .keys()
            .map(|path| format!("`{path}` is listed but missing")),
    );
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::{hash_manifest, sha256_hex, verify_hashes};
    use std::collections::BTreeMap;

    #[test]
    fn known_hash_and_round_trip() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut files = BTreeMap::from([
            ("a.txt".to_owned(), b"a".to_vec()),
            ("b/c.png".to_owned(), b"c".to_vec()),
        ]);
        let manifest = hash_manifest(&files);
        files.insert("MANIFEST.sha256".to_owned(), manifest.clone().into_bytes());
        assert!(verify_hashes(&files, &manifest).is_ok());
        files.insert("a.txt".to_owned(), b"changed".to_vec());
        files.insert("extra.txt".to_owned(), Vec::new());
        files.remove("b/c.png");
        let problems = verify_hashes(&files, &manifest).unwrap_err();
        assert_eq!(problems.len(), 3, "{problems:?}");
    }
}
