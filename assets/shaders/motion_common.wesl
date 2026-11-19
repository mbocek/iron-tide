// Wind and hovering computed per vertex on the GPU, so the CPU never touches a plant or a crystal shard and
// identical meshes still draw together. Shared by motion.wesl (main pass) and motion_prepass.wesl (depth, shadows).

import bevy_pbr::render::mesh_bindings::mesh;

// Same layout as `MotionSettings` in src/motion_material.rs.
struct MotionSettings {
    sway_lean: f32,
    gust_period_secs: f32,
    hover_bob: f32,
    hover_loop_secs: f32,
    tint_variation: f32,
    glow_pulse_speed: f32,
}

// One slot per material in bindless mode (see `Motion` in src/motion_material.rs), a plain uniform otherwise.
@if(BINDLESS) {
struct MotionIndices {
    settings: u32,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<storage> motion_indices: array<MotionIndices>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<storage> motion_array: array<MotionSettings>;
}
@else {
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> motion_uniform: MotionSettings;
}

fn motion_settings(instance_index: u32) -> MotionSettings {
@if(BINDLESS) {
    let slot = mesh[instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    return motion_array[motion_indices[slot].settings];
}
@else {
    return motion_uniform;
}
}

struct Moved {
    position: vec3<f32>,
    normal: vec3<f32>,
}

const TAU: f32 = 6.28318530718;
const WIND_DIRECTION: vec3<f32> = vec3<f32>(0.8, 0.0, 0.6);

// A floating shard turns around its pivot and bobs. `shard` is (phase, spin direction); a spin of 0 marks
// the parts that stay put. `pivot` is the shard's centre on the ground plane, in model space.
fn hovering(position: vec3<f32>, normal: vec3<f32>, pivot: vec2<f32>, shard: vec2<f32>,
            settings: MotionSettings, time: f32) -> Moved {
    if shard.y == 0.0 || settings.hover_loop_secs == 0.0 {
        return Moved(position, normal);
    }
    let cycle = time / settings.hover_loop_secs;
    let angle = shard.y * TAU * fract(cycle);
    let c = cos(angle);
    let s = sin(angle);
    let around = position.xz - pivot;
    let turned = vec2<f32>(around.x * c - around.y * s, around.x * s + around.y * c) + pivot;
    let bob = settings.hover_bob * sin(TAU * (cycle + shard.x));
    return Moved(
        vec3<f32>(turned.x, position.y + bob, turned.y),
        vec3<f32>(normal.x * c - normal.z * s, normal.y, normal.x * s + normal.z * c),
    );
}

// A pseudo-random number in 0..1 for an instance, from where it stands.
fn instance_seed(origin: vec3<f32>) -> f32 {
    return fract(sin(dot(origin.xz, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// Each instance a little lighter or darker, greener or more yellow, around the model's own colours.
fn varied_tint(color: vec4<f32>, origin: vec3<f32>, settings: MotionSettings) -> vec4<f32> {
    let seed = instance_seed(origin + vec3<f32>(31.7, 0.0, 17.3));
    let shade = 1.0 + (seed - 0.5) * 2.0 * settings.tint_variation;
    let warmth = (fract(seed * 13.7) - 0.5) * settings.tint_variation;
    return vec4<f32>(color.rgb * shade * vec3<f32>(1.0 + warmth, 1.0, 1.0 - warmth), color.a);
}

// A plant leans away from the wind around its base (the model origin), more the higher the vertex, in gusts.
// Each plant's rhythm comes from where it stands, so no per-plant data is needed.
fn swayed(world_position: vec3<f32>, origin: vec3<f32>, settings: MotionSettings, time: f32) -> vec3<f32> {
    if settings.sway_lean == 0.0 {
        return world_position;
    }
    let seed = instance_seed(origin);
    let speed = 1.2 + 0.8 * fract(seed * 7.13);
    let gust = 0.65 + 0.35 * sin(time * TAU / settings.gust_period_secs);
    let lean = settings.sway_lean * gust * (0.6 + 0.4 * sin(time * speed + seed * TAU));
    return world_position + WIND_DIRECTION * lean * (world_position.y - origin.y);
}

// A glowing crystal core swells and fades.
fn glow_pulse(settings: MotionSettings, time: f32) -> f32 {
    if settings.glow_pulse_speed == 0.0 {
        return 1.0;
    }
    return 0.75 + 0.35 * sin(time * settings.glow_pulse_speed);
}
