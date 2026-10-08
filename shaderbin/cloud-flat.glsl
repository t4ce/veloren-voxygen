// Voxy's flat cloud plane. Keep weather, world altitude, camera and time inputs.
#include <lod.glsl>
#include <sky.glsl>

// Premultiplied cloud contribution, without filling the sky underneath it.
// max_dist retains the original scene-depth limit for callers compositing over terrain.
vec4 get_flat_cloud_layer(vec3 dir, vec3 origin, float max_dist) {
    float cloud_alt = cloud_avg_alt();
    if (dir.z * (cloud_alt - origin.z) <= 0.0) {
        return vec4(0);
    }
    float dist = (cloud_alt - origin.z) / dir.z;
    if (dist >= max_dist) {
        return vec4(0);
    }
    vec2 cloud_intersect = origin.xy + focus_off.xy + dir.xy * dist;
    vec3 sky_light = get_sky_light(dir, false, 0.0);
    vec3 haze_color = mix(sky_light, sky_light * vec3(0.1, 0.3, 0.5), min(rain_density * 4, 1.0));
    vec3 cloud_color = vec3(1 + noise_3d(vec3(cloud_intersect * 0.0001, time_of_day.x * 0.001)) * 2.0) * haze_color;
    float coverage = min(cloud_tendency_at(cloud_intersect) * 30000 / dist, 1);
    return vec4(cloud_color * coverage, coverage);
}

// Retain the original full-scene fallback, including water attenuation and haze.
vec3 get_cloud_color(vec3 surf_color, vec3 dir, vec3 origin, float max_dist, float quality) {
    surf_color = water_diffuse(surf_color, dir, max_dist);
    vec3 sky_light = get_sky_light(dir, false, 0.0);
    vec3 haze_color = mix(sky_light, sky_light * vec3(0.1, 0.3, 0.5), min(rain_density * 4, 1.0));
    #ifndef EXPERIMENTAL_NOHAZE
        float haze_factor = mix(0.00025, 0.01, rain_density);
        surf_color = mix(haze_color, surf_color, 1.0 / exp(min(max_dist, 8000.0) * haze_factor));
    #endif
    vec4 cloud = get_flat_cloud_layer(dir, origin, max_dist);
    return surf_color * (1.0 - cloud.a) + cloud.rgb;
}
