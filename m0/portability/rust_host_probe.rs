//! Non-production M0 probe: UTF-8 files, argument arrays, pipes and cancellation.
use std::fs;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Selects a child probe mode or runs the parent checks.
fn main() -> io::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--echo") => echo_line(),
        Some("--sleep") => sleep_until_killed(),
        _ => verify_host(),
    }
}

/// Creates a hidden child using an executable path and individual arguments.
fn child_command(mode: &str) -> io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command.arg(mode);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    Ok(command)
}

/// Echoes one input frame and places a non-ASCII argument on stderr.
fn echo_line() -> io::Result<()> {
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    io::stdout().write_all(line.as_bytes())?;
    io::stderr().write_all(std::env::args().nth(2).unwrap_or_default().as_bytes())
}

/// Announces readiness before waiting for parent-triggered cancellation.
fn sleep_until_killed() -> io::Result<()> {
    println!("ready");
    io::stdout().flush()?;
    std::thread::sleep(Duration::from_secs(30));
    Ok(())
}

/// Checks UTF-8 bytes through piped child stdio and a Unicode argument.
fn check_echo(payload: &str) -> io::Result<()> {
    let marker = "argument with spaces ü 数据";
    let mut child = child_command("--echo")?.arg(marker)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    child.stdin.take().expect("piped stdin").write_all(payload.as_bytes())?;
    let result = child.wait_with_output()?;
    assert!(result.status.success());
    assert_eq!(result.stdout, payload.as_bytes());
    assert_eq!(result.stderr, marker.as_bytes());
    Ok(())
}

/// Kills and reaps only the direct probe child; descendant cleanup is not tested.
fn check_cancellation() -> io::Result<u128> {
    let mut child = child_command("--sleep")?.stdout(Stdio::piped()).spawn()?;
    let mut reader = io::BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut ready = String::new();
    reader.read_line(&mut ready)?;
    assert_eq!(ready.trim(), "ready");
    let timer = Instant::now();
    child.kill()?;
    assert!(!child.wait()?.success());
    Ok(timer.elapsed().as_millis())
}

/// Checks file and process behavior from the caller-provided Unicode directory.
fn verify_host() -> io::Result<()> {
    let payload = "{\"text\":\"tenant_ü_数据_😀\\nescaped newline\"}\n";
    let path = std::env::current_dir()?.join("rust UTF8 ü 数据.json");
    fs::write(&path, payload.as_bytes())?;
    assert_eq!(fs::read_to_string(&path)?, payload);
    check_echo(payload)?;
    let elapsed = check_cancellation()?;
    println!("{{\"utf8_file_roundtrip\":true,\"pipe_roundtrip\":true,\"argument_intact\":true,\"stderr_separate\":true,\"direct_cancellation\":true,\"cancel_elapsed_ms\":{elapsed},\"descendants_tested\":false}}");
    Ok(())
}
