#![forbid(unsafe_code)]
use clap::{Args, CommandFactory, Parser, Subcommand};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

mod collect;
mod webui;
mod webui_process;

pub(crate) const REMOTE: &str = "IFS= read -r token\nif [ -n \"$token\" ]; then\n    exec influxdb3 \"$@\" --token \"$token\"\nfi\nexec influxdb3 \"$@\"\n";
const COMMANDS: &[(&str, &[&str])] = &[
    ("nodes", &["show", "nodes", "--format", "json"]),
    (
        "license-info",
        &["show", "license", "info", "--format", "json"],
    ),
    ("databases", &["show", "databases", "--format", "json"]),
];

#[derive(Parser)]
#[command(
    about = "Independent kubectl plugin for use with InfluxDB® 3 Enterprise databases",
    after_help = "InfluxDB® is a trademark registered by InfluxData, which is not affiliated with, and does not endorse, this project."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Collect nodes, license info, and databases as JSON
    Support(Support),
    /// Serve a temporary local web console for the Kubernetes environment
    Webui(webui::Webui),
}
#[derive(Args, Default)]
struct Common {
    #[arg(short = 'n', long)]
    namespace: Option<String>,
    #[arg(long)]
    context: Option<String>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    timeout: Option<f64>,
}
#[derive(Args)]
struct Support {
    #[arg(long, env = "INFLUXDB3_POD")]
    pod: Option<String>,
    #[arg(short = 'c', long)]
    container: Option<String>,
    #[command(flatten)]
    common: Common,
    #[command(subcommand)]
    command: Option<SupportCommand>,
}
#[derive(Subcommand)]
enum SupportCommand {
    /// Collect logs from all pods matching a role label
    Logs(Logs),
    /// Print `influxdb3 show license info` and `influxdb3 show nodes` as JSON
    Info(Info),
}

#[derive(Args)]
struct Info {
    #[arg(long, env = "INFLUXDB3_POD")]
    pod: Option<String>,
    #[arg(short = 'c', long)]
    container: Option<String>,
    #[command(flatten)]
    common: Common,
}

enum Action {
    Json,
    Logs(Logs),
    Info,
}
pub(crate) const ROLES: [&str; 4] = ["querier", "processor", "ingester", "compactor"];
#[derive(Args)]
struct Logs {
    #[arg(default_value = "all", value_parser = ["all", "querier", "processor", "ingester", "compactor"])]
    role: String,
    #[command(flatten)]
    common: Common,
    #[arg(long)]
    selector: Option<String>,
    #[arg(long, default_value_t = 10000, allow_negative_numbers = true)]
    tail: i64,
}
impl Common {
    fn override_with(&mut self, other: Common) {
        if other.namespace.is_some() {
            self.namespace = other.namespace;
        }
        if other.context.is_some() {
            self.context = other.context;
        }
        if other.output.is_some() {
            self.output = other.output;
        }
        if other.timeout.is_some() {
            self.timeout = other.timeout;
        }
    }
}

fn usage(message: &str) -> ! {
    Cli::command()
        .error(clap::error::ErrorKind::ValueValidation, message)
        .exit()
}
fn main() {
    let cli = Cli::parse();
    let mut args = match cli.command {
        Commands::Webui(args) => {
            if let Err(error) = webui::run(args) {
                eprintln!("db3: {error}");
                std::process::exit(1);
            }
            return;
        }
        Commands::Support(args) => args,
    };
    let action = match args.command.take() {
        Some(SupportCommand::Logs(mut logs)) => {
            args.common.override_with(std::mem::take(&mut logs.common));
            if logs.tail < -1 {
                usage("--tail must be -1 or nonnegative");
            }
            Action::Logs(logs)
        }
        Some(SupportCommand::Info(mut info)) => {
            args.common.override_with(std::mem::take(&mut info.common));
            if info.pod.is_some() {
                args.pod = info.pod;
            }
            if info.container.is_some() {
                args.container = info.container;
            }
            Action::Info
        }
        None => Action::Json,
    };
    let timeout = args.common.timeout.unwrap_or(60.0);
    if !timeout.is_finite() || timeout <= 0.0 {
        usage("--timeout must be a finite positive number");
    }
    let token = std::env::var("INFLUXDB3_TOKEN").unwrap_or_default();
    if token.contains(['\r', '\n']) {
        usage("INFLUXDB3_TOKEN must be a single line");
    }
    let result = match action {
        Action::Logs(logs) => collect_logs(&args.common, &logs, &token, timeout),
        Action::Info => {
            if args.common.output.is_some() {
                usage("support info writes to stdout; --output is not used");
            }
            print_info(&args, &token, timeout)
        }
        Action::Json => {
            if args.pod.as_deref().is_none_or(str::is_empty) {
                usage("support requires --pod POD or INFLUXDB3_POD");
            }
            collect_json(&args, &token, timeout)
        }
    };
    match result {
        Ok(complete) => std::process::exit(if complete { 0 } else { 1 }),
        Err(error) => {
            eprintln!("db3: {error}");
            std::process::exit(1);
        }
    }
}

fn output_dir(common: &Common, logs: bool) -> io::Result<PathBuf> {
    if let Some(path) = &common.output {
        DirBuilder::new().mode(0o700).create(path)?;
        fs::canonicalize(path)
    } else {
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let prefix = format!("db3-support-{}{stamp}-", if logs { "logs-" } else { "" });
        let dir = tempfile::Builder::new()
            .prefix(&prefix)
            .tempdir_in(std::env::current_dir()?)?;
        Ok(dir.keep())
    }
}
fn save(path: &Path, text: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())
}
pub(crate) fn redact(text: &str, token: &str) -> String {
    if token.is_empty() {
        return text.to_owned();
    }
    let encoded = json!(token).to_string();
    let escaped = &encoded[1..encoded.len() - 1];
    let mut ascii = String::new();
    for character in escaped.chars() {
        if character.is_ascii() && character != '\u{7f}' {
            ascii.push(character);
        } else {
            let mut units = [0; 2];
            for unit in character.encode_utf16(&mut units) {
                use std::fmt::Write as _;
                let _ = write!(ascii, "\\u{unit:04x}");
            }
        }
    }
    let mut secrets = [token, escaped, ascii.as_str()];
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut result = text.to_owned();
    for secret in secrets {
        result = result.replace(secret, "[REDACTED]");
    }
    result
}
fn prefix(common: &Common) -> Vec<String> {
    let mut args = Vec::new();
    for (flag, value) in [
        ("--context", &common.context),
        ("--namespace", &common.namespace),
    ] {
        if let Some(value) = value.as_ref().filter(|s| !s.is_empty()) {
            args.extend([flag.to_owned(), value.to_owned()]);
        }
    }
    args
}
pub(crate) struct Captured {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) returncode: Option<i32>,
    pub(crate) error: Option<String>,
}
fn decoded(file: &mut fs::File) -> io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n"))
}
/// Private file-backed captures avoid pipe deadlocks and preserve partial timeout output.
pub(crate) fn capture(
    program: &str,
    args: &[String],
    token: Option<&str>,
    timeout: f64,
) -> io::Result<Captured> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    // An anonymous private input file avoids a pipe write blocking before the
    // timeout loop starts, even for very large tokens or a child not reading stdin.
    // This file is never part of the bundle and is removed on close.
    let input = if let Some(token) = token {
        let mut file = tempfile::tempfile()?;
        file.write_all(token.as_bytes())?;
        file.write_all(b"\n")?;
        file.seek(SeekFrom::Start(0))?;
        Stdio::from(file)
    } else {
        Stdio::null()
    };
    let started = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .stdin(input);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return Ok(Captured {
                stdout: String::new(),
                stderr: String::new(),
                returncode: None,
                error: Some(format!(
                    "could not execute {program}; check installation and PATH"
                )),
            });
        }
    };
    let (returncode, error) = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let code = status.code().or_else(|| status.signal().map(|s| -s));
                let error = if status.success() {
                    None
                } else {
                    Some(format!(
                        "command exited with status {}",
                        code.map_or_else(|| "unknown".to_owned(), |n| n.to_string())
                    ))
                };
                break (code, error);
            }
            Ok(None) if started.elapsed().as_secs_f64() < timeout => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Ok(None) => {
                // Always reap the child, including the race where it exited before kill.
                let _ = child.kill();
                child.wait()?;
                break (None, Some(format!("timed out after {timeout} seconds")));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    };
    Ok(Captured {
        stdout: decoded(&mut stdout)?,
        stderr: decoded(&mut stderr)?,
        returncode,
        error,
    })
}
#[cfg(test)]
mod unit_tests;

fn pod_names(text: &str) -> Result<Vec<String>, ()> {
    let value: Value = serde_json::from_str(text).map_err(|_| ())?;
    let items = value.get("items").and_then(Value::as_array).ok_or(())?;
    let mut names = std::collections::BTreeSet::new();
    for pod in items {
        let name = pod
            .get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(Value::as_str)
            .ok_or(())?;
        if name.is_empty()
            || name.contains('/')
            || name == "."
            || name == ".."
            || name.contains('\0')
        {
            return Err(());
        }
        names.insert(name.to_owned());
    }
    Ok(names.into_iter().collect())
}
fn collect_logs(
    common: &Common,
    logs: &Logs,
    token: &str,
    timeout: f64,
) -> Result<bool, Box<dyn std::error::Error>> {
    let output = output_dir(common, true)?;
    let roles: Vec<&str> = if logs.role == "all" {
        ROLES.to_vec()
    } else {
        vec![logs.role.as_str()]
    };
    let default_selector = if logs.role == "all" {
        format!("app.kubernetes.io/component in ({})", ROLES.join(","))
    } else {
        format!("app.kubernetes.io/component={}", logs.role)
    };
    let selector = logs
        .selector
        .as_ref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&default_selector);
    let mut argv = prefix(common);
    argv.extend(["get", "pods", "-l", selector, "-o", "json"].map(str::to_owned));
    let mut discovery = capture("kubectl", &argv, None, timeout)?;
    save(
        &output.join("discovery.stderr.txt"),
        &redact(&discovery.stderr, token),
    )?;
    let mut names = Vec::new();
    if discovery.error.is_none() {
        match pod_names(&discovery.stdout) {
            Ok(found) => names = found,
            Err(()) => {
                discovery.error = Some("pod discovery returned an invalid JSON pod list".to_owned())
            }
        }
    }
    if discovery.error.is_none() && names.is_empty() {
        discovery.error = Some(format!(
            "no pods matched selector {selector:?}; check namespace, context, and --selector"
        ));
    }
    if let Some(error) = &discovery.error {
        eprintln!("db3: pod discovery: {error}; see discovery.stderr.txt");
    }
    let mut results = Vec::new();
    for name in names {
        let mut argv = prefix(common);
        argv.extend(
            [
                "logs",
                &name,
                "--all-containers=true",
                "--prefix=true",
                "--timestamps=true",
            ]
            .map(str::to_owned),
        );
        argv.push(format!("--tail={}", logs.tail));
        let captured = capture("kubectl", &argv, None, timeout)?;
        let stdout = format!("{name}.log");
        let stderr = format!("{name}.stderr.txt");
        save(&output.join(&stdout), &redact(&captured.stdout, token))?;
        save(&output.join(&stderr), &redact(&captured.stderr, token))?;
        if let Some(error) = &captured.error {
            eprintln!("db3: {name}: {error}");
        }
        results.push(
            json!({"pod": name, "ok": captured.error.is_none(), "error": captured.error,
            "returncode": captured.returncode, "stdout": stdout, "stderr": stderr}),
        );
    }
    let complete =
        discovery.error.is_none() && !results.is_empty() && results.iter().all(|r| r["ok"] == true);
    let manifest = json!({"complete": complete, "role": logs.role, "requested_roles": roles, "selector": selector,
        "namespace": common.namespace, "context": common.context, "tail": logs.tail, "error": discovery.error,
        "discovery_returncode": discovery.returncode, "results": results});
    save(
        &output.join("manifest.json"),
        &(serde_json::to_string_pretty(&manifest)? + "\n"),
    )?;
    println!("Support bundle: {}", output.display());
    Ok(complete)
}

fn collect_json(
    args: &Support,
    token: &str,
    timeout: f64,
) -> Result<bool, Box<dyn std::error::Error>> {
    let output = output_dir(&args.common, false)?;
    let mut base = prefix(&args.common);
    base.extend([
        "exec".to_owned(),
        "-i".to_owned(),
        args.pod.as_deref().unwrap_or_default().to_owned(),
    ]);
    if let Some(container) = args.container.as_ref().filter(|s| !s.is_empty()) {
        base.extend(["-c".to_owned(), container.to_owned()]);
    }
    base.extend(["--", "sh", "-c", REMOTE, "db3-support"].map(str::to_owned));
    let mut results = Vec::new();
    for (name, command) in COMMANDS {
        let argv: Vec<String> = base
            .iter()
            .map(String::as_str)
            .chain(command.iter().copied())
            .map(str::to_owned)
            .collect();
        let mut captured = capture("kubectl", &argv, Some(token), timeout)?;
        if captured.error.is_none() && serde_json::from_str::<Value>(&captured.stdout).is_err() {
            captured.error = Some("command returned invalid JSON".to_owned());
        }
        let filename = format!(
            "{name}.{}",
            if captured.error.is_some() {
                "stdout.txt"
            } else {
                "json"
            }
        );
        let stderr = format!("{name}.stderr.txt");
        save(&output.join(&filename), &redact(&captured.stdout, token))?;
        save(&output.join(&stderr), &redact(&captured.stderr, token))?;
        if let Some(error) = &captured.error {
            eprintln!("db3: {name}: {error}");
        }
        let command: Vec<&str> = std::iter::once("influxdb3")
            .chain(command.iter().copied())
            .collect();
        results.push(json!({"name": name, "command": command, "returncode": captured.returncode,
            "ok": captured.error.is_none(), "error": captured.error, "stdout": filename, "stderr": stderr}));
    }
    let complete = results.iter().all(|r| r["ok"] == true);
    let manifest = json!({"complete": complete, "pod": args.pod, "namespace": args.common.namespace,
        "context": args.common.context, "container": args.container, "results": results});
    save(
        &output.join("manifest.json"),
        &(serde_json::to_string_pretty(&manifest)? + "\n"),
    )?;
    println!("Support bundle: {}", output.display());
    Ok(complete)
}

/// The two commands `support info` prints. Both are read-only views of the
/// running process, so neither needs the bundle machinery.
const INFO_COMMANDS: &[&str] = &["nodes", "license-info"];

/// Print `influxdb3 show nodes` and `influxdb3 show license info` as one JSON
/// document on stdout: `{"nodes": {...}, "license-info": {...}}`. Nothing is
/// written to disk, so a caller can pipe this straight into `jq`.
fn print_info(
    args: &Support,
    token: &str,
    timeout: f64,
) -> Result<bool, Box<dyn std::error::Error>> {
    let pod = args.pod.as_deref().unwrap_or_default();
    if pod.is_empty() {
        usage("support info requires --pod POD or INFLUXDB3_POD");
    }
    let mut base = prefix(&args.common);
    base.extend(["exec".to_owned(), "-i".to_owned(), pod.to_owned()]);
    if let Some(container) = args.container.as_ref().filter(|s| !s.is_empty()) {
        base.extend(["-c".to_owned(), container.to_owned()]);
    }
    base.extend(["--", "sh", "-c", REMOTE, "db3-support"].map(str::to_owned));

    let mut document = serde_json::Map::new();
    let mut complete = true;
    for (name, command) in COMMANDS
        .iter()
        .filter(|(name, _)| INFO_COMMANDS.contains(name))
    {
        let argv: Vec<String> = base
            .iter()
            .map(String::as_str)
            .chain(command.iter().copied())
            .map(str::to_owned)
            .collect();
        let captured = capture("kubectl", &argv, Some(token), timeout)?;
        // Redact before parsing so a token echoed into a value cannot reach
        // stdout on this path either.
        let parsed = serde_json::from_str::<Value>(&redact(&captured.stdout, token)).ok();
        let failure = captured.error.clone().or_else(|| {
            parsed
                .is_none()
                .then(|| "command returned invalid JSON".to_owned())
        });
        if let Some(error) = failure {
            complete = false;
            eprintln!("db3: {name}: {error}");
            let stderr = redact(captured.stderr.trim_end(), token);
            if !stderr.is_empty() {
                eprintln!("{stderr}");
            }
        }
        document.insert((*name).to_owned(), parsed.unwrap_or(Value::Null));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&Value::Object(document))?
    );
    Ok(complete)
}
