mod output;

use clap::{Args, Parser, Subcommand, error::ErrorKind};
use serde_json::json;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use workbench_core::{
    OperatorContext, RunControl, execute_with_context,
    request::{Problem, Request, parse_json, parse_request},
    result::OperationResult,
};

#[derive(Parser)]
#[command(
    name = "save",
    version,
    about = "Experimental bounded commands, offline tasks and Grafana reads",
    long_about = "Run literal native commands with the invoking operator account's authority. Generic execution has unclassified effects and can change files or remote systems. Attach an explicit --read-policy to restrict commands to the linux-read-v1 Git/rg grammar and required Linux containment. This does not create a protected same-account identity or authorize other operations. No service health assessment or automatic retry is performed; discovery is offline.",
    after_help = "Example: save --json exec --cwd . -- /usr/bin/printf '%s\\n' 'hello workbench'\nDefaults: 30s timeout, 1MiB per pipe, 2MiB encoded JSON, record=never, closed stdin.\nChild environment: PATH=/usr/bin:/bin LANG=C.UTF-8 LC_ALL=C.UTF-8 TZ=UTC.\nOnly bare names from /usr/bin:/bin or absolute native executable paths are admitted.\nShells and scripts need future named-task bindings. Process groups do not contain\ndeliberately escaping programs. Cleanup allows 2s grace plus 500ms final drainage.\nExit: 0 success; 1 failed/partial/timeout/unknown; 2 usage/denied/unsupported;\n130 SIGINT; 143 SIGTERM. Child exit and signal remain separate in the receipt."
)]
struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Restrict commands using an explicit absolute linux-read-v1 policy; requires --cwd, Git/rg grammar and Linux isolation. No fallback."
    )]
    read_policy: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Explicit absolute operator config for named Grafana connections; no config discovery"
    )]
    config: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Emit exactly one bounded terminal JSON result"
    )]
    json: bool,
    #[arg(
        long,
        global = true,
        default_value = "never",
        help = "Evidence recording; this increment accepts never only"
    )]
    record: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a native executable with literal arguments after --
    Exec(ProcessArgs),
    /// Inspect a command without spawning it
    Command {
        #[command(subcommand)]
        command: CommandCommands,
    },
    /// Invoke the same core using a strict 64KiB, depth-16 request file
    Call {
        #[arg(
            long,
            value_name = "FILE",
            help = "Regular UTF-8 JSON request file (stdin is not read)"
        )]
        request: PathBuf,
    },
    /// Discover installed capabilities offline
    Capabilities {
        #[command(subcommand)]
        command: CapabilityCommands,
    },
    /// Inspect or run a fixed embedded offline task
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
    /// Read a configured Grafana connection using the fixed legacy API fixture profile
    Grafana {
        #[command(subcommand)]
        command: GrafanaCommands,
    },
    /// Serve the local browser workbench with explicit immutable operation and root grants
    Ui {
        #[command(subcommand)]
        command: UiCommands,
    },
    /// Report offline runtime readiness and limitations without spawning probes
    Doctor,
}

#[derive(Subcommand)]
enum UiCommands {
    /// Bind only127.0.0.1 and print a sensitive process-scoped launch link. History is volatile.
    Serve {
        #[arg(
            long,
            default_value_t = 0,
            help = "Loopback port;0 chooses an ephemeral port"
        )]
        port: u16,
        #[arg(
            long,
            value_name = "OPERATION",
            help = "Repeat process.exec, command.inspect or task.run; empty grants expose no checks"
        )]
        allow: Vec<String>,
        #[arg(
            long,
            value_name = "ROOT_ID",
            help = "Repeat trusted policy root IDs; commands also require --read-policy"
        )]
        root: Vec<String>,
    },
}

#[derive(Subcommand)]
enum CommandCommands {
    Inspect(ProcessArgs),
}

#[derive(Subcommand)]
enum CapabilityCommands {
    List,
    Describe { operation: String },
}

#[derive(Subcommand)]
enum TaskCommands {
    List,
    Describe {
        id: String,
    },
    Run {
        id: String,
        #[arg(
            long,
            value_name = "FILE",
            help = "Bounded JSON input containing {\"file\":\"model path\"}"
        )]
        input: PathBuf,
        #[arg(long, default_value="30s", value_parser=parse_duration)]
        timeout: u64,
        #[arg(long, default_value_t = 1_048_576)]
        max_output_bytes: usize,
        #[arg(
            long,
            help = "Exit 1 for completed checks with findings; preserve their succeeded execution status"
        )]
        fail_on_findings: bool,
    },
}

#[derive(Args)]
struct GrafanaTarget {
    #[arg(long)]
    target: String,
    #[arg(long, default_value="60s", value_parser=parse_duration)]
    timeout: u64,
    #[arg(long, default_value_t = 2_097_152)]
    max_output_bytes: usize,
}

#[derive(Subcommand)]
enum GrafanaCommands {
    Dashboard {
        #[command(subcommand)]
        command: DashboardCommands,
    },
    Query {
        #[command(flatten)]
        target: GrafanaTarget,
        #[arg(long)]
        datasource: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        expr: String,
    },
}

#[derive(Subcommand)]
enum DashboardCommands {
    Get {
        #[command(flatten)]
        target: GrafanaTarget,
        #[arg(long)]
        uid: String,
    },
}

fn input_file(path: PathBuf) -> Result<std::fs::File, Problem> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|_| {
        Problem::invalid("request_read_failed", "The input file could not be opened.")
    })?;
    if !file.metadata().map(|m| m.is_file()).unwrap_or(false) {
        return Err(Problem::invalid(
            "invalid_request_file",
            "Input must be a regular JSON file.",
        ));
    }
    Ok(file)
}

#[derive(Args)]
struct ProcessArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Working directory (default .); must be explicit and a named root with --read-policy"
    )]
    cwd: Option<PathBuf>,
    #[arg(long, default_value = "30s", value_parser = parse_duration, help = "Whole-operation deadline: integer ms, s or m; maximum 300s")]
    timeout: u64,
    #[arg(
        long,
        default_value_t = 1_048_576,
        help = "Capture ceiling per pipe, 1024..2097152; host clamps to 1MiB"
    )]
    max_output_bytes: usize,
    #[arg(last = true, required = true, num_args = 1.., value_name = "PROGRAM ARG...")]
    argv: Vec<String>,
}

fn parse_duration(value: &str) -> Result<u64, &'static str> {
    let (number, factor) = if let Some(n) = value.strip_suffix("ms") {
        (n, 1)
    } else if let Some(n) = value.strip_suffix('s') {
        (n, 1000)
    } else if let Some(n) = value.strip_suffix('m') {
        (n, 60_000)
    } else {
        return Err("use an integer duration with ms, s or m suffix");
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err("duration must be a positive integer");
    }
    match number
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(factor))
    {
        Some(ms @ 1..=300_000) => Ok(ms),
        _ => Err("duration must be between 1ms and 300s"),
    }
}

fn normalize(cli: Cli) -> Result<Request, Problem> {
    let restricted = cli.read_policy.is_some();
    let mut request = match cli.command {
        Commands::Exec(args) => process_request("process.exec", args, restricted)?,
        Commands::Command {
            command: CommandCommands::Inspect(args),
        } => process_request("command.inspect", args, restricted)?,
        Commands::Call { request: path } => {
            if cli.record != "never" {
                return Err(Problem::unsupported(
                    "unsupported_record_mode",
                    "The global record option must be never; the structured request's own record field is validated separately.",
                ));
            }
            return parse_request(input_file(path)?);
        }
        Commands::Task {
            command: TaskCommands::List,
        } => Request::new("task.list", json!({})),
        Commands::Task {
            command: TaskCommands::Describe { id },
        } => Request::new("task.describe", json!({"id":id})),
        Commands::Task {
            command:
                TaskCommands::Run {
                    id,
                    input,
                    timeout,
                    max_output_bytes,
                    fail_on_findings,
                },
        } => {
            if fail_on_findings && id != "dashboard-hygiene" {
                return Err(Problem::invalid(
                    "unsupported_exit_policy",
                    "--fail-on-findings is supported only for dashboard-hygiene.",
                ));
            }
            let mut input = parse_json(input_file(input)?)?;
            if let Some(file) = input.get("file").and_then(serde_json::Value::as_str) {
                let file = PathBuf::from(file);
                if !file.is_absolute() {
                    let absolute = std::env::current_dir()
                        .map_err(|_| {
                            Problem::invalid(
                                "invalid_cwd",
                                "The invoking directory could not be resolved.",
                            )
                        })?
                        .join(file);
                    input["file"] = json!(absolute.to_str().ok_or_else(|| Problem::invalid(
                        "invalid_task_input",
                        "The model path must be UTF-8."
                    ))?);
                }
            }
            let mut request = Request::new("task.run", json!({"id":id,"version":1,"input":input}));
            request.limits.timeout_ms = timeout;
            request.limits.max_output_bytes = max_output_bytes;
            request
        }
        Commands::Capabilities {
            command: CapabilityCommands::List,
        } => Request::new("capability.list", json!({})),
        Commands::Capabilities {
            command: CapabilityCommands::Describe { operation },
        } => Request::new("capability.describe", json!({"operation":operation})),
        Commands::Grafana {
            command:
                GrafanaCommands::Dashboard {
                    command: DashboardCommands::Get { target, uid },
                },
        } => grafana_request("grafana.dashboard.get", target, json!({"uid":uid})),
        Commands::Grafana {
            command:
                GrafanaCommands::Query {
                    target,
                    datasource,
                    kind,
                    from,
                    to,
                    expr,
                },
        } => grafana_request(
            "grafana.query",
            target,
            json!({"datasource":datasource,"kind":kind,"from":from,"to":to,"expr":expr}),
        ),
        Commands::Doctor => Request::new("doctor", json!({})),
        Commands::Ui { .. } => unreachable!("UI handled before operation normalization"),
    };
    request.record = cli.record;
    Ok(request)
}

fn grafana_request(operation: &str, target: GrafanaTarget, input: serde_json::Value) -> Request {
    let mut request = Request::new(operation, input);
    request.target.kind = "connection".into();
    request.target.id = target.target;
    request.limits.timeout_ms = target.timeout;
    request.limits.max_output_bytes = target.max_output_bytes;
    request
}

fn process_request(
    operation: &str,
    args: ProcessArgs,
    restricted: bool,
) -> Result<Request, Problem> {
    if restricted && args.cwd.is_none() {
        return Err(Problem::invalid(
            "explicit_cwd_required",
            "The read profile requires an explicit --cwd matching a named policy root.",
        ));
    }
    let supplied_cwd = args.cwd.unwrap_or_else(|| PathBuf::from("."));
    let mut cwd = if supplied_cwd.is_absolute() {
        supplied_cwd
    } else {
        std::env::current_dir()
            .map_err(|_| {
                Problem::invalid(
                    "invalid_cwd",
                    "The invoking directory could not be resolved.",
                )
            })?
            .join(supplied_cwd)
    };
    if restricted {
        cwd = std::fs::canonicalize(cwd).map_err(|_| {
            Problem::invalid(
                "invalid_cwd",
                "The explicit working directory could not be resolved.",
            )
        })?;
    }
    let cwd = cwd.to_str().ok_or_else(|| {
        Problem::invalid(
            "invalid_cwd",
            "The working directory must have a UTF-8 path.",
        )
    })?;
    let mut argv = args.argv.into_iter();
    let program = argv.next().ok_or_else(|| {
        Problem::invalid("invalid_usage", "Specify a native executable after --.")
    })?;
    let mut request = Request::new(
        operation,
        json!({"program":program,"args":argv.collect::<Vec<_>>(),"cwd":cwd}),
    );
    request.limits.timeout_ms = args.timeout;
    request.limits.max_output_bytes = args.max_output_bytes;
    Ok(request)
}

fn main() -> std::process::ExitCode {
    if let Some(code) = workbench_core::read_launcher_entry() {
        return std::process::ExitCode::from(code);
    }
    std::process::ExitCode::from(run())
}

fn run() -> u8 {
    // Only flags before the literal argv separator participate in CLI parsing.
    let wants_json = std::env::args_os()
        .skip(1)
        .take_while(|a| a != "--")
        .any(|a| a == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                return if output::write(error.to_string().as_bytes(), &RunControl::default().signal)
                    .is_ok()
                {
                    0
                } else {
                    1
                };
            }
            let result = OperationResult::rejection(
                &Request::new("request.invalid", json!({})),
                Problem::invalid(
                    "invalid_usage",
                    "Invalid CLI usage; consult save --help and the selected command's --help.",
                ),
            );
            return deliver(result, wants_json, &RunControl::default());
        }
    };
    if let Commands::Ui {
        command: UiCommands::Serve { port, allow, root },
    } = &cli.command
    {
        if cli.json || cli.record != "never" || cli.config.is_some() {
            eprintln!(
                "ui serve does not accept --json, --config or recording modes other than never"
            );
            return 2;
        }
        return match workbench_ui::serve(workbench_ui::Options {
            port: *port,
            allow: allow.clone(),
            roots: root.clone(),
            read_policy: cli.read_policy.clone(),
        }) {
            Ok(code) => code,
            Err(message) => {
                eprintln!("ui serve: {message}");
                1
            }
        };
    }
    let json = cli.json;
    let context = OperatorContext {
        config_path: cli.config.clone(),
        read_policy_path: cli.read_policy.clone(),
        read_profile_launcher: cli
            .read_policy
            .as_ref()
            .and_then(|_| workbench_core::ReadProfileLauncher::current().ok()),
    };
    let fail_on_findings = matches!(
        &cli.command,
        Commands::Task {
            command: TaskCommands::Run {
                fail_on_findings: true,
                ..
            }
        }
    );
    let mut control = RunControl::default();
    #[cfg(target_os = "linux")]
    {
        control.result_fd = Some(libc::STDOUT_FILENO);
    }
    #[cfg(unix)]
    {
        use std::sync::Arc;
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            if signal_hook::flag::register_usize(
                signal,
                Arc::clone(&control.signal),
                signal as usize,
            )
            .is_err()
            {
                return deliver(
                    OperationResult::rejection(
                        &Request::new("request.invalid", json!({})),
                        Problem::invalid(
                            "signal_setup_failed",
                            "Cancellation handlers could not be initialized; nothing was dispatched.",
                        ),
                    ),
                    json,
                    &control,
                );
            }
        }
    }
    let result = match normalize(cli) {
        Ok(request) => execute_with_context(request, &control, &context),
        Err(problem) => {
            OperationResult::rejection(&Request::new("request.invalid", json!({})), problem)
        }
    };
    let completed_with_findings = result.execution.status
        == workbench_core::result::Status::Succeeded
        && result
            .data
            .get("findings_total")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|count| count > 0);
    let code = deliver(result, json, &control);
    if code == 0 && fail_on_findings && completed_with_findings {
        1
    } else {
        code
    }
}

fn deliver(mut result: OperationResult, json: bool, control: &RunControl) -> u8 {
    let signal = control.signal.load(Ordering::Relaxed);
    if signal != 0 {
        result.data["cancellation_signal"] = json!(signal);
    }
    let code = result.exit_code();
    let bytes = if json {
        result.json_bytes()
    } else {
        result.text().into_bytes()
    };
    let written = output::write(&bytes, &control.signal);
    // Delivery can outlive process supervision. Observe the signal again rather than
    // freezing normalized exit status before a potentially stalled output write.
    match control.signal.load(Ordering::Relaxed) {
        2 => return 130,
        15 => return 143,
        _ => {}
    }
    match written {
        Ok(()) => code,
        Err(_) if matches!(code, 130 | 143) => code,
        Err(_) => 1,
    }
}
