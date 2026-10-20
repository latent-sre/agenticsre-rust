use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use workbench_core::{
    DiagnosticsGrant, EvidenceCollectionGrant, EvidenceStoreOptions, GrafanaConfigIdentity,
    InvestigationGrant, OperatorContext, RunControl, request::Request, result::Status,
};

#[derive(Default)]
pub struct Options {
    pub port: u16,
    pub allow: Vec<String>,
    pub roots: Vec<String>,
    pub read_policy: Option<PathBuf>,
    pub diagnostics_config: Option<PathBuf>,
    pub targets: Vec<String>,
    pub config: Option<PathBuf>,
    pub query_targets: Vec<String>,
    pub collection_targets: Vec<String>,
    pub context_source: Option<PathBuf>,
    pub context_max_age_days: Option<u16>,
    pub evidence_store: Option<EvidenceStoreOptions>,
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
    pub diagnostic_grant: Option<DiagnosticsGrant>,
    pub investigation_grant: Option<InvestigationGrant>,
    pub query_targets: BTreeSet<String>,
    pub grafana_identity: Option<GrafanaConfigIdentity>,
    pub collection_targets: BTreeSet<String>,
    pub collection_grant: Option<EvidenceCollectionGrant>,
}

impl Grants {
    pub fn new(options: &Options) -> Result<Self, &'static str> {
        let mut operations = BTreeSet::new();
        for operation in &options.allow {
            if !matches!(
                operation.as_str(),
                "process.exec"
                    | "command.inspect"
                    | "task.run"
                    | "diagnostics.run"
                    | "context.resolve"
                    | "evidence.list"
                    | "evidence.open"
                    | "evidence.replay"
                    | "evidence.compare"
                    | "report.create"
                    | "queries.list"
                    | "queries.describe"
                    | "query.run"
                    | "evidence.packs.list"
                    | "evidence.packs.describe"
                    | "evidence.collect"
            ) || !operations.insert(operation.clone())
            {
                return Err("--allow requires unique supported operation IDs");
            }
        }
        let commands = operations
            .iter()
            .any(|op| matches!(op.as_str(), "process.exec" | "command.inspect"));
        if commands && (options.read_policy.is_none() || options.roots.is_empty()) {
            return Err(
                "command grants require an explicit --read-policy and at least one --root ID",
            );
        }
        if !commands && (!options.roots.is_empty() || options.read_policy.is_some()) {
            return Err("--root and --read-policy require a command grant");
        }
        let diagnostics = operations.contains("diagnostics.run");
        if diagnostics && (options.diagnostics_config.is_none() || options.targets.is_empty()) {
            return Err(
                "diagnostics.run requires --diagnostics-config and at least one explicit --target ID",
            );
        }
        if !diagnostics && (options.diagnostics_config.is_some() || !options.targets.is_empty()) {
            return Err("--diagnostics-config and --target require a diagnostics.run grant");
        }
        let query = operations.contains("query.run");
        let collection = operations.contains("evidence.collect");
        if query && (options.config.is_none() || options.query_targets.is_empty()) {
            return Err("query.run requires --config and at least one explicit --query-target ID");
        }
        if (!query && !options.query_targets.is_empty())
            || (!query && !collection && options.config.is_some())
        {
            return Err(
                "--query-target requires query.run; --config requires query.run or evidence.collect",
            );
        }
        if collection && (options.config.is_none() || options.collection_targets.is_empty()) {
            return Err(
                "evidence.collect requires --config and at least one explicit --collection-target ID",
            );
        }
        if !collection && !options.collection_targets.is_empty() {
            return Err("--collection-target requires an evidence.collect grant");
        }
        let collection_targets: BTreeSet<_> = options.collection_targets.iter().cloned().collect();
        if collection_targets.len() != options.collection_targets.len() {
            return Err("--collection-target IDs must be unique");
        }
        let query_targets: BTreeSet<_> = options.query_targets.iter().cloned().collect();
        if query_targets.len() != options.query_targets.len() {
            return Err("--query-target IDs must be unique");
        }
        let context_resolution = operations.contains("context.resolve");
        if (context_resolution || collection)
            && (options.context_source.is_none() || options.context_max_age_days.is_none())
        {
            return Err(
                "context.resolve and evidence.collect require --context-source and --context-max-age-days",
            );
        }
        if !context_resolution
            && !collection
            && (options.context_source.is_some() || options.context_max_age_days.is_some())
        {
            return Err(
                "--context-source and --context-max-age-days require context.resolve or evidence.collect",
            );
        }
        let evidence = operations.iter().any(|op| {
            matches!(
                op.as_str(),
                "evidence.list"
                    | "evidence.open"
                    | "evidence.replay"
                    | "evidence.compare"
                    | "report.create"
            )
        });
        if (evidence || collection) && options.evidence_store.is_none() {
            return Err("evidence and report grants require an existing explicit --state-dir");
        }
        if !evidence && !collection && options.evidence_store.is_some() {
            return Err("--state-dir requires an evidence or report grant");
        }
        if let Some(store) = &options.evidence_store {
            let defaults = EvidenceStoreOptions::new(store.directory.clone());
            if store.max_bytes != defaults.max_bytes
                || store.retention_days != defaults.retention_days
            {
                return Err("browser grants do not accept store policy overrides");
            }
        }
        let mut context = OperatorContext {
            context_source_path: options.context_source.clone(),
            context_max_age_days: options.context_max_age_days,
            evidence_store: options.evidence_store.clone(),
            ..OperatorContext::default()
        };
        let investigation_operations = options
            .allow
            .iter()
            .filter(|op| {
                matches!(
                    op.as_str(),
                    "context.resolve"
                        | "evidence.list"
                        | "evidence.open"
                        | "evidence.replay"
                        | "evidence.compare"
                        | "report.create"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let investigation_grant = if context_resolution || evidence {
            Some(workbench_core::investigation_grant(&context, &investigation_operations)
                .map_err(|_| "the explicit context source or evidence store could not be admitted; use existing valid files and start a new session")?)
        } else {
            None
        };
        let mut roots = BTreeMap::new();
        let mut policy_digest = None;
        let grafana_identity = if let Some(path) = &options.config {
            // Inspect bounded configuration and CA bytes without credentials or network work.
            let (description, identity) = workbench_core::grafana_config_description(path)
                .map_err(|_| "the explicit Grafana configuration is unavailable or invalid")?;
            for id in &query_targets {
                let target = description
                    .targets
                    .iter()
                    .find(|target| &target.id == id)
                    .ok_or("--query-target must name a target in the trusted configuration")?;
                if target.api_profile.is_none() {
                    return Err("a granted query target has an unsupported API profile");
                }
            }
            context.config_path = Some(path.clone());
            Some(identity)
        } else {
            None
        };
        let diagnostic_grant = if let Some(path) = &options.diagnostics_config {
            let grant = workbench_core::diagnostics_grant(path, &options.targets)
                .map_err(|_| "the diagnostic configuration, selected targets or CA could not be admitted; start a new session with valid regular files and unique known aliases")?;
            context.diagnostics_config_path = Some(path.clone());
            Some(grant)
        } else {
            None
        };
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
        let collection_grant = if collection {
            Some(workbench_core::evidence_collection_grant(&context, &options.collection_targets)
                .map_err(|_| "the collection configuration, context source or existing store could not be admitted")?)
        } else {
            None
        };
        Ok(Self {
            operations,
            roots,
            context,
            policy_digest,
            diagnostic_grant,
            investigation_grant,
            query_targets,
            grafana_identity,
            collection_targets,
            collection_grant,
        })
    }

    pub fn session(&self, id: &str) -> Value {
        json!({"api_version":"0.1","session_id":id,
            "roots":self.roots.iter().map(|(id, root)| json!({"id":id,"label":root.label})).collect::<Vec<_>>(),
            "targets":self.diagnostic_grant.iter().flat_map(|grant| grant.target_ids()).map(|id|json!({"id":id,"label":id})).collect::<Vec<_>>(),
            "query_targets":self.query_targets.iter().map(|id|json!({"id":id,"label":id})).collect::<Vec<_>>(),
            "collection_targets":self.collection_targets.iter().map(|id|json!({"id":id,"label":id})).collect::<Vec<_>>(),
            "operations":self.operations.iter().map(|operation| if operation == "task.run" {
                json!({"id":operation,"version":1,"tasks":["error-budget"]})
            } else if operation == "diagnostics.run" {
                json!({"id":operation,"version":1,"packs":[workbench_core::diagnostics_pack_metadata()]})
            } else if operation == "context.resolve" {
                json!({"id":operation,"version":1,"profile":"offline-context-v1"})
            } else if matches!(operation.as_str(), "queries.list" | "queries.describe") {
                json!({"id":operation,"version":1,"profile":"installed-query-catalog-v1"})
            } else if operation == "query.run" {
                json!({"id":operation,"version":1,"profile":"saved-query-v1"})
            } else if matches!(operation.as_str(), "evidence.packs.list" | "evidence.packs.describe") {
                json!({"id":operation,"version":1,"profile":"installed-evidence-catalog-v1"})
            } else if operation == "evidence.collect" {
                json!({"id":operation,"version":1,"profile":"evidence-collection-v1"})
            } else if self.investigation_grant.as_ref().is_some_and(|grant| grant.allows(operation)) {
                json!({"id":operation,"version":1,"profile":"verified-evidence-v1"})
            } else { json!({"id":operation,"version":1,"profile":"linux-read-v1"}) }).collect::<Vec<_>>(),
            "limits":{"active_runs":1,"history_entries":50,"history_bytes":16777216,"max_request_bytes":65536,"max_submissions":1024}})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_query_catalog_is_independent_and_execution_requires_its_own_configuration() {
        for operation in ["queries.list", "queries.describe"] {
            let options = Options {
                allow: vec![operation.into()],
                ..Options::default()
            };
            let grants = Grants::new(&options).unwrap();
            let session = grants.session("test");
            assert_eq!(session["operations"].as_array().unwrap().len(), 1);
            assert_eq!(session["operations"][0]["id"], operation);
            assert_eq!(session["query_targets"], json!([]));
            assert!(grants.grafana_identity.is_none());
        }
        for options in [
            Options {
                allow: vec!["query.run".into()],
                ..Options::default()
            },
            Options {
                allow: vec!["queries.list".into()],
                config: Some("/missing.json".into()),
                ..Options::default()
            },
            Options {
                allow: vec!["queries.describe".into()],
                query_targets: vec!["fixture".into()],
                ..Options::default()
            },
            Options {
                allow: vec!["query.run".into()],
                config: Some("/missing.json".into()),
                targets: vec!["fixture".into()],
                ..Options::default()
            },
            Options {
                allow: vec!["queries.list".into(), "queries.list".into()],
                ..Options::default()
            },
        ] {
            assert!(Grants::new(&options).is_err());
        }
    }
}
