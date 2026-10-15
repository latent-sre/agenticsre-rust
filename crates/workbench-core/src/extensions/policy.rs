use super::*;
use package::Package;
use std::collections::{BTreeMap, BTreeSet};
pub(super) struct Policy {
    pub digest: String,
    grants: BTreeMap<String, BTreeSet<String>>,
}
impl Policy {
    pub fn load(
        context: &OperatorContext,
        control: &RunControl,
        deadline: Instant,
    ) -> Result<Option<Self>, Fault> {
        let Some(path) = &context.extension_policy_path else {
            return Ok(None);
        };
        let invalid = || Fault::new("extension_policy_invalid");
        if !path.is_absolute()
            || path
                .to_str()
                .is_none_or(|s| s.len() > 4096 || s.chars().any(char::is_control))
        {
            return Err(invalid());
        }
        let bytes = storage::source(path, control, deadline, 64 * 1024, true).map_err(|e| {
            if matches!(
                e.code,
                "extension_cancelled" | "extension_deadline_exceeded" | "extension_consumer_closed"
            ) {
                e
            } else {
                invalid()
            }
        })?;
        let value = crate::request::parse_bounded_json(&bytes, 8).map_err(|_| invalid())?;
        active(control, deadline)?;
        if !keys(
            &value,
            &["policy_version", "execution_profile", "packages"],
            &[],
        ) || value["policy_version"] != 1
            || value["execution_profile"] != "operator-reviewed-v1"
        {
            return Err(invalid());
        }
        let packages = value["packages"].as_array().ok_or_else(invalid)?;
        if packages.len() > 32 {
            return Err(invalid());
        }
        let mut grants = BTreeMap::new();
        for package in packages {
            if !keys(package, &["package_sha256", "capabilities"], &[])
                || package["package_sha256"]
                    .as_str()
                    .is_none_or(|s| !digest_string(s))
            {
                return Err(invalid());
            }
            let capabilities = package["capabilities"].as_array().ok_or_else(invalid)?;
            if !(1..=8).contains(&capabilities.len()) {
                return Err(invalid());
            }
            let mut names = BTreeSet::new();
            for name in capabilities {
                let name = name.as_str().ok_or_else(invalid)?;
                if !name.starts_with("ext.")
                    || !crate::request::valid_operation(name)
                    || !names.insert(name.to_owned())
                {
                    return Err(invalid());
                }
            }
            if grants
                .insert(
                    package["package_sha256"].as_str().unwrap().to_owned(),
                    names,
                )
                .is_some()
            {
                return Err(invalid());
            }
        }
        Ok(Some(Self {
            digest: digest(&bytes),
            grants,
        }))
    }
    pub fn grants(&self, package: &Package, operation: &str) -> bool {
        self.grants
            .get(&package.digest)
            .is_some_and(|names| names.contains(operation))
    }
}
pub(super) fn counts(packages: &[Package], policy: Option<&Policy>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    if let Some(policy) = policy {
        for package in packages {
            for cap in package.manifest["capabilities"].as_array().unwrap() {
                let op = cap["operation"].as_str().unwrap();
                if policy.grants(package, op) {
                    *counts.entry(op.to_owned()).or_default() += 1;
                }
            }
        }
    }
    counts
}
pub(super) fn enabled(
    package: &Package,
    policy: Option<&Policy>,
    counts: &BTreeMap<String, usize>,
) -> bool {
    cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_pointer_width = "64"
    )) && policy.is_some_and(|policy| {
        package.manifest["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cap| {
                let op = cap["operation"].as_str().unwrap();
                policy.grants(package, op) && counts.get(op) == Some(&1)
            })
    })
}
pub(super) fn inventory(
    packages: &[Package],
    policy: Option<&Policy>,
    result: &mut OperationResult,
) {
    let counts = counts(packages, policy);
    let descriptions: Vec<_> = packages
        .iter()
        .map(|package| {
            let enabled = enabled(package, policy, &counts);
            let mut description = package.description();
            description["enabled"] = json!(enabled);
            description["execution_available"] = json!(enabled);
            description
        })
        .collect();
    let any = descriptions.iter().any(|p| p["enabled"] == true);
    result.data["packages"] = json!(descriptions);
    result.data["enabled"] = json!(any);
    result.data["execution_available"] = json!(any);
    result.data["policy_sha256"] = json!(policy.map(|p| &p.digest));
}
