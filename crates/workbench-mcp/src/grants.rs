use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use workbench_core::{
    DiagnosticsGrant, EvidenceCollectionGrant, EvidenceStoreOptions, GrafanaConfigIdentity,
    InvestigationGrant, OperatorContext,
};

pub(crate) const INVESTIGATION_OPERATIONS: [&str; 6] = [
    "context.resolve",
    "evidence.list",
    "evidence.open",
    "evidence.replay",
    "evidence.compare",
    "report.create",
];

#[derive(Default)]
pub struct Options {
    pub allow: Vec<String>,
    pub roots: Vec<String>,
    pub targets: Vec<String>,
    pub diagnostic_targets: Vec<String>,
    pub read_policy: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub diagnostics_config: Option<PathBuf>,
    pub context_source: Option<PathBuf>,
    pub context_max_age_days: Option<u16>,
    pub evidence_store: Option<EvidenceStoreOptions>,
}

pub(crate) struct Grants {
    pub operations: BTreeSet<String>,
    pub roots: BTreeMap<String, String>,
    pub targets: BTreeSet<String>,
    pub context: OperatorContext,
    pub policy_digest: Option<String>,
    pub grafana_identity: Option<GrafanaConfigIdentity>,
    pub diagnostic_grant: Option<DiagnosticsGrant>,
    pub investigation_grant: Option<InvestigationGrant>,
    pub collection_grant: Option<EvidenceCollectionGrant>,
}

impl Grants {
    pub(crate) fn new(options: &Options) -> Result<Self, &'static str> {
        let mut operations = BTreeSet::new();
        for operation in &options.allow {
            if !matches!(
                operation.as_str(),
                "process.exec"
                    | "command.inspect"
                    | "task.run"
                    | "grafana.dashboard.get"
                    | "grafana.query"
                    | "queries.list"
                    | "queries.describe"
                    | "query.run"
                    | "diagnostics.list"
                    | "diagnostics.describe"
                    | "diagnostics.run"
                    | "evidence.packs.list"
                    | "evidence.packs.describe"
                    | "evidence.collect"
            ) && !INVESTIGATION_OPERATIONS.contains(&operation.as_str())
                || !operations.insert(operation.clone())
            {
                return Err("--allow requires unique supported operation IDs");
            }
        }
        let commands =
            operations.contains("process.exec") || operations.contains("command.inspect");
        let collection = operations.contains("evidence.collect");
        let grafana = collection
            || operations.contains("grafana.dashboard.get")
            || operations.contains("grafana.query")
            || operations.contains("query.run");
        if commands && (options.read_policy.is_none() || options.roots.is_empty()) {
            return Err(
                "command grants require an explicit --read-policy and at least one --root ID",
            );
        }
        if !commands && (options.read_policy.is_some() || !options.roots.is_empty()) {
            return Err("--read-policy and --root require a command grant");
        }
        if grafana && (options.config.is_none() || options.targets.is_empty()) {
            return Err("Grafana grants require an explicit --config and at least one --target ID");
        }
        if !grafana && (options.config.is_some() || !options.targets.is_empty()) {
            return Err("--config and --target require a Grafana grant");
        }
        let diagnostics = operations.contains("diagnostics.run");
        if diagnostics
            && (options.diagnostics_config.is_none() || options.diagnostic_targets.is_empty())
        {
            return Err(
                "diagnostics.run requires --diagnostics-config and at least one --diagnostic-target ID",
            );
        }
        if !diagnostics
            && (options.diagnostics_config.is_some() || !options.diagnostic_targets.is_empty())
        {
            return Err(
                "--diagnostics-config and --diagnostic-target require a diagnostics.run grant",
            );
        }
        let context_resolution = operations.contains("context.resolve");
        let context_required = context_resolution || collection;
        if context_required
            && (options.context_source.is_none() || options.context_max_age_days.is_none())
        {
            return Err(
                "context.resolve or evidence.collect requires --context-source and --context-max-age-days",
            );
        }
        if !context_required
            && (options.context_source.is_some() || options.context_max_age_days.is_some())
        {
            return Err(
                "--context-source and --context-max-age-days require context.resolve or evidence.collect",
            );
        }
        let evidence = operations
            .iter()
            .any(|operation| INVESTIGATION_OPERATIONS[1..].contains(&operation.as_str()));
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
                return Err("offline investigation grants do not accept store policy overrides");
            }
        }
        let root_ids: BTreeSet<_> = options.roots.iter().collect();
        let targets: BTreeSet<_> = options.targets.iter().cloned().collect();
        let diagnostic_targets: BTreeSet<_> = options.diagnostic_targets.iter().cloned().collect();
        if root_ids.len() != options.roots.len() || targets.len() != options.targets.len() {
            return Err("--root and --target IDs must be unique");
        }
        if diagnostic_targets.len() != options.diagnostic_targets.len() {
            return Err("--diagnostic-target IDs must be unique");
        }
        let investigation_grant = if context_resolution || evidence {
            // Only the opaque grant retains offline paths. The generic hosted context below
            // remains separate from this sanitized and identity-pinned authority.
            let offline_context = OperatorContext {
                context_source_path: options.context_source.clone(),
                context_max_age_days: options.context_max_age_days,
                evidence_store: options.evidence_store.clone(),
                ..OperatorContext::default()
            };
            let selected = operations
                .iter()
                .filter(|operation| INVESTIGATION_OPERATIONS.contains(&operation.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            Some(
                workbench_core::investigation_grant(&offline_context, &selected).map_err(
                    |_| "the explicit context source or evidence store could not be admitted",
                )?,
            )
        } else {
            None
        };
        let mut context = OperatorContext::default();
        let diagnostic_grant = if let Some(path) = &options.diagnostics_config {
            let grant = workbench_core::diagnostics_grant(path, &options.diagnostic_targets)
                .map_err(|_| "the diagnostic configuration, selected targets or CA could not be admitted")?;
            context.diagnostics_config_path = Some(path.clone());
            Some(grant)
        } else {
            None
        };
        let mut roots = BTreeMap::new();
        let mut policy_digest = None;
        if let Some(path) = &options.read_policy {
            let policy = workbench_core::read_policy_description(path)
                .map_err(|_| "the explicit read policy is unavailable or invalid")?;
            for id in root_ids {
                let root = policy
                    .roots
                    .iter()
                    .find(|root| &root.id == id)
                    .ok_or("--root must name a root in the trusted policy")?;
                roots.insert(id.clone(), root.path.clone());
            }
            context.read_policy_path = Some(path.clone());
            context.read_profile_launcher = Some(
                workbench_core::ReadProfileLauncher::current()
                    .map_err(|_| "the current executable cannot provide the restricted launcher")?,
            );
            policy_digest = Some(policy.digest);
        }
        let mut grafana_identity = None;
        if let Some(path) = &options.config {
            // Descriptions load bounded configuration/CA bytes, never credentials or HTTP.
            let (description, identity) = workbench_core::grafana_config_description(path)
                .map_err(|_| "the explicit Grafana configuration is unavailable or invalid")?;
            for id in &targets {
                let target = description
                    .targets
                    .iter()
                    .find(|target| &target.id == id)
                    .ok_or("--target must name a target in the trusted configuration")?;
                if target.api_profile.is_none() {
                    return Err("a granted Grafana target has an unsupported API profile");
                }
            }
            context.config_path = Some(path.clone());
            grafana_identity = Some(identity);
        }
        if operations.contains("task.run")
            && workbench_core::tasks::availability()["status"] != "available"
        {
            return Err("the numerical task runtime is unavailable");
        }
        let collection_grant = if collection {
            let collection_context = OperatorContext {
                config_path: options.config.clone(),
                context_source_path: options.context_source.clone(),
                context_max_age_days: options.context_max_age_days,
                evidence_store: options.evidence_store.clone(),
                ..OperatorContext::default()
            };
            Some(workbench_core::evidence_collection_grant(&collection_context, &options.targets)
                .map_err(|_| "the collection configuration, context source or existing store could not be admitted")?)
        } else {
            None
        };
        Ok(Self {
            operations,
            roots,
            targets,
            context,
            policy_digest,
            grafana_identity,
            diagnostic_grant,
            investigation_grant,
            collection_grant,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_authority_is_empty_and_unknown_or_duplicate_operations_fail() {
        let grants = Grants::new(&Options::default()).unwrap();
        assert!(grants.operations.is_empty());
        assert!(grants.roots.is_empty());
        assert!(grants.targets.is_empty());
        for operations in [vec!["capability.list"], vec!["task.run", "task.run"]] {
            assert_eq!(
                Grants::new(&Options {
                    allow: operations.into_iter().map(str::to_owned).collect(),
                    ..Options::default()
                })
                .err(),
                Some("--allow requires unique supported operation IDs")
            );
        }
    }

    #[test]
    fn privileged_operations_require_explicit_scope_and_configuration() {
        for operation in ["process.exec", "command.inspect"] {
            assert_eq!(
                Grants::new(&Options {
                    allow: vec![operation.into()],
                    ..Options::default()
                })
                .err(),
                Some("command grants require an explicit --read-policy and at least one --root ID")
            );
        }
        for operation in ["grafana.dashboard.get", "grafana.query", "query.run"] {
            assert_eq!(
                Grants::new(&Options {
                    allow: vec![operation.into()],
                    ..Options::default()
                })
                .err(),
                Some("Grafana grants require an explicit --config and at least one --target ID")
            );
        }
    }

    #[test]
    fn saved_query_catalog_grants_are_offline_and_query_execution_needs_a_target() {
        let catalog = Grants::new(&Options {
            allow: vec!["queries.list".into(), "queries.describe".into()],
            ..Options::default()
        })
        .unwrap();
        assert!(catalog.targets.is_empty());
        assert!(catalog.grafana_identity.is_none());
        assert_eq!(catalog.operations.len(), 2);

        let denied = Grants::new(&Options {
            allow: vec!["queries.list".into()],
            config: Some("/missing".into()),
            targets: vec!["fixture".into()],
            ..Options::default()
        });
        assert_eq!(
            denied.err(),
            Some("--config and --target require a Grafana grant")
        );
    }

    #[test]
    fn diagnostic_catalog_is_offline_and_execution_requires_its_own_authority() {
        let catalog = Grants::new(&Options {
            allow: vec!["diagnostics.list".into(), "diagnostics.describe".into()],
            ..Options::default()
        })
        .unwrap();
        assert_eq!(catalog.operations.len(), 2);
        assert!(catalog.targets.is_empty());
        assert!(catalog.grafana_identity.is_none());

        assert_eq!(
            Grants::new(&Options {
                allow: vec!["diagnostics.run".into()],
                ..Options::default()
            })
            .err(),
            Some(
                "diagnostics.run requires --diagnostics-config and at least one --diagnostic-target ID"
            )
        );
    }

    #[test]
    fn unused_or_duplicate_selectors_fail_before_reading_configuration() {
        for options in [
            Options {
                roots: vec!["repo".into()],
                ..Options::default()
            },
            Options {
                read_policy: Some("/missing".into()),
                ..Options::default()
            },
            Options {
                targets: vec!["fixture".into()],
                ..Options::default()
            },
            Options {
                config: Some("/missing".into()),
                ..Options::default()
            },
        ] {
            assert!(Grants::new(&options).is_err());
        }
        for options in [
            Options {
                allow: vec!["process.exec".into()],
                roots: vec!["repo".into(), "repo".into()],
                read_policy: Some("/missing".into()),
                ..Options::default()
            },
            Options {
                allow: vec!["grafana.query".into()],
                targets: vec!["fixture".into(), "fixture".into()],
                config: Some("/missing".into()),
                ..Options::default()
            },
        ] {
            assert_eq!(
                Grants::new(&options).err(),
                Some("--root and --target IDs must be unique")
            );
        }
    }

    #[test]
    fn investigation_grants_require_exact_paired_startup_bindings() {
        for options in [
            Options {
                allow: vec!["context.resolve".into()],
                ..Default::default()
            },
            Options {
                allow: vec!["context.resolve".into()],
                context_source: Some("/missing".into()),
                ..Default::default()
            },
            Options {
                context_source: Some("/missing".into()),
                context_max_age_days: Some(30),
                ..Default::default()
            },
            Options {
                allow: vec!["evidence.list".into()],
                ..Default::default()
            },
            Options {
                evidence_store: Some(EvidenceStoreOptions::new("/missing".into())),
                ..Default::default()
            },
            Options {
                allow: vec!["evidence.list".into()],
                context_source: Some("/missing".into()),
                context_max_age_days: Some(30),
                evidence_store: Some(EvidenceStoreOptions::new("/missing".into())),
                ..Default::default()
            },
            Options {
                allow: vec!["context.resolve".into(), "context.resolve".into()],
                context_source: Some("/missing".into()),
                context_max_age_days: Some(30),
                ..Default::default()
            },
        ] {
            assert!(Grants::new(&options).is_err());
        }
        for policy in ["max_bytes", "retention_days"] {
            let mut options = EvidenceStoreOptions::new("/missing".into());
            if policy == "max_bytes" {
                options.max_bytes = 10;
            } else {
                options.retention_days = 1;
            }
            assert_eq!(
                Grants::new(&Options {
                    allow: vec!["evidence.list".into()],
                    evidence_store: Some(options),
                    ..Default::default()
                })
                .err(),
                Some("offline investigation grants do not accept store policy overrides")
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn admitted_context_authority_stays_inside_the_exact_opaque_grant() {
        use std::fs;
        let directory = std::env::temp_dir().join(workbench_core::result::new_id("mcp-grant"));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("source.json");
        fs::write(
            &path,
            include_bytes!("../../../tests/fixtures/context/unique.json"),
        )
        .unwrap();
        let options = Options {
            allow: vec!["context.resolve".into()],
            context_source: Some(path.clone()),
            context_max_age_days: Some(30),
            ..Default::default()
        };
        let grants = Grants::new(&options).unwrap();
        assert!(
            grants
                .investigation_grant
                .as_ref()
                .unwrap()
                .allows("context.resolve")
        );
        for operation in [
            "evidence.list",
            "evidence.open",
            "evidence.compare",
            "report.create",
            "task.run",
        ] {
            assert!(
                !grants
                    .investigation_grant
                    .as_ref()
                    .unwrap()
                    .allows(operation)
            );
        }
        assert!(grants.context.context_source_path.is_none());
        assert!(grants.context.context_max_age_days.is_none());
        assert!(grants.context.evidence_store.is_none());
        assert!(
            Grants::new(&Options {
                context_max_age_days: Some(0),
                ..options
            })
            .is_err()
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
