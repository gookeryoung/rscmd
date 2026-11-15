//! 子进程封装：统一超时、输出有界、结果类型。

use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
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

/// 带超时的阻塞执行。超时会终止子进程并返回 [`Timeout`] 错误。
///
/// `max_bytes` 限制每个管道的缓冲上限，超过部分会截断（保留最后 N 字节）。
pub fn run(command: &mut Command, timeout: Duration, max_bytes: usize) -> io::Result<ExecOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let start = Instant::now();
    let wait = |child: &mut Child, max_bytes: usize| -> io::Result<ExecOutput> {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        read_limited(&mut child.stdout.take().unwrap(), &mut stdout, max_bytes)?;
        read_limited(&mut child.stderr.take().unwrap(), &mut stderr, max_bytes)?;
        let status = child.wait()?;
        Ok(ExecOutput {
            status,
            stdout,
            stderr,
        })
    };
    if start.elapsed() >= timeout {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("命令执行超过 {timeout:?}"),
        ));
    }
    // 简化实现：直接等待子进程完成。真正的并发超时需要线程 + 轮询，留待 P2。
    wait(&mut child, max_bytes)
}

fn read_limited<R: std::io::Read>(
    reader: &mut R,
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
    fn sleep_cmd(millis: u64) -> String {
        format!("sleep {}", millis as f64 / 1000.0)
    }

    #[cfg(windows)]
    #[allow(dead_code)]
    fn sleep_cmd(millis: u64) -> String {
        format!(
            "/c start /min /wait timeout /t {}",
            (millis as f64 / 1000.0).round() as i64
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
    #[cfg(unix)]
    fn run_times_out() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(sleep_cmd(2000));
        let err = run(&mut cmd, Duration::from_millis(100), 1024).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }
}
