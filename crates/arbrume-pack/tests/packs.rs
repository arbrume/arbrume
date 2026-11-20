//! Packs end to end: authoring, validation, deterministic archives, installation, use,
//! and the packs that must be rejected.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use arbrume_gen::{GenParams, Generator, TextureCache, TextureOptions};
use arbrume_pack::{
    HASHES_FILE, Pack, PackError, PackStore, add_species, build_archive, new_pack, read_archive,
    validate, validate_path, write_archive, write_thumbnails,
};
use arbrume_texture::{Image, decode_image};

/// A square sRGB RGBA PNG of one colour.
fn png(side: u32, pixel: [u8; 4]) -> Vec<u8> {
    Image {
        width: side,
        height: side,
        channels: 4,
        srgb: true,
        data: pixel.repeat((side * side) as usize),
    }
    .to_png()
}

/// A species file next to its images, as an author would have it before adding it.
fn textured_source(dir: &Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir.join("textures")).unwrap();
    std::fs::write(dir.join("textures/bark.png"), png(32, [120, 80, 40, 255])).unwrap();
    std::fs::write(dir.join("textures/leaf.png"), png(32, [40, 150, 30, 255])).unwrap();
    let mut text = arbrume_gen::preset_toml("rosette_herb").unwrap().to_owned();
    text = text.replace("[bark]", "[bark]\ntexture = \"textures/bark.png\"");
    text = text.replace("[foliage]", "[foliage]\ntexture = \"textures/leaf.png\"");
    assert!(
        text.contains("textures/leaf.png"),
        "the preset has a [foliage] table"
    );
    if !text.contains("textures/bark.png") {
        text.push_str("\n[bark]\ntexture = \"textures/bark.png\"\n");
    }
    let path = dir.join("textured.toml");
    std::fs::write(&path, text).unwrap();
    path
}

/// A small valid pack directory `demo` with two presets and a textured species.
fn demo_pack(root: &Path) -> std::path::PathBuf {
    let dir = root.join("demo");
    new_pack(&dir, None, Some("Demo")).unwrap();
    add_species(&dir, "rosette_herb", None, None, Some("herb")).unwrap();
    add_species(&dir, "sapling", Some("young"), Some("Young tree"), None).unwrap();
    let source = textured_source(&root.join("authoring"));
    add_species(&dir, source.to_str().unwrap(), None, None, None).unwrap();
    dir
}

#[test]
fn authoring_validation_and_thumbnails() {
    let root = tempfile::tempdir().unwrap();
    let dir = demo_pack(root.path());
    assert!(dir.join("textures/bark.png").is_file(), "images are copied");
    let report = validate_path(&dir);
    assert!(report.is_ok(), "{:?}", report.errors);
    let warned: Vec<&str> = report.warnings.iter().map(|w| w.file.as_str()).collect();
    assert!(warned.contains(&"LICENCE-CONTENT.txt"), "{warned:?}");
    assert!(warned.contains(&"thumbnails/young.png"), "{warned:?}");
    let again = add_species(&dir, "sapling", Some("young"), None, None).unwrap_err();
    assert!(again.to_string().contains("already has"), "{again}");
    let written = write_thumbnails(&dir, 64, false).unwrap();
    assert_eq!(written.len(), 3);
    let thumb = decode_image(&std::fs::read(dir.join("thumbnails/young.png")).unwrap()).unwrap();
    assert_eq!((thumb.width, thumb.height), (64, 64));
    assert_eq!(
        write_thumbnails(&dir, 64, false).unwrap(),
        Vec::<String>::new()
    );
    let report = validate_path(&dir);
    assert!(
        report
            .warnings
            .iter()
            .all(|w| !w.file.starts_with("thumbnails/"))
    );
}

#[test]
fn build_install_and_generate_round_trip() {
    let root = tempfile::tempdir().unwrap();
    let dir = demo_pack(root.path());
    let pack = Pack::open(&dir).unwrap();
    let archive = build_archive(&pack).unwrap();
    assert_eq!(archive, build_archive(&Pack::open(&dir).unwrap()).unwrap());
    let files = read_archive(&archive).unwrap();
    assert!(files.contains_key(HASHES_FILE));
    assert!(
        files.contains_key("thumbnails/textured.png"),
        "thumbnails are rendered"
    );
    let file = root.path().join("demo.arbpack");
    std::fs::write(&file, &archive).unwrap();
    assert!(validate_path(&file).is_ok());

    let store = PackStore::new(root.path().join("store"));
    let outcome = store.install(&file).unwrap();
    assert_eq!((outcome.id.as_str(), outcome.replaced), ("demo", None));
    let outcome = store.install(&file).unwrap();
    assert_eq!(outcome.replaced.as_deref(), Some("0.1.0"));
    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].as_ref().unwrap().manifest().species.len(), 3);

    let entry = store.species("demo/textured").unwrap();
    assert!(entry.thumbnail.as_ref().is_some_and(|path| path.is_file()));
    let species = entry.species().unwrap();
    assert_eq!(species.asset_dir.as_deref(), Some(outcome.path.as_path()));
    let mut generator = Generator::new();
    let cache = Arc::new(TextureCache::new(TextureOptions::default()));
    generator.set_textures(Some(cache));
    let plant = generator.generate(&species, &GenParams::new(2, 1)).unwrap();
    let bark = &plant.mesh.parts[0].material;
    let albedo = decode_image(bark.base_color_texture.as_ref().unwrap()).unwrap();
    assert_eq!(
        &albedo.data[..4],
        &[120, 80, 40, 255],
        "the pack image is the bark"
    );

    let opened = Pack::open(&file).unwrap();
    let from_archive = opened.species("textured").unwrap().species().unwrap();
    let asset_dir = from_archive.asset_dir.clone().unwrap();
    assert!(
        asset_dir.join("textures/bark.png").is_file(),
        "images are extracted"
    );
    assert!(
        generator
            .generate(&from_archive, &GenParams::new(2, 1))
            .is_ok()
    );

    assert!(matches!(
        store.species("demo/nothing"),
        Err(PackError::NotFound(_))
    ));
    assert!(matches!(
        store.species("other/x"),
        Err(PackError::NotFound(_))
    ));
    store.remove("demo").unwrap();
    assert!(store.list().unwrap().is_empty());
    assert!(store.remove("demo").is_err());
}

/// Builds an archive of `files` and returns the errors of validating it.
fn archive_errors(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> String {
    let path = root.join("case.arbpack");
    std::fs::write(&path, write_archive(files).unwrap()).unwrap();
    let report = validate_path(&path);
    let errors: Vec<String> = report.errors.iter().map(ToString::to_string).collect();
    errors.join("\n")
}

/// The built files of the demo pack, to break one way or another.
fn built_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let dir = demo_pack(root);
    arbrume_pack::build_files(&Pack::open(&dir).unwrap()).unwrap()
}

/// `files` with their hash manifest written again.
fn rehashed(mut files: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    files.remove(HASHES_FILE);
    let hashes = arbrume_pack::hash_manifest(&files);
    files.insert(HASHES_FILE.to_owned(), hashes.into_bytes());
    files
}

#[test]
fn broken_packs_are_rejected_with_clear_errors() {
    let root = tempfile::tempdir().unwrap();
    let files = built_files(root.path());
    let pack_toml = String::from_utf8(files["pack.toml"].clone()).unwrap();
    let with = |path: &str, bytes: &[u8]| {
        let mut changed = files.clone();
        changed.insert(path.to_owned(), bytes.to_vec());
        rehashed(changed)
    };
    let cases: Vec<(BTreeMap<String, Vec<u8>>, &str)> = vec![
        (
            with("species/young.toml", b"[growth\n"),
            "species/young.toml",
        ),
        (
            with("species/young.toml", b"[growth]\nbranching_angel = 3\n"),
            "unknown field",
        ),
        (
            with("species/young.toml", b"schema_version = 4\n"),
            "update Arbrume",
        ),
        (
            with(
                "pack.toml",
                pack_toml.replace("format = 1", "format = 7").as_bytes(),
            ),
            "update Arbrume",
        ),
        (
            with(
                "pack.toml",
                format!("{pack_toml}\n[[species]]\nname = \"young\"\ndisplay_name = \"Again\"\n")
                    .as_bytes(),
            ),
            "listed twice",
        ),
        (with("run.exe", b"MZ\x90\x00"), "may not hold"),
        (with("textures/evil.png", b"MZ\x90\x00"), "executable"),
        (with("textures/bark.png", &png(24, [0; 4])), "24 x 24"),
        (
            {
                let mut f = files.clone();
                f.remove("LICENCE-CONTENT.txt");
                rehashed(f)
            },
            "licence file",
        ),
        (
            {
                let mut f = files.clone();
                f.remove("species/young.toml");
                rehashed(f)
            },
            "missing",
        ),
        (
            {
                let mut f = files.clone();
                f.remove("textures/bark.png");
                rehashed(f)
            },
            "not in the pack",
        ),
        (
            {
                let mut f = files.clone();
                f.remove(HASHES_FILE);
                f
            },
            "lists its hashes",
        ),
        (
            {
                let mut f = files.clone();
                f.insert("species/young.toml".to_owned(), b"name = \"x\"".to_vec());
                f
            },
            "does not match",
        ),
    ];
    for (case, expected) in cases {
        let errors = archive_errors(root.path(), &case);
        assert!(
            errors.contains(expected),
            "expected `{expected}` in:\n{errors}"
        );
    }
}

#[test]
fn unsafe_archives_are_refused_before_anything_is_read() {
    let root = tempfile::tempdir().unwrap();
    let mut files = built_files(root.path());
    files.insert("species/zz_evil.toml".to_owned(), b"name = \"e\"".to_vec());
    let mut bytes = write_archive(&files).unwrap();
    let (from, to) = (b"species/zz_evil.toml", b"../../outside/x.toml");
    let mut at = 0;
    while let Some(found) = bytes[at..].windows(from.len()).position(|w| w == from) {
        bytes[at + found..at + found + to.len()].copy_from_slice(to);
        at += found + to.len();
    }
    let path = root.path().join("evil.arbpack");
    std::fs::write(&path, &bytes).unwrap();
    let error = Pack::open(&path).unwrap_err().to_string();
    assert!(error.contains("../../outside/x.toml"), "{error}");
    let store = PackStore::new(root.path().join("store"));
    assert!(store.install(&path).is_err());
    assert!(!root.path().join("outside").exists());
    assert!(store.list().unwrap().is_empty(), "nothing is installed");
}

#[test]
fn an_invalid_pack_does_not_replace_an_installed_one() {
    let root = tempfile::tempdir().unwrap();
    let dir = demo_pack(root.path());
    let store = PackStore::new(root.path().join("store"));
    store.install(&dir).unwrap();
    std::fs::write(dir.join("species/young.toml"), "[growth\n").unwrap();
    let error = store.install(&dir).unwrap_err();
    assert!(matches!(error, PackError::Invalid { .. }), "{error}");
    let installed = store.find("demo").unwrap();
    assert!(validate(&installed).is_ok());
}
