use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("../../web/dist");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    files.sort();
    assert!(
        files.iter().any(|(name, _)| name == "/index.html"),
        "web/dist/index.html is missing; build the checked-in frontend assets first (see docs/ui.md)"
    );
    let total: u64 = files
        .iter()
        .map(|(_, path)| fs::metadata(path).expect("asset metadata").len())
        .sum();
    assert!(
        total <= 2 * 1024 * 1024,
        "embedded frontend assets exceed2MiB"
    );
    let mut source = String::from(
        "pub(crate) fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {\nmatch path {\n",
    );
    for (name, path) in files {
        let content_type = match path.extension().and_then(|e| e.to_str()) {
            Some("html") => "text/html; charset=utf-8",
            Some("js") => "text/javascript; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("svg") => "image/svg+xml",
            Some("woff2") => "font/woff2",
            _ => panic!("unsupported asset extension"),
        };
        let pattern = if name == "/index.html" {
            "\"/\" | \"/index.html\"".to_owned()
        } else {
            format!("{name:?}")
        };
        source.push_str(&format!(
            "{pattern} => Some(({content_type:?}, include_bytes!({:?}))),\n",
            path.canonicalize().expect("asset path")
        ));
    }
    source.push_str("_ => None,\n}\n}\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("output directory")).join("assets.rs"),
        source,
    )
    .expect("generated allowlist");
}

fn collect(root: &Path, directory: &Path, files: &mut Vec<(String, PathBuf)>) {
    for entry in fs::read_dir(directory).expect("prebuilt web/dist must be present") {
        let entry = entry.expect("asset entry");
        let ty = entry.file_type().expect("asset type");
        assert!(!ty.is_symlink(), "asset symlinks are not permitted");
        if ty.is_dir() {
            collect(root, &entry.path(), files);
        } else if ty.is_file() {
            assert!(files.len() < 128, "too many embedded assets");
            let path = entry.path();
            files.push((
                format!(
                    "/{}",
                    path.strip_prefix(root)
                        .expect("asset below root")
                        .to_str()
                        .expect("UTF-8 asset path")
                ),
                path,
            ));
        }
    }
}
