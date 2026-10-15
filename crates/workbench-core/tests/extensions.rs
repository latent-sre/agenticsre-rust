#![cfg(target_os = "linux")]
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use workbench_core::{
    OperatorContext, RunControl, execute_with_authority_identities, execute_with_context,
    request::Request,
    result::{OperationResult, Status},
};
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    package: PathBuf,
    context: OperatorContext,
    bytes: Vec<u8>,
}
fn digest(bytes: &[u8]) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
fn package(id: &str) -> Value {
    let mut executable = [0_u8; 64];
    executable[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    executable[16] = 2;
    executable[18] = 62;
    executable[20] = 1;
    executable[52] = 64;
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"properties":{"count":{"type":"integer","minimum":0,"maximum":9007199254740991_i64}},"required":["count"]});
    json!({"package_format":1,"manifest":{"id":id,"version":1,"protocol":{"name":"save-extension","version":1},"compatibility":{"min_spec":"0.1","max_spec":"0.1"},"executable":{"platform":"linux-x86_64","format":"elf","sha256":digest(&executable),"bytes":64},"capabilities":[{"operation":format!("ext.{id}.calculate"),"version":1,"effects":"observe","requested_access":[],"input_schema":schema,"output_schema":schema}],"provenance":{"source":"local:synthetic","revision":"fixture1","license":"MIT"}},"executable_base64":STANDARD.encode(executable)})
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "extension-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let file = root.join("source.json");
        let bytes = serde_json::to_vec(&package("fixture.capacity")).unwrap();
        fs::write(&file, &bytes).unwrap();
        Self {
            context: OperatorContext {
                extension_state_dir: Some(root.join("store")),
                ..OperatorContext::default()
            },
            root,
            package: file,
            bytes,
        }
    }
    fn request(&self, op: &str) -> Request {
        Request::new(
            op,
            if op == "extension.list" {
                json!({})
            } else {
                json!({"package":self.package,"expected_package_sha256":digest(&fs::read(&self.package).unwrap())})
            },
        )
    }
    fn run(&self, op: &str) -> OperationResult {
        execute_with_context(self.request(op), &RunControl::default(), &self.context)
    }
    fn store(&self) -> &Path {
        self.context.extension_state_dir.as_deref().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn has(result: &OperationResult, code: &str) -> bool {
    result.errors.iter().any(|e| e.code == code)
}
#[test]
fn inspect_install_and_list_remain_inert_digest_bound_and_private() {
    let fixture = Fixture::new();
    let inspect = fixture.run("extension.inspect");
    assert_eq!(
        inspect.execution.status,
        Status::Succeeded,
        "{:?}",
        inspect.errors
    );
    assert!(!fixture.store().exists());
    assert_eq!(inspect.data["installed_count"], 0);
    assert_eq!(
        inspect.data["packages"][0]["package_sha256"],
        digest(&fixture.bytes)
    );
    assert!(
        inspect.data["packages"][0]
            .get("executable_base64")
            .is_none()
    );
    let installed = fixture.run("extension.install");
    assert_eq!(
        installed.execution.status,
        Status::Succeeded,
        "{:?}",
        installed.errors
    );
    assert_eq!(installed.data["action"], "installed");
    assert_eq!(installed.effect_outcome, "succeeded");
    assert_eq!(
        fs::metadata(fixture.store()).unwrap().mode() & 0o7777,
        0o700
    );
    let mut names = Vec::new();
    for entry in fs::read_dir(fixture.store()).unwrap() {
        let entry = entry.unwrap();
        assert_eq!(entry.metadata().unwrap().mode() & 0o7777, 0o600);
        names.push(entry.file_name());
    }
    assert_eq!(names.len(), 3);
    let installed_file = fixture
        .store()
        .join(format!("package-{}.json", &digest(&fixture.bytes)[7..]));
    assert_eq!(fs::read(installed_file).unwrap(), fixture.bytes);
    let repeated = fixture.run("extension.install");
    assert_eq!(repeated.data["action"], "already_installed");
    assert_eq!(repeated.effect_outcome, "not_applicable");
    let list = fixture.run("extension.list");
    assert_eq!(list.data["installed_count"], 1);
    assert_eq!(list.data["packages"], installed.data["packages"]);
    assert_eq!(list.data["enabled"], false);
    assert_eq!(list.data["execution_available"], false);
    assert_eq!(list.data["provenance_verified"], false);
    assert_eq!(list.execution.child_exit_code, None);
    assert!(
        list.text()
            .contains("Disabled; execution unavailable; provenance unverified")
    );
}
#[test]
fn pins_inputs_hosted_and_cancelled_requests_refuse_before_store_creation() {
    let fixture = Fixture::new();
    let mut mismatch = fixture.request("extension.install");
    mismatch.inputs["expected_package_sha256"] = json!(format!("sha256:{}", "0".repeat(64)));
    assert!(has(
        &execute_with_context(mismatch, &RunControl::default(), &fixture.context),
        "extension_package_digest_mismatch"
    ));
    for input in [
        json!({"package":fixture.package}),
        json!({"package":fixture.package,"expected_package_sha256":null}),
        json!({"package":fixture.package,"grant":true}),
        json!([fixture.package]),
        json!({"package":"/tmp/x\ny","expected_package_sha256":digest(&fixture.bytes)}),
    ] {
        let expected = if input.is_object() {
            "invalid_extension_input"
        } else {
            "invalid_request"
        };
        let result = execute_with_context(
            Request::new("extension.install", input),
            &RunControl::default(),
            &fixture.context,
        );
        assert!(
            has(&result, expected),
            "expected {expected}, got {:?}",
            result.errors
        );
    }
    for operation in ["extension.inspect", "extension.install", "extension.list"] {
        let result = execute_with_authority_identities(
            fixture.request(operation),
            &RunControl::default(),
            &fixture.context,
            None,
            None,
        );
        assert!(has(&result, "extension_grants_unavailable"));
    }
    let control = RunControl::default();
    control.signal.store(2, Ordering::Relaxed);
    let cancelled = execute_with_context(
        fixture.request("extension.install"),
        &control,
        &fixture.context,
    );
    assert_eq!(cancelled.execution.status, Status::Cancelled);
    assert_eq!(cancelled.data["cancellation_signal"], 2);
    assert_eq!(cancelled.effect_outcome, "not_attempted");
    assert!(!fixture.store().exists());
}
#[test]
fn malicious_manifest_fields_schema_extensions_and_executable_mismatch_are_rejected() {
    let fixture = Fixture::new();
    let original = package("fixture.capacity");
    let mutations: &[(&str, Value)] = &[
        (
            "/manifest/capabilities/0/operation",
            json!("ext.other.calculate"),
        ),
        ("/manifest/capabilities/0/effects", json!("local_write")),
        (
            "/manifest/capabilities/0/requested_access",
            json!(["network"]),
        ),
        ("/manifest/protocol/version", json!(2)),
        ("/manifest/compatibility/max_spec", json!("0.2")),
        (
            "/manifest/executable/sha256",
            json!(format!("sha256:{}", "0".repeat(64))),
        ),
        ("/manifest/capabilities/0/input_schema/required", json!([])),
        (
            "/manifest/capabilities/0/input_schema/properties/count/maximum",
            json!(9007199254740992_u64),
        ),
        (
            "/manifest/capabilities/0/input_schema/additionalProperties",
            json!(true),
        ),
        ("/manifest/provenance/source", json!("\u{1b}[31m")),
    ];
    for (pointer, value) in mutations {
        let mut bad = original.clone();
        *bad.pointer_mut(pointer).unwrap() = value.clone();
        fs::write(&fixture.package, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(
            has(
                &fixture.run("extension.install"),
                "invalid_extension_package"
            ),
            "{pointer}"
        );
        assert!(!fixture.store().exists());
    }
    for pointer in [
        "/manifest",
        "/manifest/capabilities/0",
        "/manifest/capabilities/0/input_schema",
        "/manifest/capabilities/0/input_schema/properties/count",
    ] {
        let mut bad = original.clone();
        bad.pointer_mut(pointer).unwrap()["$ref"] = json!("file:///private");
        fs::write(&fixture.package, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(has(
            &fixture.run("extension.install"),
            "invalid_extension_package"
        ));
    }
}
#[test]
fn duplicate_json_keys_and_noncanonical_base64_are_rejected() {
    let fixture = Fixture::new();
    let source = String::from_utf8(fixture.bytes.clone()).unwrap().replacen(
        "\"package_format\":1",
        "\"package_format\":1,\"package_format\":1",
        1,
    );
    fs::write(&fixture.package, source).unwrap();
    assert!(has(
        &fixture.run("extension.inspect"),
        "invalid_extension_package"
    ));
    let mut source = package("fixture.capacity");
    source["executable_base64"] = json!(format!(
        "{}\n",
        source["executable_base64"].as_str().unwrap()
    ));
    fs::write(&fixture.package, serde_json::to_vec(&source).unwrap()).unwrap();
    assert!(has(
        &fixture.run("extension.inspect"),
        "invalid_extension_package"
    ));
}
#[test]
fn source_links_directories_fifo_and_oversize_files_are_refused() {
    let fixture = Fixture::new();
    let link = fixture.root.join("link.json");
    symlink(&fixture.package, &link).unwrap();
    let via_directory = fixture.root.join("alias");
    symlink(&fixture.root, &via_directory).unwrap();
    let hard = fixture.root.join("hard.json");
    fs::hard_link(&fixture.package, &hard).unwrap();
    for path in [
        &link,
        &hard,
        &fixture.root,
        &via_directory.join("source.json"),
    ] {
        let result = execute_with_context(
            Request::new("extension.inspect", json!({"package":path})),
            &RunControl::default(),
            &fixture.context,
        );
        assert!(has(&result, "extension_source_unsafe"));
    }
    fs::remove_file(hard).unwrap();
    let fifo = fixture.root.join("fifo");
    let name = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: name is live and NUL terminated; creates a synthetic FIFO inside this fixture.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let result = execute_with_context(
        Request::new("extension.inspect", json!({"package":fifo})),
        &RunControl::default(),
        &fixture.context,
    );
    assert!(has(&result, "extension_source_unsafe"));
    fs::OpenOptions::new()
        .write(true)
        .open(&fixture.package)
        .unwrap()
        .set_len(12 * 1024 * 1024 + 1)
        .unwrap();
    assert!(has(
        &fixture.run("extension.inspect"),
        "extension_package_too_large"
    ));
}
#[test]
fn conflict_corruption_staging_unknown_files_and_partial_layout_never_get_repaired() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.run("extension.install").execution.status,
        Status::Succeeded
    );
    let mut conflict = package("fixture.capacity");
    conflict["manifest"]["provenance"]["revision"] = json!("different");
    fs::write(&fixture.package, serde_json::to_vec(&conflict).unwrap()).unwrap();
    assert!(has(
        &fixture.run("extension.install"),
        "extension_package_conflict"
    ));
    for name in ["pending-interrupted", "unexpected"] {
        let path = fixture.store().join(name);
        fs::write(&path, b"preserve").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(has(
            &fixture.run("extension.list"),
            "extension_recovery_required"
        ));
        assert!(has(
            &fixture.run("extension.install"),
            "extension_recovery_required"
        ));
        assert_eq!(fs::read(&path).unwrap(), b"preserve");
        fs::remove_file(path).unwrap();
    }
    let installed = fixture
        .store()
        .join(format!("package-{}.json", &digest(&fixture.bytes)[7..]));
    fs::write(&installed, b"corrupt").unwrap();
    assert!(has(
        &fixture.run("extension.list"),
        "extension_recovery_required"
    ));
    assert_eq!(fs::read(installed).unwrap(), b"corrupt");
    let partial = Fixture::new();
    fs::create_dir(partial.store()).unwrap();
    fs::set_permissions(partial.store(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(partial.store().join("lock"), b"").unwrap();
    fs::set_permissions(
        partial.store().join("lock"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(has(
        &partial.run("extension.install"),
        "extension_recovery_required"
    ));
    assert_eq!(fs::read_dir(partial.store()).unwrap().count(), 1);
}
#[test]
fn omitted_descriptions_preserve_installation_and_inventory_receipts() {
    let fixture = Fixture::new();
    let mut request = fixture.request("extension.install");
    request.limits.max_output_bytes = 1024;
    let installed = execute_with_context(request, &RunControl::default(), &fixture.context);
    assert_eq!(installed.execution.status, Status::Partial);
    assert_eq!(installed.effect_outcome, "succeeded");
    assert_eq!(installed.data["action"], "installed");
    assert_eq!(installed.data["installed_count"], 1);
    assert_eq!(installed.data["total_packages"], 1);
    assert_eq!(installed.data["omitted_packages"], 1);
    assert_eq!(installed.data["packages"], json!([]));
    assert!(serde_json::to_vec(&installed.data).unwrap().len() <= 1024);
    assert_eq!(fixture.run("extension.list").data["installed_count"], 1);
}
#[test]
fn package_count_quota_and_effective_output_budget_preserve_inventory() {
    let fixture = Fixture::new();
    let properties: serde_json::Map<String, Value> = (0..16)
        .map(|index| {
            (
                format!("f{index:02}{}", "x".repeat(61)),
                json!({"type":"string","minLength":0,"maxLength":4096}),
            )
        })
        .collect();
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"required":properties.keys().collect::<Vec<_>>(),"properties":properties});
    let mut manifest_bytes = 0;
    for index in 0..32 {
        let mut value = package(&format!("fixture{index}"));
        value["manifest"]["capabilities"] = Value::Array((0..8).map(|capability| {
            json!({"operation":format!("ext.fixture{index}.calculate{capability}"),"version":1,"effects":"observe","requested_access":[],"input_schema":schema,"output_schema":schema})
        }).collect());
        manifest_bytes += serde_json::to_vec(&value["manifest"]).unwrap().len();
        fs::write(&fixture.package, serde_json::to_vec(&value).unwrap()).unwrap();
        let installed = fixture.run("extension.install");
        assert!(
            matches!(
                installed.execution.status,
                Status::Succeeded | Status::Partial
            ),
            "{:?}",
            installed.errors
        );
        assert_eq!(installed.data["action"], "installed");
    }
    assert!(
        manifest_bytes > 1024 * 1024,
        "fixture must exceed effective budget"
    );
    let mut request = fixture.request("extension.list");
    request.limits.max_output_bytes = 2 * 1024 * 1024;
    let listed = execute_with_context(request, &RunControl::default(), &fixture.context);
    assert_eq!(listed.effective_limits.max_output_bytes, 1024 * 1024);
    assert_eq!(listed.execution.status, Status::Partial);
    assert_eq!(listed.data["installed_count"], 32);
    assert_eq!(listed.data["total_packages"], 32);
    assert!(listed.data["omitted_packages"].as_u64().unwrap() > 0);
    assert!(
        serde_json::to_vec(&listed.data).unwrap().len() <= listed.effective_limits.max_output_bytes
    );
    assert!(serde_json::to_vec(&listed).unwrap().len() < 2 * 1024 * 1024);
    fs::write(
        &fixture.package,
        serde_json::to_vec(&package("overflow")).unwrap(),
    )
    .unwrap();
    assert!(has(
        &fixture.run("extension.install"),
        "extension_quota_exceeded"
    ));
    assert_eq!(fs::read_dir(fixture.store()).unwrap().count(), 34);
}

fn policy_file(fixture: &mut Fixture, packages: Value) {
    let path = fixture.root.join("policy.json");
    fs::write(&path,serde_json::to_vec(&json!({"policy_version":1,"execution_profile":"operator-reviewed-v1","packages":packages})).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fixture.context.extension_policy_path = Some(path);
}
#[test]
fn execution_policy_is_separate_and_dynamic_discovery_reports_effective_authority() {
    let mut fixture = Fixture::new();
    let invoke = || Request::new("ext.fixture.capacity.calculate", json!({"count":1}));
    let denied = execute_with_context(invoke(), &RunControl::default(), &fixture.context);
    assert!(has(&denied, "extension_policy_required"));
    assert!(!fixture.store().exists());
    assert_eq!(
        fixture.run("extension.install").execution.status,
        Status::Succeeded
    );
    let describe = || {
        Request::new(
            "capability.describe",
            json!({"operation":"ext.fixture.capacity.calculate"}),
        )
    };
    assert_eq!(
        execute_with_context(describe(), &RunControl::default(), &fixture.context).data["availability"]
            ["status"],
        "requires_configuration"
    );
    let package_digest = digest(&fixture.bytes);
    policy_file(
        &mut fixture,
        json!([{"package_sha256":package_digest,"capabilities":["ext.fixture.capacity.calculate"]}]),
    );
    let listed = fixture.run("extension.list");
    assert_eq!(listed.data["execution_available"], true);
    assert_eq!(listed.data["packages"][0]["enabled"], true);
    assert!(listed.data["policy_sha256"].is_string());
    let inspected = fixture.run("extension.inspect");
    assert_eq!(inspected.data["execution_available"], false);
    assert_eq!(inspected.data["scope"], "package_inspection");
    assert_eq!(inspected.data["policy_sha256"], Value::Null);
    let described = execute_with_context(describe(), &RunControl::default(), &fixture.context);
    assert_eq!(described.data["availability"]["status"], "available");
    assert_eq!(described.data["capability"]["effects"], "unclassified");
    assert_eq!(
        described.data["capability"]["output_schema"],
        json!({"$ref":"urn:sre-workbench:spec:result:0.1"})
    );
    assert_eq!(
        described.data["capability"]["output_schema"],
        workbench_core::discovery::capability("config.check").unwrap()["output_schema"]
    );
    // The flat peer schema still belongs to the package and validates terminal data.
    assert_eq!(
        listed.data["packages"][0]["manifest"]["capabilities"][0]["output_schema"]["type"],
        "object"
    );
    assert!(
        listed.data["packages"][0]["manifest"]["capabilities"][0]["output_schema"]
            .get("$ref")
            .is_none()
    );
    let mut invalid = invoke();
    invalid.inputs = json!({"count":"1"});
    let result = execute_with_context(invalid, &RunControl::default(), &fixture.context);
    assert!(has(&result, "invalid_extension_arguments"));
    assert_eq!(result.execution.child_exit_code, None);
    let hosted = execute_with_authority_identities(
        invoke(),
        &RunControl::default(),
        &fixture.context,
        None,
        None,
    );
    assert!(has(&hosted, "extension_grants_unavailable"));
    let hosted_discovery = execute_with_authority_identities(
        Request::new("capability.list", json!({})),
        &RunControl::default(),
        &fixture.context,
        None,
        None,
    );
    assert!(
        hosted_discovery.data["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| !c["capability"]["id"].as_str().unwrap().starts_with("ext."))
    );
}
#[test]
fn ambiguous_policy_grants_never_enable_or_select_an_external_operation() {
    let mut fixture = Fixture::new();
    assert_eq!(
        fixture.run("extension.install").execution.status,
        Status::Succeeded
    );
    let mut second = package("fixture.capacity");
    second["manifest"]["version"] = json!(2);
    let bytes = serde_json::to_vec(&second).unwrap();
    fs::write(&fixture.package, &bytes).unwrap();
    assert_eq!(
        fixture.run("extension.install").execution.status,
        Status::Succeeded
    );
    let first_digest = digest(&fixture.bytes);
    policy_file(
        &mut fixture,
        json!([{"package_sha256":first_digest,"capabilities":["ext.fixture.capacity.calculate"]},{"package_sha256":digest(&bytes),"capabilities":["ext.fixture.capacity.calculate"]}]),
    );
    let listed = fixture.run("extension.list");
    assert_eq!(listed.data["enabled"], false);
    assert!(
        listed.data["packages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["execution_available"] == false)
    );
    let result = execute_with_context(
        Request::new("ext.fixture.capacity.calculate", json!({"count":1})),
        &RunControl::default(),
        &fixture.context,
    );
    assert!(has(&result, "extension_ambiguous_operation"));
    assert_eq!(result.execution.child_exit_code, None);
    let result = execute_with_context(
        Request::new(
            "capability.describe",
            json!({"operation":"ext.fixture.capacity.calculate"}),
        ),
        &RunControl::default(),
        &fixture.context,
    );
    assert!(has(&result, "extension_ambiguous_operation"));
}
#[test]
fn invalid_or_nonprivate_policy_refuses_install_before_store_effects() {
    let mut fixture = Fixture::new();
    policy_file(&mut fixture, json!([]));
    fs::set_permissions(
        fixture.context.extension_policy_path.as_ref().unwrap(),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(has(
        &fixture.run("extension.install"),
        "extension_policy_invalid"
    ));
    assert!(!fixture.store().exists());
    policy_file(&mut fixture, json!([]));
    let path = fixture.context.extension_policy_path.as_ref().unwrap();
    let mut value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    value["execution_profile"] = json!("untrusted-sandbox");
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(has(
        &fixture.run("extension.install"),
        "extension_policy_invalid"
    ));
    assert!(!fixture.store().exists());
}
