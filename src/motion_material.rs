use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

/// A standard material whose vertices the GPU moves: plants lean in the wind and crystal shards hover.
/// Doing it per vertex keeps the CPU out of it and lets identical meshes keep drawing together.
pub type MovingMaterial = ExtendedMaterial<StandardMaterial, Motion>;

const GUST_PERIOD_SECS: f32 = 9.0;
const SHARD_BOB: f32 = 0.08;
const SHARD_LOOP_SECS: f32 = 4.0;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct Motion {
    #[uniform(100)]
    pub settings: MotionSettings,
}

pub use uniforms::MotionSettings;

mod uniforms {
    // The ShaderType derive emits per-field `check` helpers that rustc reports as unused.
    #![allow(dead_code)]
    use bevy::prelude::*;
    use bevy::render::render_resource::ShaderType;

    /// Same layout as `MotionSettings` in assets/shaders/motion_common.wesl.
    #[derive(Clone, Copy, Debug, Reflect, ShaderType)]
    pub struct MotionSettings {
        /// Radians a plant leans at its tip in a full gust; zero keeps it still.
        pub sway_lean: f32,
        pub gust_period_secs: f32,
        pub hover_bob: f32,
        /// Zero leaves shard data in the mesh unused.
        pub hover_loop_secs: f32,
    }
}

impl Motion {
    pub fn swaying(lean: f32) -> Self {
        Self {
            settings: MotionSettings {
                sway_lean: lean,
                gust_period_secs: GUST_PERIOD_SECS,
                hover_bob: 0.0,
                hover_loop_secs: 0.0,
            },
        }
    }

    pub fn hovering() -> Self {
        Self {
            settings: MotionSettings {
                sway_lean: 0.0,
                gust_period_secs: GUST_PERIOD_SECS,
                hover_bob: SHARD_BOB,
                hover_loop_secs: SHARD_LOOP_SECS,
            },
        }
    }
}

impl MaterialExtension for Motion {
    fn vertex_shader() -> ShaderRef {
        "shaders/motion.wesl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/motion_prepass.wesl".into()
    }
}
