//! 子进程封装：统一超时、输出有界、结果类型。

use std::io;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 子进程执行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ExecOutput {
    pub fn success(&self) -> bool {
        self.status.success()
    }

    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// 带超时的阻塞执行。超时会终止子进程并返回 `TimedOut` 错误。
///
/// 实现策略：
/// - 主线程持有 `Child`，以固定间隔 `try_wait` 检测子进程是否退出、是否超时；
/// - 后台线程独占 `stdout`/`stderr` 管道，持续读取直到 EOF，结束后通过 channel 发回缓冲；
/// - 主线程检测到超时即 `child.kill()` + `child.wait()` 清理，返回错误。
///
/// `max_bytes` 限制每个管道的缓冲上限，超过部分会截断（保留最后 N 字节）。
pub fn run(command: &mut Command, timeout: Duration, max_bytes: usize) -> io::Result<ExecOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let (tx, rx) = mpsc::channel::<(Vec<u8>, Vec<u8>)>();

    std::thread::spawn(move || {
        let mut out_buf = Vec::new();
        let mut err_buf = Vec::new();
        let _ = read_limited(stdout, &mut out_buf, max_bytes);
        let _ = read_limited(stderr, &mut err_buf, max_bytes);
        let _ = tx.send((out_buf, err_buf));
    });

    let start = Instant::now();
    let poll_interval = Duration::from_millis(10);

    loop {
        // 子进程自然退出 → 等线程读完管道 → 组装结果
        if let Ok(Some(status)) = child.try_wait() {
            let (stdout_vec, stderr_vec) = rx.recv().unwrap_or_default();
            return Ok(ExecOutput {
                status,
                stdout: stdout_vec,
                stderr: stderr_vec,
            });
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let status = child.wait().unwrap_or(ExitStatus::default());
            let _ = rx.recv(); // 丢弃线程已累积的部分输出
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "命令执行超过 {timeout:?}（子进程已终止，exit={:?}）",
                    status.code()
                ),
            ));
        }

        std::thread::sleep(poll_interval);
    }
}

fn read_limited<R: std::io::Read>(
    mut reader: R,
    out: &mut Vec<u8>,
    limit: usize,
) -> io::Result<()> {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                if out.len() > limit {
                    let extra = out.len() - limit;
                    out.drain(0..extra);
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn sleep_cmd(millis: u64) -> (String, Vec<String>) {
        (
            "sleep".to_string(),
            vec![format!("{}", millis as f64 / 1000.0)],
        )
    }

    #[cfg(windows)]
    fn sleep_cmd(millis: u64) -> (String, Vec<String>) {
        (
            "powershell".to_string(),
            vec![
                "-Command".to_string(),
                format!("Start-Sleep -Milliseconds {millis}"),
            ],
        )
    }

    #[cfg(windows)]
    fn echo_cmd(msg: &str) -> (String, Vec<String>) {
        (
            "powershell".to_string(),
            vec!["-Command".to_string(), format!("Write-Output '{msg}'")],
        )
    }

    #[cfg(not(windows))]
    fn echo_cmd(msg: &str) -> (String, Vec<String>) {
        (
            "sh".to_string(),
            vec!["-c".to_string(), format!("echo {msg}")],
        )
    }

    #[test]
    fn run_captures_output() {
        let (prog, args) = echo_cmd("hello");
        let mut cmd = Command::new(prog);
        cmd.args(&args);
        let out = run(&mut cmd, Duration::from_secs(2), 1024 * 1024).unwrap();
        assert!(out.success());
        assert!(out.stdout_text().contains("hello"));
    }

    #[test]
    fn run_times_out() {
        let (prog, args) = sleep_cmd(2000);
        let mut cmd = Command::new(prog);
        cmd.args(&args);
        let err = run(&mut cmd, Duration::from_millis(100), 1024).expect_err("应在 100ms 内超时");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn run_does_not_timeout_on_fast_command() {
        let (prog, args) = echo_cmd("quick");
        let mut cmd = Command::new(prog);
        cmd.args(&args);
        let out = run(&mut cmd, Duration::from_secs(5), 1024 * 1024).unwrap();
        assert!(out.success());
        assert!(out.stdout_text().contains("quick"));
    }

    #[test]
    fn run_truncates_large_output() {
        let (prog, args) = echo_cmd(&"x".repeat(10000));
        let mut cmd = Command::new(prog);
        cmd.args(&args);
        let out = run(&mut cmd, Duration::from_secs(2), 1024).unwrap();
        assert!(out.stdout.len() <= 1024, "max_bytes 应截断输出");
    }
}
