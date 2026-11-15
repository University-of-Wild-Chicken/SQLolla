//! Private process transport with bounded output and per-request deadlines.
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

/// Reap an owned process on every success, timeout and early-error path.
struct OwnedChild(Child);

impl Drop for OwnedChild {
    /// Terminate remaining work before releasing its process handle.
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A private persistent child with bounded request and response queues.
pub struct Worker {
    child: OwnedChild,
    requests: SyncSender<String>,
    responses: Receiver<Result<String, String>>,
    failed: bool,
}

impl Worker {
    /// Start private pipe threads so deadlines also cover a blocked child stdin.
    pub fn spawn(command: Command) -> Result<Self, String> {
        let mut child = spawn_owned(command)?;
        let input = child.0.stdin.take().ok_or("missing worker stdin")?;
        let stdout = child.0.stdout.take().ok_or("missing worker stdout")?;
        let (requests, pending) = mpsc::sync_channel(1);
        let (sender, responses) = mpsc::sync_channel(1);
        let errors = sender.clone();
        std::thread::spawn(move || write_requests(input, pending, errors));
        std::thread::spawn(move || read_responses(stdout, sender));
        Ok(Self {
            child,
            requests,
            responses,
            failed: false,
        })
    }

    /// Bound queue admission, writing and response receipt under one request deadline.
    pub fn request(&mut self, line: &str) -> Result<String, String> {
        if self.failed {
            return Err("worker unavailable after an earlier transport failure".into());
        }
        if line.len() > 8_388_608 {
            return Err("worker request exceeds eight MiB".into());
        }
        let result = self
            .requests
            .try_send(line.to_owned())
            .map_err(|e| format!("worker request admission: {e}"))
            .and_then(|()| {
                self.responses
                    .recv_timeout(Duration::from_secs(30))
                    .map_err(|e| format!("worker timeout/closed pipe: {e}"))?
            });
        if result.is_err() {
            self.failed = true;
            let _ = self.child.0.kill();
        }
        result
    }
}

/// Configure hidden shell-free child execution with private stdin/stdout pipes.
fn spawn_owned(mut command: Command) -> Result<OwnedChild, String> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .spawn()
        .map(OwnedChild)
        .map_err(|e| format!("process startup: {e}"))
}

/// Serialize bounded queued requests; a failed write closes this worker's request lane.
fn write_requests(
    mut input: ChildStdin,
    pending: Receiver<String>,
    errors: SyncSender<Result<String, String>>,
) {
    for line in pending {
        let result = writeln!(input, "{line}").and_then(|()| input.flush());
        if let Err(error) = result {
            let _ = errors.send(Err(format!("worker stdin: {error}")));
            break;
        }
    }
}

/// Read a complete bounded newline frame, never a partial record or oversized remainder.
fn read_record(reader: &mut impl BufRead, maximum: usize) -> Result<String, String> {
    let mut bytes = Vec::new();
    let size = reader
        .take((maximum + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if size == 0 {
        return Err("worker stdout closed".into());
    }
    if size > maximum {
        return Err("worker output exceeds eight MiB".into());
    }
    if bytes.last() != Some(&b'\n') {
        return Err("worker stdout closed inside a record".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Stop after framing failure and retain at most one queued response.
fn read_responses(stdout: ChildStdout, sender: SyncSender<Result<String, String>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let response = read_record(&mut reader, 8_388_608);
        let failed = response.is_err();
        if sender.send(response).is_err() || failed {
            break;
        }
    }
}

/// Write one raw SQL input asynchronously and close stdin to complete request framing.
fn write_parser(mut input: ChildStdin, sql: String, errors: SyncSender<Result<String, String>>) {
    if let Err(error) = input.write_all(sql.as_bytes()) {
        let _ = errors.send(Err(format!("parser stdin: {error}")));
    }
}

/// Capture all parser stdout so trailing records cannot hide behind a valid first line.
fn read_parser(stdout: ChildStdout, sender: SyncSender<Result<String, String>>) {
    let mut bytes = Vec::new();
    let result = stdout
        .take(1_048_578)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())
        .and_then(|size| {
            if size > 1_048_577 {
                Err("parser output exceeds one MiB plus newline".into())
            } else {
                String::from_utf8(bytes).map_err(|e| e.to_string())
            }
        });
    let _ = sender.send(result);
}

/// Reap only after process completion, with the same deadline used for pipe operations.
fn await_exit(child: &mut Child, deadline: Instant) -> Result<ExitStatus, String> {
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err("parser process exit deadline exceeded".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Preserve SQL on stdin and require complete UTF-8 output plus successful process exit.
pub fn parse_sql(executable: &Path, sql: &str) -> Result<String, String> {
    if sql.len() > 65_536 {
        return Err("SQL exceeds 64 KiB".into());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut child = spawn_owned(Command::new(executable))?;
    let input = child.0.stdin.take().ok_or("parser stdin missing")?;
    let stdout = child.0.stdout.take().ok_or("parser stdout missing")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let errors = sender.clone();
    let source = sql.to_owned();
    std::thread::spawn(move || write_parser(input, source, errors));
    std::thread::spawn(move || read_parser(stdout, sender));
    let result = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|e| format!("parser timeout/closed pipe: {e}"))??;
    if !await_exit(&mut child.0, deadline)?.success() {
        return Err("parser process exited unsuccessfully".into());
    }
    Ok(result)
}

/// Locate the workspace without relying on the caller's current directory.
pub fn root() -> PathBuf {
    std::env::var_os("SQLolla_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

/// Prefer a deployment override, otherwise use an explicitly documented local default.
pub fn tool(variable: &str, fallback: impl AsRef<Path>) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join(fallback))
}

#[cfg(test)]
mod tests {
    use super::read_record;
    use std::io::Cursor;

    /// A nonterminated JSON fragment is not a complete persistent-worker response.
    #[test]
    fn rejects_partial_records() {
        assert!(read_record(&mut Cursor::new(b"{}"), 32).is_err());
        assert_eq!(read_record(&mut Cursor::new(b"{}\n"), 32).unwrap(), "{}\n");
    }

    /// Refuse frames beyond the configured capture budget before decoding them.
    #[test]
    fn rejects_oversized_records() {
        assert!(read_record(&mut Cursor::new(b"012345\n"), 4).is_err());
    }
}
