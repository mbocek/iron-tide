// Wind and hovering computed per vertex on the GPU, so the CPU never touches a plant or a crystal shard and
// identical meshes still draw together. Shared by motion.wesl (main pass) and motion_prepass.wesl (depth, shadows).

// Same layout as `MotionSettings` in src/motion_material.rs.
struct MotionSettings {
    sway_lean: f32,
    gust_period_secs: f32,
    hover_bob: f32,
    hover_loop_secs: f32,
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

// A plant leans away from the wind around its base (the model origin), more the higher the vertex, in gusts.
// Each plant's rhythm comes from where it stands, so no per-plant data is needed.
fn swayed(world_position: vec3<f32>, origin: vec3<f32>, settings: MotionSettings, time: f32) -> vec3<f32> {
    if settings.sway_lean == 0.0 {
        return world_position;
    }
    let seed = fract(sin(dot(origin.xz, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    let speed = 1.2 + 0.8 * fract(seed * 7.13);
    let gust = 0.65 + 0.35 * sin(time * TAU / settings.gust_period_secs);
    let lean = settings.sway_lean * gust * (0.6 + 0.4 * sin(time * speed + seed * TAU));
    return world_position + WIND_DIRECTION * lean * (world_position.y - origin.y);
}
