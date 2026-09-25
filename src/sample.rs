//! The sampling loop: `count + 1` snapshots, `interval` seconds apart.

use std::time::{Duration, Instant};

use crate::check::Check;
use crate::source::Source;

pub fn run(checks: &mut [Box<dyn Check>], src: &dyn Source, interval: f64, count: usize) {
    let start = Instant::now();
    for i in 0..=count {
        if i > 0 {
            // Sleep until the next tick boundary so per-check read time doesn't drift the window.
            let target = start + Duration::from_secs_f64(interval * i as f64);
            if let Some(wait) = target.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
        let t = start.elapsed().as_secs_f64();
        for c in checks.iter_mut() {
            c.sample(src, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Context, Section};
    use crate::source::MemSource;

    #[test]
    fn takes_count_plus_one_samples_spaced_by_interval() {
        struct Times(std::rc::Rc<std::cell::RefCell<Vec<f64>>>);
        impl Check for Times {
            fn id(&self) -> &'static str {
                "times"
            }
            fn sample(&mut self, _: &dyn Source, t: f64) {
                self.0.borrow_mut().push(t);
            }
            fn evaluate(&self, _: &Context) -> Section {
                Section::new("times", "Times", "-")
            }
        }
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut checks: Vec<Box<dyn Check>> = vec![Box::new(Times(seen.clone()))];
        run(&mut checks, &MemSource::new(), 0.01, 3);
        let t = seen.borrow();
        assert_eq!(t.len(), 4);
        assert!(t.windows(2).all(|w| w[1] > w[0]));
        assert!(t[3] >= 0.03);
    }
}
