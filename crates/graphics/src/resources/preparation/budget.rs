//! Shared pixel/workspace reservations and bounded preparation admission.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use anyhow::{Result, anyhow, ensure};

#[derive(Clone, Debug)]
pub struct ResourceBudget(Arc<Budget>);

#[derive(Debug)]
struct Budget {
    bytes: AtomicUsize,
    resources: AtomicUsize,
    jobs: AtomicUsize,
    max_bytes: usize,
    max_resources: usize,
    max_jobs: usize,
}

#[derive(Clone, Debug)]
pub struct ResourceLease(Arc<Reservation>);

#[derive(Debug)]
struct Reservation {
    budget: ResourceBudget,
    bytes: usize,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.0.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
        self.budget.0.resources.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct JobPermit(ResourceBudget);

impl Drop for JobPermit {
    fn drop(&mut self) {
        self.0.0.jobs.fetch_sub(1, Ordering::AcqRel);
    }
}

fn acquire(counter: &AtomicUsize, amount: usize, limit: usize) -> bool {
    counter
        .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
            used.checked_add(amount).filter(|next| *next <= limit)
        })
        .is_ok()
}

impl ResourceBudget {
    pub fn new(max_bytes: usize, max_resources: usize, max_jobs: usize) -> Self {
        Self(Arc::new(Budget {
            bytes: AtomicUsize::new(0),
            resources: AtomicUsize::new(0),
            jobs: AtomicUsize::new(0),
            max_bytes,
            max_resources,
            max_jobs,
        }))
    }

    pub fn reserve(&self, bytes: usize) -> Result<ResourceLease> {
        ensure!(
            acquire(&self.0.resources, 1, self.0.max_resources),
            "graphics resource count budget exceeded"
        );
        if !acquire(&self.0.bytes, bytes, self.0.max_bytes) {
            self.0.resources.fetch_sub(1, Ordering::AcqRel);
            return Err(anyhow!("graphics pixel byte budget exceeded"));
        }

        Ok(ResourceLease(Arc::new(Reservation {
            budget: self.clone(),
            bytes,
        })))
    }

    pub fn job(&self) -> Result<JobPermit> {
        ensure!(
            acquire(&self.0.jobs, 1, self.0.max_jobs),
            "graphics preparation job budget exceeded"
        );

        Ok(JobPermit(self.clone()))
    }

    pub fn usage(&self) -> (usize, usize, usize) {
        (
            self.0.bytes.load(Ordering::Acquire),
            self.0.resources.load(Ordering::Acquire),
            self.0.jobs.load(Ordering::Acquire),
        )
    }
}

impl ResourceLease {
    pub fn bytes(&self) -> usize {
        self.0.bytes
    }

    pub fn reserve_related(&self, bytes: usize) -> Result<Self> {
        self.0.budget.reserve(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_leases_keep_queued_resources_accounted_until_the_last_reference() {
        let budget = ResourceBudget::new(100, 2, 1);
        let first = budget.reserve(60).unwrap();
        let queued = first.clone();
        assert!(budget.reserve(41).is_err());
        let second = budget.reserve(40).unwrap();
        assert!(budget.reserve(0).is_err());
        drop(first);
        assert_eq!(budget.usage(), (100, 2, 0));
        drop(queued);
        assert_eq!(budget.usage(), (40, 1, 0));
        drop(second);
        assert_eq!(budget.usage(), (0, 0, 0));
    }
}
