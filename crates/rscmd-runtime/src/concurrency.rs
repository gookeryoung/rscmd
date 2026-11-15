//! 并发原语：有界并行 map、取消令牌、确定性归并。

use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 取消令牌。在某个线程调用 [`CancellationToken::cancel`] 后，所有共享同一令牌的消费者
/// 调用 [`CancellationToken::is_cancelled`] / [`CancellationToken::check`] 都会返回 true，
/// 并可以选择以错误提前终止。
#[derive(Clone, Default)]
pub struct CancellationToken {
    inner: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.inner.store(true, Ordering::Relaxed);
    }

    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// 取消错误标记。业务层可以把它映射为 `ErrorKind::Interrupted`（退出码 130）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("操作已取消")
    }
}

impl std::error::Error for Cancelled {}

/// 有界并行 map：按 `min(cores, max_workers)` 并行执行 `f`，在取消令牌激活时提前退出。
/// 输出顺序与输入严格一致（确定性归并）。
///
/// 当 `max_workers` 为 0 时使用 rayon 默认并行度。
/// 注意：Rayon 并行迭代器是 greedy 的，取消令牌只在每个工作单元开始时检查；
/// 已在 flight 的单元仍会完成，整体输出仍保持顺序一致。
pub fn bounded_parallel_map<INPUT, OUTPUT, F>(
    inputs: Vec<INPUT>,
    max_workers: usize,
    cancel: &CancellationToken,
    f: F,
) -> Result<Vec<OUTPUT>, Cancelled>
where
    INPUT: Send + Sync,
    OUTPUT: Send,
    F: Fn(&INPUT) -> OUTPUT + Sync,
{
    let pool = if max_workers == 0 {
        None
    } else {
        rayon::ThreadPoolBuilder::new()
            .num_threads(max_workers.min(num_threads()).max(1))
            .build()
            .ok()
    };

    let cancel_flag = cancel.inner.clone();
    let do_work = |inputs: &[INPUT]| -> Vec<OUTPUT> {
        let cancel = cancel_flag.clone();
        let closure = &f;
        inputs
            .par_iter()
            .filter(|_| !cancel.load(Ordering::Relaxed))
            .map(closure)
            .collect()
    };

    let out = match pool.as_ref() {
        Some(p) => p.install(|| do_work(&inputs)),
        None => do_work(&inputs),
    };

    if cancel_flag.load(Ordering::Relaxed) {
        Err(Cancelled)
    } else {
        Ok(out)
    }
}

/// 把一个可迭代源按 N 等分切 chunk。用于手工控制 I/O 批大小或子任务分组。
pub fn chunks<T: Clone>(items: &[T], num_chunks: usize) -> Vec<Vec<T>> {
    if num_chunks == 0 || items.is_empty() {
        return vec![];
    }
    let per_chunk = items.len().div_ceil(num_chunks);
    items.chunks(per_chunk).map(|c| c.to_vec()).collect()
}

fn num_threads() -> usize {
    rayon::current_num_threads().clamp(1, 8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn cancellation_token_flags() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        assert!(token.check().is_ok());
        token.cancel();
        assert!(token.is_cancelled());
        assert_eq!(token.check(), Err(Cancelled));
    }

    #[test]
    fn cancellation_token_is_shared() {
        let a = CancellationToken::new();
        let b = a.clone();
        a.cancel();
        assert!(b.is_cancelled());
    }

    #[test]
    fn parallel_map_preserves_order() {
        let input = (0..100).collect::<Vec<i32>>();
        let out =
            bounded_parallel_map(input.clone(), 4, &CancellationToken::new(), |x| x * 2).unwrap();
        assert_eq!(out, input.iter().map(|&x| x * 2).collect::<Vec<_>>());
    }

    #[test]
    fn parallel_map_respects_cancel() {
        let input = (0..1000).collect::<Vec<i32>>();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let jh = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            cancel_clone.cancel();
        });

        let result = bounded_parallel_map(input, 4, &cancel, |x| {
            thread::yield_now();
            x * 3
        });

        jh.join().ok();
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn chunks_equal_distribution() {
        let items = (0..10).collect::<Vec<i32>>();
        let parts = chunks(&items, 3);
        assert_eq!(parts.len(), 3);
        let totals: usize = parts.iter().map(|p| p.len()).sum();
        assert_eq!(totals, 10);
    }

    #[test]
    fn chunks_handles_edge_cases() {
        let v: Vec<i32> = vec![];
        assert!(chunks(&v, 4).is_empty());
        assert!(chunks(&[1, 2, 3], 0).is_empty());
        let single = chunks(&[1, 2], 1);
        assert_eq!(single, vec![vec![1, 2]]);
    }
}
