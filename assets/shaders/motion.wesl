import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_view_bindings::globals,
    view_transformations::position_world_to_clip,
};
import super::motion_common::{MotionSettings, hovering, swayed, varied_tint};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> motion: MotionSettings;

// Bevy's mesh vertex shader without skinning and morph targets, plus the motion.
@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let origin = world_from_local[3].xyz;

    var position = vertex.position;
    var normal = vertex.normal;
@if(VERTEX_UVS_A && VERTEX_UVS_B) {
    let moved = hovering(position, normal, vertex.uv, vertex.uv_b, motion, globals.time);
    position = moved.position;
    normal = moved.normal;
}

    let world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(position, 1.0));
    out.world_position = vec4<f32>(swayed(world_position.xyz, origin, motion, globals.time), 1.0);
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(normal, vertex_in.instance_index);

@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_COLORS)
    out.color = varied_tint(vertex.color, origin, motion);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index, world_from_local[3]);

    return out;
}
