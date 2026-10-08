#version 440 core
#include <constants.glsl>
#include <globals.glsl>
#include <cloud.glsl>

// Same inverse world-to-clip transform used by the original cloud pass.
layout(std140, set = 0, binding = 15) uniform u_cloud_view {
    mat4 all_mat_inv;
};
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 tgt_color;

void main() {
    vec4 point = all_mat_inv * vec4((uv * 2.0 - 1.0) * vec2(1, -1), 0.5, 1);
    vec3 dir = normalize(point.xyz / point.w - cam_pos.xyz);
    vec4 cloud = get_flat_cloud_layer(dir, cam_pos.xyz, 524288.0);
    // Fit the HDR cloud color to a premultiplied RGBA8 display layer.
    // Gamma belongs to the display engine; this only bounds the stored color.
    tgt_color = vec4(clamp(cloud.rgb, vec3(0), vec3(cloud.a)), cloud.a);
}
