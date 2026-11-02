//! Ready-resource metadata and bounded backend-owned storage. No CPU readback contract.
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceKey {
    pub id: u64,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelLayout {
    Rgba8,
    Yuva420p8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alpha {
    Straight,
    PremultipliedEncoded,
    PremultipliedLinear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorEncoding {
    /// Unknown is explicit, not a request to assume sRGB or BT.709.
    Unspecified,
    Srgb,
    Bt601Limited,
    Bt709Limited,
}

/// Describes currently supported CPU results without requiring byte access.
/// More layouts/encodings can be added with a backend that actually implements them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceDescriptor {
    pub width: u32,
    pub height: u32,
    pub layout: PixelLayout,
    pub alpha: Alpha,
    pub color: ColorEncoding,
    pub storage_bytes: usize,
}

impl SurfaceDescriptor {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.width > 0 && self.height > 0,
            "surface dimensions must be positive"
        );
        ensure!(
            self.storage_bytes > 0,
            "surface storage must be accounted for"
        );
        let pixels = u128::from(self.width) * u128::from(self.height);
        let minimum = match self.layout {
            PixelLayout::Rgba8 => pixels * 4,
            PixelLayout::Yuva420p8 => pixels * 5 / 2,
        };
        ensure!(
            self.storage_bytes as u128 >= minimum,
            "surface storage is smaller than its pixel layout"
        );
        if self.layout == PixelLayout::Yuva420p8 {
            ensure!(
                self.width.is_multiple_of(2) && self.height.is_multiple_of(2),
                "YUVA420 dimensions must be even"
            );
            ensure!(
                self.alpha == Alpha::Straight,
                "the CPU YUV backend requires straight alpha"
            );
        }

        Ok(())
    }
}

/// Scenes keep this lease, never a backend frame or texture. The backend keeps
/// pixels alive until every queued scene/frame has released its lease.
#[derive(Debug)]
pub struct Resource {
    pub key: ResourceKey,
    pub descriptor: SurfaceDescriptor,
}

struct Entry<T> {
    lease: Weak<Resource>,
    pixels: Arc<T>,
    bytes: usize,
}

pub struct SurfaceStore<T> {
    entries: BTreeMap<ResourceKey, Entry<T>>,
    max_entries: usize,
    max_bytes: usize,
}

impl<T> SurfaceStore<T> {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            max_entries,
            max_bytes,
        }
    }

    /// Preparation-side operation. Active/queued leases are never evicted to
    /// make space: overload rejects the new surface instead of growing memory.
    pub fn insert(
        &mut self,
        key: ResourceKey,
        descriptor: SurfaceDescriptor,
        pixels: Arc<T>,
    ) -> Result<Arc<Resource>> {
        descriptor.validate()?;
        self.retire();
        ensure!(
            !self.entries.contains_key(&key),
            "resource revision already exists"
        );
        ensure!(
            self.entries.len() < self.max_entries,
            "surface count budget exceeded"
        );
        let used = self
            .entries
            .values()
            .map(|entry| entry.bytes)
            .sum::<usize>();
        ensure!(
            descriptor.storage_bytes <= self.max_bytes.saturating_sub(used),
            "surface byte budget exceeded"
        );
        let lease = Arc::new(Resource { key, descriptor });
        self.entries.insert(
            key,
            Entry {
                lease: Arc::downgrade(&lease),
                pixels,
                bytes: descriptor.storage_bytes,
            },
        );

        Ok(lease)
    }

    pub fn get(&self, lease: &Arc<Resource>) -> Option<&Arc<T>> {
        let entry = self.entries.get(&lease.key)?;
        let actual = entry.lease.upgrade()?;

        Arc::ptr_eq(&actual, lease).then_some(&entry.pixels)
    }

    /// Call outside per-frame composition; never enqueue an unbounded retirement list.
    pub fn retire(&mut self) {
        self.entries
            .retain(|_, entry| entry.lease.strong_count() > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_leases_are_bounded_and_retirement_waits_for_queued_consumers() {
        let mut store = SurfaceStore::new(1, 16);
        let descriptor = SurfaceDescriptor {
            width: 2,
            height: 2,
            layout: PixelLayout::Rgba8,
            alpha: Alpha::Straight,
            color: ColorEncoding::Srgb,
            storage_bytes: 16,
        };
        let pixels = Arc::new([0_u8; 16]);
        let key = ResourceKey { id: 1, revision: 1 };
        let lease = store.insert(key, descriptor, pixels.clone()).unwrap();
        let queued = lease.clone();
        assert!(Arc::ptr_eq(store.get(&lease).unwrap(), &pixels));
        assert!(
            store
                .insert(
                    ResourceKey { id: 2, revision: 2 },
                    descriptor,
                    pixels.clone()
                )
                .is_err()
        );
        let fake = Arc::new(Resource { key, descriptor });
        assert!(store.get(&fake).is_none());
        drop(lease);
        store.retire();
        assert!(store.get(&queued).is_some());
        drop(queued);
        store.retire();
        assert_eq!(Arc::strong_count(&pixels), 1);
        assert!(store.insert(key, descriptor, pixels).is_ok());
    }
}
