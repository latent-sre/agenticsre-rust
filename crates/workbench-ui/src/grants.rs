use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use workbench_core::{OperatorContext, RunControl, request::Request, result::Status};

pub struct Options {
    pub port: u16,
    pub allow: Vec<String>,
    pub roots: Vec<String>,
    pub read_policy: Option<PathBuf>,
}

pub(crate) struct Root {
    pub path: String,
    label: String,
}

pub(crate) struct Grants {
    pub operations: BTreeSet<String>,
    pub roots: BTreeMap<String, Root>,
    pub context: OperatorContext,
    pub policy_digest: Option<String>,
}

impl Grants {
    pub fn new(options: &Options) -> Result<Self, &'static str> {
        let mut operations = BTreeSet::new();
        for operation in &options.allow {
            if !matches!(
                operation.as_str(),
                "process.exec" | "command.inspect" | "task.run"
            ) || !operations.insert(operation.clone())
            {
                return Err(
                    "--allow requires unique process.exec, command.inspect or task.run IDs",
                );
            }
        }
        let commands = operations.iter().any(|op| op != "task.run");
        if commands && (options.read_policy.is_none() || options.roots.is_empty()) {
            return Err(
                "command grants require an explicit --read-policy and at least one --root ID",
            );
        }
        if !commands && (!options.roots.is_empty() || options.read_policy.is_some()) {
            return Err("--root and --read-policy require a command grant");
        }
        let mut context = OperatorContext::default();
        let mut roots = BTreeMap::new();
        let mut policy_digest = None;
        if let Some(path) = &options.read_policy {
            let policy = workbench_core::read_policy_description(path)
                .map_err(|_| "the explicit read policy is unavailable or invalid")?;
            for id in &options.roots {
                let root = policy
                    .roots
                    .iter()
                    .find(|root| &root.id == id)
                    .ok_or("--root must name a root in the trusted policy")?;
                let label = std::path::Path::new(&root.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(id)
                    .chars()
                    .take(128)
                    .collect();
                if roots
                    .insert(
                        id.clone(),
                        Root {
                            path: root.path.clone(),
                            label,
                        },
                    )
                    .is_some()
                {
                    return Err("--root IDs must be unique");
                }
            }
            context.read_policy_path = Some(path.clone());
            context.read_profile_launcher =
                Some(workbench_core::ReadProfileLauncher::current().map_err(
                    |_| "the current executable cannot provide the restricted command launcher",
                )?);
            policy_digest = Some(policy.digest);
            // Core inspection pins actual installed dependencies without launching a command
            // or claiming that the kernel namespace/landlock controls have been probed.
            for root in roots.values() {
                for (program, args) in [("git", vec!["status", "--short"]), ("rg", vec!["--files"])]
                {
                    let request = Request::new(
                        "command.inspect",
                        json!({"program":program,"args":args,"cwd":root.path}),
                    );
                    let receipt = workbench_core::execute_with_policy_identity(
                        request,
                        &RunControl::default(),
                        &context,
                        policy_digest.as_deref(),
                    );
                    if receipt.execution.status != Status::Succeeded {
                        return Err(
                            "a granted root, executable or isolation dependency failed core inspection; use CLI command inspect for details",
                        );
                    }
                }
            }
        }
        if operations.contains("task.run")
            && workbench_core::tasks::availability()["status"] != "available"
        {
            operations.remove("task.run");
        }
        Ok(Self {
            operations,
            roots,
            context,
            policy_digest,
        })
    }

    pub fn session(&self, id: &str) -> Value {
        json!({"api_version":"0.1","session_id":id,
            "roots":self.roots.iter().map(|(id, root)| json!({"id":id,"label":root.label})).collect::<Vec<_>>(),
            "operations":self.operations.iter().map(|operation| if operation == "task.run" {
                json!({"id":operation,"version":1,"tasks":["error-budget"]})
            } else { json!({"id":operation,"version":1,"profile":"linux-read-v1"}) }).collect::<Vec<_>>(),
            "limits":{"active_runs":1,"history_entries":50,"history_bytes":16777216,"max_request_bytes":65536,"max_submissions":1024}})
    }
}
