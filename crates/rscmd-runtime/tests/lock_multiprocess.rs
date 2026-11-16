//! 跨进程锁的集成测试：8 个进程争抢同一把锁，只允许一个成功。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rscmd_runtime::fs::temp_dir;
use rscmd_runtime::lock::DirLock;

const CHILD_ENV: &str = "RSCMD_LOCK_CHILD_DIR";
const START_FILE: &str = "start";
const CONTENDERS: usize = 8;

/// 子进程入口：父测试用 `--exact` 单独拉起；常规全量运行时不带环境变量，直接返回。
#[test]
fn child_lock_contender() {
    let Some(root) = std::env::var_os(CHILD_ENV).map(PathBuf::from) else {
        return;
    };
    std::fs::write(root.join(format!("ready-{}", std::process::id())), b"1").unwrap();
    wait_until(&root.join(START_FILE), Duration::from_secs(60));
    match DirLock::try_acquire(&root, "contention") {
        Ok(lock) => {
            // 持锁一段时间，保证其他竞争者必然观察到占用状态。
            std::thread::sleep(Duration::from_millis(500));
            std::fs::write(root.join(format!("won-{}", std::process::id())), b"1").unwrap();
            drop(lock);
        }
        Err(err) => {
            std::fs::write(
                root.join(format!("busy-{}", std::process::id())),
                err.code(),
            )
            .unwrap();
        }
    }
}

#[test]
fn eight_processes_race_for_one_lock() {
    let dir = temp_dir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let mut children = Vec::new();
    for _ in 0..CONTENDERS {
        children.push(
            Command::new(&exe)
                .args(["--exact", "child_lock_contender", "--nocapture"])
                .env(CHILD_ENV, dir.path())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("拉起子测试进程失败"),
        );
    }

    // 等全部子进程就绪后再放行，避免进程启动耗时决定谁先拿到锁。
    wait_until_count(dir.path(), "ready-", CONTENDERS, Duration::from_secs(60));
    std::fs::write(dir.path().join(START_FILE), b"1").unwrap();

    for mut child in children {
        let status = child.wait().unwrap();
        assert!(status.success(), "子进程异常退出: {status}");
    }

    let won = count_prefixed(dir.path(), "won-");
    let busy = count_prefixed(dir.path(), "busy-");
    assert_eq!(won, 1, "应只有一个进程拿到锁，实际 {won}");
    assert_eq!(busy, CONTENDERS - 1, "其余进程应报告忙碌，实际 {busy}");
    for entry in std::fs::read_dir(dir.path()).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("busy-") {
            assert_eq!(
                std::fs::read_to_string(entry.path()).unwrap(),
                "LOCK_BUSY",
                "{name} 的错误码不是 LOCK_BUSY"
            );
        }
    }
    // 锁文件在持有者释放后必须消失。
    assert!(!dir.path().join("contention.lock").exists());
}

fn count_prefixed(dir: &Path, prefix: &str) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .count()
}

fn wait_until_count(dir: &Path, prefix: &str, expected: usize, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while count_prefixed(dir, prefix) < expected {
        assert!(
            Instant::now() < deadline,
            "等待 {expected} 个子进程就绪超时"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_until(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(Instant::now() < deadline, "等待 {} 超时", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}
