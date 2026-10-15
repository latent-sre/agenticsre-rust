mod output;

use clap::{Args, Parser, Subcommand, error::ErrorKind};
use serde_json::json;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use workbench_core::{
    EvidenceStoreOptions, OperatorContext, RunControl, execute_with_context,
    request::{Problem, Request, parse_json, parse_request},
    result::OperationResult,
    serve_runner, work_job, work_schedule,
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
        value_name = "DIRECTORY",
        help = "Explicit private schedule definitions and occurrence ledger"
    )]
    schedule_state_dir: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Explicit private finite fleet inventory and runner profile references"
    )]
    fleet_inventory: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "DIRECTORY",
        help = "Explicit private fleet admission and reconciliation store"
    )]
    fleet_state_dir: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Explicit private remote-runner controller profile and certificate references"
    )]
    runner_config: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "DIRECTORY",
        help = "Explicit private durable-job store; submission never starts a worker"
    )]
    job_state_dir: Option<PathBuf>,
    #[arg(long, global = true, requires = "job_state_dir", value_parser = clap::value_parser!(u64).range(1..=3_600_000), help = "Operator startup ceiling for job runtime in milliseconds; at most one hour")]
    job_max_runtime_ms: Option<u64>,
    #[arg(
        long,
        global = true,
        value_name = "DIRECTORY",
        help = "Explicit private extension package store; installing a package never enables execution"
    )]
    extension_state_dir: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Explicit private operator-reviewed extension policy; grants exact installed package hashes and capabilities"
    )]
    extension_policy: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "DIRECTORY",
        help = "Explicit absolute private evidence store; no default or wire-selected path"
    )]
    state_dir: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "DIRECTORY",
        help = "Explicit private guided-runbook journal, separate from the evidence store"
    )]
    runbook_state_dir: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "DIRECTORY",
        help = "Explicit private opt-in toil metadata store; separate from evidence and runbook journals"
    )]
    toil_state_dir: Option<PathBuf>,
    #[arg(long, global = true, requires = "state_dir", value_parser = clap::value_parser!(u64).range(1..=1_073_741_824), help = "Evidence store byte cap, at most 1GiB; defaults to 1GiB")]
    store_max_bytes: Option<u64>,
    #[arg(long, global = true, requires = "state_dir", value_parser = clap::value_parser!(u16).range(1..=3650), help = "Unprotected sealed evidence retention in days, 1..3650; defaults to 7")]
    retention_days: Option<u16>,
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Explicit absolute diagnostics target configuration; no target, resolver, CA or PID can come from wire input"
    )]
    diagnostics_config: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        requires = "context_max_age_days",
        help = "Explicit absolute offline fixture export; no source discovery or dispatch authority"
    )]
    context_source: Option<PathBuf>,
    #[arg(long, global = true, requires = "context_source", value_parser = clap::value_parser!(u16).range(1..=365), help = "Maximum review age in days, 1..365; paired with --context-source")]
    context_max_age_days: Option<u16>,
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
        value_name = "PATH",
        help = "Explicit official promtool 3.15.0 Linux amd64 executable for prometheus-rules checks; bytes are pinned before dispatch"
    )]
    promtool: Option<PathBuf>,
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
        help = "Evidence collection, runbook execution, toil, job, fleet and schedule writes require auto or always; read operations accept never"
    )]
    record: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Plan, approve, apply and reconcile an explicitly granted rule-file replacement
    Change {
        #[command(subcommand)]
        command: ChangeCommands,
    },
    /// Manage persistent observational schedules and explicitly process due work
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommands,
    },
    /// Collect bounded observations across a frozen target set and reconcile original admissions
    Fleet {
        #[command(subcommand)]
        command: FleetCommands,
    },
    /// Connect to an authenticated remote runner or explicitly serve configured local grants
    Runner {
        #[command(subcommand)]
        command: RunnerCommands,
    },
    /// Submit durable local work, inspect events, request cancellation or run an explicit worker
    Job {
        #[command(subcommand)]
        command: JobCommands,
    },
    /// Inspect or install inert extension packages without executing or enabling them
    Extension {
        #[command(subcommand)]
        command: ExtensionCommands,
    },
    /// Check declared configuration bytes with an installed offline validator
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Record opted-in workflow metadata and summarize effort, latency and automation costs
    Toil {
        #[command(subcommand)]
        command: ToilCommands,
    },
    /// Render a handoff from verified historical evidence and pending knowledge proposals
    Report {
        #[command(subcommand)]
        command: ReportCommands,
    },
    /// Run reviewed observations with typed branches and durable human checkpoints
    Runbook {
        #[command(subcommand)]
        command: RunbookCommands,
    },
    /// Collect fixed saved queries and manage private, versioned local evidence
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommands,
    },
    /// Discover fixed installed diagnostic packs without reading configuration or probing targets
    Diagnostics {
        #[command(subcommand)]
        command: DiagnosticsCommands,
    },
    /// Observe DNS, TCP, TLS, HTTP and one exact configured local process; no health assessment
    Diagnose {
        #[arg(long)]
        target: String,
        #[arg(long, default_value = "connectivity")]
        pack: String,
        #[arg(
            long,
            value_name = "FILE",
            help = "Typed JSON input: optional capture_body boolean and on_failure continue/stop only"
        )]
        input: PathBuf,
        #[arg(long, default_value = "60s", value_parser = parse_duration)]
        timeout: u64,
        #[arg(long, default_value_t = 16384)]
        max_output_bytes: usize,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..=2))]
        concurrency: u64,
    },
    /// Resolve offline service metadata without selecting a live execution target
    Context {
        #[command(subcommand)]
        command: ContextCommands,
    },
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
    /// Discover or run installed versioned PromQL/LogQL queries with literal parameters
    Queries {
        #[command(subcommand)]
        command: QueriesCommands,
    },
    /// Serve the local browser workbench with explicit immutable operation and root grants
    Ui {
        #[command(subcommand)]
        command: UiCommands,
    },
    /// Serve bounded MCP over local stdio pipes with explicit immutable launcher grants
    Mcp {
        #[command(subcommand)]
        command: McpCommands,
    },
    /// Report offline runtime readiness and limitations without spawning probes
    Doctor,
}

#[derive(Subcommand)]
enum ContextCommands {
    Resolve {
        #[arg(long)]
        service: String,
        #[arg(long)]
        env: String,
        #[arg(long)]
        team: Option<String>,
        #[arg(long)]
        deployment: Option<String>,
    },
}

#[derive(Subcommand)]
enum DiagnosticsCommands {
    List,
    Describe { id: String },
}

#[derive(Subcommand)]
enum RunbookCommands {
    List,
    Show {
        id: String,
        #[arg(long, default_value_t = 1)]
        version: u64,
    },
    Run {
        id: String,
        #[arg(long, default_value_t = 1)]
        version: u64,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        expected_definition_sha256: Option<String>,
        #[arg(long, requires = "fresh_from_revision")]
        fresh_from: Option<String>,
        #[arg(long, requires = "fresh_from")]
        fresh_from_revision: Option<u64>,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 1_048_576)]
        max_output_bytes: usize,
    },
    Resume {
        run_id: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        acknowledge: Option<String>,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 1_048_576)]
        max_output_bytes: usize,
    },
    Status {
        run_id: String,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Check a declared kind and validator; never establishes live deployment correctness
    Check {
        #[arg(
            long,
            value_name = "FILE",
            help = "Strict JSON with kind, validator identity and absolute input file path"
        )]
        input: PathBuf,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 1_048_576)]
        max_output_bytes: usize,
        #[arg(
            long,
            help = "Return exit 1 for a complete check with findings; preserve execution status"
        )]
        fail_on_findings: bool,
    },
}

#[derive(Subcommand)]
enum ToilCommands {
    /// Explicitly enable or disable collection and set retention for this private store
    Configure {
        #[arg(long, action = clap::ArgAction::Set, required = true)]
        enabled: bool,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u16).range(1..=3650))]
        keep_days: u16,
    },
    /// Record bounded operator-reported metadata; no raw command, query or person fields
    Record(ToilInput),
    /// Run one installed task and retain only measured runtime metadata
    Capture(ToilInput),
    /// Attach operator-reported effort to an existing sample without duplicating it
    Annotate(ToilInput),
    /// Compare explicitly selected baseline/candidate samples and declared costs
    Summary(ToilInput),
    /// Delete one generated sample ID
    Delete { id: String },
    /// Remove expired metadata according to the configured retention policy
    Prune,
}

#[derive(Args)]
struct FleetLimits {
    #[arg(long, default_value_t = 30_000, value_parser = clap::value_parser!(u64).range(1..=300_000))]
    timeout_ms: u64,
    #[arg(long, default_value_t = 1_048_576)]
    max_output_bytes: usize,
}

#[derive(Args)]
struct ScheduleRevision {
    id: String,
    #[arg(long)]
    expected_revision: u64,
    #[command(flatten)]
    limits: FleetLimits,
}

#[derive(Subcommand)]
enum ScheduleCommands {
    /// Persist an owned definition and freeze its target/profile identities
    Create {
        #[arg(long, value_name = "FILE")]
        input: PathBuf,
        #[command(flatten)]
        limits: FleetLimits,
    },
    /// Replace an exact revision, increment generation and preserve unresolved work
    Update {
        #[arg(long, value_name = "FILE")]
        input: PathBuf,
        #[command(flatten)]
        limits: FleetLimits,
    },
    /// Stop future cadence admissions without cancelling active occurrences
    Pause(ScheduleRevision),
    /// Queue a deduplicated manual occurrence; explicit work performs dispatch
    RunNow {
        #[command(flatten)]
        revision: ScheduleRevision,
        #[arg(long)]
        request_id: String,
    },
    /// Explicitly request cancellation of already admitted work
    CancelActive(ScheduleRevision),
    /// Read one definition and its retained occurrence outcomes
    Status {
        id: String,
        #[command(flatten)]
        limits: FleetLimits,
    },
    /// List owned definitions and their current schedule state
    List {
        #[command(flatten)]
        limits: FleetLimits,
    },
    /// Preview due instants in local time and UTC under the pinned timezone rules
    Preview {
        id: String,
        #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u64).range(1..=32))]
        count: u64,
        #[command(flatten)]
        limits: FleetLimits,
    },
    /// Make one bounded admission/reconciliation pass; starts no background service
    Work {
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..=16))]
        max_occurrences: u64,
        #[command(flatten)]
        limits: FleetLimits,
    },
}

fn schedule_request(command: ScheduleCommands) -> Result<Request, Problem> {
    let (operation, inputs, limits) = match command {
        ScheduleCommands::Create { input, limits } => {
            ("schedule.create", parse_json(input_file(input)?)?, limits)
        }
        ScheduleCommands::Update { input, limits } => {
            ("schedule.update", parse_json(input_file(input)?)?, limits)
        }
        ScheduleCommands::Pause(args) => (
            "schedule.pause",
            json!({"id":args.id,"expected_revision":args.expected_revision}),
            args.limits,
        ),
        ScheduleCommands::RunNow {
            revision,
            request_id,
        } => (
            "schedule.run-now",
            json!({"id":revision.id,"expected_revision":revision.expected_revision,"request_id":request_id}),
            revision.limits,
        ),
        ScheduleCommands::CancelActive(args) => (
            "schedule.cancel-active",
            json!({"id":args.id,"expected_revision":args.expected_revision}),
            args.limits,
        ),
        ScheduleCommands::Status { id, limits } => ("schedule.status", json!({"id":id}), limits),
        ScheduleCommands::List { limits } => ("schedule.list", json!({}), limits),
        ScheduleCommands::Preview { id, count, limits } => {
            ("schedule.preview", json!({"id":id,"count":count}), limits)
        }
        ScheduleCommands::Work {
            max_occurrences,
            limits,
        } => (
            "schedule.work",
            json!({"max_occurrences":max_occurrences}),
            limits,
        ),
    };
    let mut request = Request::new(operation, inputs);
    request.limits.timeout_ms = limits.timeout_ms;
    request.limits.max_output_bytes = limits.max_output_bytes;
    Ok(request)
}

#[derive(Args)]
struct FleetLookup {
    request_id: String,
    #[command(flatten)]
    limits: FleetLimits,
}

#[derive(Subcommand)]
enum FleetCommands {
    /// Freeze selected targets and collect one explicitly identified observation request
    Collect {
        #[arg(long, value_name = "FILE")]
        input: PathBuf,
        #[command(flatten)]
        limits: FleetLimits,
    },
    /// Read the retained parent and every selected target's outcome
    Status(FleetLookup),
    /// Reconcile original remote request IDs without submitting again
    Resume(FleetLookup),
    /// Stop new admissions and request cancellation of original remote work
    Cancel(FleetLookup),
}

fn fleet_request(command: FleetCommands) -> Result<Request, Problem> {
    let (operation, inputs, limits) = match command {
        FleetCommands::Collect { input, limits } => {
            ("fleet.collect", parse_json(input_file(input)?)?, limits)
        }
        FleetCommands::Status(args) => (
            "fleet.status",
            json!({"request_id":args.request_id}),
            args.limits,
        ),
        FleetCommands::Resume(args) => (
            "fleet.resume",
            json!({"request_id":args.request_id}),
            args.limits,
        ),
        FleetCommands::Cancel(args) => (
            "fleet.cancel",
            json!({"request_id":args.request_id}),
            args.limits,
        ),
    };
    let mut request = Request::new(operation, inputs);
    request.limits.timeout_ms = limits.timeout_ms;
    request.limits.max_output_bytes = limits.max_output_bytes;
    Ok(request)
}

#[derive(Args)]
struct ChangeArgs {
    #[arg(long, value_name = "ID")]
    runner: String,
    #[arg(long, value_name = "FILE")]
    input: PathBuf,
    #[command(flatten)]
    limits: FleetLimits,
}

#[derive(Subcommand)]
enum ChangeCommands {
    /// Validate and retain an immutable plan without replacing the target
    Plan(ChangeArgs),
    /// Read the retained plan and original attempt without dispatching a change
    Status(ChangeArgs),
    /// Approve an exact plan digest using a distinct configured principal
    Approve(ChangeArgs),
    /// Consume the retained approval once and verify the replacement by readback
    Apply(ChangeArgs),
    /// Observe the original attempt without repeating replacement
    Reconcile(ChangeArgs),
}

fn change_request(command: ChangeCommands) -> Result<Request, Problem> {
    let (operation, args) = match command {
        ChangeCommands::Plan(args) => ("change.plan", args),
        ChangeCommands::Status(args) => ("change.status", args),
        ChangeCommands::Approve(args) => ("change.approve", args),
        ChangeCommands::Apply(args) => ("change.apply", args),
        ChangeCommands::Reconcile(args) => ("change.reconcile", args),
    };
    let mut request = Request::new(operation, parse_json(input_file(args.input)?)?);
    request.target.kind = "runner".into();
    request.target.id = args.runner;
    request.limits.timeout_ms = args.limits.timeout_ms;
    request.limits.max_output_bytes = args.limits.max_output_bytes;
    Ok(request)
}

#[derive(Subcommand)]
enum RunnerCommands {
    /// Read the authenticated runner's protocol, grants and effective limits
    Handshake { runner_id: String },
    /// Submit a typed request with an explicit stable request ID; never retry implicitly
    Submit {
        runner_id: String,
        #[arg(long, value_name = "FILE")]
        input: PathBuf,
    },
    /// Query the authenticated caller's original request ID, including after a lost reply
    Status {
        runner_id: String,
        request_id: String,
    },
    /// Reconnect to events for the authenticated caller's request
    Watch {
        runner_id: String,
        request_id: String,
        #[arg(long, default_value_t = 0)]
        after_sequence: u64,
    },
    /// Request target-side cancellation; acknowledgment does not prove termination
    Cancel {
        runner_id: String,
        request_id: String,
    },
    /// Start the explicit TLS runner using only its configured identities and target grants
    Serve {
        #[arg(long, value_name = "FILE")]
        server_config: PathBuf,
    },
}

fn runner_request(command: RunnerCommands) -> Result<Request, Problem> {
    let (operation, runner_id, inputs) = match command {
        RunnerCommands::Handshake { runner_id } => ("runner.handshake", runner_id, json!({})),
        RunnerCommands::Submit { runner_id, input } => {
            ("runner.submit", runner_id, parse_json(input_file(input)?)?)
        }
        RunnerCommands::Status {
            runner_id,
            request_id,
        } => ("runner.status", runner_id, json!({"request_id":request_id})),
        RunnerCommands::Watch {
            runner_id,
            request_id,
            after_sequence,
        } => (
            "runner.watch",
            runner_id,
            json!({"request_id":request_id,"after_sequence":after_sequence}),
        ),
        RunnerCommands::Cancel {
            runner_id,
            request_id,
        } => ("runner.cancel", runner_id, json!({"request_id":request_id})),
        RunnerCommands::Serve { .. } => unreachable!("runner server handled before normalization"),
    };
    let mut request = Request::new(operation, inputs);
    request.target.kind = "runner".into();
    request.target.id = runner_id;
    Ok(request)
}

fn runner_server_conflicts(cli: &Cli) -> bool {
    !matches!(cli.record.as_str(), "auto" | "always")
        || cli.fleet_inventory.is_some()
        || cli.fleet_state_dir.is_some()
        || cli.schedule_state_dir.is_some()
        || cli.runner_config.is_some()
        || cli.job_state_dir.is_some()
        || cli.job_max_runtime_ms.is_some()
        || cli.extension_state_dir.is_some()
        || cli.extension_policy.is_some()
        || cli.state_dir.is_some()
        || cli.runbook_state_dir.is_some()
        || cli.toil_state_dir.is_some()
        || cli.store_max_bytes.is_some()
        || cli.retention_days.is_some()
        || cli.diagnostics_config.is_some()
        || cli.context_source.is_some()
        || cli.context_max_age_days.is_some()
        || cli.read_policy.is_some()
        || cli.config.is_some()
        || cli.promtool.is_some()
}

#[derive(Subcommand)]
enum JobCommands {
    /// Persist one operation request; reuse its request ID to deduplicate admission
    Submit {
        #[arg(long, value_name = "FILE")]
        request: PathBuf,
        #[arg(long, default_value_t = 30_000, value_parser = clap::value_parser!(u64).range(1..=3_600_000))]
        max_runtime_ms: u64,
        #[arg(long, default_value_t = 32, value_parser = clap::value_parser!(u16).range(1..=256))]
        retain_events: u16,
    },
    /// Read the durable state and any committed terminal result
    Status { job_id: String },
    /// Read retained events strictly after the supplied sequence; repeat to reconnect
    Watch {
        job_id: String,
        #[arg(long, default_value_t = 0)]
        after_sequence: u64,
    },
    /// Persist cancellation intent; completion requires observed termination
    Cancel { job_id: String },
    /// Explicitly run one queued job; interrupted dispatch is never replayed
    Work { job_id: String },
}

fn job_request(command: JobCommands) -> Result<Request, Problem> {
    let (operation, inputs) = match command {
        JobCommands::Submit {
            request,
            max_runtime_ms,
            retain_events,
        } => (
            "job.submit",
            json!({
                "request": parse_request(input_file(request)?)?,
                "max_runtime_ms": max_runtime_ms,
                "retain_events": retain_events,
            }),
        ),
        JobCommands::Status { job_id } => ("job.status", json!({"job_id": job_id})),
        JobCommands::Watch {
            job_id,
            after_sequence,
        } => (
            "job.watch",
            json!({"job_id": job_id, "after_sequence": after_sequence}),
        ),
        JobCommands::Cancel { job_id } => ("job.cancel", json!({"job_id": job_id})),
        JobCommands::Work { job_id } => ("job.work", json!({"job_id": job_id})),
    };
    Ok(Request::new(operation, inputs))
}

#[derive(Subcommand)]
enum ExtensionCommands {
    /// Invoke a separately authorized installed external capability
    Run {
        operation: String,
        #[arg(long, value_name = "FILE")]
        input: PathBuf,
        #[arg(long, default_value_t = 1)]
        operation_version: u64,
        #[command(flatten)]
        limits: ExtensionLimits,
    },
    /// Verify and describe a local package without creating a store
    Inspect {
        package: PathBuf,
        #[arg(long)]
        expected_package_sha256: Option<String>,
        #[command(flatten)]
        limits: ExtensionLimits,
    },
    /// Install exactly the selected package bytes; execution remains disabled
    Install {
        package: PathBuf,
        #[arg(long)]
        expected_package_sha256: String,
        #[command(flatten)]
        limits: ExtensionLimits,
    },
    /// Verify and list installed packages without running them
    List {
        #[command(flatten)]
        limits: ExtensionLimits,
    },
}

#[derive(Args)]
struct ExtensionLimits {
    #[arg(long, default_value_t = 30_000)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 1_048_576)]
    max_output_bytes: usize,
}

fn extension_request(command: ExtensionCommands) -> Result<Request, Problem> {
    let (operation, inputs, limits) = match command {
        ExtensionCommands::Run {
            operation,
            input,
            operation_version,
            limits,
        } => {
            if !operation.starts_with("ext.") {
                return Err(Problem::invalid(
                    "invalid_extension_operation",
                    "Extension run requires a namespaced ext.* capability.",
                ));
            }
            let mut request = Request::new(&operation, parse_json(input_file(input)?)?);
            request.operation_version = operation_version;
            request.limits.timeout_ms = limits.timeout_ms;
            request.limits.max_output_bytes = limits.max_output_bytes;
            return Ok(request);
        }
        ExtensionCommands::Inspect {
            package,
            expected_package_sha256,
            limits,
        } => {
            let mut input = json!({"package":package});
            if let Some(pin) = expected_package_sha256 {
                input["expected_package_sha256"] = json!(pin);
            }
            ("extension.inspect", input, limits)
        }
        ExtensionCommands::Install {
            package,
            expected_package_sha256,
            limits,
        } => (
            "extension.install",
            json!({"package":package,"expected_package_sha256":expected_package_sha256}),
            limits,
        ),
        ExtensionCommands::List { limits } => ("extension.list", json!({}), limits),
    };
    let mut request = Request::new(operation, inputs);
    request.limits.timeout_ms = limits.timeout_ms;
    request.limits.max_output_bytes = limits.max_output_bytes;
    Ok(request)
}

#[derive(Args)]
struct ToilInput {
    #[arg(
        long,
        value_name = "FILE",
        help = "Strict typed metadata, capture or summary selection JSON"
    )]
    input: PathBuf,
    #[arg(long, default_value_t = 30_000)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 1_048_576)]
    max_output_bytes: usize,
}

fn toil_request(operation: &str, options: ToilInput) -> Result<Request, Problem> {
    let mut request = Request::new(operation, parse_json(input_file(options.input)?)?);
    request.limits.timeout_ms = options.timeout_ms;
    request.limits.max_output_bytes = options.max_output_bytes;
    Ok(request)
}

#[derive(Subcommand)]
enum ReportCommands {
    /// Create Markdown or JSON from explicit bundle references without live access
    Create {
        #[arg(
            long,
            value_name = "FILE",
            help = "Strict JSON selection: bundles, format, optional title and pending proposals"
        )]
        input: PathBuf,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
        #[arg(
            long,
            default_value_t = 1_048_576,
            help = "Rendered document byte budget, at most 1MiB effective"
        )]
        max_output_bytes: usize,
    },
}

#[derive(Subcommand)]
enum EvidenceCommands {
    /// Inspect installed packs without accessing a store, credential or network
    Packs {
        #[command(subcommand)]
        command: EvidencePackCommands,
    },
    /// Resolve context, execute the installed pack and retain sanitized receipts
    Collect {
        #[command(flatten)]
        target: GrafanaTarget,
        #[arg(
            long,
            value_name = "FILE",
            help = "Strict collection selection, pack, window, metrics/logs parameters and failure policy"
        )]
        input: PathBuf,
    },
    /// Compare retained metric samples and log entries without contacting their sources
    Compare {
        /// Generated run ID for the earlier observation
        before: String,
        /// Generated run ID for the later observation
        after: String,
        #[arg(long, value_parser = ["exact-window-v1", "window-offset-v1"])]
        normalization: String,
        #[arg(long)]
        before_manifest_sha256: Option<String>,
        #[arg(long)]
        after_manifest_sha256: Option<String>,
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u64).range(1..=1000))]
        max_items: u64,
        #[arg(
            long,
            default_value_t = 1_048_576,
            help = "Encoded difference detail budget; at most 1MiB is effective"
        )]
        max_output_bytes: usize,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
    },
    List,
    /// Inspect verified historical receipts with network and command execution disabled
    Open(EvidenceOffline),
    /// Analyze retained observations without contacting their original sources
    Replay(EvidenceOffline),
    Show(EvidenceReference),
    Protect(EvidenceReference),
    Unprotect(EvidenceReference),
    Delete(EvidenceReference),
    /// Expire sufficiently old, unprotected sealed runs; no younger eviction
    Prune,
    /// Reconcile interrupted writes without repeating observations
    Recover,
}

#[derive(Subcommand)]
enum EvidencePackCommands {
    List,
    Describe {
        id: String,
        #[arg(long, default_value_t = 1)]
        version: u64,
    },
}

#[derive(Args)]
struct EvidenceReference {
    /// Generated run ID in the selected store, never an artifact path
    id: String,
    #[arg(
        long,
        value_name = "SHA256",
        help = "Expected manifest digest, sha256: followed by 64 lowercase hexadecimal digits"
    )]
    expected_manifest_sha256: Option<String>,
}

#[derive(Args)]
struct EvidenceOffline {
    #[command(flatten)]
    reference: EvidenceReference,
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u64).range(1..=1000))]
    max_items: u64,
    #[arg(
        long,
        default_value_t = 1_048_576,
        help = "Encoded retained-detail budget; at most 1MiB is effective"
    )]
    max_output_bytes: usize,
    #[arg(long, default_value_t = 30_000)]
    timeout_ms: u64,
}

fn evidence_offline(operation: &str, options: EvidenceOffline) -> Request {
    let mut request = evidence_reference(operation, options.reference);
    request.limits.max_items = Some(options.max_items);
    request.limits.max_output_bytes = options.max_output_bytes;
    request.limits.timeout_ms = options.timeout_ms;
    request
}

fn evidence_reference(operation: &str, reference: EvidenceReference) -> Request {
    let mut inputs = json!({"id":reference.id});
    if let Some(digest) = reference.expected_manifest_sha256 {
        inputs["expected_manifest_sha256"] = json!(digest);
    }
    Request::new(operation, inputs)
}

#[derive(Subcommand)]
enum McpCommands {
    /// Stdout is protocol-only. Close stdin to stop; history and retries are not provided.
    Serve {
        #[arg(long, default_value = "stdio", value_parser = ["stdio"])]
        transport: String,
        #[arg(
            long,
            value_name = "OPERATION",
            help = "Repeat process.exec, command.inspect, task.run, grafana.dashboard.get, grafana.query, context.resolve, evidence.list, evidence.open, evidence.compare or report.create; empty grants expose no tools"
        )]
        allow: Vec<String>,
        #[arg(
            long,
            value_name = "ROOT_ID",
            help = "Repeat trusted --read-policy root IDs for command grants"
        )]
        root: Vec<String>,
        #[arg(
            long,
            value_name = "TARGET_ID",
            help = "Repeat trusted --config target IDs for Grafana grants"
        )]
        target: Vec<String>,
    },
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
            help = "Repeat process.exec, command.inspect, task.run or diagnostics.run; empty grants expose no checks"
        )]
        allow: Vec<String>,
        #[arg(
            long,
            value_name = "ROOT_ID",
            help = "Repeat trusted policy root IDs; commands also require --read-policy"
        )]
        root: Vec<String>,
        #[arg(
            long,
            value_name = "TARGET_ID",
            help = "Repeat unique trusted diagnostic aliases; requires diagnostics.run and --diagnostics-config"
        )]
        target: Vec<String>,
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
enum QueriesCommands {
    /// List installed definitions without reading configuration or contacting a server
    List,
    /// Inspect one installed definition and its exact SHA-256 identity
    Describe {
        id: String,
        #[arg(long)]
        version: u64,
    },
    /// Run an installed query using explicit target, datasource, parameters and time window
    Run {
        id: String,
        #[arg(long)]
        version: u64,
        #[command(flatten)]
        target: GrafanaTarget,
        #[arg(long)]
        datasource: String,
        #[arg(
            long,
            value_name = "FILE",
            help = "Strict JSON object containing the literal job string"
        )]
        parameters: PathBuf,
        #[arg(
            long,
            value_name = "FILE",
            help = "Explicit absolute from/to or relative lookback_seconds JSON window"
        )]
        window: PathBuf,
        #[arg(long, value_name = "SHA256")]
        expected_definition_sha256: Option<String>,
    },
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
        Commands::Change { command } => change_request(command)?,
        Commands::Fleet { command } => fleet_request(command)?,
        Commands::Schedule { command } => schedule_request(command)?,
        Commands::Runner { command } => runner_request(command)?,
        Commands::Job { command } => job_request(command)?,
        Commands::Extension { command } => extension_request(command)?,
        Commands::Config {
            command:
                ConfigCommands::Check {
                    input,
                    timeout_ms,
                    max_output_bytes,
                    ..
                },
        } => {
            let mut request = Request::new("config.check", parse_json(input_file(input)?)?);
            request.limits.timeout_ms = timeout_ms;
            request.limits.max_output_bytes = max_output_bytes;
            request
        }
        Commands::Toil { command } => match command {
            ToilCommands::Configure { enabled, keep_days } => Request::new(
                "toil.configure",
                json!({"enabled":enabled,"retention_days":keep_days}),
            ),
            ToilCommands::Record(options) => toil_request("toil.record", options)?,
            ToilCommands::Capture(options) => toil_request("toil.capture", options)?,
            ToilCommands::Annotate(options) => toil_request("toil.annotate", options)?,
            ToilCommands::Summary(options) => toil_request("toil.summary", options)?,
            ToilCommands::Delete { id } => Request::new("toil.delete", json!({"id":id})),
            ToilCommands::Prune => Request::new("toil.prune", json!({})),
        },
        Commands::Report { command } => match command {
            ReportCommands::Create {
                input,
                timeout_ms,
                max_output_bytes,
            } => {
                let mut request = Request::new("report.create", parse_json(input_file(input)?)?);
                request.limits.timeout_ms = timeout_ms;
                request.limits.max_output_bytes = max_output_bytes;
                request
            }
        },
        Commands::Runbook { command } => match command {
            RunbookCommands::List => Request::new("runbook.list", json!({})),
            RunbookCommands::Show { id, version } => {
                Request::new("runbook.show", json!({"id":id,"version":version}))
            }
            RunbookCommands::Status { run_id } => {
                Request::new("runbook.status", json!({"run_id":run_id}))
            }
            RunbookCommands::Run {
                id,
                version,
                input,
                expected_definition_sha256,
                fresh_from,
                fresh_from_revision,
                timeout_ms,
                max_output_bytes,
            } => {
                let mut inputs =
                    json!({"id":id,"version":version,"input":parse_json(input_file(input)?)?});
                if let Some(pin) = expected_definition_sha256 {
                    inputs["expected_definition_sha256"] = json!(pin);
                }
                if let Some(id) = fresh_from {
                    inputs["fresh_from"] = json!({"id":id,"expected_revision":fresh_from_revision.expect("paired CLI option")});
                }
                let mut request = Request::new("runbook.run", inputs);
                request.limits.timeout_ms = timeout_ms;
                request.limits.max_output_bytes = max_output_bytes;
                request
            }
            RunbookCommands::Resume {
                run_id,
                expected_revision,
                input,
                acknowledge,
                timeout_ms,
                max_output_bytes,
            } => {
                let mut inputs = json!({"run_id":run_id,"expected_revision":expected_revision,"input":parse_json(input_file(input)?)?});
                if let Some(step) = acknowledge {
                    inputs["acknowledge"] = json!(step);
                }
                let mut request = Request::new("runbook.resume", inputs);
                request.limits.timeout_ms = timeout_ms;
                request.limits.max_output_bytes = max_output_bytes;
                request
            }
        },
        Commands::Evidence { command } => match command {
            EvidenceCommands::Packs {
                command: EvidencePackCommands::List,
            } => Request::new("evidence.packs.list", json!({})),
            EvidenceCommands::Packs {
                command: EvidencePackCommands::Describe { id, version },
            } => Request::new(
                "evidence.packs.describe",
                json!({"id":id,"version":version}),
            ),
            EvidenceCommands::Collect { target, input } => {
                grafana_request("evidence.collect", target, parse_json(input_file(input)?)?)
            }
            EvidenceCommands::Compare {
                before,
                after,
                normalization,
                before_manifest_sha256,
                after_manifest_sha256,
                max_items,
                max_output_bytes,
                timeout_ms,
            } => {
                let mut inputs = json!({"before":{"id":before},"after":{"id":after},"normalization":normalization});
                if let Some(digest) = before_manifest_sha256 {
                    inputs["before"]["expected_manifest_sha256"] = json!(digest);
                }
                if let Some(digest) = after_manifest_sha256 {
                    inputs["after"]["expected_manifest_sha256"] = json!(digest);
                }
                let mut request = Request::new("evidence.compare", inputs);
                request.limits.max_items = Some(max_items);
                request.limits.max_output_bytes = max_output_bytes;
                request.limits.timeout_ms = timeout_ms;
                request
            }
            EvidenceCommands::List => Request::new("evidence.list", json!({})),
            EvidenceCommands::Open(options) => evidence_offline("evidence.open", options),
            EvidenceCommands::Replay(options) => evidence_offline("evidence.replay", options),
            EvidenceCommands::Show(reference) => evidence_reference("evidence.show", reference),
            EvidenceCommands::Protect(reference) => {
                evidence_reference("evidence.protect", reference)
            }
            EvidenceCommands::Unprotect(reference) => {
                evidence_reference("evidence.unprotect", reference)
            }
            EvidenceCommands::Delete(reference) => evidence_reference("evidence.delete", reference),
            EvidenceCommands::Prune => Request::new("evidence.prune", json!({})),
            EvidenceCommands::Recover => Request::new("evidence.recover", json!({})),
        },
        Commands::Diagnostics {
            command: DiagnosticsCommands::List,
        } => Request::new("diagnostics.list", json!({})),
        Commands::Diagnostics {
            command: DiagnosticsCommands::Describe { id },
        } => Request::new("diagnostics.describe", json!({"id":id})),
        Commands::Diagnose {
            target,
            pack,
            input,
            timeout,
            max_output_bytes,
            concurrency,
        } => {
            let input = parse_json(input_file(input)?)?;
            let mut request = Request::new(
                "diagnostics.run",
                json!({"id":pack,"version":1,"input":input}),
            );
            request.target.kind = "connection".into();
            request.target.id = target;
            request.limits.timeout_ms = timeout;
            request.limits.max_output_bytes = max_output_bytes;
            request.limits.concurrency = Some(concurrency);
            request
        }
        Commands::Context {
            command:
                ContextCommands::Resolve {
                    service,
                    env,
                    team,
                    deployment,
                },
        } => {
            let mut inputs = json!({"service":service,"environment":env});
            if let Some(team) = team {
                inputs["team"] = json!(team);
            }
            if let Some(deployment) = deployment {
                inputs["deployment"] = json!(deployment);
            }
            Request::new("context.resolve", inputs)
        }
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
        Commands::Queries {
            command: QueriesCommands::List,
        } => Request::new("queries.list", json!({})),
        Commands::Queries {
            command: QueriesCommands::Describe { id, version },
        } => Request::new("queries.describe", json!({"id":id,"version":version})),
        Commands::Queries {
            command:
                QueriesCommands::Run {
                    id,
                    version,
                    target,
                    datasource,
                    parameters,
                    window,
                    expected_definition_sha256,
                },
        } => {
            let parameters = parse_json(input_file(parameters)?)?;
            let window = parse_json(input_file(window)?)?;
            let mut input = json!({"id":id,"version":version,"datasource":datasource,"parameters":parameters,"window":window});
            if let Some(digest) = expected_definition_sha256 {
                input["expected_definition_sha256"] = json!(digest);
            }
            grafana_request("query.run", target, input)
        }
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
        Commands::Ui { .. } | Commands::Mcp { .. } => {
            unreachable!("adapters handled before operation normalization")
        }
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
    let arguments: Vec<_> = std::env::args_os().take_while(|arg| arg != "--").collect();
    let mcp_invocation = arguments
        .windows(2)
        .any(|pair| pair[0] == "mcp" && pair[1] == "serve");
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
            if mcp_invocation {
                workbench_mcp::invalid_startup();
                return 2;
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
    if let Commands::Mcp {
        command:
            McpCommands::Serve {
                allow,
                root,
                target,
                ..
            },
    } = &cli.command
    {
        if cli.json
            || cli.record != "never"
            || cli.runbook_state_dir.is_some()
            || cli.toil_state_dir.is_some()
            || cli.extension_state_dir.is_some()
            || cli.extension_policy.is_some()
            || cli.job_state_dir.is_some()
            || cli.runner_config.is_some()
            || cli.fleet_inventory.is_some()
            || cli.fleet_state_dir.is_some()
            || cli.schedule_state_dir.is_some()
            || cli.store_max_bytes.is_some()
            || cli.retention_days.is_some()
            || ((cli.context_source.is_some() || cli.context_max_age_days.is_some())
                && !allow.iter().any(|operation| operation == "context.resolve"))
            || (cli.state_dir.is_some()
                && !allow.iter().any(|operation| {
                    matches!(
                        operation.as_str(),
                        "evidence.list" | "evidence.open" | "evidence.compare" | "report.create"
                    )
                }))
            || cli.diagnostics_config.is_some()
            || cli.promtool.is_some()
        {
            workbench_mcp::invalid_startup();
            return 2;
        }
        return workbench_mcp::serve(workbench_mcp::Options {
            allow: allow.clone(),
            roots: root.clone(),
            targets: target.clone(),
            read_policy: cli.read_policy.clone(),
            config: cli.config.clone(),
            context_source: cli.context_source.clone(),
            context_max_age_days: cli.context_max_age_days,
            evidence_store: cli.state_dir.clone().map(EvidenceStoreOptions::new),
        });
    }
    if let Commands::Ui {
        command:
            UiCommands::Serve {
                port,
                allow,
                root,
                target,
            },
    } = &cli.command
    {
        if cli.json
            || cli.record != "never"
            || cli.runbook_state_dir.is_some()
            || cli.toil_state_dir.is_some()
            || cli.extension_state_dir.is_some()
            || cli.extension_policy.is_some()
            || cli.job_state_dir.is_some()
            || cli.runner_config.is_some()
            || cli.fleet_inventory.is_some()
            || cli.fleet_state_dir.is_some()
            || cli.schedule_state_dir.is_some()
            || cli.store_max_bytes.is_some()
            || cli.retention_days.is_some()
            || cli.config.is_some()
            || cli.promtool.is_some()
            || ((cli.context_source.is_some() || cli.context_max_age_days.is_some())
                && !allow.iter().any(|operation| operation == "context.resolve"))
            || (cli.state_dir.is_some()
                && !allow.iter().any(|operation| {
                    matches!(
                        operation.as_str(),
                        "evidence.list" | "evidence.open" | "evidence.compare" | "report.create"
                    )
                }))
            || (cli.diagnostics_config.is_some()
                && !allow.iter().any(|operation| operation == "diagnostics.run"))
        {
            eprintln!(
                "ui serve does not accept --json, --config, --promtool, runbook/toil/extension store options, extension policy, ungranted source/store/diagnostic configuration or recording modes other than never"
            );
            return 2;
        }
        return match workbench_ui::serve(workbench_ui::Options {
            port: *port,
            allow: allow.clone(),
            roots: root.clone(),
            read_policy: cli.read_policy.clone(),
            diagnostics_config: cli.diagnostics_config.clone(),
            targets: target.clone(),
            context_source: cli.context_source.clone(),
            context_max_age_days: cli.context_max_age_days,
            evidence_store: cli.state_dir.clone().map(EvidenceStoreOptions::new),
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
        fleet_inventory_path: cli.fleet_inventory.clone(),
        fleet_state_dir: cli.fleet_state_dir.clone(),
        schedule_state_dir: cli.schedule_state_dir.clone(),
        runner_config_path: cli.runner_config.clone(),
        job_state_dir: cli.job_state_dir.clone(),
        job_max_runtime_ms: cli.job_max_runtime_ms,
        extension_state_dir: cli.extension_state_dir.clone(),
        extension_policy_path: cli.extension_policy.clone(),
        toil_state_dir: cli.toil_state_dir.clone(),
        runbook_state_dir: cli.runbook_state_dir.clone(),
        evidence_store: cli.state_dir.clone().map(|directory| {
            let mut options = EvidenceStoreOptions::new(directory);
            if let Some(max_bytes) = cli.store_max_bytes {
                options.max_bytes = max_bytes;
            }
            if let Some(retention_days) = cli.retention_days {
                options.retention_days = retention_days;
            }
            options
        }),
        diagnostics_config_path: cli.diagnostics_config.clone(),
        context_source_path: cli.context_source.clone(),
        context_max_age_days: cli.context_max_age_days,
        config_path: cli.config.clone(),
        promtool_path: cli.promtool.clone(),
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
        } | Commands::Config {
            command: ConfigCommands::Check {
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
    if let Commands::Runner {
        command: RunnerCommands::Serve { server_config },
    } = &cli.command
    {
        if runner_server_conflicts(&cli) {
            return deliver(
                OperationResult::rejection(
                    &Request::new("runner.serve", json!({})),
                    Problem::invalid(
                        "invalid_runner_startup",
                        "Runner serving requires recording consent and its server configuration only; other operator startup overrides are not inherited.",
                    ),
                ),
                json,
                &control,
            );
        }
        let served = serve_runner(server_config, &control, |ready| {
            let bytes = if json {
                let mut bytes = serde_json::to_vec(&ready).expect("runner readiness");
                bytes.push(b'\n');
                bytes
            } else {
                format!(
                    "Runner {} listening at {}\n",
                    ready["runner_id"].as_str().unwrap_or("unknown"),
                    ready["address"].as_str().unwrap_or("unknown")
                )
                .into_bytes()
            };
            output::write(&bytes, &control.signal).map_err(|_| ())
        });
        return match served {
            Ok(()) => match control.signal.load(Ordering::Relaxed) {
                2 => 130,
                15 => 143,
                _ => 0,
            },
            Err(error) => {
                let mut result = OperationResult::rejection(
                    &Request::new("runner.serve", json!({})),
                    Problem::invalid(
                        error.code,
                        "The configured runner could not start or continue; inspect its explicit configuration and retained job state.",
                    ),
                );
                result.record_mode = if cli.record == "always" {
                    "always"
                } else {
                    "auto"
                };
                if error.effects {
                    result.execution.status = workbench_core::result::Status::Unknown;
                    result.effect_outcome = "unknown".into();
                }
                deliver(result, json, &control)
            }
        };
    }
    let explicit_job_worker = matches!(
        &cli.command,
        Commands::Job {
            command: JobCommands::Work { .. }
        }
    );
    let explicit_schedule_worker = matches!(
        &cli.command,
        Commands::Schedule {
            command: ScheduleCommands::Work { .. }
        }
    );
    let result = match normalize(cli) {
        Ok(request) if explicit_job_worker => work_job(request, &control, &context),
        Ok(request) if explicit_schedule_worker => work_schedule(request, &control, &context),
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

fn note_delivery_signal(result: &mut OperationResult, signal: usize) {
    if signal != 0 {
        // A collection payload may already be sealed. Delivery is a later boundary:
        // preserve its observations and capture state, and report the signal separately.
        let field = if result.operation == "evidence.collect" && result.data.get("bundle").is_some()
        {
            "delivery_cancellation_signal"
        } else {
            "cancellation_signal"
        };
        result.data[field] = json!(signal);
    }
}

fn deliver(mut result: OperationResult, json: bool, control: &RunControl) -> u8 {
    note_delivery_signal(&mut result, control.signal.load(Ordering::Relaxed));
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

#[cfg(test)]
mod delivery_tests {
    use super::*;

    #[test]
    fn a_late_delivery_signal_preserves_the_sealed_collection_payload() {
        for signal in [2, 15] {
            let mut result = OperationResult::new(&Request::new("evidence.collect", json!({})));
            let sealed_payload = json!({"capture":{"state":"complete","expected_steps":2,"retained_receipts":2},"cancellation_signal":null});
            let sealed_bytes = serde_json::to_vec(&sealed_payload).unwrap();
            result.data = sealed_payload;
            result.data["bundle"] =
                json!({"id":"run-1-ab-c","digest":format!("sha256:{}", "a".repeat(64))});
            result.data["delivery_cancellation_signal"] = serde_json::Value::Null;
            // Same hook that delivery invokes when a signal arrives after core finalization.
            note_delivery_signal(&mut result, signal);
            assert_eq!(result.data["delivery_cancellation_signal"], signal);
            assert!(result.data["cancellation_signal"].is_null());
            assert_eq!(result.data["capture"]["state"], "complete");
            result.data.as_object_mut().unwrap().remove("bundle");
            result
                .data
                .as_object_mut()
                .unwrap()
                .remove("delivery_cancellation_signal");
            assert_eq!(serde_json::to_vec(&result.data).unwrap(), sealed_bytes);
        }
    }
}
