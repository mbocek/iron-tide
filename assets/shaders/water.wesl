import bevy_pbr::render::pbr_fragment::pbr_input_from_standard_material;
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};
import bevy_pbr::render::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var fog_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var fog_sampler: sampler;

// Same as MAP_WORLD_SIZE in src/map.rs and FOG_GAMMA in terrain.wesl.
const MAP_WORLD_SIZE: f32 = 128.0;
const FOG_GAMMA: f32 = 2.2;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);

    let map_uv = in.world_position.xz / MAP_WORLD_SIZE;
    let inside_map = all(map_uv >= vec2<f32>(0.0)) && all(map_uv <= vec2<f32>(1.0));
    let fog = select(1.0, textureSampleLevel(fog_texture, fog_sampler, map_uv, 0.0).r, inside_map);
    out.color = vec4<f32>(out.color.rgb * pow(fog, FOG_GAMMA), out.color.a);
    return out;
}
