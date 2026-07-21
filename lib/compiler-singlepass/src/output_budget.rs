use std::sync::atomic::{AtomicUsize, Ordering};

use wasmer_types::CompileError;

use crate::config::output_size_limit_error;

/// Shared emitted-code allowance for one module compilation.
#[derive(Debug)]
pub(crate) struct OutputBudget {
    limit: usize,
    total: AtomicUsize,
}

impl OutputBudget {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            total: AtomicUsize::new(0),
        }
    }

    pub(crate) fn reserve(&self, delta: usize) -> Result<(), CompileError> {
        if delta == 0 {
            return Ok(());
        }

        match self
            .total
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                total
                    .checked_add(delta)
                    .filter(|&new_total| new_total <= self.limit)
            }) {
            Ok(_) => Ok(()),
            Err(total) => Err(output_size_limit_error(
                total.saturating_add(delta),
                self.limit,
            )),
        }
    }

    #[cfg(test)]
    fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[test]
    fn reservation_limit_is_inclusive() {
        const LIMIT: usize = 10 * 1024 * 1024;
        let budget = OutputBudget::new(LIMIT);
        assert!(budget.reserve(LIMIT).is_ok());
        assert_eq!(budget.total(), LIMIT);
        assert!(matches!(
            budget.reserve(1),
            Err(CompileError::Codegen(message))
                if message == format!(
                    "singlepass compiler output exceeds limit: {} > {LIMIT} bytes",
                    LIMIT + 1
                )
        ));
        assert_eq!(budget.total(), LIMIT);
    }

    #[test]
    fn parallel_reservations_cannot_exceed_limit() {
        const LIMIT: usize = 10_000;
        let budget = Arc::new(OutputBudget::new(LIMIT));
        let successful = Arc::new(AtomicUsize::new(0));

        std::thread::scope(|scope| {
            for _ in 0..8 {
                let budget = Arc::clone(&budget);
                let successful = Arc::clone(&successful);
                scope.spawn(move || {
                    for _ in 0..2_000 {
                        if budget.reserve(1).is_ok() {
                            successful.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });

        assert_eq!(successful.load(Ordering::Relaxed), LIMIT);
        assert_eq!(budget.total(), LIMIT);
        assert!(matches!(
            budget.reserve(1),
            Err(CompileError::Codegen(message))
                if message == "singlepass compiler output exceeds limit: 10001 > 10000 bytes"
        ));
    }
}
