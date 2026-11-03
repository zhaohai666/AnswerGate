//! 可注入时钟（架构 C11 / T01）：生产用 `SystemClock`，测试用 `MockClock`。
//!
//! T01 仅需 epoch 秒精度（`round_started_epoch` 与 `remaining_sec` 均为秒级）；
//! 后续 monitor tokio 循环如需 sleep 注入，再扩展 trait，不提前抽象。

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// 时钟抽象：`now()` 返回 Unix epoch 秒。
pub trait Clock: Send + Sync + 'static {
    /// 当前 Unix epoch 秒。
    fn now(&self) -> u64;
}

/// 生产实现：系统真实时间。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// 测试实现：固定起点 + 手动 `advance` 推进（测试不真等）。
#[derive(Debug, Default)]
pub struct MockClock {
    current: Mutex<u64>,
}

impl MockClock {
    /// 以固定 epoch 秒为起点构造。
    pub fn new(start_epoch: u64) -> Self {
        MockClock {
            current: Mutex::new(start_epoch),
        }
    }

    /// 时钟前进 sec 秒（饱和，不回绕）。
    pub fn advance(&self, sec: u64) {
        let mut cur = self.current.lock().unwrap();
        *cur = cur.saturating_add(sec);
    }

    /// 直接设置时间（构造"回拨/重启"等场景）。
    pub fn set(&self, epoch: u64) {
        *self.current.lock().unwrap() = epoch;
    }
}

impl Clock for MockClock {
    fn now(&self) -> u64 {
        *self.current.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_returns_plausible_epoch_seconds() {
        // 2020-01-01 之后的合理值，防呆不校验精确时间
        assert!(SystemClock.now() > 1_577_836_800);
    }

    #[test]
    fn mock_clock_advance_and_set() {
        let clock = MockClock::new(100);
        assert_eq!(clock.now(), 100);
        clock.advance(30);
        assert_eq!(clock.now(), 130);
        clock.set(50);
        assert_eq!(clock.now(), 50);
    }

    #[test]
    fn mock_clock_advance_saturates() {
        let clock = MockClock::new(u64::MAX);
        clock.advance(10);
        assert_eq!(clock.now(), u64::MAX);
    }
}
