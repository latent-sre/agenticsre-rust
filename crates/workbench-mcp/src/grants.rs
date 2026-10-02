use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use workbench_core::{GrafanaConfigIdentity, OperatorContext};

#[derive(Default)]
pub struct Options {
    pub allow: Vec<String>,
    pub roots: Vec<String>,
    pub targets: Vec<String>,
    pub read_policy: Option<PathBuf>,
    pub config: Option<PathBuf>,
}

pub(crate) struct Grants {
    pub operations: BTreeSet<String>,
    pub roots: BTreeMap<String, String>,
    pub targets: BTreeSet<String>,
    pub context: OperatorContext,
    pub policy_digest: Option<String>,
    pub grafana_identity: Option<GrafanaConfigIdentity>,
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
            ) || !operations.insert(operation.clone())
            {
                return Err("--allow requires unique supported operation IDs");
            }
        }
        let commands =
            operations.contains("process.exec") || operations.contains("command.inspect");
        let grafana =
            operations.contains("grafana.dashboard.get") || operations.contains("grafana.query");
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
        let root_ids: BTreeSet<_> = options.roots.iter().collect();
        let targets: BTreeSet<_> = options.targets.iter().cloned().collect();
        if root_ids.len() != options.roots.len() || targets.len() != options.targets.len() {
            return Err("--root and --target IDs must be unique");
        }
        let mut context = OperatorContext::default();
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
        Ok(Self {
            operations,
            roots,
            targets,
            context,
            policy_digest,
            grafana_identity,
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
        for operation in ["grafana.dashboard.get", "grafana.query"] {
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
}
