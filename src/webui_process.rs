//! Bounded, cancellable WebUI collectors. Support bundle capture is unchanged.
use crate::Captured;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};
use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const LIMIT: u64 = 4 * 1024 * 1024;

pub(crate) fn capture(
    program: &str,
    args: &[String],
    timeout: f64,
    stop: &AtomicBool,
) -> io::Result<Captured> {
    if stop.load(Ordering::SeqCst) {
        return Err(io::Error::other("collector cancelled"));
    }
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // Stderr can contain arbitrary credentials, metadata, and key names.
        // Never retain or forward it into the dashboard or log endpoint.
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| io::Error::other("collector executable unavailable"))?;
    let pid = Pid::from_raw(i32::try_from(child.id()).expect("process ID fits pid_t"))
        .expect("positive process ID");
    let output = child.stdout.take().expect("piped stdout");
    let overflow = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&overflow);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = output.take(LIMIT + 1).read_to_end(&mut bytes);
        flag.store(bytes.len() as u64 > LIMIT, Ordering::SeqCst);
        result.map(|_| bytes)
    });
    let started = Instant::now();
    let error = loop {
        if stop.load(Ordering::SeqCst) {
            break Some("collector cancelled".to_owned());
        }
        if overflow.load(Ordering::SeqCst) {
            break Some("collector output exceeded 4 MiB limit".to_owned());
        }
        if started.elapsed().as_secs_f64() >= timeout {
            break Some("collector timed out".to_owned());
        }
        // WNOWAIT observes termination without reaping. The child retains its
        // PID/group ID until killpg below, avoiding a recycled process-group ID.
        match waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Ok(None) => {}
            Ok(_) => break None,
            Err(_) => break Some("collector wait failed".to_owned()),
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = kill_process_group(pid, Signal::KILL);
    let status = child.wait()?; // Reap the direct child on every completion path.
    let bytes = reader
        .join()
        .map_err(|_| io::Error::other("collector reader failed"))??;
    let error = if bytes.len() as u64 > LIMIT {
        Some("collector output exceeded 4 MiB limit".to_owned())
    } else {
        error.or_else(|| {
            (!status.success()).then(|| {
                format!(
                    "collector exited with status {}",
                    status.code().unwrap_or(-1)
                )
            })
        })
    };
    Ok(Captured {
        stdout: if error.is_none() {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            String::new()
        },
        stderr: String::new(),
        returncode: status.code(),
        error,
    })
}

const STDERR_LIMIT: u64 = 64 * 1024;

/// Run a command with one line on stdin and a retained, bounded stderr.
///
/// Used only by the opt-in query terminal. It needs stdin to hand the admin
/// token to the remote shell without putting it in the argument list, and it
/// needs the `influxdb3` error text so the operator can see why a statement
/// failed. Ambient collectors keep using [`capture`], which discards stderr.
pub(crate) fn capture_exec(
    program: &str,
    args: &[String],
    stdin_text: &str,
    timeout: f64,
    stop: &AtomicBool,
) -> io::Result<Captured> {
    if stop.load(Ordering::SeqCst) {
        return Err(io::Error::other("query cancelled"));
    }
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|_| io::Error::other("collector executable unavailable"))?;
    let pid = Pid::from_raw(i32::try_from(child.id()).expect("process ID fits pid_t"))
        .expect("positive process ID");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = stdin_text.as_bytes().to_vec();
    // A separate writer avoids blocking when the child never reads stdin.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
        // Dropping the handle closes the pipe so the remote `read` sees EOF.
    });
    let overflow = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&overflow);
    let out_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(LIMIT + 1).read_to_end(&mut bytes);
        flag.store(bytes.len() as u64 > LIMIT, Ordering::SeqCst);
        result.map(|_| bytes)
    });
    let err_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(STDERR_LIMIT).read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    let error = loop {
        if stop.load(Ordering::SeqCst) {
            break Some("query cancelled".to_owned());
        }
        if overflow.load(Ordering::SeqCst) {
            break Some("query output exceeded 4 MiB limit".to_owned());
        }
        if started.elapsed().as_secs_f64() >= timeout {
            break Some("query timed out".to_owned());
        }
        match waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Ok(None) => {}
            Ok(_) => break None,
            Err(_) => break Some("query wait failed".to_owned()),
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = kill_process_group(pid, Signal::KILL);
    let status = child.wait()?;
    let _ = writer.join();
    let bytes = out_reader
        .join()
        .map_err(|_| io::Error::other("query reader failed"))??;
    let stderr_bytes = err_reader.join().unwrap_or_default();
    let error = if bytes.len() as u64 > LIMIT {
        Some("query output exceeded 4 MiB limit".to_owned())
    } else {
        error.or_else(|| {
            (!status.success())
                .then(|| format!("query exited with status {}", status.code().unwrap_or(-1)))
        })
    };
    Ok(Captured {
        stdout: String::from_utf8_lossy(&bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
        returncode: status.code(),
        error,
    })
}
