//! Read-only environment snapshot for the local web console.
//!
//! Every collector is an independent `kubectl` invocation with its own
//! timeout. A failed collector degrades one panel; it never aborts the
//! snapshot. Nothing is written to the cluster and nothing is uploaded.

use crate::ROLES;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

pub struct Options {
    pub namespace: Option<String>,
    pub context: Option<String>,
    pub timeout: f64,
    pub no_secrets: bool,
    pub helm: bool,
    pub stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

struct Job {
    name: &'static str,
    program: &'static str,
    args: Vec<String>,
}

struct Outcome {
    name: &'static str,
    ok: bool,
    error: Option<String>,
    duration_ms: u64,
    count: usize,
    value: Option<Value>,
}

impl Outcome {
    fn failed(name: &'static str, error: impl Into<String>) -> Self {
        Self {
            name,
            ok: false,
            error: Some(error.into()),
            duration_ms: 0,
            count: 0,
            value: None,
        }
    }
}

const RESOURCE_LIMIT: usize = 500;
const SELECTOR_BUDGET: usize = 50_000;
// Shared across ConfigMaps and opt-in Secret metadata. Only one raw job result
// is retained at a time; command output is separately bounded by capture().
const CONFIG_NAMESPACE_LIMIT: usize = 16;
const CONFIG_ROW_LIMIT: usize = 500;
const CONFIG_BYTE_LIMIT: usize = 1024 * 1024;

fn bound_arrays(value: &mut Value) -> bool {
    let mut truncated = false;
    match value {
        Value::Array(rows) => {
            if rows.len() > RESOURCE_LIMIT {
                rows.truncate(RESOURCE_LIMIT);
                truncated = true;
            }
            for row in rows {
                truncated |= bound_arrays(row);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                truncated |= bound_arrays(value);
            }
        }
        _ => {}
    }
    truncated
}

fn run_job(job: Job, opts: &Options) -> Outcome {
    let started = Instant::now();
    let mut argv = Vec::with_capacity(job.args.len() + 2);
    if let Some(context) = opts.context.as_deref().filter(|value| !value.is_empty()) {
        argv.push("--context".to_owned());
        argv.push(context.to_owned());
    }
    argv.extend(job.args);
    let captured = match crate::webui_process::capture(job.program, &argv, opts.timeout, &opts.stop)
    {
        Ok(captured) => captured,
        Err(error) => {
            return Outcome {
                duration_ms: started.elapsed().as_millis() as u64,
                ..Outcome::failed(job.name, format!("could not run {}: {error}", job.program))
            };
        }
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    if let Some(error) = captured.error {
        return Outcome {
            duration_ms,
            ..Outcome::failed(job.name, error)
        };
    }
    match serde_json::from_str::<Value>(&captured.stdout) {
        Ok(mut value) => {
            let truncated = bound_arrays(&mut value);
            let count = match &value {
                Value::Array(rows) => rows.len(),
                _ => value
                    .get("items")
                    .and_then(Value::as_array)
                    .map_or(1, Vec::len),
            };
            Outcome {
                name: job.name,
                ok: !truncated,
                error: truncated.then(|| {
                    "resource list truncated at 500 entries; observations are partial".to_owned()
                }),
                duration_ms,
                count,
                value: Some(value),
            }
        }
        Err(_) => Outcome {
            duration_ms,
            ..Outcome::failed(job.name, "command did not return JSON")
        },
    }
}

fn run_jobs(jobs: Vec<Job>, opts: &Options) -> Vec<Outcome> {
    let mut results = Vec::with_capacity(jobs.len());
    // Four concurrent processes keeps latency low without stampeding a small
    // API server.
    for chunk in jobs.chunks(4) {
        let batch: Vec<Outcome> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|job| {
                    let job = Job {
                        name: job.name,
                        program: job.program,
                        args: job.args.clone(),
                    };
                    scope.spawn(move || run_job(job, opts))
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        Outcome::failed("collector", "collector thread terminated unexpectedly")
                    })
                })
                .collect()
        });
        results.extend(batch);
    }
    results
}

// --------------------------------------------------------------- JSON helpers

fn at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    at(value, path).and_then(Value::as_str)
}

fn items(value: &Value) -> &[Value] {
    value
        .get("items")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn sorted_labels(value: &Value) -> Value {
    let mut map = Map::new();
    if let Some(labels) = at(value, &["metadata", "labels"]).and_then(Value::as_object) {
        for (key, label) in labels {
            if let Some(label) = label.as_str() {
                map.insert(key.clone(), Value::String(label.to_owned()));
            }
        }
    }
    Value::Object(map)
}

fn human_age(stamp: Option<&str>) -> String {
    let Some(stamp) = stamp else {
        return "—".to_owned();
    };
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(stamp) else {
        return "—".to_owned();
    };
    let seconds = (chrono::Utc::now() - created.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

fn creation_age(value: &Value) -> String {
    human_age(text(value, &["metadata", "creationTimestamp"]))
}

fn parse_bytes(quantity: &str) -> u64 {
    let quantity = quantity.trim();
    let split = quantity
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(quantity.len());
    let (number, suffix) = quantity.split_at(split);
    let Ok(value) = number.parse::<f64>() else {
        return 0;
    };
    let multiplier: f64 = match suffix {
        "Ki" => 1024.0,
        "Mi" => 1024.0_f64.powi(2),
        "Gi" => 1024.0_f64.powi(3),
        "Ti" => 1024.0_f64.powi(4),
        "Pi" => 1024.0_f64.powi(5),
        "Ei" => 1024.0_f64.powi(6),
        "k" | "K" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        "T" => 1e12,
        "P" => 1e15,
        "E" => 1e18,
        _ => 1.0,
    };
    (value * multiplier).round().max(0.0) as u64
}

// ------------------------------------------------------------ role detection

fn normalize_role(raw: &str) -> Option<&'static str> {
    let lowered = raw.trim().to_ascii_lowercase();
    let candidate = lowered.rsplit('/').next().unwrap_or(&lowered);
    ROLES.iter().copied().find(|role| {
        candidate == *role
            || candidate.starts_with(role)
            || (candidate.starts_with("query") && *role == "querier")
            || (candidate.starts_with("ingest") && *role == "ingester")
            || (candidate.starts_with("compact") && *role == "compactor")
            || (candidate.starts_with("process") && *role == "processor")
    })
}

fn container_images(pod: &Value) -> Vec<String> {
    at(pod, &["spec", "containers"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|container| text(container, &["image"]).map(str::to_owned))
        .collect()
}

fn is_influxdb(pod: &Value) -> bool {
    container_images(pod)
        .iter()
        .any(|image| image.to_ascii_lowercase().contains("influxdb"))
}

fn labels_of(value: &Value) -> BTreeMap<String, String> {
    at(value, &["metadata", "labels"])
        .and_then(Value::as_object)
        .map(|labels| {
            labels
                .iter()
                .filter_map(|(key, label)| {
                    label.as_str().map(|label| (key.clone(), label.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

const COMPONENT_LABELS: [&str; 5] = [
    "app.kubernetes.io/component",
    "influxdb3/component",
    "component",
    "role",
    "app",
];

fn role_from_labels(labels: &BTreeMap<String, String>) -> Option<&'static str> {
    COMPONENT_LABELS
        .iter()
        .find_map(|key| labels.get(*key).and_then(|value| normalize_role(value)))
}

fn detect_component(pod: &Value) -> Option<(String, bool)> {
    if !is_influxdb(pod) {
        return None;
    }
    let labels = labels_of(pod);
    if let Some(role) = role_from_labels(&labels) {
        return Some((role.to_owned(), false));
    }
    let name = text(pod, &["metadata", "name"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    if let Some(role) = normalize_role(&name) {
        return Some((role.to_owned(), true));
    }
    None
}

fn detect_cluster(pod: &Value, namespace: &str) -> String {
    let labels = labels_of(pod);
    for key in ["influxdb3/cluster-id", "influxdb3-cluster-id", "cluster-id"] {
        if let Some(value) = labels.get(key).filter(|value| !value.is_empty()) {
            return value.clone();
        }
    }
    for container in at(pod, &["spec", "containers"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(cluster) = argument_value(container, "--cluster-id") {
            return cluster;
        }
        if let Some(cluster) = env_value(container, "INFLUXDB3_CLUSTER_ID") {
            return cluster;
        }
    }
    if let Some(instance) = labels
        .get("app.kubernetes.io/instance")
        .filter(|value| !value.is_empty())
    {
        return instance.clone();
    }
    namespace.to_owned()
}

fn argument_value(container: &Value, flag: &str) -> Option<String> {
    let args = container.get("args").and_then(Value::as_array)?;
    let mut iter = args.iter().filter_map(Value::as_str);
    while let Some(argument) = iter.next() {
        if argument == flag {
            return iter.next().map(str::to_owned);
        }
        if let Some(value) = argument
            .strip_prefix(flag)
            .and_then(|rest| rest.strip_prefix('='))
        {
            return Some(value.to_owned());
        }
    }
    None
}

fn env_value(container: &Value, key: &str) -> Option<String> {
    container
        .get("env")
        .and_then(Value::as_array)?
        .iter()
        .find(|entry| text(entry, &["name"]) == Some(key))
        .and_then(|entry| text(entry, &["value"]).map(str::to_owned))
        .filter(|value| !value.is_empty())
}

fn detect_mode(container: &Value) -> Option<String> {
    argument_value(container, "--mode")
        .or_else(|| env_value(container, "INFLUXDB3_ENTERPRISE_MODE"))
        .or_else(|| env_value(container, "INFLUXDB3_MODE"))
}

// ----------------------------------------------------------------- sensitive

const ENV_ALLOW: [&str; 12] = [
    "INFLUXDB3_LICENSE_TYPE",
    "INFLUXDB3_LICENSE_EMAIL",
    "INFLUXDB3_ENTERPRISE_LICENSE_TYPE",
    "INFLUXDB3_MODE",
    "INFLUXDB3_ENTERPRISE_MODE",
    "INFLUXDB3_OBJECT_STORE",
    "INFLUXDB3_BUCKET",
    "INFLUXDB3_CLUSTER_ID",
    "INFLUXDB3_NODE_ID",
    "INFLUXDB3_HTTP_BIND",
    "INFLUXDB3_LOG_FILTER",
    "RUST_LOG",
];

const SENSITIVE_MARKERS: [&str; 12] = [
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "PRIVATE_KEY",
    "ACCESS_KEY",
    "API_KEY",
    "APIKEY",
    "JWT",
    "LICENSE",
    "AUTH",
];

fn sensitive(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    if ENV_ALLOW.contains(&upper.as_str()) {
        return false;
    }
    SENSITIVE_MARKERS
        .iter()
        .any(|marker| upper.contains(marker))
}

fn truncate_value(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_owned();
    }
    let mut shortened: String = value.chars().take(limit).collect();
    shortened.push('…');
    shortened
}

fn env_rows(container: &Value) -> Vec<Value> {
    let mut rows = Vec::new();
    let Some(entries) = container.get("env").and_then(Value::as_array) else {
        return rows;
    };
    for entry in entries.iter().take(80) {
        let Some(name) = text(entry, &["name"]) else {
            continue;
        };
        let masked = sensitive(name);
        let value = match (entry.get("value"), entry.get("valueFrom")) {
            (Some(Value::String(value)), _) => Some(if masked {
                "[redacted]".to_owned()
            } else {
                truncate_value(value, 300)
            }),
            (_, Some(source)) => {
                let reference = source
                    .get("secretKeyRef")
                    .map(|reference| {
                        format!(
                            "secret {}/{}",
                            text(reference, &["name"]).unwrap_or("?"),
                            text(reference, &["key"]).unwrap_or("?")
                        )
                    })
                    .or_else(|| {
                        source.get("configMapKeyRef").map(|reference| {
                            format!(
                                "configMap {}/{}",
                                text(reference, &["name"]).unwrap_or("?"),
                                text(reference, &["key"]).unwrap_or("?")
                            )
                        })
                    })
                    .or_else(|| {
                        source.get("fieldRef").map(|reference| {
                            format!("field {}", text(reference, &["fieldPath"]).unwrap_or("?"))
                        })
                    })
                    .unwrap_or_else(|| "valueFrom".to_owned());
                Some(format!("<{reference}>"))
            }
            _ => None,
        };
        rows.push(json!({"key": name, "value": value, "redacted": masked}));
    }
    rows
}

fn container_rows(pod: &Value) -> Vec<Value> {
    let statuses: Vec<&Value> = pod
        .get("status")
        .and_then(|status| status.get("containerStatuses"))
        .and_then(Value::as_array)
        .map(|statuses| statuses.iter().collect())
        .unwrap_or_default();
    let mut rows = Vec::new();
    for container in at(pod, &["spec", "containers"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = text(container, &["name"]).unwrap_or("container");
        let status = statuses
            .iter()
            .copied()
            .find(|status| text(status, &["name"]) == Some(name));
        let state_object = status
            .and_then(|status| status.get("state"))
            .and_then(Value::as_object);
        let state = state_object
            .and_then(|object| object.keys().next().cloned())
            .unwrap_or_else(|| "unknown".to_owned());
        let reason = state_object
            .and_then(|object| object.get(&state))
            .and_then(|phase| phase.get("reason"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut commands: Vec<String> = Vec::new();
        for key in ["command", "args"] {
            if let Some(values) = container.get(key).and_then(Value::as_array) {
                commands.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
            }
        }
        rows.push(json!({
            "name": name,
            "image": text(container, &["image"]).unwrap_or(""),
            "ready": status.and_then(|status| status.get("ready")).and_then(Value::as_bool).unwrap_or(false),
            "restarts": status.and_then(|status| status.get("restartCount")).and_then(Value::as_u64).unwrap_or(0),
            "state": state,
            "reason": reason,
            "env": env_rows(container),
            "commands": commands,
        }));
    }
    rows
}

fn pod_ready(pod: &Value) -> bool {
    at(pod, &["status", "conditions"])
        .and_then(Value::as_array)
        .and_then(|conditions| {
            conditions
                .iter()
                .find(|condition| text(condition, &["type"]) == Some("Ready"))
        })
        .and_then(|condition| condition.get("status"))
        .and_then(Value::as_str)
        == Some("True")
}

fn total_restarts(pod: &Value) -> u64 {
    at(pod, &["status", "containerStatuses"])
        .and_then(Value::as_array)
        .map(|statuses| {
            statuses
                .iter()
                .filter_map(|status| status.get("restartCount").and_then(Value::as_u64))
                .sum()
        })
        .unwrap_or(0)
}

fn owner_of(pod: &Value) -> (Option<String>, Option<String>) {
    at(pod, &["metadata", "ownerReferences"])
        .and_then(Value::as_array)
        .and_then(|owners| {
            let controllers: Vec<_> = owners
                .iter()
                .filter(|owner| owner["controller"] == true)
                .collect();
            if controllers.len() == 1 {
                Some(controllers[0])
            } else if controllers.is_empty() && owners.len() == 1 {
                owners.first()
            } else {
                None
            }
        })
        .map_or((None, None), |owner| {
            (
                text(owner, &["kind"]).map(str::to_owned),
                text(owner, &["name"]).map(str::to_owned),
            )
        })
}

// -------------------------------------------------------------- conversions

fn pod_row(pod: &Value) -> Value {
    let namespace = text(pod, &["metadata", "namespace"])
        .unwrap_or("default")
        .to_owned();
    let (component, inferred) = match detect_component(pod) {
        Some((role, inferred)) => (Some(role), inferred),
        None => (None, false),
    };
    let mode = at(pod, &["spec", "containers"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(detect_mode);
    let (owner_kind, owner_name) = owner_of(pod);
    json!({
        "namespace": namespace,
        "name": text(pod, &["metadata", "name"]).unwrap_or(""),
        "node": text(pod, &["spec", "nodeName"]),
        "phase": text(pod, &["status", "phase"]).unwrap_or("Unknown"),
        "ready": pod_ready(pod),
        "restarts": total_restarts(pod),
        "start_time": text(pod, &["status", "startTime"]),
        "age": creation_age(pod),
        "component": component,
        "cluster_id": component.as_ref().map(|_| detect_cluster(pod, &namespace)),
        "inferred_component": inferred,
        "images": container_images(pod),
        "containers": container_rows(pod),
        "owner_kind": owner_kind,
        "owner_name": owner_name,
        "labels": sorted_labels(pod),
        "mode": mode,
    })
}

fn node_row(node: &Value) -> Value {
    let ready = at(node, &["status", "conditions"])
        .and_then(Value::as_array)
        .and_then(|conditions| {
            conditions
                .iter()
                .find(|condition| text(condition, &["type"]) == Some("Ready"))
        })
        .and_then(|condition| condition.get("status"))
        .and_then(Value::as_str)
        == Some("True");
    let roles: Vec<String> = labels_of(node)
        .keys()
        .filter_map(|key| key.strip_prefix("node-role.kubernetes.io/"))
        .map(|role| {
            if role.is_empty() {
                "control-plane".to_owned()
            } else {
                role.to_owned()
            }
        })
        .collect();
    let capacity = node.get("status").and_then(|status| status.get("capacity"));
    let allocatable = node
        .get("status")
        .and_then(|status| status.get("allocatable"));
    let taints: Vec<String> = at(node, &["spec", "taints"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|taint| {
            format!(
                "{}:{}",
                text(taint, &["key"]).unwrap_or(""),
                text(taint, &["effect"]).unwrap_or("")
            )
        })
        .collect();
    let internal_ip = at(node, &["status", "addresses"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|address| text(address, &["type"]) == Some("InternalIP"))
        .and_then(|address| text(address, &["address"]))
        .unwrap_or("");
    json!({
        "name": text(node, &["metadata", "name"]).unwrap_or(""),
        "ready": ready,
        "roles": roles,
        "version": text(node, &["status", "nodeInfo", "kubeletVersion"]).unwrap_or(""),
        "os_image": text(node, &["status", "nodeInfo", "osImage"]).unwrap_or(""),
        "kernel": text(node, &["status", "nodeInfo", "kernelVersion"]).unwrap_or(""),
        "cpu_capacity": capacity.and_then(|value| value.get("cpu")).and_then(Value::as_str).unwrap_or("—"),
        "mem_capacity_bytes": capacity.and_then(|value| value.get("memory")).and_then(Value::as_str).map(parse_bytes).unwrap_or(0),
        "cpu_allocatable": allocatable.and_then(|value| value.get("cpu")).and_then(Value::as_str).unwrap_or("—"),
        "mem_allocatable_bytes": allocatable.and_then(|value| value.get("memory")).and_then(Value::as_str).map(parse_bytes).unwrap_or(0),
        "pods_capacity": allocatable.and_then(|value| value.get("pods")).and_then(Value::as_str).map(|value| parse_bytes(value) as u32).unwrap_or(0),
        "pods_used": 0,
        "zone": labels_of(node).get("topology.kubernetes.io/zone").cloned().unwrap_or_default(),
        "internal_ip": internal_ip,
        "age": creation_age(node),
        "taints": taints,
    })
}

fn workload_row(item: &Value) -> Option<Value> {
    let kind = text(item, &["kind"]).unwrap_or("Workload").to_owned();
    let namespace = text(item, &["metadata", "namespace"])
        .unwrap_or("default")
        .to_owned();
    let name = text(item, &["metadata", "name"]).unwrap_or("").to_owned();
    let (desired, ready, updated) = match kind.as_str() {
        "DaemonSet" => (
            at(item, &["status", "desiredNumberScheduled"])
                .and_then(Value::as_u64)
                .unwrap_or(0),
            at(item, &["status", "numberReady"])
                .and_then(Value::as_u64)
                .unwrap_or(0),
            at(item, &["status", "updatedNumberScheduled"])
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
        _ => (
            at(item, &["spec", "replicas"])
                .and_then(Value::as_u64)
                .unwrap_or(1),
            at(item, &["status", "readyReplicas"])
                .and_then(Value::as_u64)
                .unwrap_or(0),
            at(item, &["status", "updatedReplicas"])
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
    };
    let template = item.get("spec").and_then(|spec| spec.get("template"));
    let labels: BTreeMap<String, String> = template
        .and_then(|template| template.get("metadata"))
        .and_then(|metadata| metadata.get("labels"))
        .and_then(Value::as_object)
        .map(|labels| {
            labels
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    let component = role_from_labels(&labels)
        .map(str::to_owned)
        .or_else(|| normalize_role(&name).map(str::to_owned));
    let images: Vec<String> = template
        .and_then(|template| at(template, &["spec", "containers"]))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|container| text(container, &["image"]).map(str::to_owned))
        .collect();
    Some(json!({
        "namespace": namespace,
        "kind": kind,
        "name": name,
        "desired": desired,
        "ready": ready,
        "updated": updated,
        "component": component,
        "cluster_id": Value::Null,
        "images": images,
        "age": creation_age(item),
    }))
}

fn service_row(service: &Value) -> Value {
    let ports: Vec<Value> = at(service, &["spec", "ports"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|port| {
            json!({
                "name": text(port, &["name"]).unwrap_or(""),
                "port": port.get("port").and_then(Value::as_u64),
                "target_port": match port.get("targetPort") {
                    Some(Value::String(value)) => value.clone(),
                    Some(Value::Number(value)) => value.to_string(),
                    _ => String::new(),
                },
                "protocol": text(port, &["protocol"]).unwrap_or("TCP"),
            })
        })
        .collect();
    let selector: Map<String, Value> = at(service, &["spec", "selector"])
        .and_then(Value::as_object)
        .map(|selector| {
            selector
                .iter()
                .filter_map(|(key, value)| {
                    value
                        .as_str()
                        .map(|value| (key.clone(), Value::String(value.to_owned())))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut external: Vec<String> = at(service, &["spec", "externalIPs"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    for ingress in at(service, &["status", "loadBalancer", "ingress"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(ip) = text(ingress, &["ip"]) {
            external.push(ip.to_owned());
        }
        if let Some(host) = text(ingress, &["hostname"]) {
            external.push(host.to_owned());
        }
    }
    json!({
        "namespace": text(service, &["metadata", "namespace"]).unwrap_or("default"),
        "name": text(service, &["metadata", "name"]).unwrap_or(""),
        "type": text(service, &["spec", "type"]).unwrap_or("ClusterIP"),
        "cluster_ip": text(service, &["spec", "clusterIP"]).unwrap_or(""),
        "external_ips": external,
        "ports": ports,
        "selector": Value::Object(selector),
        "endpoints": Vec::<Value>::new(),
        "component": Value::Null,
        "cluster_id": Value::Null,
    })
}

fn endpoint_map(slices: &Value) -> BTreeMap<(String, String), Vec<Value>> {
    let mut map: BTreeMap<(String, String), Vec<Value>> = BTreeMap::new();
    for slice in items(slices) {
        let namespace = text(slice, &["metadata", "namespace"])
            .unwrap_or("default")
            .to_owned();
        let Some(service) = at(slice, &["metadata", "labels", "kubernetes.io/service-name"])
            .and_then(Value::as_str)
        else {
            continue;
        };
        for endpoint in slice
            .get("endpoints")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let ready = at(endpoint, &["conditions", "ready"])
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let target = text(endpoint, &["targetRef", "name"])
                .unwrap_or("")
                .to_owned();
            for address in endpoint
                .get("addresses")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(address) = address.as_str() {
                    map.entry((namespace.clone(), service.to_owned()))
                        .or_default()
                        .push(json!({"address": address, "ready": ready, "target": target}));
                }
            }
        }
    }
    map
}

fn configmap_row(configmap: &Value) -> Value {
    let mut shown = Map::new();
    let mut keys = Vec::new();
    for (key, value) in configmap
        .get("data")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let Some(value) = value.as_str() else {
            continue;
        };
        keys.push(json!({"key": key, "bytes": value.len()}));
        shown.insert(
            key.clone(),
            Value::String(if sensitive(key) {
                "[redacted]".to_owned()
            } else {
                truncate_value(value, 2000)
            }),
        );
    }
    json!({
        "namespace": text(configmap, &["metadata", "namespace"]).unwrap_or("default"),
        "name": text(configmap, &["metadata", "name"]).unwrap_or(""),
        "keys": keys,
        "data": Value::Object(shown),
        "age": creation_age(configmap),
    })
}

fn secret_row(secret: &Value) -> Value {
    // Values are dropped here, before the object can enter any snapshot.
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for field in ["data", "stringData"] {
        if let Some(map) = secret.get(field).and_then(Value::as_object) {
            keys.extend(map.keys().cloned());
        }
    }
    json!({
        "namespace": text(secret, &["metadata", "namespace"]).unwrap_or("default"),
        "name": text(secret, &["metadata", "name"]).unwrap_or(""),
        "type": text(secret, &["type"]).unwrap_or("Opaque"),
        "keys": keys.into_iter().collect::<Vec<_>>(),
        "age": creation_age(secret),
    })
}

fn event_row(event: &Value) -> Value {
    let last = text(event, &["lastTimestamp"])
        .or_else(|| text(event, &["eventTime"]))
        .or_else(|| text(event, &["metadata", "creationTimestamp"]));
    let object = match (
        text(event, &["involvedObject", "kind"]),
        text(event, &["involvedObject", "name"]),
    ) {
        (Some(kind), Some(name)) => format!("{kind}/{name}"),
        (_, Some(name)) => name.to_owned(),
        _ => "—".to_owned(),
    };
    json!({
        "namespace": text(event, &["metadata", "namespace"]).unwrap_or("default"),
        "type": text(event, &["type"]).unwrap_or("Normal"),
        "reason": text(event, &["reason"]).unwrap_or(""),
        "object": object,
        "message": truncate_value(text(event, &["message"]).unwrap_or(""), 600),
        "count": event.get("count").and_then(Value::as_u64).unwrap_or(1),
        "last_seen": last,
    })
}

fn network_policy_row(policy: &Value) -> Value {
    let selector = at(policy, &["spec", "podSelector", "matchLabels"])
        .and_then(Value::as_object)
        .map(|labels| {
            labels
                .iter()
                .map(|(key, value)| format!("{key}={}", value.as_str().unwrap_or("")))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "all pods".to_owned());
    let policy_types: Vec<String> = at(policy, &["spec", "policyTypes"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    json!({
        "namespace": text(policy, &["metadata", "namespace"]).unwrap_or("default"),
        "name": text(policy, &["metadata", "name"]).unwrap_or(""),
        "pod_selector": selector,
        "policy_types": policy_types,
        "ingress_rules": at(policy, &["spec", "ingress"]).and_then(Value::as_array).map_or(0, Vec::len),
        "egress_rules": at(policy, &["spec", "egress"]).and_then(Value::as_array).map_or(0, Vec::len),
    })
}

// ------------------------------------------------------------------- issues

fn push_issue(
    issues: &mut Vec<Value>,
    severity: &str,
    title: String,
    detail: String,
    namespace: Option<&str>,
) {
    issues.push(json!({
        "severity": severity,
        "title": title,
        "detail": detail,
        "namespace": namespace,
    }));
}

fn namespace_scoped(opts: &Options, mut args: Vec<String>) -> Vec<String> {
    if let Some(namespace) = opts.namespace.as_deref().filter(|value| !value.is_empty()) {
        args.push("-n".to_owned());
        args.push(namespace.to_owned());
    } else if !args.iter().any(|argument| argument == "-A") {
        args.push("-A".to_owned());
    }
    args
}

pub fn snapshot(opts: &Options) -> Value {
    let mut phase_one = vec![
        Job {
            name: "kubectl-version",
            program: "kubectl",
            args: vec!["version".to_owned(), "-o".to_owned(), "json".to_owned()],
        },
        Job {
            name: "kubeconfig",
            program: "kubectl",
            args: vec![
                "config".to_owned(),
                "view".to_owned(),
                "--minify".to_owned(),
                "-o".to_owned(),
                "json".to_owned(),
            ],
        },
        Job {
            name: "nodes",
            program: "kubectl",
            args: vec![
                "get".to_owned(),
                "nodes".to_owned(),
                "-o".to_owned(),
                "json".to_owned(),
            ],
        },
        Job {
            name: "namespaces",
            program: "kubectl",
            args: vec![
                "get".to_owned(),
                "namespaces".to_owned(),
                "-o".to_owned(),
                "json".to_owned(),
            ],
        },
        Job {
            name: "pods",
            program: "kubectl",
            args: namespace_scoped(
                opts,
                vec![
                    "get".to_owned(),
                    "pods".to_owned(),
                    "-o".to_owned(),
                    "json".to_owned(),
                ],
            ),
        },
        Job {
            name: "workloads",
            program: "kubectl",
            args: namespace_scoped(
                opts,
                vec![
                    "get".to_owned(),
                    "statefulsets,deployments,daemonsets".to_owned(),
                    "-o".to_owned(),
                    "json".to_owned(),
                ],
            ),
        },
        Job {
            name: "services",
            program: "kubectl",
            args: namespace_scoped(
                opts,
                vec![
                    "get".to_owned(),
                    "services".to_owned(),
                    "-o".to_owned(),
                    "json".to_owned(),
                ],
            ),
        },
        Job {
            name: "endpointslices",
            program: "kubectl",
            args: namespace_scoped(
                opts,
                vec![
                    "get".to_owned(),
                    "endpointslices.discovery.k8s.io".to_owned(),
                    "-o".to_owned(),
                    "json".to_owned(),
                ],
            ),
        },
        Job {
            name: "storageclasses",
            program: "kubectl",
            args: vec![
                "get".to_owned(),
                "storageclasses".to_owned(),
                "-o".to_owned(),
                "json".to_owned(),
            ],
        },
        Job {
            name: "persistentvolumes",
            program: "kubectl",
            args: vec![
                "get".to_owned(),
                "persistentvolumes".to_owned(),
                "-o".to_owned(),
                "json".to_owned(),
            ],
        },
        Job {
            name: "persistentvolumeclaims",
            program: "kubectl",
            args: namespace_scoped(
                opts,
                vec![
                    "get".to_owned(),
                    "persistentvolumeclaims".to_owned(),
                    "-o".to_owned(),
                    "json".to_owned(),
                ],
            ),
        },
        Job {
            name: "networkpolicies",
            program: "kubectl",
            args: namespace_scoped(
                opts,
                vec![
                    "get".to_owned(),
                    "networkpolicies".to_owned(),
                    "-o".to_owned(),
                    "json".to_owned(),
                ],
            ),
        },
        Job {
            name: "events",
            program: "kubectl",
            args: namespace_scoped(
                opts,
                vec![
                    "get".to_owned(),
                    "events".to_owned(),
                    "-o".to_owned(),
                    "json".to_owned(),
                ],
            ),
        },
    ];
    if opts.helm {
        phase_one.push(Job {
            name: "helm",
            program: "helm",
            args: {
                let mut args = vec![
                    "list".to_owned(),
                    "-A".to_owned(),
                    "-o".to_owned(),
                    "json".to_owned(),
                ];
                if let Some(context) = opts.context.as_deref().filter(|value| !value.is_empty()) {
                    args.push("--kube-context".to_owned());
                    args.push(context.to_owned());
                }
                args
            },
        });
    }
    let results = run_jobs(phase_one, opts);
    let find = |name: &str| results.iter().find(|outcome| outcome.name == name);

    // ------------------------------------------------------------- cluster info
    let server_version = find("kubectl-version")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| {
            json!({
                "version": text(value, &["serverVersion", "gitVersion"]).unwrap_or(""),
                "platform": text(value, &["serverVersion", "platform"]).unwrap_or(""),
            })
        })
        .unwrap_or_else(|| json!({"version": "", "platform": ""}));
    let server = find("kubeconfig")
        .and_then(|outcome| outcome.value.as_ref())
        .and_then(|value| {
            at(value, &["clusters"])
                .and_then(Value::as_array)
                .and_then(|clusters| clusters.first())
                .and_then(|cluster| text(cluster, &["cluster", "server"]))
        })
        .unwrap_or("")
        .to_owned();
    let context_name = find("kubeconfig")
        .and_then(|outcome| outcome.value.as_ref())
        .and_then(|value| text(value, &["current-context"]))
        .map(str::to_owned)
        .or_else(|| opts.context.clone());

    // ---------------------------------------------------------------- objects
    let mut nodes: Vec<Value> = find("nodes")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| items(value).iter().map(node_row).collect())
        .unwrap_or_default();
    let namespaces: Vec<Value> = find("namespaces")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| {
            items(value)
                .iter()
                .map(|namespace| {
                    json!({
                        "name": text(namespace, &["metadata", "name"]).unwrap_or(""),
                        "phase": text(namespace, &["status", "phase"]).unwrap_or("Active"),
                        "age": creation_age(namespace),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let pods: Vec<Value> = find("pods")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| items(value).iter().map(pod_row).collect())
        .unwrap_or_default();
    let workloads: Vec<Value> = find("workloads")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| items(value).iter().filter_map(workload_row).collect())
        .unwrap_or_default();
    let endpoints = find("endpointslices")
        .and_then(|outcome| outcome.value.as_ref())
        .map(endpoint_map)
        .unwrap_or_default();
    let helm: Vec<Value> = find("helm")
        .filter(|outcome| outcome.ok)
        .and_then(|outcome| outcome.value.as_ref())
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut issues: Vec<Value> = Vec::new();
    for outcome in &results {
        if outcome
            .error
            .as_deref()
            .is_some_and(|e| e.contains("truncated"))
        {
            push_issue(
                &mut issues,
                "warning",
                format!("{} collection truncated", outcome.name),
                "Resource limit reached; observations are partial, not evidence of absence."
                    .to_owned(),
                None,
            );
        }
    }
    for node in &nodes {
        if node["ready"] != Value::Bool(true) {
            push_issue(
                &mut issues,
                "critical",
                format!("Node {} is NotReady", node["name"].as_str().unwrap_or("?")),
                "Workloads on this node may be unreachable; check kubelet and node pressure."
                    .to_owned(),
                None,
            );
        }
    }

    for pod in &pods {
        if opts.stop.load(std::sync::atomic::Ordering::SeqCst) {
            return json!({"error":"collection cancelled"});
        }
        let namespace = pod["namespace"].as_str().unwrap_or("default");
        let name = pod["name"].as_str().unwrap_or("?");
        let phase = pod["phase"].as_str().unwrap_or("Unknown");
        if pod["ready"].as_bool() != Some(true) {
            let reasons: Vec<String> = pod["containers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|container| container["reason"].as_str())
                .map(str::to_owned)
                .collect();
            push_issue(
                &mut issues,
                if pod["component"].is_string() {
                    "critical"
                } else {
                    "warning"
                },
                if phase == "Running" {
                    format!("Pod {name} is not Ready")
                } else {
                    format!("Pod {name} is {phase}")
                },
                if reasons.is_empty() {
                    format!("{namespace}: pod is not Ready.")
                } else {
                    format!("{namespace}: {}.", reasons.join(", "))
                },
                Some(namespace),
            );
        }
        let restarts = pod["restarts"].as_u64().unwrap_or(0);
        if restarts >= 5 {
            push_issue(
                &mut issues,
                "warning",
                format!("Pod {name} restarted {restarts} times"),
                format!(
                    "{namespace}: repeated restarts usually indicate a crash loop or failing probe."
                ),
                Some(namespace),
            );
        }
    }

    for workload in &workloads {
        let desired = workload["desired"].as_u64().unwrap_or(0);
        let ready = workload["ready"].as_u64().unwrap_or(0);
        if desired != ready {
            push_issue(
                &mut issues,
                if ready == 0 { "critical" } else { "warning" },
                format!(
                    "{} {} has {ready}/{desired} ready replicas",
                    workload["kind"].as_str().unwrap_or("Workload"),
                    workload["name"].as_str().unwrap_or("?")
                ),
                format!(
                    "{}: check scheduling, image pulls, and resource limits.",
                    workload["namespace"].as_str().unwrap_or("default")
                ),
                workload["namespace"].as_str(),
            );
        }
    }

    let mut services: Vec<Value> = find("services")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| items(value).iter().map(service_row).collect())
        .unwrap_or_default();
    let mut comparisons = 0usize;
    let mut selector_partial = false;
    for service in &mut services {
        if opts.stop.load(std::sync::atomic::Ordering::SeqCst) {
            return json!({"error":"collection cancelled"});
        }
        let namespace = service["namespace"]
            .as_str()
            .unwrap_or("default")
            .to_owned();
        let name = service["name"].as_str().unwrap_or("").to_owned();
        if let Some(found) = endpoints.get(&(namespace.clone(), name.clone())) {
            service["endpoints"] = Value::Array(found.clone());
        }
        let selector = service["selector"].as_object().cloned().unwrap_or_default();
        if selector.is_empty() {
            continue;
        }
        let matched: Vec<&Value> = pods
            .iter()
            .filter(|pod| {
                if comparisons >= SELECTOR_BUDGET
                    || opts.stop.load(std::sync::atomic::Ordering::SeqCst)
                {
                    selector_partial = true;
                    return false;
                }
                comparisons += 1;
                pod["namespace"].as_str() == Some(namespace.as_str())
                    && selector.iter().all(|(key, value)| {
                        comparisons += 1;
                        if comparisons > SELECTOR_BUDGET {
                            selector_partial = true;
                            return false;
                        }
                        pod["labels"].get(key) == Some(value)
                    })
            })
            .collect();
        if let Some(component) = matched.iter().find_map(|pod| pod["component"].as_str()) {
            service["component"] = Value::String(component.to_owned());
        }
        if let Some(cluster) = matched.iter().find_map(|pod| pod["cluster_id"].as_str()) {
            service["cluster_id"] = Value::String(cluster.to_owned());
        }
        if matched.is_empty() && !selector_partial {
            push_issue(
                &mut issues,
                "warning",
                format!("Service {name} selects no pods"),
                format!("{namespace}: the selector does not match any collected pod."),
                Some(namespace.as_str()),
            );
        }
    }

    if selector_partial {
        push_issue(
            &mut issues,
            "warning",
            "Service matching truncated".to_owned(),
            "50,000 selector comparison budget reached; routing observations are partial."
                .to_owned(),
            None,
        );
    }
    let claims: Vec<Value> = find("persistentvolumeclaims")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| {
            items(value)
                .iter()
                .map(|claim| {
                    json!({
                        "namespace": text(claim, &["metadata", "namespace"]).unwrap_or("default"),
                        "name": text(claim, &["metadata", "name"]).unwrap_or(""),
                        "storage_class": text(claim, &["spec", "storageClassName"]).unwrap_or(""),
                        "capacity_bytes": at(claim, &["status", "capacity", "storage"]).and_then(Value::as_str).map(parse_bytes).unwrap_or(0),
                        "status": at(claim, &["status", "accessModes"]).and_then(Value::as_array).map(|modes| modes.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(",")).unwrap_or_default(),
                        "volume": text(claim, &["spec", "volumeName"]).unwrap_or(""),
                        "phase": text(claim, &["status", "phase"]).unwrap_or("Pending"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    for claim in &claims {
        if claim["phase"].as_str() != Some("Bound") {
            push_issue(
                &mut issues,
                "warning",
                format!(
                    "Claim {} is {}",
                    claim["name"].as_str().unwrap_or("?"),
                    claim["phase"].as_str().unwrap_or("Pending")
                ),
                format!(
                    "{}: storage class {:?} may have no provisioner, or the cluster may be out of capacity.",
                    claim["namespace"].as_str().unwrap_or("default"),
                    claim["storage_class"].as_str().unwrap_or("")
                ),
                claim["namespace"].as_str(),
            );
        }
    }

    let storage_classes: Vec<Value> = find("storageclasses")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| {
            items(value)
                .iter()
                .map(|class| {
                    json!({
                        "name": text(class, &["metadata", "name"]).unwrap_or(""),
                        "provisioner": text(class, &["provisioner"]).unwrap_or(""),
                        "reclaim_policy": text(class, &["reclaimPolicy"]).unwrap_or(""),
                        "binding_mode": text(class, &["volumeBindingMode"]).unwrap_or(""),
                        "is_default": at(class, &["metadata", "annotations", "storageclass.kubernetes.io/is-default-class"])
                            .and_then(Value::as_str) == Some("true"),
                        "allow_expansion": at(class, &["allowVolumeExpansion"]).and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let volumes: Vec<Value> = find("persistentvolumes")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| {
            items(value)
                .iter()
                .map(|volume| {
                    json!({
                        "name": text(volume, &["metadata", "name"]).unwrap_or(""),
                        "capacity_bytes": at(volume, &["spec", "capacity", "storage"]).and_then(Value::as_str).map(parse_bytes).unwrap_or(0),
                        "storage_class": text(volume, &["spec", "storageClassName"]).unwrap_or(""),
                        "status": text(volume, &["status", "phase"]).unwrap_or(""),
                        "claim": text(volume, &["spec", "claimRef", "name"]).unwrap_or(""),
                        "reclaim_policy": text(volume, &["spec", "persistentVolumeReclaimPolicy"]).unwrap_or(""),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    for release in &helm {
        let status = release["status"].as_str().unwrap_or("");
        if status != "deployed" {
            push_issue(
                &mut issues,
                if status == "failed" {
                    "critical"
                } else {
                    "warning"
                },
                format!(
                    "Helm release {} is {status}",
                    release["name"].as_str().unwrap_or("?")
                ),
                format!(
                    "{}: chart {} revision {}",
                    release["namespace"].as_str().unwrap_or("default"),
                    release["chart"].as_str().unwrap_or("?"),
                    release["revision"].as_str().map_or_else(
                        || release["revision"]
                            .as_u64()
                            .map_or_else(String::new, |value| value.to_string()),
                        str::to_owned
                    )
                ),
                release["namespace"].as_str(),
            );
        }
    }

    let netpols: Vec<Value> = find("networkpolicies")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| items(value).iter().map(network_policy_row).collect())
        .unwrap_or_default();
    let mut events: Vec<Value> = find("events")
        .and_then(|outcome| outcome.value.as_ref())
        .map(|value| items(value).iter().map(event_row).collect())
        .unwrap_or_default();
    events.sort_by(|left, right| {
        right["last_seen"]
            .as_str()
            .unwrap_or("")
            .cmp(left["last_seen"].as_str().unwrap_or(""))
    });
    events.truncate(400);

    // ---------------------------------------------- configuration collection
    let mut config_namespaces: BTreeSet<String> = BTreeSet::new();
    for pod in &pods {
        if opts.stop.load(std::sync::atomic::Ordering::SeqCst) {
            return json!({"error":"collection cancelled"});
        }
        if pod["component"].is_string() {
            if let Some(namespace) = pod["namespace"].as_str() {
                config_namespaces.insert(namespace.to_owned());
            }
        }
    }
    let mut configmaps: Vec<Value> = Vec::new();
    let mut secrets: Vec<Value> = Vec::new();
    let mut config_errors: Vec<String> = Vec::new();
    if config_namespaces.is_empty() {
        config_errors
            .push("no InfluxDB 3 pods found; configuration collection was skipped".to_owned());
    } else {
        if config_namespaces.len() > CONFIG_NAMESPACE_LIMIT {
            config_errors
                .push("configuration namespace cap (16); observations are partial".to_owned());
        }
        let mut config_bytes = 4usize; // two JSON array delimiters
        'namespaces: for namespace in config_namespaces.iter().take(CONFIG_NAMESPACE_LIMIT) {
            for resource in ["configmaps", "secrets"] {
                if resource == "secrets" && opts.no_secrets {
                    continue;
                }
                if opts.stop.load(std::sync::atomic::Ordering::SeqCst) {
                    return json!({"error":"collection cancelled"});
                }
                let outcome = run_job(
                    Job {
                        name: resource,
                        program: "kubectl",
                        args: vec![
                            "get".to_owned(),
                            resource.to_owned(),
                            "-n".to_owned(),
                            namespace.clone(),
                            "-o".to_owned(),
                            "json".to_owned(),
                        ],
                    },
                    opts,
                );
                config_errors.extend(outcome.error);
                if let Some(value) = outcome.value {
                    for item in items(&value) {
                        if opts.stop.load(std::sync::atomic::Ordering::SeqCst) {
                            return json!({"error":"collection cancelled"});
                        }
                        let mut row = if resource == "configmaps" {
                            configmap_row(item)
                        } else {
                            secret_row(item)
                        };
                        // Budget the final representation: masking a short value
                        // can grow it. No raw configuration values are needed below.
                        if let Some(data) = row["data"].as_object_mut() {
                            for value in data.values_mut() {
                                *value = json!("[redacted]");
                            }
                        }
                        let bytes = serde_json::to_vec(&row)
                            .expect("JSON value serializes")
                            .len()
                            + 1;
                        if configmaps.len() + secrets.len() >= CONFIG_ROW_LIMIT
                            || config_bytes + bytes > CONFIG_BYTE_LIMIT
                        {
                            config_errors.push("configuration aggregate cap (500 rows / 1 MiB serialized rows); observations are partial".to_owned());
                            break 'namespaces;
                        }
                        config_bytes += bytes;
                        if resource == "configmaps" {
                            configmaps.push(row);
                        } else {
                            secrets.push(row);
                        }
                    }
                }
            }
        }
        configmaps.sort_by(|left, right| {
            left["namespace"]
                .as_str()
                .cmp(&right["namespace"].as_str())
                .then(left["name"].as_str().cmp(&right["name"].as_str()))
        });
        secrets.sort_by(|left, right| {
            left["namespace"]
                .as_str()
                .cmp(&right["namespace"].as_str())
                .then(left["name"].as_str().cmp(&right["name"].as_str()))
        });
    }

    // ------------------------------------------------------ database clusters
    let mut clusters: BTreeMap<String, Value> = BTreeMap::new();
    for pod in &pods {
        if opts.stop.load(std::sync::atomic::Ordering::SeqCst) {
            return json!({"error":"collection cancelled"});
        }
        let Some(component) = pod["component"].as_str() else {
            continue;
        };
        let namespace = pod["namespace"].as_str().unwrap_or("default").to_owned();
        let cluster_id = pod["cluster_id"].as_str().unwrap_or(&namespace).to_owned();
        let key = format!("{namespace}/{cluster_id}");
        let entry = clusters.entry(key.clone()).or_insert_with(|| {
            json!({
                "key": key,
                "namespace": namespace,
                "cluster_id": cluster_id,
                "object_store": Value::Null,
                "roles": Vec::<Value>::new(),
                "services": Vec::<Value>::new(),
                "license": Value::Null,
                "helm_releases": Vec::<Value>::new(),
                "pod_count": 0,
            })
        });
        entry["pod_count"] = json!(entry["pod_count"].as_u64().unwrap_or(0) + 1);

        if entry["object_store"].is_null() {
            let store = pod["containers"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|container| container["commands"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .find_map(|argument| {
                    argument
                        .strip_prefix("--object-store=")
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                });
            if let Some(store) = store {
                entry["object_store"] = Value::String(store);
            }
        }

        if entry["license"].is_null() {
            let mut license_type: Option<String> = None;
            let mut email: Option<String> = None;
            let mut token_present = false;
            for container in pod["containers"].as_array().into_iter().flatten() {
                for env in container["env"].as_array().into_iter().flatten() {
                    let Some(key) = env["key"].as_str() else {
                        continue;
                    };
                    let upper = key.to_ascii_uppercase();
                    if !upper.contains("LICENSE") {
                        continue;
                    }
                    token_present = token_present
                        || upper.contains("FILE")
                        || upper.contains("JWT")
                        || upper.contains("TOKEN");
                    if upper.ends_with("LICENSE_TYPE") {
                        license_type = env["value"].as_str().map(str::to_owned);
                    }
                    if upper.ends_with("LICENSE_EMAIL") {
                        email = env["value"].as_str().map(str::to_owned);
                    }
                }
            }
            if token_present || license_type.is_some() {
                entry["license"] = json!({
                    "type": license_type,
                    "email": email,
                    "token_present": token_present,
                });
            }
        }

        let roles = entry["roles"].as_array_mut().expect("roles is an array");
        let index = match roles
            .iter()
            .position(|role| role["role"].as_str() == Some(component))
        {
            Some(index) => index,
            None => {
                roles.push(json!({
                    "role": component,
                    "desired": 0,
                    "ready": 0,
                    "pods": Vec::<Value>::new(),
                    "workload": Value::Null,
                    "mode": Value::Null,
                }));
                roles.len() - 1
            }
        };
        let role = &mut roles[index];
        role["ready"] = json!(
            role["ready"].as_u64().unwrap_or(0)
                + u64::from(pod["ready"].as_bool().unwrap_or(false))
        );
        role["pods"].as_array_mut().expect("pods").push(json!({
            "name": pod["name"],
            "node": pod["node"],
            "ready": pod["ready"],
            "restarts": pod["restarts"],
            "mode": pod["mode"],
        }));
        if role["mode"].is_null() {
            role["mode"] = pod["mode"].clone();
        }
    }

    for cluster in clusters.values_mut() {
        if opts.stop.load(std::sync::atomic::Ordering::SeqCst) {
            return json!({"error":"collection cancelled"});
        }
        let namespace = cluster["namespace"]
            .as_str()
            .unwrap_or("default")
            .to_owned();
        let cluster_id = cluster["cluster_id"].as_str().unwrap_or("").to_owned();
        for service in &services {
            if service["namespace"].as_str() != Some(namespace.as_str()) {
                continue;
            }
            let matches_cluster = service["cluster_id"].as_str() == Some(cluster_id.as_str());
            let matches_namespace_cluster = cluster_id == namespace
                && service["cluster_id"].is_null()
                && service["component"].is_string();
            if matches_cluster || matches_namespace_cluster {
                if let Some(name) = service["name"].as_str() {
                    cluster["services"]
                        .as_array_mut()
                        .expect("services")
                        .push(Value::String(name.to_owned()));
                }
            }
        }
        for release in &helm {
            if release["namespace"].as_str() == Some(namespace.as_str()) {
                if let Some(name) = release["name"].as_str() {
                    cluster["helm_releases"]
                        .as_array_mut()
                        .expect("releases")
                        .push(Value::String(name.to_owned()));
                }
            }
        }
        let roles = cluster["roles"].as_array_mut().expect("roles");
        for role in roles.iter_mut() {
            let name = role["role"].as_str().unwrap_or("").to_owned();
            let mut owners = BTreeSet::new();
            let mut uncertain = false;
            for observed in role["pods"].as_array().into_iter().flatten() {
                let pod = pods.iter().find(|pod| {
                    pod["namespace"].as_str() == Some(namespace.as_str())
                        && pod["name"] == observed["name"]
                });
                let workload = pod.and_then(|pod| {
                    workloads.iter().find(|workload| {
                        workload["namespace"] == pod["namespace"]
                            && pod["owner_kind"].is_string()
                            && pod["owner_name"].is_string()
                            && workload["kind"] == pod["owner_kind"]
                            && workload["name"] == pod["owner_name"]
                    })
                });
                if let Some(workload) = workload {
                    owners.insert((
                        workload["kind"].as_str().unwrap_or("").to_owned(),
                        workload["name"].as_str().unwrap_or("").to_owned(),
                    ));
                } else {
                    uncertain = true; // ReplicaSet -> Deployment is not inferred.
                }
            }
            role["desired"] = Value::Null;
            role["workload"] = Value::Null;
            if !uncertain && !owners.is_empty() {
                let desired: u64 = workloads
                    .iter()
                    .filter(|workload| {
                        workload["namespace"].as_str() == Some(namespace.as_str())
                            && owners.contains(&(
                                workload["kind"].as_str().unwrap_or("").to_owned(),
                                workload["name"].as_str().unwrap_or("").to_owned(),
                            ))
                    })
                    .map(|workload| workload["desired"].as_u64().unwrap_or(0))
                    .sum();
                role["desired"] = json!(desired);
                if owners.len() == 1 {
                    role["workload"] = json!(owners.first().expect("nonempty owners").1);
                }
            }
            let ready = role["ready"].as_u64().unwrap_or(0);
            let desired = role["desired"].as_u64().unwrap_or(0);
            if ready < desired {
                push_issue(
                    &mut issues,
                    if ready == 0 { "critical" } else { "warning" },
                    format!("{name} role has {ready}/{desired} ready replicas"),
                    format!("cluster {cluster_id} in namespace {namespace}"),
                    Some(namespace.as_str()),
                );
            }
        }
        if !roles
            .iter()
            .any(|role| role["role"].as_str() == Some("querier"))
        {
            push_issue(
                &mut issues,
                "warning",
                format!("Querier not observed in collected scope for cluster {cluster_id}"),
                format!(
                    "{namespace}: role detection is heuristic and collection may be partial; check deployment configuration before drawing conclusions about query availability."
                ),
                Some(namespace.as_str()),
            );
        }
    }

    // ---------------------------------------------------------- pod-per-node
    let pods_per_node: BTreeMap<&str, u64> = pods.iter().fold(BTreeMap::new(), |mut map, pod| {
        if let Some(node) = pod["node"].as_str() {
            *map.entry(node).or_insert(0) += 1;
        }
        map
    });
    for node in &mut nodes {
        if let Some(count) = node["name"]
            .as_str()
            .and_then(|name| pods_per_node.get(name))
        {
            node["pods_used"] = json!(count);
        }
    }

    // --------------------------------------------------------- collector log
    let mut collectors: Vec<Value> = results
        .iter()
        .map(|outcome| {
            json!({
                "name": outcome.name,
                "ok": outcome.ok,
                "error": outcome.error,
                "duration_ms": outcome.duration_ms,
                "count": outcome.count,
            })
        })
        .collect();
    collectors.push(json!({
        "name": "configuration",
        "ok": config_errors.is_empty(),
        "error": (!config_errors.is_empty()).then(|| config_errors.join("; ")),
        "duration_ms": 0,
        "count": configmaps.len() + secrets.len(),
    }));

    let mut db3: Vec<Value> = clusters.into_values().collect();
    db3.sort_by(|left, right| left["key"].as_str().cmp(&right["key"].as_str()));

    let mut result = json!({
        "generated_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "tool_version": env!("CARGO_PKG_VERSION"),
        "context": context_name,
        "namespace_filter": opts.namespace,
        "cluster": {
            "server": server,
            "version": server_version["version"],
            "platform": server_version["platform"],
        },
        "nodes": nodes,
        "namespaces": namespaces,
        "pods": pods,
        "workloads": workloads,
        "services": services,
        "storage": {
            "classes": storage_classes,
            "volumes": volumes,
            "claims": claims,
        },
        "helm": helm,
        "helm_enabled": opts.helm,
        "configmaps": configmaps,
        "secrets": secrets,
        "secret_values_exposed": false,
        "network_policies": netpols,
        "events": events,
        "db3": db3,
        "issues": issues,
        "collectors": collectors,
    });
    // Derive database observations above before removing opaque values. Raw
    // command arguments and inline env values are not a safe display surface.
    for pod in result["pods"].as_array_mut().into_iter().flatten() {
        for container in pod["containers"].as_array_mut().into_iter().flatten() {
            container["commands"] = json!(["[redacted]"]);
            for env in container["env"].as_array_mut().into_iter().flatten() {
                env["value"] = json!("[redacted]");
                env["redacted"] = json!(true);
            }
        }
    }
    for config in result["configmaps"].as_array_mut().into_iter().flatten() {
        if let Some(data) = config["data"].as_object_mut() {
            for value in data.values_mut() {
                *value = json!("[redacted]");
            }
        }
    }
    result
}
