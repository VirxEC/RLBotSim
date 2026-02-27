use std::time::{Duration, Instant};

pub struct LockstepSpeedTracker {
    window_start: Option<Instant>,
    steps_in_window: u64,
    last_report: Option<Instant>,
    window_duration: Duration,
    report_interval: Duration,
}

impl LockstepSpeedTracker {
    pub const fn new(window_duration: Duration, report_interval: Duration) -> Self {
        Self {
            window_start: None,
            steps_in_window: 0,
            last_report: None,
            window_duration,
            report_interval,
        }
    }

    pub const fn reset(&mut self) {
        self.window_start = None;
        self.steps_in_window = 0;
        self.last_report = None;
    }

    pub fn on_step(&mut self, tps: u8) -> Option<f64> {
        let now = Instant::now();
        let window_start = self.window_start.get_or_insert(now);
        self.steps_in_window += 1;

        let window_elapsed = now.duration_since(*window_start);
        if window_elapsed >= self.window_duration {
            let steps_per_sec = self.steps_in_window as f64 / window_elapsed.as_secs_f64();
            let speed = steps_per_sec / f64::from(tps);

            *window_start = now;
            self.steps_in_window = 0;

            let should_report = self
                .last_report
                .is_none_or(|last| now.duration_since(last) >= self.report_interval);

            if should_report {
                self.last_report = Some(now);
                return Some(speed);
            }
        }

        None
    }
}
