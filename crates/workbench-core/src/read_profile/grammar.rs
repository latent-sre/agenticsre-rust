use crate::request::{Problem, ProcessInput};

pub(crate) struct Command {
    pub name: &'static str,
    pub args: Vec<String>,
    pub paths: Vec<String>,
}

fn denied() -> Problem {
    Problem::invalid(
        "read_command_denied",
        "The selected command or argument form is outside linux-read-v1. Consult the installed profile grammar.",
    )
}

pub(super) fn relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && path != "-"
        && !path.starts_with('/')
        && !path.contains(['\\', '\0'])
        && path.split('/').all(|part| !part.is_empty() && part != "..")
}

pub(crate) fn normalize(input: &ProcessInput) -> Result<Command, Problem> {
    let args = &input.args;
    let mut split = args.len();
    let mut index = 0;
    while index < args.len() {
        // An explicit pattern is data, including a literal "--" pattern.
        if input.program == "rg" && args[index] == "-e" {
            index += 2;
            continue;
        }
        if args[index] == "--" {
            split = index;
            break;
        }
        index += 1;
    }
    let options = &args[..split];
    let paths = if split < args.len() {
        &args[split + 1..]
    } else {
        &[]
    };
    if paths.len() > 32 || paths.iter().any(|path| !relative(path)) {
        return Err(denied());
    }
    let mut normalized: Vec<String> = Vec::new();
    let name = match input.program.as_str() {
        "git" => {
            normalized.extend(
                [
                    "--no-pager",
                    "--literal-pathspecs",
                    "--no-optional-locks",
                    "--no-replace-objects",
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "core.hooksPath=/nonexistent",
                    "-c",
                    "core.attributesFile=/dev/null",
                    "-c",
                    "safe.directory=/workspace",
                ]
                .map(str::to_owned),
            );
            let Some((verb, options)) = options.split_first() else {
                return Err(denied());
            };
            normalized.push(verb.clone());
            let mut seen = std::collections::HashSet::<&str>::new();
            match verb.as_str() {
                "status" => {
                    normalized.extend(
                        [
                            "--porcelain=v1",
                            "--untracked-files=no",
                            "--ignore-submodules=all",
                        ]
                        .map(str::to_owned),
                    );
                    for option in options {
                        let key = if option == "--short" {
                            "--porcelain=v1"
                        } else {
                            option
                        };
                        if !seen.insert(key)
                            || !matches!(option.as_str(), "--short" | "--porcelain=v1" | "--branch")
                        {
                            return Err(denied());
                        }
                        if option == "--branch" {
                            normalized.push(option.clone());
                        }
                    }
                }
                "diff" => {
                    normalized.extend(
                        [
                            "--no-ext-diff",
                            "--no-textconv",
                            "--no-color",
                            "--no-renames",
                            "--ignore-submodules=all",
                        ]
                        .map(str::to_owned),
                    );
                    let mut formats = 0;
                    for option in options {
                        if !seen.insert(option.as_str())
                            || !matches!(
                                option.as_str(),
                                "--cached"
                                    | "--stat"
                                    | "--name-only"
                                    | "--name-status"
                                    | "--exit-code"
                            )
                        {
                            return Err(denied());
                        }
                        formats += usize::from(matches!(
                            option.as_str(),
                            "--stat" | "--name-only" | "--name-status"
                        ));
                        normalized.push(option.clone());
                    }
                    if formats > 1 {
                        return Err(denied());
                    }
                }
                "log" => {
                    normalized.extend(
                        [
                            "--format=%h %s",
                            "--no-decorate",
                            "--no-show-signature",
                            "--no-color",
                            "--no-patch",
                            "--no-ext-diff",
                            "--no-textconv",
                        ]
                        .map(str::to_owned),
                    );
                    let mut count = None;
                    let mut index = 0;
                    while index < options.len() {
                        let option = &options[index];
                        if option == "--oneline" {
                            if !seen.insert(option.as_str()) {
                                return Err(denied());
                            }
                        } else {
                            let number = if option == "-n" {
                                index += 1;
                                options.get(index).ok_or_else(denied)?.as_str()
                            } else {
                                option.strip_prefix("--max-count=").ok_or_else(denied)?
                            };
                            if count.is_some()
                                || number.is_empty()
                                || !number.bytes().all(|b| b.is_ascii_digit())
                            {
                                return Err(denied());
                            }
                            let parsed = number.parse::<u32>().map_err(|_| denied())?;
                            if !(1..=100).contains(&parsed) {
                                return Err(denied());
                            }
                            count = Some(parsed);
                        }
                        index += 1;
                    }
                    normalized.push(format!("--max-count={}", count.unwrap_or(20)));
                }
                _ => return Err(denied()),
            }
            "git"
        }
        "rg" => {
            // Explicit operands override ripgrep glob exclusions, including
            // directory operands. Refuse metadata components before resolution;
            // the fixed glob separately covers traversal from an ordinary path.
            if paths
                .iter()
                .any(|path| path.split('/').any(|part| part == ".git"))
            {
                return Err(denied());
            }
            normalized.extend(
                [
                    "--no-config",
                    "--no-follow",
                    "--color=never",
                    "--no-heading",
                    "--line-number",
                    "--threads=1",
                    "--max-filesize=8M",
                    "--max-count=1000",
                    "--glob=!**/.git",
                ]
                .map(str::to_owned),
            );
            let mut files = false;
            let mut pattern = None;
            let mut seen = std::collections::HashSet::new();
            let mut index = 0;
            while index < options.len() {
                let option = &options[index];
                let key = match option.as_str() {
                    "-n" => "--line-number",
                    "-i" => "--ignore-case",
                    "-F" => "--fixed-strings",
                    value => value,
                };
                if !seen.insert(key) {
                    return Err(denied());
                }
                match option.as_str() {
                    "--files" => {
                        files = true;
                        normalized.push(option.clone());
                    }
                    "--hidden" | "-i" | "--ignore-case" | "-F" | "--fixed-strings" | "-n"
                    | "--line-number" => normalized.push(option.clone()),
                    "-e" => {
                        index += 1;
                        let value = options.get(index).ok_or_else(denied)?;
                        if value.len() > 4096 {
                            return Err(denied());
                        }
                        pattern = Some(value.clone());
                    }
                    _ => return Err(denied()),
                }
                index += 1;
            }
            if files == pattern.is_some()
                || (files && seen.iter().any(|v| !matches!(*v, "--files" | "--hidden")))
            {
                return Err(denied());
            }
            if let Some(pattern) = pattern {
                normalized.extend(["-e".into(), pattern]);
            }
            "rg"
        }
        _ => return Err(denied()),
    };
    normalized.push("--".into());
    normalized.extend(paths.iter().cloned());
    if name == "rg" && paths.is_empty() {
        normalized.push(".".into());
    }
    Ok(Command {
        name,
        args: normalized,
        paths: paths.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(program: &str, args: &[&str]) -> ProcessInput {
        ProcessInput {
            program: program.into(),
            args: args.iter().map(|a| (*a).into()).collect(),
            cwd: "/fixture".into(),
        }
    }
    #[test]
    fn exact_forms_preserve_literal_paths_and_patterns() {
        for args in [
            vec!["status"],
            vec!["status", "--branch", "--", "$(literal)"],
            vec!["diff", "--cached", "--stat"],
            vec!["log", "-n", "100", "--", ":(literal)"],
        ] {
            assert!(normalize(&request("git", &args)).is_ok(), "{args:?}");
        }
        let normalized = normalize(&request(
            "rg",
            &["-F", "-e", "$(touch x); unicode λ", "--", "-name"],
        ))
        .unwrap();
        assert!(normalized.args.iter().any(|v| v == "$(touch x); unicode λ"));
        assert_eq!(normalized.paths, ["-name"]);
        assert!(normalize(&request("rg", &["--files"])).is_ok());
        assert!(normalize(&request("rg", &["-e", "--", "--", "read.txt"])).is_ok());
    }
    #[test]
    fn dangerous_or_ambiguous_forms_are_denied() {
        for args in [
            vec!["-c", "alias.x=!touch marker", "x"],
            vec!["status", "--porcelain=v2"],
            vec!["diff", "--ext-diff"],
            vec!["log", "--format=%G?"],
            vec!["log", "-n", "101"],
            vec!["diff", "--", "../outside"],
            vec!["diff", "HEAD"],
            vec!["log", "--max-count=0"],
            vec!["status", "--short", "--porcelain=v1"],
        ] {
            assert_eq!(
                normalize(&request("git", &args)).err().unwrap().code,
                "read_command_denied",
                "{args:?}"
            );
        }
        for args in [
            vec!["--pre", "touch marker", "-e", "x"],
            vec!["--follow", "-e", "x"],
            vec!["--files", "-e", "x"],
            vec!["-e", "x", "--", "/outside"],
            vec!["-e", "x", "--", "-"],
            vec!["--pcre2", "-e", "x"],
            vec!["-e", "x", "unseparated"],
            vec!["--files", "-i"],
            vec!["--files", "--fixed-strings"],
            vec!["--files", "--line-number"],
            vec!["-n", "--line-number", "-e", "x"],
            vec!["-i", "--ignore-case", "-e", "x"],
            vec!["-F", "--fixed-strings", "-e", "x"],
        ] {
            assert!(normalize(&request("rg", &args)).is_err(), "{args:?}");
        }
        for program in ["/usr/bin/git", "git.exe", "bash", "touch"] {
            assert!(normalize(&request(program, &["status"])).is_err());
        }
    }

    #[test]
    fn rg_metadata_operands_are_refused_without_rejecting_ordinary_names() {
        for path in [
            ".git",
            ".git/config",
            "./.git/config",
            "././.git/config",
            ".git//config",
            "nested/.git",
            "nested/.git/config",
            "nested/./.git/config",
            "nested//.git/config",
        ] {
            for args in [
                vec!["--files", "--", path],
                vec!["-F", "-e", "repositoryformatversion", "--", path],
            ] {
                assert!(
                    normalize(&request("rg", &args)).is_err(),
                    "metadata operand admitted: {args:?}"
                );
            }
        }
        for path in [
            ".",
            "./read.txt",
            "nested/./read.txt",
            ".github/read.txt",
            "nested/file.git",
        ] {
            for args in [
                vec!["--files", "--", path],
                vec!["-F", "-e", ".git", "--", path],
            ] {
                assert!(
                    normalize(&request("rg", &args)).is_ok(),
                    "ordinary operand refused: {args:?}"
                );
            }
        }
    }
}
