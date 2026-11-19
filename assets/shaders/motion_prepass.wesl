import bevy_render::globals::Globals;
import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{mesh_functions, view_transformations::position_world_to_clip},
};
import super::motion_common::{hovering, motion_settings, swayed};

// The prepass and shadow views bind the globals here, not at the main pass's binding.
@group(0) @binding(1) var<uniform> globals: Globals;

// Bevy's prepass vertex shader without skinning and morph targets, plus the same motion as motion.wesl, so
// depth, shadows and the lit surface stay in step.
@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let origin = world_from_local[3].xyz;
    let motion = motion_settings(vertex_in.instance_index);

    var position = vertex.position;
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)
    var normal = vertex.normal;
@else
    var normal = vec3<f32>(0.0, 1.0, 0.0);
@if(VERTEX_UVS_A && VERTEX_UVS_B) {
    let moved = hovering(position, normal, vertex.uv, vertex.uv_b, motion, globals.time);
    position = moved.position;
    normal = moved.normal;
}

    let world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(position, 1.0));
    out.world_position = vec4<f32>(swayed(world_position.xyz, origin, motion, globals.time), 1.0);
    out.position = position_world_to_clip(out.world_position.xyz);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}

@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(normal, vertex_in.instance_index);
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local, vertex.tangent, vertex_in.instance_index);
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = out.world_position;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index, world_from_local[3]);

    return out;
}
