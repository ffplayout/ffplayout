//! Fixed-capacity, immutable containers for already evaluated layers.
use super::LayerId;

/// An already evaluated layer. Content stays backend-owned; ordering has the
/// same meaning as in `SceneRevision`. A content value may be a prepared group.
#[derive(Clone)]
pub struct FrameLayer<T> {
    pub id: LayerId,
    pub z: i32,
    pub content: T,
}

/// Small immutable frame-boundary snapshots without a heap allocation. This
/// also accommodates compatibility layers with their own established clocks.
#[derive(Clone)]
pub struct FrameLayers<T, const N: usize> {
    layers: [Option<FrameLayer<T>>; N],
}

impl<T, const N: usize> FrameLayers<T, N> {
    pub fn new(mut layers: [Option<FrameLayer<T>>; N]) -> Self {
        layers.sort_unstable_by_key(|layer| layer.as_ref().map(|layer| (layer.z, layer.id)));

        Self { layers }
    }

    pub fn layers(&self) -> impl Iterator<Item = &FrameLayer<T>> {
        self.layers.iter().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(Option::is_none)
    }
}

impl<T, const N: usize> Default for FrameLayers<T, N> {
    fn default() -> Self {
        Self {
            layers: std::array::from_fn(|_| None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn ready_frame_layers_sort_by_z_and_id_and_keep_owned_resources() {
        let pixels = Arc::new([1_u8; 4]);
        let layer = |id, z| {
            Some(FrameLayer {
                id: LayerId(id),
                z,
                content: pixels.clone(),
            })
        };
        let snapshot = FrameLayers::new([layer(3, 10), None, layer(2, 5), layer(1, 10)]);
        assert_eq!(
            snapshot
                .layers()
                .map(|layer| layer.id.0)
                .collect::<Vec<_>>(),
            [2, 1, 3]
        );
        let queued = snapshot.clone();
        drop(snapshot);
        assert_eq!(Arc::strong_count(&pixels), 4);
        let empty = FrameLayers::<Arc<[u8; 4]>, 4>::default();
        assert!(empty.is_empty());
        assert!(!queued.is_empty());
        drop(queued);
        assert_eq!(Arc::strong_count(&pixels), 1);
    }
}
