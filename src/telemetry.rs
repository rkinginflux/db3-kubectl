//! Opt-in connection to the fixed, loopback migration telemetry provider.
use serde_json::Value;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::AtomicBool;

pub(crate) struct Access {
    token: String,
}

impl Access {
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|_| "telemetry credential file unavailable")?;
        let metadata = file
            .metadata()
            .map_err(|_| "telemetry credential metadata unavailable")?;
        if !metadata.is_file()
            || metadata.mode() & 0o077 != 0
            || metadata.uid() != rustix::process::getuid().as_raw()
        {
            return Err("telemetry credential must be an owner-only regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(16385)
            .read_to_end(&mut bytes)
            .map_err(|_| "telemetry credential read failed")?;
        if bytes.len() > 16384 {
            return Err("telemetry credential file exceeds limit".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "invalid telemetry credential JSON")?;
        Self::from_value(&value)
    }

    fn from_value(value: &Value) -> Result<Self, String> {
        let token = value["token"].as_str().ok_or("telemetry token missing")?;
        if !(32..=128).contains(&token.len())
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err("invalid telemetry credential format".into());
        }
        if value["auth_type"].as_str() != Some("bearer")
            || value["port"].as_u64() != Some(9091)
            || value["expires_at_unix"].as_f64().unwrap_or(0.0)
                <= chrono::Utc::now().timestamp() as f64
        {
            return Err("telemetry credential expired or incompatible".into());
        }
        Ok(Self {
            token: token.to_owned(),
        })
    }

    pub(crate) fn fetch(
        &self,
        history: Option<(&str, f64, f64, f64)>,
        stop: &AtomicBool,
    ) -> Result<Value, (u16, String)> {
        let path = if let Some((metric, start, end, step)) = history {
            if !valid_metric(metric) || !valid_range(start, end, step) {
                return Err((400, "invalid telemetry history range".into()));
            }
            format!("/v1/history?metric={metric}&start={start}&end={end}&step={step}")
        } else {
            "/v1/snapshot".to_owned()
        };
        // Curl's config is passed on stdin, never in argv, a URL, or a browser.
        // -q ignores ambient .curlrc; no redirects or proxies are permitted.
        let args: Vec<String> = [
            "-q",
            "--silent",
            "--noproxy",
            "*",
            "--proxy",
            "",
            "--proto",
            "=http",
            "--connect-timeout",
            "2",
            "--max-time",
            "8",
            "--config",
            "-",
            "--write-out",
            "\n%{http_code}",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .chain(std::iter::once(format!("http://127.0.0.1:9091{path}")))
        .collect();
        let input = format!("header = \"Authorization: Bearer {}\"\n", self.token);
        let captured = crate::webui_process::capture_exec("curl", &args, &input, 10.0, stop)
            .map_err(|_| (503, "telemetry transport unavailable".to_owned()))?;
        if captured.error.is_some() {
            return Err((503, "telemetry provider unavailable".into()));
        }
        let (body, code) = captured
            .stdout
            .rsplit_once('\n')
            .ok_or((503, "telemetry response unavailable".to_owned()))?;
        if code == "429" {
            return Err((429, "telemetry history busy; retry shortly".into()));
        }
        if code != "200" {
            return Err((
                503,
                "telemetry provider unavailable or credential expired".into(),
            ));
        }
        let safe = crate::redact(body, &self.token);
        let result: Value = serde_json::from_str(&safe)
            .map_err(|_| (503, "telemetry response invalid".to_owned()))?;
        if result["schema_version"].as_str() != Some("1.0") {
            return Err((503, "unsupported telemetry contract".into()));
        }
        Ok(result)
    }
}

pub(crate) fn valid_metric(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub(crate) fn valid_range(start: f64, end: f64, step: f64) -> bool {
    [start, end, step].iter().all(|n| n.is_finite())
        && start >= 0.0
        && start < end
        && end - start <= 86400.0
        && end <= chrono::Utc::now().timestamp() as f64 + 15.0
        && step >= 15.0
        && (end - start) / step <= 2000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> Value {
        json!({"token": "t".repeat(43), "auth_type": "bearer", "port": 9091,
               "expires_at_unix": 9999999999u64})
    }

    #[test]
    fn rejects_expired_and_injectable_credentials() {
        let mut value = fixture();
        assert!(Access::from_value(&value).is_ok());
        value["token"] = json!("x\"\nurl = http://evil.test");
        assert!(Access::from_value(&value).is_err());
        value = fixture();
        value["expires_at_unix"] = json!(0);
        assert!(Access::from_value(&value).is_err());
    }

    #[test]
    fn requires_private_credential_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("access.json");
        std::fs::write(&path, fixture().to_string()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Access::load(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(Access::load(&path).is_ok());
    }

    #[test]
    fn bounds_and_validates_history_input() {
        assert!(valid_metric("compactor_plans_active"));
        assert!(!valid_metric("up&query=secret"));
        assert!(!valid_metric("../secret"));
        assert!(valid_range(1.0, 301.0, 30.0));
        assert!(!valid_range(1.0, 999999.0, 30.0));
        assert!(!valid_range(1.0, 301.0, f64::NAN));
        assert!(!valid_range(1.0, 301.0, 0.0));
    }
}
