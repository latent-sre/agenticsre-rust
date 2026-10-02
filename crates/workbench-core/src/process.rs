use crate::request::{Problem, ProcessInput, bounded};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod descriptor;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use descriptor::DescriptorLaunch;
#[cfg(target_os = "linux")]
pub(crate) use linux::run;

pub const CHILD_ENV: [(&str, &str); 4] = [
    ("PATH", "/usr/bin:/bin"),
    ("LANG", "C.UTF-8"),
    ("LC_ALL", "C.UTF-8"),
    ("TZ", "UTC"),
];

pub(crate) struct PreparedProcess {
    pub input: ProcessInput,
    pub executable: PathBuf,
    pub cwd: PathBuf,
    #[cfg(target_os = "linux")]
    pub descriptor_launch: Option<DescriptorLaunch>,
}

impl PreparedProcess {
    pub fn identity(&self) -> Value {
        json!({ "executable": self.executable, "cwd": self.cwd,
            "resolution": "canonical path; not a protected installation claim",
            "profile": "operator-local", "effects": "unclassified" })
    }
}

pub(crate) fn prepare(inputs: &Value) -> Result<PreparedProcess, Problem> {
    let input = validate_input(inputs)?;
    prepare_input(input)
}

pub(crate) fn validate_input(inputs: &Value) -> Result<ProcessInput, Problem> {
    let input: ProcessInput = serde_json::from_value(inputs.clone()).map_err(|_| {
        Problem::invalid(
            "invalid_process_input",
            "Process inputs require only program, args and an absolute cwd.",
        )
    })?;
    if !bounded(&input.program, 1, 1024)
        || !bounded(&input.cwd, 1, 1024)
        || input.args.len() > 100
        || input.args.iter().any(|arg| !bounded(arg, 0, 16_384))
    {
        return Err(Problem::invalid(
            "invalid_process_input",
            "Program, cwd or arguments exceed their bounds or contain a NUL byte.",
        ));
    }
    if !Path::new(&input.cwd).is_absolute() {
        return Err(Problem::invalid(
            "absolute_cwd_required",
            "Structured process requests require an absolute working directory.",
        ));
    }
    Ok(input)
}

fn prepare_input(input: ProcessInput) -> Result<PreparedProcess, Problem> {
    let cwd = fs::canonicalize(&input.cwd).map_err(|_| {
        Problem::invalid("invalid_cwd", "The working directory cannot be resolved.")
    })?;
    if !cwd.is_dir() || cwd.to_str().is_none() {
        return Err(Problem::invalid(
            "invalid_cwd",
            "The working directory must be a directory with a UTF-8 path.",
        ));
    }
    if blocked_name(&input.program) {
        return Err(Problem::unsupported(
            "shell_not_supported",
            "Shells and batch files require a named-task capability, which is not implemented.",
        ));
    }
    if !cfg!(target_os = "linux") {
        return Err(Problem::unsupported(
            "unsupported_platform",
            "Process execution is currently implemented and verified only on Linux.",
        ));
    }
    let supplied = Path::new(&input.program);
    let executable = if supplied.is_absolute() {
        fs::canonicalize(supplied).map_err(|_| {
            Problem::invalid("executable_not_found", "The executable cannot be resolved.")
        })?
    } else {
        if supplied.components().count() != 1 || input.program.contains(['/', '\\']) {
            return Err(Problem::invalid(
                "absolute_program_required",
                "Select a bare system command name or an explicit absolute executable path.",
            ));
        }
        ["/usr/bin", "/bin"]
            .iter()
            .map(|dir| Path::new(dir).join(supplied))
            .find_map(|path| fs::canonicalize(path).ok())
            .ok_or_else(|| {
                Problem::invalid(
                    "executable_not_found",
                    "No executable with that name exists in the fixed system search path.",
                )
            })?
    };
    if executable.to_str().is_none() || blocked_name(&executable.to_string_lossy()) {
        return Err(Problem::unsupported(
            "shell_not_supported",
            "The resolved executable is a shell, batch file or unsupported path.",
        ));
    }
    let metadata = fs::metadata(&executable).map_err(|_| {
        Problem::invalid(
            "executable_unavailable",
            "The resolved executable cannot be inspected.",
        )
    })?;
    if !metadata.is_file() {
        return Err(Problem::invalid(
            "not_executable",
            "The selected executable must be a regular native executable file.",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(Problem::invalid(
                "not_executable",
                "The selected file has no executable permission bits.",
            ));
        }
    }
    // Refuse script dispatch, including an ENOEXEC shell fallback. Named task bindings will
    // define interpreters separately. This is format admission, not arbitrary-code isolation.
    use std::io::Read;
    let mut header = [0_u8; 4];
    let mut file = fs::File::open(&executable).map_err(|_| {
        Problem::invalid(
            "executable_unavailable",
            "The resolved executable cannot be read.",
        )
    })?;
    if file.read_exact(&mut header).is_err() || header != *b"\x7fELF" {
        return Err(Problem::unsupported(
            "script_not_supported",
            "Only native Linux executables are supported; scripts require named-task bindings.",
        ));
    }
    Ok(PreparedProcess {
        input,
        executable,
        cwd,
        #[cfg(target_os = "linux")]
        descriptor_launch: None,
    })
}

fn blocked_name(program: &str) -> bool {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "sh" | "bash"
            | "dash"
            | "ash"
            | "zsh"
            | "fish"
            | "ksh"
            | "csh"
            | "tcsh"
            | "pwsh"
            | "powershell"
            | "cmd"
            | "cmd.exe"
            | "powershell.exe"
            | "pwsh.exe"
    ) || name.ends_with(".bat")
        || name.ends_with(".cmd")
        || name.ends_with(".ps1")
}
