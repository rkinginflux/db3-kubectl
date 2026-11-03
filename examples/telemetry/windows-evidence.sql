-- Optional Windows-side evidence database. Never execute this against InfluxDB.
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS runs (
    run_id TEXT PRIMARY KEY,
    target_cluster_id TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    telemetry_contract TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS configuration_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    first_observed_at TEXT NOT NULL,
    configuration_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS observations (
    observation_id INTEGER PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    served_at TEXT NOT NULL,
    config_snapshot_id TEXT REFERENCES configuration_snapshots(snapshot_id),
    configuration_fresh INTEGER NOT NULL,
    metrics_fresh INTEGER NOT NULL,
    collection_status TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS metric_samples (
    observation_id INTEGER NOT NULL REFERENCES observations(observation_id),
    metric_id TEXT NOT NULL,
    labels_json TEXT NOT NULL,
    evaluated_at TEXT,
    value REAL,
    unit TEXT NOT NULL,
    quality_status TEXT NOT NULL,
    PRIMARY KEY (observation_id, metric_id, labels_json)
);
CREATE TABLE IF NOT EXISTS writer_samples (
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    observed_at TEXT NOT NULL,
    measurements_json TEXT NOT NULL,
    PRIMARY KEY (run_id, observed_at)
);
CREATE TABLE IF NOT EXISTS container_health (
    observation_id INTEGER NOT NULL REFERENCES observations(observation_id),
    pod TEXT NOT NULL,
    pod_uid TEXT NOT NULL,
    container_id TEXT,
    restarts INTEGER NOT NULL,
    current_state TEXT NOT NULL,
    last_termination_json TEXT,
    current_termination_json TEXT,
    PRIMARY KEY (observation_id, pod)
);
CREATE TABLE IF NOT EXISTS throttle_decisions (
    decision_id INTEGER PRIMARY KEY,
    observation_id INTEGER REFERENCES observations(observation_id),
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    decided_at TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('observe', 'advisory', 'automatic', 'manual')),
    policy_version TEXT NOT NULL,
    before_caps_json TEXT NOT NULL,
    after_caps_json TEXT NOT NULL,
    reasons_json TEXT NOT NULL
);
