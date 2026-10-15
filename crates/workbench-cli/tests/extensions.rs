#![cfg(target_os = "linux")]
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::Command,
};

const SAVE: &str = env!("CARGO_BIN_EXE_save");
fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}
struct Fixture {
    root: PathBuf,
    package: PathBuf,
    store: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(workbench_core::result::new_id("extension-test"));
        fs::create_dir(&root).unwrap();
        let f = Self {
            package: root.join("package.json"),
            store: root.join("store"),
            root,
        };
        f.write(&Self::package());
        f
    }
    fn package() -> Value {
        // Header-valid inert bytes: inspection must not imply executable/protocol conformance.
        let mut bytes = vec![0u8; 64];
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        bytes[16] = 2;
        bytes[18] = 62;
        bytes[20] = 1;
        bytes[52] = 64;
        let mut manifest: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/extensions/manifest.json"
        ))
        .unwrap();
        manifest["executable"]["sha256"] = json!(digest(&bytes));
        manifest["executable"]["bytes"] = json!(bytes.len());
        json!({"package_format":1,"manifest":manifest,"executable_base64":STANDARD.encode(bytes)})
    }
    fn write(&self, package: &Value) {
        fs::write(&self.package, serde_json::to_vec(package).unwrap()).unwrap();
    }
    fn pin(&self) -> String {
        digest(&fs::read(&self.package).unwrap())
    }
    fn run(&self, args: &[&str], ok: bool) -> Value {
        let output = Command::new(SAVE)
            .env_clear()
            .args(["--json", "--extension-state-dir"])
            .arg(&self.store)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.len() <= 2 * 1024 * 1024);
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.success(), ok, "{result}");
        assert_eq!(result["assessment"], "not_assessed");
        assert!(result["execution"]["child_exit_code"].is_null());
        result
    }
    fn install(&self, ok: bool) -> Value {
        self.run(
            &[
                "extension",
                "install",
                self.package.to_str().unwrap(),
                "--expected-package-sha256",
                &self.pin(),
            ],
            ok,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn inspection_is_inert_and_installation_never_enables_capability() {
    let f = Fixture::new();
    let inspected = f.run(&["extension", "inspect", f.package.to_str().unwrap()], true);
    assert!(!f.store.exists());
    assert_eq!(inspected["data"]["packages"][0]["package_sha256"], f.pin());
    assert_eq!(
        inspected["data"]["packages"][0]["execution_available"],
        false
    );
    let installed = f.install(true);
    assert_eq!(installed["data"]["action"], "installed");
    assert_eq!(installed["data"]["enabled"], false);
    assert_eq!(
        fs::metadata(&f.store).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for file in fs::read_dir(&f.store).unwrap() {
        assert_eq!(
            file.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let repeat = f.install(true);
    assert_eq!(repeat["data"]["action"], "already_installed");
    let listed = f.run(&["extension", "list"], true);
    assert_eq!(listed["data"]["installed_count"], 1);
    assert_eq!(listed["data"]["packages"], inspected["data"]["packages"]);
    let request = f.root.join("dispatch.json");
    let r = workbench_core::request::Request::new("ext.fixture.capacity.project", json!({}));
    fs::write(&request, serde_json::to_vec(&r).unwrap()).unwrap();
    f.run(&["call", "--request", request.to_str().unwrap()], false);
}

#[test]
fn package_pin_and_unknown_metadata_refuse_before_store_creation() {
    let f = Fixture::new();
    f.run(
        &[
            "extension",
            "install",
            f.package.to_str().unwrap(),
            "--expected-package-sha256",
            &format!("sha256:{}", "0".repeat(64)),
        ],
        false,
    );
    assert!(!f.store.exists());
    for (key, value) in [
        ("post_install", json!(["/bin/sh"])),
        ("enabled", json!(true)),
        ("grant", json!("all")),
    ] {
        let mut package = Fixture::package();
        package["manifest"][key] = value;
        f.write(&package);
        f.install(false);
        assert!(!f.store.exists());
    }
    let mut package = Fixture::package();
    package["manifest"]["protocol"]["version"] = json!(2);
    f.write(&package);
    f.install(false);
    assert!(!f.store.exists());
}

#[test]
fn installation_refuses_replacement_and_staging_without_destroying_data() {
    let f = Fixture::new();
    f.install(true);
    let before: Vec<_> = fs::read_dir(&f.store)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (p.clone(), fs::read(p).unwrap())
        })
        .collect();
    let mut changed = Fixture::package();
    changed["manifest"]["provenance"]["revision"] = json!("replacement");
    f.write(&changed);
    f.install(false);
    for (p, b) in &before {
        assert_eq!(fs::read(p).unwrap(), *b);
    }
    let stage = f.store.join("stage-interrupted.json");
    fs::write(&stage, b"unfinished").unwrap();
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o600)).unwrap();
    f.run(&["extension", "list"], false);
    assert_eq!(fs::read(stage).unwrap(), b"unfinished");
}

#[test]
fn linked_packages_and_payload_mismatch_are_refused() {
    let f = Fixture::new();
    let original = f.root.join("original.json");
    fs::rename(&f.package, &original).unwrap();
    symlink(&original, &f.package).unwrap();
    f.run(
        &["extension", "inspect", f.package.to_str().unwrap()],
        false,
    );
    fs::remove_file(&f.package).unwrap();
    fs::hard_link(&original, &f.package).unwrap();
    f.run(
        &["extension", "inspect", f.package.to_str().unwrap()],
        false,
    );
    fs::remove_file(&f.package).unwrap();
    let mut package = Fixture::package();
    package["executable_base64"] = json!(STANDARD.encode([0u8; 64]));
    f.write(&package);
    f.install(false);
    assert!(!f.store.exists());
}

#[test]
fn output_omission_and_structured_call_are_explicit() {
    let f = Fixture::new();
    f.install(true);
    let limited = f.run(&["extension", "list", "--max-output-bytes", "1024"], false);
    assert_eq!(limited["execution"]["status"], "partial");
    assert_eq!(limited["data"]["omitted_packages"], 1);
    assert_eq!(limited["data"]["installed_count"], 1);
    let request = f.root.join("request.json");
    let r = workbench_core::request::Request::new(
        "extension.inspect",
        json!({"package":f.package,"expected_package_sha256":f.pin()}),
    );
    fs::write(&request, serde_json::to_vec(&r).unwrap()).unwrap();
    let structured = f.run(&["call", "--request", request.to_str().unwrap()], true);
    assert_eq!(structured["data"]["packages"][0]["package_sha256"], f.pin());
}
