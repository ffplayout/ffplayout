//! Scene/layer descriptions and immutable program-time evaluation.
use std::sync::Arc;

mod controller;
mod frame;
pub mod playlist;

use crate::{
    playback::Presentation,
    surface::{Resource, ResourceKey},
};

pub use controller::{
    Acknowledgement, Command, Receipt, Rejection, ResourceStatus, SceneControl, SceneController,
    SubmitError,
};
pub use frame::{FrameLayer, FrameLayers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LayerId(pub u64);

/// The pipeline selects a scope and supplies its reference canvas. No output
/// protocol or encoder identifier is part of the graphics model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scope(pub u64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerSpec {
    pub id: LayerId,
    pub resource: ResourceKey,
    pub z: i32,
    pub scope: Scope,
    pub canvas: (u32, u32),
    /// Reference-canvas pixels; backends apply their documented chroma alignment.
    pub position: (i32, i32),
    pub opacity: f64,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub spec: LayerSpec,
    pub resource: Arc<Resource>,
    pub presentation: Presentation,
    pub visible: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SceneRevision {
    pub number: u64,
    pub layers: Arc<[Layer]>,
}

pub struct EvaluatedLayer<'a> {
    pub layer: &'a Layer,
    pub opacity: f64,
}

impl SceneRevision {
    /// Borrows ready surfaces in deterministic (z, layer ID) order. No allocation,
    /// rasterization, I/O, mutex or mutable animation clock is involved.
    pub fn evaluate(
        &self,
        program_us: i64,
        scope: Scope,
    ) -> impl Iterator<Item = FrameLayer<EvaluatedLayer<'_>>> {
        self.layers.iter().filter_map(move |layer| {
            if !layer.visible || layer.spec.scope != scope {
                return None;
            }
            let opacity = layer.presentation.opacity(program_us)? * layer.spec.opacity;

            (opacity > 0.0).then_some(FrameLayer {
                id: layer.spec.id,
                z: layer.spec.z,
                content: EvaluatedLayer { layer, opacity },
            })
        })
    }
}
