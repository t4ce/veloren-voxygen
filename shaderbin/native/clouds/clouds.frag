#version 450
layout(set=0,binding=3) uniform texture2D packed_input;
layout(set=0,binding=4) uniform sampler packed_sampler;
float scalar_at(ivec2 p) {
    uvec4 b = uvec4(round(texelFetch(sampler2D(packed_input,packed_sampler),p,0)*255.0));
    return uintBitsToFloat(b.r | (b.g<<8) | (b.b<<16) | (b.a<<24));
}
float param(int i) { return scalar_at(ivec2(i,0)); }
float noise_sample(vec2 uv) {
    ivec2 size = ivec2(param(28),param(29));
    vec2 p = fract(uv)*vec2(size)-0.5;
    ivec2 base = ivec2(floor(p)); vec2 f=fract(p);
    ivec2 a = (base % size + size) % size;
    ivec2 b = ((base+1) % size + size) % size;
    return mix(mix(texelFetch(sampler2D(packed_input,packed_sampler),ivec2(a.x,a.y+1),0).r,
                   texelFetch(sampler2D(packed_input,packed_sampler),ivec2(b.x,a.y+1),0).r,f.x),
               mix(texelFetch(sampler2D(packed_input,packed_sampler),ivec2(a.x,b.y+1),0).r,
                   texelFetch(sampler2D(packed_input,packed_sampler),ivec2(b.x,b.y+1),0).r,f.x),f.y);
}
vec4 weather_sample(vec2 uv) {
    ivec2 size = ivec2(param(30),param(31));
    vec2 p=uv*vec2(size)-0.5; ivec2 base=ivec2(floor(p)); vec2 f=fract(p);
    ivec2 a=clamp(base,ivec2(0),size-1), b=clamp(base+1,ivec2(0),size-1);
    int y=1+int(param(29));
    float v=mix(mix(scalar_at(ivec2(a.x,a.y+y)),scalar_at(ivec2(b.x,a.y+y)),f.x),
                mix(scalar_at(ivec2(a.x,b.y+y)),scalar_at(ivec2(b.x,b.y+y)),f.x),f.y);
    return vec4(v,0,0,0);
}
/* NOTE: When included, this file will contain values for the automatically defined settings specified below. */

/* TODO: Add the ability to control the tendency to do stuff in the vertex vs. fragment shader.
 * Currently this flag is ignored and always set to prefer fragment, but this tradeoff is not correct on all
 * machines in all cases (mine, for instance). */
#define VOXYGEN_COMPUTATION_PREFERENCE_FRAGMENT 0
#define VOXYGEN_COMPUTATION_PREFERENCE_VERTEX 1

#define FLUID_MODE_LOW 0
#define FLUID_MODE_MEDIUM 1
#define FLUID_MODE_HIGH 2

#define REFLECTION_MODE_LOW 0
#define REFLECTION_MODE_MEDIUM 1
#define REFLECTION_MODE_HIGH 2

#define CLOUD_MODE_FLAT 0
#define CLOUD_MODE_MINIMAL 1
#define CLOUD_MODE_LOW 2
#define CLOUD_MODE_MEDIUM 3
#define CLOUD_MODE_HIGH 4
#define CLOUD_MODE_ULTRA 5

#define LIGHTING_ALGORITHM_LAMBERTIAN 0
#define LIGHTING_ALGORITHM_BLINN_PHONG 1
#define LIGHTING_ALGORITHM_ASHIKHMIN 2

#define SHADOW_MODE_NONE 0
#define SHADOW_MODE_CHEAP 1
#define SHADOW_MODE_MAP 2

/* Unlike the other flags (for now anyway), these are bitmask values */
#define LIGHTING_TYPE_REFLECTION 0x01
#define LIGHTING_TYPE_TRANSMISSION 0x02

/* Currently ignored, but ideally shoud be helpful for determining light transport properties. */
#define LIGHTING_REFLECTION_KIND_DIFFUSE 0
#define LIGHTING_REFLECTION_KIND_GLOSSY 1
#define LIGHTING_REFLECTION_KIND_SPECULAR 2

#define LIGHTING_TRANSPORT_MODE_IMPORTANCE 0
/* Radiance mode is currently used as a proxy for "attenuation and medium materials
 * matter," but we may make it more granular. */
#define LIGHTING_TRANSPORT_MODE_RADIANCE 1

#define LIGHTING_DISTRIBUTION_SCHEME_MICROFACET 0
#define LIGHTING_DISTRIBUTION_SCHEME_VOXEL 1

#define LIGHTING_DISTRIBUTION_BECKMANN 0
#define LIGHTING_DISTRIBUTION_TROWBRIDGE 1

#define MEDIUM_AIR 0
#define MEDIUM_WATER 1

#define MAT_SKY 0
#define MAT_BLOCK 1
#define MAT_WATER 2
#define MAT_FIGURE 3
#define MAT_LOD 4
#define MAT_PUDDLE 5

#define BLOCK_SNOW 0x21
#define BLOCK_ART_SNOW 0x22
#define BLOCK_ICE 0x43

// An arbitrary value that represents a very far distance (at least as far as the player should be able to see) without
// being too far that we end up with precision issues (used in clouds and elsewhere).
#define DIST_CAP 500000

/* Constants expected to be defined automatically by configuration: */

/*
#define VOXYGEN_COMPUTATION_PREFERENCE <preference>
#define FLUID_MODE <mode>
#define CLOUD_MODE <mode>
#define LIGHTING_ALGORITHM <algorithm>
#define SHADOW_MODE <mode>
*/

/* Constants possibly defined automatically by configuration: */

/*
#define POINT_GLOW_FACTOR <0.0..1.0>
*/

/* Constants expected to be defined by any shader that needs to perform lighting calculations
 * (but whose values may take automatically defined constants into account): */

/*
// At least one of LIGHTING_TYPE_REFLECTION or LIGHTING_TYPE_TRANSMISSION should be set.
#define LIGHTING_TYPE <type bitmask>
#define LIGHTING_REFLECTION_KIND <kind>
#define LIGHTING_TRANSPORT_MODE <mode>
#define LIGHTING_DISTRIBUTION_SCHEME <scheme>
#define LIGHTING_DISTRIBUTION <distribution>
*/

/* Constants that *may* be defined by any shader.
 * (and whose values may take automatically defined constants into account): */

/*
// When sets, shadow maps are used to cast shadows.
#define HAS_SHADOW_MAPS
// When set, "full" LOD terrain informatino is available (e.g. terrain colors).
#define HAS_LOD_FULL_INFO
*/

#define CLOUD_MODE CLOUD_MODE_FLAT
#define SHADOW_MODE SHADOW_MODE_NONE
#define FLUID_MODE FLUID_MODE_LOW
#define LIGHTING_ALGORITHM LIGHTING_ALGORITHM_LAMBERTIAN

#ifndef GLOBALS_GLSL
#define GLOBALS_GLSL


    mat4 view_mat;
    mat4 proj_mat;
    mat4 all_mat;
    vec4 cam_pos;
    vec4 focus_off;
    vec4 focus_pos;
    vec4 view_distance;
    // .x = time of day, repeats every day.
    // .y = a continuous value for what day it is. Repeats every `tick_overflow` for precisions sake.
    vec4 time_of_day;
    vec4 sun_dir;
    vec4 moon_dir;
    // .x = The `Time` resource, repeated every `tick_overflow`
    // .y = a floored (`Time` / `tick_overflow`)
    // .z = Time local to client, not synced between clients.
    vec4 tick;
    vec4 screen_res;
    uvec4 light_shadow_count;
    vec4 shadow_proj_factors;
    uvec4 medium;
    ivec4 select_pos;
    vec4 gamma_exposure;
    vec4 last_lightning;
    vec2 wind_vel;
    vec2 internal_res;
    float ambiance;
    // 0 - FirstPerson
    // 1 - ThirdPerson
    uint cam_mode;
    float sprite_render_distance;
    float u_rotation;
    float screen_fade;


float distance_divider = 2.0;
float shadow_dithering = 0.5;

float tick_overflow = 300000.0;

// Get a scaled time with an offset that loops at a period.
float tick_loop(float period, float scale, float offset) {
    float loop = tick_overflow * scale;
    float rem = mod(loop, period);
    float rest = rem * tick.y;

    return mod(rest + tick.x * scale + offset, period);
}

float tick_loop(float period) {
    return tick_loop(period, 1.0, 0.0);
}

vec3 tick_loop(float period, vec3 scale, vec3 offset) {
    vec3 loop = tick_overflow * scale;
    vec3 rem = mod(loop, period);
    vec3 rest = rem * tick.y;

    return mod(rest + tick.x * scale + offset, period);
}

// Only works if t happened within tick_overflow
float time_since(float t) {
    return tick.x < t ? (tick_overflow - t + tick.x) : (tick.x - t);
}

#endif

// Voxy's flat cloud plane. Keep weather, world altitude, camera and time inputs.
#ifndef LOD_GLSL
#define LOD_GLSL

#ifndef RANDOM_GLSL
#define RANDOM_GLSL

#define t_noise packed_input

#define s_noise packed_sampler


float hash(vec4 p) {
    p = fract(p * 0.3183099 + 0.1) - fract(p + 23.22121);
    p *= 17.0;
    return (fract(p.x * p.y * (1.0 - p.z) * p.w * (p.x + p.y + p.z + p.w)) - 0.5) * 2.0;
}

#define M1 2047667443U
#define M2 3883706873U
#define M3 3961281721U

float hash_one(uint q) {
    uint n = ((M3 * q) ^ M2) * M1;

    return float(n) * (1.0 / float(0xffffffffU));
}

float hash_two(uvec2 q) {
    q *= uvec2(M1, M2);
    uint n = q.x ^ q.y;
    n = n * (n ^ (n >> 15));
    return float(n) * (1.0 / float(0xffffffffU));
}

vec3 hash_two_3(uvec2 q) {
    q *= uvec2(M1, M2);
    uvec3 n = uvec3(q.x ^ q.y ^ uvec3(M1, M2, M3));
    n = n * (n ^ (n >> 15));
    return vec3(n) * (1.0 / vec3(0xffffffffU));
}

float hash_three(uvec3 q) {
    q *= uvec3(M1, M2, M3);
    uint n = q.x ^ q.y ^ q.z;
    n = n * (n ^ (n >> 15));
    return float(n) * (1.0 / float(0xffffffffU));
}

float hash_fast(uvec3 q) {
    q *= uvec3(M1, M2, M3);

    uint n = (q.x ^ q.y ^ q.z) * M1;

    return float(n) * (1.0 / float(0xffffffffU));
}

// 2D, but using shifted 2D textures
float noise_2d(vec2 pos) {
    return noise_sample(pos);
}

// 3D, but using shifted 2D textures
float noise_3d(vec3 pos) {
    pos.z *= 15.0;
    uint z = uint(trunc(pos.z));
    vec2 offs0 = vec2(hash_one(z), hash_one(z + 73u));
    vec2 offs1 = vec2(hash_one(z + 1u), hash_one(z + 1u + 73u));
    return mix(noise_sample(pos.xy + offs0), noise_sample(pos.xy + offs1), fract(pos.z));
}

// 3D version of `snoise`
float snoise3(in vec3 x) {
    uvec3 p = uvec3(floor(x) + 10000.0);
    vec3 f = fract(x);
    //f = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(
            mix(hash_fast(p + uvec3(0, 0, 0)), hash_fast(p + uvec3(1, 0, 0)), f.x),
            mix(hash_fast(p + uvec3(0, 1, 0)), hash_fast(p + uvec3(1, 1, 0)), f.x),
            f.y),
        mix(
            mix(hash_fast(p + uvec3(0, 0, 1)), hash_fast(p + uvec3(1, 0, 1)), f.x),
            mix(hash_fast(p + uvec3(0, 1, 1)), hash_fast(p + uvec3(1, 1, 1)), f.x),
            f.y),
        f.z);
}

// 4D noise
float snoise(in vec4 x) {
    vec4 p = floor(x);
    vec4 f = fract(x);
    f = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(
            mix(
                mix(hash(p + vec4(0, 0, 0, 0)), hash(p + vec4(1, 0, 0, 0)), f.x),
                mix(hash(p + vec4(0, 1, 0, 0)), hash(p + vec4(1, 1, 0, 0)), f.x),
                f.y),
            mix(
                mix(hash(p + vec4(0, 0, 1, 0)), hash(p + vec4(1, 0, 1, 0)), f.x),
                mix(hash(p + vec4(0, 1, 1, 0)), hash(p + vec4(1, 1, 1, 0)), f.x),
                f.y),
            f.z),
        mix(
            mix(
                mix(hash(p + vec4(0, 0, 0, 1)), hash(p + vec4(1, 0, 0, 1)), f.x),
                mix(hash(p + vec4(0, 1, 0, 1)), hash(p + vec4(1, 1, 0, 1)), f.x),
                f.y),
            mix(
                mix(hash(p + vec4(0, 0, 1, 1)), hash(p + vec4(1, 0, 1, 1)), f.x),
                mix(hash(p + vec4(0, 1, 1, 1)), hash(p + vec4(1, 1, 1, 1)), f.x),
                f.y),
            f.z),
        f.w);
}

vec3 rand_perm_3(vec3 pos) {
    return abs(sin(pos * vec3(1473.7 * pos.z + 472.3, 8891.1 * pos.x + 723.1, 3813.3 * pos.y + 982.5)));
}

vec4 rand_perm_4(vec4 pos) {
    return sin(473.3 * pos * vec4(317.3 * pos.w + 917.7, 1473.7 * pos.z + 472.3, 8891.1 * pos.x + 723.1, 3813.3 * pos.y + 982.5) / pos.yxwz);
}

vec3 smooth_rand(vec3 pos, float lerp_axis) {
    return vec3(snoise(vec4(pos, lerp_axis)), snoise(vec4(pos + 400.0, lerp_axis)), snoise(vec4(pos + 1000.0, lerp_axis)));
}

// Transform normal distribution to triangle distribution.
float norm2tri(float n) {
   // TODO: compare perf with adding two normal noise distributions
   bool flip = n > 0.5;
   n = flip ? 1.0 - n : n;
   n = sqrt(n / 2.0);
   n = flip ? 1.0 - n : n;
   return n;
}

// Caustics, ported and modified from https://www.shadertoy.com/view/3tlfR7, originally David Hoskins.
// License Creative Commons Attribution-NonCommercial-ShareAlike 3.0 Unported License: https://creativecommons.org/licenses/by-nc-sa/3.0/legalcode.
// Modifying these three functions mean that you agree to release your changes under the above license, *not* under GPL 3 as with the rest of the project.

float hashvec2(vec2 p) {return fract(sin(p.x * 1e2 + p.y) * 1e5 + sin(p.y * 1e3) * 1e3 + sin(p.x * 735. + p.y * 11.1) * 1.5e2); }

float n12(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f *= f * (3.-2.*f);
    return mix(
        mix(hashvec2(i+vec2(0.,0.)),hashvec2(i+vec2(1.,0.)),f.x),
        mix(hashvec2(i+vec2(0.,1.)),hashvec2(i+vec2(1.,1.)),f.x),
        f.y
    );
}

float caustics(vec2 p, float t) {
    vec3 k = vec3(p,t);
    float l;
    mat3 m = mat3(-2.,-1.,2.,3.,-2.,1.,1.,2.,2.);
    float n = n12(p);
    k = k*m*.5;
    l = length(.5 - fract(k+n));
    k = k*m*.4;
    l = min(l, length(.5-fract(k+n)));
    k = k*m*.3;
    l = min(l, length(.5-fract(k+n)));
    return pow(l,3.)*5.5;
}

#ifdef EXPERIMENTAL_DISCARDTRANSPARENCY
bool dither(vec2 frag_coord, float a, uint id) {
    if (a < 1.0 / 17.0) {
        return true;
    }
    if (a > 16.0 / 17.0) {
        return false;
    }

    // Use the id to try to discard different pixels from different objects, causing
    // them to be visible behind eachother.
    float r0 = floor(hash_one(id) * 16.0);
    vec2 r1 = vec2(floor(r0 * 0.25), mod(r0, 4.0));

    // We sample the bayer multiple times to have a smoother gradient of dithering.
    // This could be achieved by having a larger bayer matrix, but would then have
    // to define a larger one and not sure it would be much more efficient.
    uvec2 pos0 = uvec2(frag_coord + r1) % 4;
    uvec2 pos1 = uvec2(frag_coord / 4.0 + r1) % 4;
    uvec2 pos2 = uvec2(frag_coord / 16.0 + r1) % 4;

    mat4 bayer = mat4(
        16.0, 4.0, 13.0, 1.0,
        8.0, 12.0, 5.0, 9.0,
        14.0, 2.0, 15.0, 3.0,
        6.0, 10.0, 7.0, 11.0
    );
    mat4 bayer0 = bayer / 17.0;
    mat4 bayer1 = bayer / (17.0 * 17.0);
    mat4 bayer2 = bayer / (17.0 * 17.0 * 17.0);

    return a < bayer0[pos0.x][pos0.y] + bayer1[pos1.x][pos1.y] + bayer2[pos2.x][pos2.y];
}
#endif

#endif

#ifndef SKY_GLSL
#define SKY_GLSL

#ifndef RANDOM_GLSL
#define RANDOM_GLSL

#define t_noise packed_input

#define s_noise packed_sampler


float hash(vec4 p) {
    p = fract(p * 0.3183099 + 0.1) - fract(p + 23.22121);
    p *= 17.0;
    return (fract(p.x * p.y * (1.0 - p.z) * p.w * (p.x + p.y + p.z + p.w)) - 0.5) * 2.0;
}

#define M1 2047667443U
#define M2 3883706873U
#define M3 3961281721U

float hash_one(uint q) {
    uint n = ((M3 * q) ^ M2) * M1;

    return float(n) * (1.0 / float(0xffffffffU));
}

float hash_two(uvec2 q) {
    q *= uvec2(M1, M2);
    uint n = q.x ^ q.y;
    n = n * (n ^ (n >> 15));
    return float(n) * (1.0 / float(0xffffffffU));
}

vec3 hash_two_3(uvec2 q) {
    q *= uvec2(M1, M2);
    uvec3 n = uvec3(q.x ^ q.y ^ uvec3(M1, M2, M3));
    n = n * (n ^ (n >> 15));
    return vec3(n) * (1.0 / vec3(0xffffffffU));
}

float hash_three(uvec3 q) {
    q *= uvec3(M1, M2, M3);
    uint n = q.x ^ q.y ^ q.z;
    n = n * (n ^ (n >> 15));
    return float(n) * (1.0 / float(0xffffffffU));
}

float hash_fast(uvec3 q) {
    q *= uvec3(M1, M2, M3);

    uint n = (q.x ^ q.y ^ q.z) * M1;

    return float(n) * (1.0 / float(0xffffffffU));
}

// 2D, but using shifted 2D textures
float noise_2d(vec2 pos) {
    return noise_sample(pos);
}

// 3D, but using shifted 2D textures
float noise_3d(vec3 pos) {
    pos.z *= 15.0;
    uint z = uint(trunc(pos.z));
    vec2 offs0 = vec2(hash_one(z), hash_one(z + 73u));
    vec2 offs1 = vec2(hash_one(z + 1u), hash_one(z + 1u + 73u));
    return mix(noise_sample(pos.xy + offs0), noise_sample(pos.xy + offs1), fract(pos.z));
}

// 3D version of `snoise`
float snoise3(in vec3 x) {
    uvec3 p = uvec3(floor(x) + 10000.0);
    vec3 f = fract(x);
    //f = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(
            mix(hash_fast(p + uvec3(0, 0, 0)), hash_fast(p + uvec3(1, 0, 0)), f.x),
            mix(hash_fast(p + uvec3(0, 1, 0)), hash_fast(p + uvec3(1, 1, 0)), f.x),
            f.y),
        mix(
            mix(hash_fast(p + uvec3(0, 0, 1)), hash_fast(p + uvec3(1, 0, 1)), f.x),
            mix(hash_fast(p + uvec3(0, 1, 1)), hash_fast(p + uvec3(1, 1, 1)), f.x),
            f.y),
        f.z);
}

// 4D noise
float snoise(in vec4 x) {
    vec4 p = floor(x);
    vec4 f = fract(x);
    f = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(
            mix(
                mix(hash(p + vec4(0, 0, 0, 0)), hash(p + vec4(1, 0, 0, 0)), f.x),
                mix(hash(p + vec4(0, 1, 0, 0)), hash(p + vec4(1, 1, 0, 0)), f.x),
                f.y),
            mix(
                mix(hash(p + vec4(0, 0, 1, 0)), hash(p + vec4(1, 0, 1, 0)), f.x),
                mix(hash(p + vec4(0, 1, 1, 0)), hash(p + vec4(1, 1, 1, 0)), f.x),
                f.y),
            f.z),
        mix(
            mix(
                mix(hash(p + vec4(0, 0, 0, 1)), hash(p + vec4(1, 0, 0, 1)), f.x),
                mix(hash(p + vec4(0, 1, 0, 1)), hash(p + vec4(1, 1, 0, 1)), f.x),
                f.y),
            mix(
                mix(hash(p + vec4(0, 0, 1, 1)), hash(p + vec4(1, 0, 1, 1)), f.x),
                mix(hash(p + vec4(0, 1, 1, 1)), hash(p + vec4(1, 1, 1, 1)), f.x),
                f.y),
            f.z),
        f.w);
}

vec3 rand_perm_3(vec3 pos) {
    return abs(sin(pos * vec3(1473.7 * pos.z + 472.3, 8891.1 * pos.x + 723.1, 3813.3 * pos.y + 982.5)));
}

vec4 rand_perm_4(vec4 pos) {
    return sin(473.3 * pos * vec4(317.3 * pos.w + 917.7, 1473.7 * pos.z + 472.3, 8891.1 * pos.x + 723.1, 3813.3 * pos.y + 982.5) / pos.yxwz);
}

vec3 smooth_rand(vec3 pos, float lerp_axis) {
    return vec3(snoise(vec4(pos, lerp_axis)), snoise(vec4(pos + 400.0, lerp_axis)), snoise(vec4(pos + 1000.0, lerp_axis)));
}

// Transform normal distribution to triangle distribution.
float norm2tri(float n) {
   // TODO: compare perf with adding two normal noise distributions
   bool flip = n > 0.5;
   n = flip ? 1.0 - n : n;
   n = sqrt(n / 2.0);
   n = flip ? 1.0 - n : n;
   return n;
}

// Caustics, ported and modified from https://www.shadertoy.com/view/3tlfR7, originally David Hoskins.
// License Creative Commons Attribution-NonCommercial-ShareAlike 3.0 Unported License: https://creativecommons.org/licenses/by-nc-sa/3.0/legalcode.
// Modifying these three functions mean that you agree to release your changes under the above license, *not* under GPL 3 as with the rest of the project.

float hashvec2(vec2 p) {return fract(sin(p.x * 1e2 + p.y) * 1e5 + sin(p.y * 1e3) * 1e3 + sin(p.x * 735. + p.y * 11.1) * 1.5e2); }

float n12(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f *= f * (3.-2.*f);
    return mix(
        mix(hashvec2(i+vec2(0.,0.)),hashvec2(i+vec2(1.,0.)),f.x),
        mix(hashvec2(i+vec2(0.,1.)),hashvec2(i+vec2(1.,1.)),f.x),
        f.y
    );
}

float caustics(vec2 p, float t) {
    vec3 k = vec3(p,t);
    float l;
    mat3 m = mat3(-2.,-1.,2.,3.,-2.,1.,1.,2.,2.);
    float n = n12(p);
    k = k*m*.5;
    l = length(.5 - fract(k+n));
    k = k*m*.4;
    l = min(l, length(.5-fract(k+n)));
    k = k*m*.3;
    l = min(l, length(.5-fract(k+n)));
    return pow(l,3.)*5.5;
}

#ifdef EXPERIMENTAL_DISCARDTRANSPARENCY
bool dither(vec2 frag_coord, float a, uint id) {
    if (a < 1.0 / 17.0) {
        return true;
    }
    if (a > 16.0 / 17.0) {
        return false;
    }

    // Use the id to try to discard different pixels from different objects, causing
    // them to be visible behind eachother.
    float r0 = floor(hash_one(id) * 16.0);
    vec2 r1 = vec2(floor(r0 * 0.25), mod(r0, 4.0));

    // We sample the bayer multiple times to have a smoother gradient of dithering.
    // This could be achieved by having a larger bayer matrix, but would then have
    // to define a larger one and not sure it would be much more efficient.
    uvec2 pos0 = uvec2(frag_coord + r1) % 4;
    uvec2 pos1 = uvec2(frag_coord / 4.0 + r1) % 4;
    uvec2 pos2 = uvec2(frag_coord / 16.0 + r1) % 4;

    mat4 bayer = mat4(
        16.0, 4.0, 13.0, 1.0,
        8.0, 12.0, 5.0, 9.0,
        14.0, 2.0, 15.0, 3.0,
        6.0, 10.0, 7.0, 11.0
    );
    mat4 bayer0 = bayer / 17.0;
    mat4 bayer1 = bayer / (17.0 * 17.0);
    mat4 bayer2 = bayer / (17.0 * 17.0 * 17.0);

    return a < bayer0[pos0.x][pos0.y] + bayer1[pos1.x][pos1.y] + bayer2[pos2.x][pos2.y];
}
#endif

#endif

#ifndef SRGB_GLSL
#define SRGB_GLSL

#extension GL_EXT_samplerless_texture_functions : enable

// Linear RGB, attenuation coefficients for water at roughly R, G, B wavelengths.
// See https://en.wikipedia.org/wiki/Electromagnetic_absorption_by_water
const vec3 MU_WATER = vec3(0.6, 0.04, 0.01);

//https://gamedev.stackexchange.com/questions/92015/optimized-linear-to-srgb-glsl
vec3 srgb_to_linear(vec3 srgb) {
    bvec3 cutoff = lessThan(srgb, vec3(0.04045));
    vec3 higher = pow((srgb + vec3(0.055))/vec3(1.055), vec3(2.4));
    vec3 lower = srgb/vec3(12.92);

    return mix(higher, lower, cutoff);
}

vec3 linear_to_srgb(vec3 col) {
    vec3 s1 = vec3(sqrt(col.r), sqrt(col.g), sqrt(col.b));
    vec3 s2 = vec3(sqrt(s1.r), sqrt(s1.g), sqrt(s1.b));
    vec3 s3 = vec3(sqrt(s2.r), sqrt(s2.g), sqrt(s2.b));
    return vec3(
            mix(11.500726 * col.r, (0.585122381 * s1.r + 0.783140355 * s2.r - 0.368262736 * s3.r), clamp((col.r - 0.0060) * 10000.0, 0.0, 1.0)),
            mix(11.500726 * col.g, (0.585122381 * s1.g + 0.783140355 * s2.g - 0.368262736 * s3.g), clamp((col.g - 0.0060) * 10000.0, 0.0, 1.0)),
            mix(11.500726 * col.b, (0.585122381 * s1.b + 0.783140355 * s2.b - 0.368262736 * s3.b), clamp((col.b - 0.0060) * 10000.0, 0.0, 1.0))
    );
}

float pow5(float x) {
    float x2 = x * x;
    return x2 * x2 * x;
}

vec4 pow5(vec4 x) {
    vec4 x2 = x * x;
    return x2 * x2 * x;
}

// Fresnel angle for perfectly specular dialectric materials.

// Schlick approximation
vec3 schlick_fresnel(vec3 Rs, float cosTheta) {
    return Rs + pow5(1.0 - cosTheta) * (1.0 - Rs);
}

// Beckmann Distribution
float BeckmannDistribution_D(float NdotH, float alpha) {
    const float PI = 3.1415926535897932384626433832795;
    float NdotH2 = NdotH * NdotH;
    float NdotH2m2 = NdotH2 * alpha * alpha;
    float k_spec = exp((NdotH2 - 1.0) / NdotH2m2) / (PI * NdotH2m2 * NdotH2);
    return mix(k_spec, 0.0, NdotH == 0.0);
}

// Voxel Distribution
float BeckmannDistribution_D_Voxel(vec3 wh, vec3 voxel_norm, float alpha) {
    vec3 sides = sign(voxel_norm);
    
    vec3 NdotH = wh * sides;

    const float PI = 3.1415926535897932384626433832795;
    vec3 NdotH2 = NdotH * NdotH;
    vec3 NdotH2m2 = NdotH2 * alpha * alpha;
    vec3 k_spec = exp((NdotH2 - 1.0) / NdotH2m2) / (PI * NdotH2m2 * NdotH2);
    return dot(mix(k_spec, vec3(0.0), equal(NdotH, vec3(0.0))), abs(voxel_norm));
}

float TrowbridgeReitzDistribution_D_Voxel(vec3 wh, vec3 voxel_norm, float alpha) {
    vec3 sides = sign(voxel_norm);

    vec3 NdotH = wh * sides;

    const float PI = 3.1415926535897932384626433832795;
    vec3 NdotH2 = NdotH * NdotH;
    vec3 NdotH2m2 = NdotH2 * alpha * alpha;
    vec3 e = (1.0 - NdotH2) / NdotH2m2;
    vec3 k_spec = 1.0 / (PI * NdotH2m2 * NdotH2 * (1.0 + e) * (1.0 + e));
    return dot(mix(k_spec, vec3(0.0), equal(NdotH, vec3(0.0))), abs(voxel_norm));
}

float BeckmannDistribution_Lambda(vec3 norm, vec3 dir, float alpha) {
    float CosTheta = dot(norm, dir);
    float SinTheta = sqrt(1.0 - CosTheta * CosTheta);
    float TanTheta = SinTheta / CosTheta;
    float absTanTheta = abs(TanTheta);
    float a = 1.0 / (alpha * absTanTheta);
    
    return mix(max(0.0, (1.0 - 1.259 * a + 0.396 * a * a) / (3.535 * a + 2.181 * a * a)), 0.0, isinf(absTanTheta) || a >= 1.6);
}

float BeckmannDistribution_G(vec3 norm, vec3 dir, vec3 light_dir, float alpha) {
    return 1.0 / (1.0 + BeckmannDistribution_Lambda(norm, dir, alpha) + BeckmannDistribution_Lambda(norm, -light_dir, alpha));
}

// Fresnel blending
//
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Microfacet_Models.html#fragment-MicrofacetDistributionPublicMethods-2
// and
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Fresnel_Incidence_Effects.html
vec3 FresnelBlend_f(vec3 norm, vec3 dir, vec3 light_dir, vec3 R_d, vec3 R_s, float alpha) {
    const float PI = 3.1415926535897932384626433832795;
    alpha = alpha * sqrt(2.0);
    float cos_wi = dot(-light_dir, norm);
    float cos_wo = dot(dir, norm);

    vec3 diffuse = (28.0 / (23.0 * PI)) * R_d *
        (1.0 - R_s) *
        (1.0 - pow5(1.0 - 0.5 * abs(cos_wi))) *
        (1.0 - pow5(1.0 - 0.5 * abs(cos_wo)));
    vec3 wh = -light_dir + dir;
#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    bool is_blocked = cos_wi == 0.0 || cos_wo == 0.0;
#else
    bool is_blocked = cos_wi <= 0.0 || cos_wo <= 0.0;
#endif
    if (is_blocked) {
        return vec3(0.0);
    }
    wh = normalize(wh);
    float dot_wi_wh = dot(-light_dir, wh);
    vec3 specular = dot(norm, dir) > 0.0 ? vec3(0.0) : (BeckmannDistribution_D(dot(wh, norm), alpha) /
        (4.0 * abs(dot_wi_wh) *
        max(abs(cos_wi), abs(cos_wo))) *
        schlick_fresnel(R_s, dot_wi_wh));
    return mix(diffuse + specular, vec3(0.0), bvec3(all(equal(light_dir, dir))));
}

// Fresnel blending
//
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Microfacet_Models.html#fragment-MicrofacetDistributionPublicMethods-2
// and
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Fresnel_Incidence_Effects.html
vec3 FresnelBlend_Voxel_f(vec3 norm, vec3 dir, vec3 light_dir, vec3 R_d, vec3 R_s, float alpha, vec3 voxel_norm, float dist) {
    const float PI = 3.1415926535897932384626433832795;
    alpha = alpha * sqrt(2.0);
    float cos_wi = dot(-light_dir, norm);
    float cos_wo = dot(dir, norm);

#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    vec4 AbsNdotL = abs(vec4(light_dir, cos_wi));
    vec4 AbsNdotV = abs(vec4(dir, cos_wo));
#else
    vec3 sides = sign(voxel_norm);
    vec4 AbsNdotL = vec4(max(-light_dir * sides, 0.0), abs(cos_wi));
    vec4 AbsNdotV = vec4(max(dir * sides, 0.0), abs(cos_wo));
#endif

    vec4 diffuse_factor = (1.0 - pow5(1.0 - 0.5 * AbsNdotL)) * (1.0 - pow5(1.0 - 0.5 * AbsNdotV));

    vec3 diffuse = (28.0 / (23.0 * PI)) * R_d * (1.0 - R_s) * dot(diffuse_factor, /*R_r * */vec4(abs(norm) * (1.0 - dist), dist));

    vec3 wh = -light_dir + dir;
#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    bool is_blocked = cos_wi == 0.0 || cos_wo == 0.0;
#else
    bool is_blocked = cos_wi <= 0.0 || cos_wo <= 0.0;
#endif
    if (is_blocked) {
        return vec3(0.0);
    }
    wh = normalize(wh);
    float dot_wi_wh = dot(-light_dir, wh);
    float distr = BeckmannDistribution_D_Voxel(wh, voxel_norm, alpha);
    vec3 specular = distr /
        (4.0 * abs(dot_wi_wh) *
        max(abs(cos_wi), abs(cos_wo))) *
        schlick_fresnel(R_s, dot_wi_wh);
    return mix(diffuse + specular, vec3(0.0), bvec3(all(equal(light_dir, dir))));
}

// Phong reflection.
//
// Note: norm, dir, light_dir must all be normalizd.
vec3 light_reflection_factor2(vec3 norm, vec3 dir, vec3 light_dir, vec3 k_d, vec3 k_s, float alpha) {
    // TODO: These are supposed to be the differential changes in the point location p, in tangent space.
    // That is, assuming we can parameterize a 2D surface by some function p : R² → R³, mapping from
    // points in a plane to 3D points on the surface, we can define
    // ∂p(u,v)/∂u and ∂p(u,v)/∂v representing the changes in the pont location as we move along these
    // coordinates.
    //
    // Then we can define the normal at a point, n(u,v) = ∂p(u,v)/∂u × ∂p(u,v)/∂v.
    //
    // Additionally, we can define the change in *normals* at each point using the
    // Weingarten equations (see http://www.pbr-book.org/3ed-2018/Shapes/Spheres.html):
    //
    // ∂n/∂u = (fF - eG) / (EG - F²) ∂p/∂u + (eF - fE) / (EG - F²) ∂p/∂v
    // ∂n/∂v = (gF - fG) / (EG - F²) ∂p/∂u + (fF - gE) / (EG - F²) ∂p/∂v
    //
    // where
    //
    // E = |∂p/∂u ⋅ ∂p/∂u|
    // F = ∂p/∂u ⋅ ∂p/∂u
    // G = |∂p/∂v ⋅ ∂p/∂v|
    //
    // and
    //
    // e = n ⋅ ∂²p/∂u²
    // f = n ⋅ ∂²p/(∂u∂v)
    // g = n ⋅ ∂²p/∂v²
    //
    // For planes (see http://www.pbr-book.org/3ed-2018/Shapes/Triangle_Meshes.html) we have
    // e = f = g = 0 (since the plane has no curvature of any sort) so we get:
    //
    // ∂n/∂u = (0, 0, 0)
    // ∂n/∂v = (0, 0, 0)
    //
    // To find ∂p/∂u and ∂p/∂v, we first write p and u parametrically:
    //    p(u, v) = p0 + u ∂p/∂u + v ∂p/∂v
    //
    // ( u₀ - u₂    v₀ - v₂
    //   u₁ - u₂    v₁ - v₂ )
    //
    // Basis: plane norm = norm = (0, 0, 1), x vector = any orthgonal vector on the plane.
    // vec3 w_i =
    // vec3 w_i = vec3(view_mat * vec4(-light_dir, 1.0));
    // vec3 w_o = vec3(view_mat * vec4(light_dir, 1.0));
    return FresnelBlend_f(norm, dir, light_dir, k_d, k_s, alpha);
}

vec3 light_reflection_factor(vec3 norm, vec3 dir, vec3 light_dir, vec3 k_d, vec3 k_s, float alpha, vec3 voxel_norm, float voxel_lighting) {
#if (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_LAMBERTIAN)
    const float PI = 3.141592;
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    vec4 AbsNdotL = abs(vec4(light_dir, dot(norm, light_dir)));
        #else
    vec3 sides = sign(voxel_norm);
    vec4 AbsNdotL = max(vec4(-light_dir * sides, dot(norm, -light_dir)), 0.0);
        #endif
    float diffuse = dot(AbsNdotL, vec4(abs(voxel_norm) * (1.0 - voxel_lighting), voxel_lighting));
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    float diffuse = abs(dot(norm, light_dir));
        #else
    float diffuse = max(dot(norm, -light_dir), 0.0);
        #endif
    #endif
    return k_d / PI * diffuse;
#elif (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_BLINN_PHONG)
    const float PI = 3.141592;
    alpha = alpha * sqrt(2.0);
    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    float ndotL = abs(dot(norm, light_dir));
    #else
    float ndotL = max(dot(norm, -light_dir), 0.0);
    #endif

    if (ndotL > 0.0) {
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        vec4 AbsNdotL = abs(vec4(light_dir, ndotL));
        #else
        vec3 sides = sign(voxel_norm);
        vec4 AbsNdotL = max(vec4(-light_dir * sides, ndotL), 0.0);
        #endif
        float diffuse = dot(AbsNdotL, vec4(abs(voxel_norm) * (1.0 - voxel_lighting), voxel_lighting));
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        float diffuse = ndotL;
    #endif
        vec3 H = normalize(-light_dir + dir);

    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        float NdotH = abs(dot(norm, H));
    #else
        float NdotH = max(dot(norm, H), 0.0);
    #endif
        return (1.0 - k_s) / PI * k_d * diffuse + k_s * pow(NdotH, alpha/* * 4.0*/);
    }

    return vec3(0.0);
#elif (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_ASHIKHMIN)
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        return FresnelBlend_Voxel_f(norm, dir, light_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting);
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        return FresnelBlend_f(norm, dir, light_dir, k_d, k_s, alpha);
    #endif
#endif
}

float rel_luminance(vec3 rgb)
{
    // https://en.wikipedia.org/wiki/Relative_luminance
    const vec3 W = vec3(0.2126, 0.7152, 0.0722);
    return dot(rgb, W);
}

// From https://discourse.vvvv.org/t/infinite-ray-intersects-with-infinite-plane/10537
// out of laziness.
bool IntersectRayPlane(vec3 rayOrigin, vec3 rayDirection, vec3 posOnPlane, vec3 planeNormal, inout vec3 intersectionPoint)
{
  float rDotn = dot(rayDirection, planeNormal);

  //parallel to plane or pointing away from plane?
  if (rDotn < 0.0000001 )
    return false;

  float s = dot(planeNormal, (posOnPlane - rayOrigin)) / rDotn;

  intersectionPoint = rayOrigin + s * rayDirection;

  return true;
}

// Compute uniform attenuation due to beam passing through a substance that fills an area below a horizontal plane
// (e.g. in most cases, water below the water surface depth) using the simplest form of the Beer-Lambert law
// (https://en.wikipedia.org/wiki/Beer%E2%80%93Lambert_law):
//
// I(z) = I₀ e^(-μz)
//
// We compute this value, except for the initial intensity which may be multiplied out later.
//
// wpos is the position of the point being hit.
// ray_dir is the reversed direction of the ray (going "out" of the point being hit).
// mu is the attenuation coefficient for R, G, and B wavelenghts.
// surface_alt is the estimated altitude of the horizontal surface separating the substance from air.
// defaultpos is the position to use in computing the distance along material at this point if there was a failure.
//
// Ideally, defaultpos is set so we can avoid branching on error.
vec3 compute_attenuation(vec3 wpos, vec3 ray_dir, vec3 mu, float surface_alt, vec3 defaultpos) {
#if (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_IMPORTANCE)
    return vec3(1.0);
#elif (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_RADIANCE)
    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        return vec3(1.0);
    #else
    ray_dir = faceforward(ray_dir, vec3(0.0, 0.0, -1.0), ray_dir);
    vec3 surface_dir = surface_alt < wpos.z ? vec3(0.0, 0.0, -1.0) : vec3(0.0, 0.0, 1.0);
    bool _intersects_surface = IntersectRayPlane(wpos, ray_dir, vec3(0.0, 0.0, surface_alt), surface_dir, defaultpos);
    float depth = length(defaultpos - wpos);
    return exp(-mu * depth);
    #endif
#endif
}

// Same as compute_attenuation but since both point are known, set a maximum to make sure we don't exceed the length
// from the default point.
vec3 compute_attenuation_point(vec3 wpos, vec3 ray_dir, vec3 mu, float surface_alt, vec3 defaultpos) {
#if (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_IMPORTANCE)
    return pow(1.0 - mu, vec3(3));
#elif (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_RADIANCE)
    return vec3(1.0);
#endif
}

vec3 greedy_extract_col_light_attr(texture2D t_col_light, sampler s_col_light, vec2 f_uv_pos, out float f_light, out float f_glow, out float f_ao, out uint f_attr, out float f_sky_exposure) {
    // TODO: Figure out how to use `texture` and modulation to avoid needing to do manual filtering
    // TODO: Use `texture` instead

    uvec4 tex_00 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(0, 0), 0) * 255.0);
    uvec4 tex_10 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(1, 0), 0) * 255.0);
    uvec4 tex_01 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(0, 1), 0) * 255.0);
    uvec4 tex_11 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(1, 1), 0) * 255.0);
    vec3 light_00 = vec3(tex_00.rg >> 3u, tex_00.a & 1u);
    vec3 light_10 = vec3(tex_10.rg >> 3u, tex_10.a & 1u);
    vec3 light_01 = vec3(tex_01.rg >> 3u, tex_01.a & 1u);
    vec3 light_11 = vec3(tex_11.rg >> 3u, tex_11.a & 1u);
    vec3 light_0 = mix(light_00, light_01, fract(f_uv_pos.y));
    vec3 light_1 = mix(light_10, light_11, fract(f_uv_pos.y));
    vec3 light = mix(light_0, light_1, fract(f_uv_pos.x));

    vec3 f_col = vec3(
        float(((tex_00.r & 0x7u) << 1u) | (tex_00.b & 0xF0u)),
        float(tex_00.a & 0xFEu),
        float(((tex_00.g & 0x7u) << 1u) | ((tex_00.b & 0x0Fu) << 4u))
    ) / 255.0;

    f_ao = light.z;
    f_light = light.x / 31.0;
    f_sky_exposure = light.x / 31.0 + (1.0 - f_ao) * 0.5;
    f_glow = light.y / 31.0;
    f_attr = tex_00.g >> 3u;
    return srgb_to_linear(f_col);
}

vec3 greedy_extract_col_light_kind_terrain(
    texture2D t_col_light, sampler s_col_light,
    utexture2D t_kind,
    vec2 f_uv_pos,
    out float f_light, out float f_glow, out float f_ao, out float f_sky_exposure, out uint f_kind
) {
    uint _f_attr;
    f_kind = uint(texelFetch(t_kind, ivec2(f_uv_pos), 0).r);
    return greedy_extract_col_light_attr(t_col_light, s_col_light, f_uv_pos, f_light, f_glow, f_ao, _f_attr, f_sky_exposure);
}

vec3 greedy_extract_col_light_figure(texture2D t_col_light, sampler s_col_light, vec2 f_uv_pos, out float f_light, out uint f_attr) {
    float _f_sky_exposure, _f_light, _f_glow, _f_ao;
    return greedy_extract_col_light_attr(t_col_light, s_col_light, f_uv_pos, f_light, _f_glow, _f_ao, f_attr, _f_sky_exposure);
}

#endif

#ifndef SHADOWS_GLSL
#define SHADOWS_GLSL

#ifdef HAS_SHADOW_MAPS
    #if (SHADOW_MODE == SHADOW_MODE_MAP)
        layout (std140, set = 0, binding = 9)
        uniform u_light_shadows {
            mat4 shadowMatrices;
            mat4 texture_mat;
        };
        
        // Use with sampler2DShadow
        #define t_directed_shadow_maps packed_input

        #define s_directed_shadow_maps packed_sampler

        
        // Use with samplerCubeShadow
        #define t_point_shadow_maps packed_input

        #define s_point_shadow_maps packed_sampler

        
        float VectorToDepth(vec3 Vec) {
            vec3 AbsVec = abs(Vec);
            float LocalZcomp = max(AbsVec.x, max(AbsVec.y, AbsVec.z));
        
            float NormZComp = shadow_proj_factors.x - shadow_proj_factors.y / LocalZcomp;
            return NormZComp;
        }
        
        const vec3 sampleOffsetDirections[20] = vec3[](
            vec3( 1,  1,  1), vec3( 1, -1,  1), vec3(-1, -1,  1), vec3(-1,  1,  1),
            vec3( 1,  1, -1), vec3( 1, -1, -1), vec3(-1, -1, -1), vec3(-1,  1, -1),
            vec3( 1,  1,  0), vec3( 1, -1,  0), vec3(-1, -1,  0), vec3(-1,  1,  0),
            vec3( 1,  0,  1), vec3(-1,  0,  1), vec3( 1,  0, -1), vec3(-1,  0, -1),
            vec3( 0,  1,  1), vec3( 0, -1,  1), vec3( 0, -1, -1), vec3( 0,  1, -1)
        );
        
        float ShadowCalculationPoint(uint lightIndex, vec3 fragToLight, vec3 fragNorm, vec3 fragPos) {
            if (lightIndex != 0u) {
                return 1.0;
            };
        
            float currentDepth = VectorToDepth(fragToLight);
        
            return textureGrad(samplerCubeShadow(t_point_shadow_maps, s_point_shadow_maps), vec4(fragToLight, currentDepth), vec3(0), vec3(0));
        }
        
        float ShadowCalculationDirected(in vec3 fragPos) {
            // Don't try to calculate directed shadows if there are no directed light sources
            // Applies, for example, in the char select menu
            if (light_shadow_count.z < 1) { return 1.0; }
        
            float bias = 0.0;
            float diskRadius = 0.01;
            vec4 sun_pos = texture_mat * vec4(fragPos, 1.0);
            return textureProj(sampler2DShadow(t_directed_shadow_maps, s_directed_shadow_maps), sun_pos);
        }
    #elif (SHADOW_MODE == SHADOW_MODE_NONE || SHADOW_MODE == SHADOW_MODE_CHEAP)
        float ShadowCalculationPoint(uint lightIndex, vec3 fragToLight, vec3 fragNorm, vec3 fragPos) {
            return 1.0;
        }
    #endif
#else
    float ShadowCalculationPoint(uint lightIndex, vec3 fragToLight, vec3 fragNorm, vec3 fragPos) {
        return 1.0;
    }
#endif

#endif

#ifndef GLOBALS_GLSL
#define GLOBALS_GLSL


    mat4 view_mat;
    mat4 proj_mat;
    mat4 all_mat;
    vec4 cam_pos;
    vec4 focus_off;
    vec4 focus_pos;
    vec4 view_distance;
    // .x = time of day, repeats every day.
    // .y = a continuous value for what day it is. Repeats every `tick_overflow` for precisions sake.
    vec4 time_of_day;
    vec4 sun_dir;
    vec4 moon_dir;
    // .x = The `Time` resource, repeated every `tick_overflow`
    // .y = a floored (`Time` / `tick_overflow`)
    // .z = Time local to client, not synced between clients.
    vec4 tick;
    vec4 screen_res;
    uvec4 light_shadow_count;
    vec4 shadow_proj_factors;
    uvec4 medium;
    ivec4 select_pos;
    vec4 gamma_exposure;
    vec4 last_lightning;
    vec2 wind_vel;
    vec2 internal_res;
    float ambiance;
    // 0 - FirstPerson
    // 1 - ThirdPerson
    uint cam_mode;
    float sprite_render_distance;
    float u_rotation;
    float screen_fade;


float distance_divider = 2.0;
float shadow_dithering = 0.5;

float tick_overflow = 300000.0;

// Get a scaled time with an offset that loops at a period.
float tick_loop(float period, float scale, float offset) {
    float loop = tick_overflow * scale;
    float rem = mod(loop, period);
    float rest = rem * tick.y;

    return mod(rest + tick.x * scale + offset, period);
}

float tick_loop(float period) {
    return tick_loop(period, 1.0, 0.0);
}

vec3 tick_loop(float period, vec3 scale, vec3 offset) {
    vec3 loop = tick_overflow * scale;
    vec3 rem = mod(loop, period);
    vec3 rest = rem * tick.y;

    return mod(rest + tick.x * scale + offset, period);
}

// Only works if t happened within tick_overflow
float time_since(float t) {
    return tick.x < t ? (tick_overflow - t + tick.x) : (tick.x - t);
}

#endif


#ifndef RAIN_OCCLUSION_GLSL
#define RAIN_OCCLUSION_GLSL

// Use with sampler2DShadow
#define t_directed_occlusion_maps packed_input

#define s_directed_occlusion_maps packed_sampler



    mat4 rain_occlusion_matrices;
    mat4 rain_occlusion_texture_mat;
    mat4 rain_dir_mat;
    float integrated_rain_vel;
    float rain_density;
    vec2 occlusion_dummy; // Fix alignment.


float rain_occlusion_at(in vec3 fragPos)
{
    vec4 rain_pos = rain_occlusion_texture_mat * vec4(fragPos, 1.0);

    float visibility = textureProj(sampler2DShadow(t_directed_occlusion_maps, s_directed_occlusion_maps), rain_pos);

    return visibility;
}
#endif

// Information about an approximately directional light, like the sun or moon.
struct DirectionalLight {
    float shadow;
    // Fully blocks all light, including ambience
    float block;
};

const float PI = 3.141592653;

const vec3 SKY_DAWN_TOP = vec3(0.10, 0.1, 0.10);
const vec3 SKY_DAWN_MID = vec3(1.2, 0.3, 0.2);
const vec3 SKY_DAWN_BOT = vec3(0.0, 0.1, 0.23);
const vec3 DAWN_LIGHT   = vec3(5.0, 2.0, 1.15);
const vec3 SUN_HALO_DAWN = vec3(3.0, 0.5, 0.1);

const vec3 SKY_DAY_TOP = vec3(0.1, 0.5, 0.9);
const vec3 SKY_DAY_MID = vec3(0.18, 0.28, 0.6);
const vec3 SKY_DAY_BOT = vec3(0.1, 0.2, 0.3);
const vec3 DAY_LIGHT   = vec3(3.8, 3.0, 1.8);
const vec3 SUN_HALO_DAY = vec3(0.15, 0.15, 0.001);

const vec3 SKY_DUSK_TOP = vec3(1.06, 0.1, 0.20);
const vec3 SKY_DUSK_MID = vec3(2.5, 0.3, 0.1);
const vec3 SKY_DUSK_BOT = vec3(0.0, 0.1, 0.23);
const vec3 DUSK_LIGHT   = vec3(8.0, 1.5, 0.15);
const vec3 SUN_HALO_DUSK = vec3(3.0, 0.5, 0.05);

const vec3 SKY_NIGHT_TOP = vec3(0.001, 0.001, 0.0025);
const vec3 SKY_NIGHT_MID = vec3(0.001, 0.005, 0.02);
const vec3 SKY_NIGHT_BOT = vec3(0.002, 0.004, 0.004);
const vec3 NIGHT_LIGHT   = vec3(5.0, 0.75, 0.2);

// Linear RGB, scattering coefficients for atmosphere at roughly R, G, B wavelengths.
//
// See https://en.wikipedia.org/wiki/Diffuse_sky_radiation
const vec3 MU_SCATTER = vec3(0.05, 0.10, 0.23);

const float SUN_COLOR_FACTOR = 5.0;
const float MOON_COLOR_FACTOR = 5.0;

const float UNDERWATER_MIST_DIST = 100.0;

const float PERSISTENT_AMBIANCE = 1.0 / 32.0;

// Glow from static light sources
// Allowed to be > 1 due to HDR
const vec3 GLOW_COLOR = vec3(0.89, 0.95, 0.52);

// Calculate glow from static light sources, + some noise for flickering.
// TODO: Optionally disable the flickering for performance?
vec3 glow_light(vec3 pos) {
    #if (SHADOW_MODE <= SHADOW_MODE_NONE)
        return GLOW_COLOR;
    #else
        return GLOW_COLOR * (1.0 + (noise_3d(vec3(pos.xy * 0.005, tick.x * 0.5)) - 0.5) * 0.5);
    #endif
}

float cloud_avg_alt() { return view_distance.z + (view_distance.w - view_distance.z) * 1.25; }

const float wind_speed = 0.25;
vec2 wind_offset() { return vec2(time_of_day.y * wind_speed * (3600.0 * 24.0)); }

float cloud_scale() { return view_distance.z / 150.0; }

#define t_alt packed_input

#define s_alt packed_sampler


// Transforms coordinate in the range 0..WORLD_SIZE to 0..1
vec2 wpos_to_uv(vec2 wpos) { return (wpos+16.0)/(32.0*vec2(param(24),param(25))); }

// Weather texture
#define t_weather packed_input

#define s_weather packed_sampler


vec4 sample_weather(vec2 wpos) { return weather_sample(wpos_to_uv(wpos)); }

float cloud_tendency_at(vec2 wpos) {
    return sample_weather(wpos).r;
}

float rain_density_at(vec2 wpos) {
    return sample_weather(wpos).g;
}

float cloud_shadow(vec3 pos, vec3 light_dir) {
    #if (CLOUD_MODE <= CLOUD_MODE_MINIMAL)
        return 1.0;
    #else
        vec2 xy_offset = light_dir.xy * ((cloud_avg_alt() - pos.z) / -light_dir.z);

        // Fade out shadow if the sun angle is too steep (simulates a widening penumbra with distance)
        const vec2 FADE_RANGE = vec2(1500, 10000);
        float fade = 1.0 - clamp((length(xy_offset) - FADE_RANGE.x) / (FADE_RANGE.y - FADE_RANGE.x), 0, 1);
        float cloud = cloud_tendency_at(pos.xy + focus_off.xy - xy_offset);

        return clamp(1 - fade * cloud * 16.0, 0, 1);
    #endif
}

float magnetosphere() { return sin(time_of_day.y); }

vec3 magnetosphere_tint() {
    #if (CLOUD_MODE <= CLOUD_MODE_LOW)
        return vec3(1);
    #else
        float magnetosphere = magnetosphere();
        float magnetosphere2 = pow(magnetosphere, 2) * 2 - 1;
        float magnetosphere3 = pow(magnetosphere2, 2) * 2 - 1;
        vec3 magnetosphere_change = vec3(1.0) + vec3(
            (magnetosphere + 1.0) * 2.0,
            (-magnetosphere2 + 1.0) * 2.0,
            (-magnetosphere3 + 1.0) * 1.0
        ) * 0.4;
        return normalize(magnetosphere_change);
    #endif
 }

#if (CLOUD_MODE > CLOUD_MODE_FLAT)
float emission_strength() {
    return clamp((magnetosphere() - 0.3) * 1.3, 0, 1) * max(sun_dir.z, 0);
}

float emission_br() {
    #if (CLOUD_MODE >= CLOUD_MODE_MEDIUM)
        return abs(pow(fract(time_of_day.y * 0.5) * 2 - 1, 2));
    #else
        return 0.5;
    #endif
}
#endif


float get_sun_brightness() {
    return max(-sun_dir.z + 0.5, 0.0);
}

float get_moon_brightness() {
    return max(sun_dir.z + 0.6, 0.0) * 0.1;
}

vec3 get_sun_color() {
    vec3 light = (sun_dir.x > 0) ? DUSK_LIGHT : DAWN_LIGHT;

    return mix(
        mix(
            light * magnetosphere_tint(),
            NIGHT_LIGHT,
            max(sun_dir.z, 0)
        ),
        DAY_LIGHT,
        max(-sun_dir.z, 0)
    );
}

// Average sky colour (i.e: perfectly scattered light from the sky)
vec3 get_sky_color() {
    return mix(
        mix(
            (SKY_DUSK_TOP + SKY_DUSK_MID) / 2 * magnetosphere_tint(),
            (SKY_NIGHT_TOP + SKY_NIGHT_MID) / 2,
            max(sun_dir.z, 0)
        ),
        (SKY_DAY_TOP + SKY_DAY_MID) / 2,
        max(-sun_dir.z, 0)
    );
}

vec3 get_moon_color() {
    return vec3(0.5, 0.5, 1.6);
}

DirectionalLight get_sun_info(vec4 _dir, float shade_frac, vec3 f_pos) {
    float shadow = shade_frac;
    float block = 1.0;
#ifdef HAS_SHADOW_MAPS
    #if (SHADOW_MODE == SHADOW_MODE_MAP)
        if (sun_dir.z < 0.0) {
            shadow = min(shadow, ShadowCalculationDirected(f_pos));
        }
    #endif
#endif
    return DirectionalLight(shadow, block);
}

DirectionalLight get_moon_info(vec4 _dir, float shade_frac) {
    float shadow = shade_frac;
    float block = 1.0;
    return DirectionalLight(shadow, block);
}

const float LIGHTNING_HEIGHT = 25.0;
const float MAX_LIGHTNING_PERIOD = 5.0;

float lightning_intensity() {
    float time_since_lightning = time_since(last_lightning.w);
    return
        // Strength
        1000000
        // Flash
        * max(0.0, 1.0 - time_since_lightning * 1.0)
        // Reverb
        * max(sin(time_of_day.x * 0.4), 0.0);
}

vec3 lightning_at(vec3 wpos) {
    float time_since_lightning = time_since(last_lightning.w);
    if (time_since_lightning < MAX_LIGHTNING_PERIOD) {
        vec3 diff = wpos + focus_off.xyz - (last_lightning.xyz + vec3(0, 0, LIGHTNING_HEIGHT));
        float dist = length(diff);
        return vec3(0.5, 0.8, 1.0)
            * lightning_intensity()
            // Attenuation
            / pow(50.0 + dist, 2);
    } else {
        return vec3(0.0);
    }
}

// Returns computed maximum intensity.
//
// wpos is the position of this fragment.
// mu is the attenuation coefficient for any substance on a horizontal plane.
// cam_attenuation is the total light attenuation due to the substance for beams between the point and the camera.
// surface_alt is the altitude of the attenuating surface.
float get_sun_diffuse2(
    DirectionalLight sun_info,
    DirectionalLight moon_info,
    vec3 norm,
    vec3 dir,
    vec3 wpos,
    vec3 mu,
    vec3 cam_attenuation,
    float surface_alt,
    vec3 k_a,
    vec3 k_d,
    vec3 k_s,
    float alpha,
    vec3 voxel_norm,
    float voxel_lighting,
    out vec3 emitted_light,
    out vec3 reflected_light
) {
    const vec3 SUN_AMBIANCE = MU_SCATTER;
    #ifdef EXPERIMENTAL_PHOTOREALISTIC
        const vec3 MOON_AMBIANCE = MU_SCATTER;
    #else
        // Boost ambiance, because we don't properly compensate for pupil dilation (which should occur *before* HDR,
        // not in the end user's eye). Also, real nights are too dark to be fun.
        const vec3 MOON_AMBIANCE = vec3(0.15, 0.25, 0.23) * 5;
    #endif

    vec3 sun_dir = sun_dir.xyz;
    // TODO: Use real moon dir here and have other ways to light up night.
    // So this is a hack to just pretend the moon is still opposite to the sun
    // for this and `get_moon_brightness`.
    vec3 moon_dir = -sun_dir.xyz;

    float sun_light = get_sun_brightness() * sun_info.block;
    float moon_light = get_moon_brightness() * moon_info.block * ambiance;

    vec3 sun_color = get_sun_color() * SUN_COLOR_FACTOR;
    vec3 moon_color = get_moon_color() * MOON_COLOR_FACTOR;

    // If the sun is facing the wrong way, we currently just want zero light, hence default point is wpos.
    vec3 sun_attenuation = compute_attenuation(wpos, -sun_dir, mu, surface_alt, wpos);
    vec3 moon_attenuation = compute_attenuation(wpos, -moon_dir, mu, surface_alt, wpos);

    vec3 sun_chroma = sun_color * sun_light * cam_attenuation * sun_attenuation;
    vec3 moon_chroma = moon_color * moon_light * cam_attenuation * moon_attenuation;

    float sun_shadow = sun_info.shadow * cloud_shadow(wpos, sun_dir);
    float moon_shadow = moon_info.shadow * cloud_shadow(wpos, moon_dir);

    // https://en.m.wikipedia.org/wiki/Diffuse_sky_radiation
    //
    // HdRd radiation should come in at angle normal to us.
    // const float H_d = 0.23;
    //
    // Let β be the angle from horizontal
    // (for objects exposed to the sky, where positive when sloping towards south and negative when sloping towards north):
    //
    //     sin β = (north ⋅ norm) / |north||norm|
    //           = dot(vec3(0, 1, 0), norm)
    //
    //     cos β = sqrt(1.0 - dot(vec3(0, 1, 0), norm))
    //
    // Let h be the hour angle (180/0.0 at midnight, 90/1.0 at dawn, 0/0.0 at noon, -90/-1.0 at dusk, -180 at midnight/0.0):
    //     cos h = (midnight ⋅ -light_dir) / |midnight||-light_dir|
    //           = (noon ⋅ light_dir) / |noon||light_dir|
    //           = dot(vec3(0, 0, 1), light_dir)
    //
    // Let φ be the latitude at this point. 0 at equator, -90 at south pole / 90 at north pole.
    //
    // Let δ be the solar declination (angular distance of the sun's rays north [or south[]
    // of the equator), i.e. the angle made by the line joining the centers of the sun and Earth with its projection on the
    // equatorial plane.  Caused by axial tilt, and 0 at equinoxes.  Normally varies between -23.45 and 23.45 degrees.
    //
    // Let α (the solar altitude / altitud3 angle) be the vertical angle between the projection of the sun's rays on the
    // horizontal plane and the direction of the sun's rays (passing through a point).
    //
    // Let Θ_z be the vertical angle between sun's rays and a line perpendicular to the horizontal plane through a point,
    // i.e.
    //
    // Θ_z = (π/2) - α
    //
    // i.e. cos Θ_z = sin α and
    //      cos α = sin Θ_z
    //
    // Let γ_s be the horizontal angle measured from north to the horizontal projection of the sun's rays (positive when
    // measured westwise).
    //
    // cos Θ_z = cos φ cos h cos δ + sin φ sin δ
    // cos γ_s = sec α (cos φ sin δ - cos δ sin φ cos h)
    //         = (1  / √(1 - cos² Θ_z)) (cos φ sin δ - cos δ sin φ cos h)
    // sin γ_s = sec α cos δ sin h
    //         = (1 / cos α) cos δ sin h
    //         = (1 / sin Θ_z) cos δ sin h
    //         = (1  / √(1 - cos² Θ_z)) cos δ sin h
    //
    // R_b = (sin(δ)sin(φ - β) + cos(δ)cos(h)cos(φ - β))/(sin(δ)sin(φ) + cos(δ)cos(h)cos(φ))
    //
    // Assuming we are on the equator (i.e. φ = 0), and there is no axial tilt or we are at an equinox (i.e. δ = 0):
    //
    // cos Θ_z = 1 * cos h * 1 + 0 * 0 = cos h
    // cos γ_s = (1  / √(1 - cos² h)) (1 * 0 - 1 * 0 * cos h)
    //         = (1  / √(1 - cos² h)) * 0
    //         = 0
    // sin γ_s = (1  / √(1 - cos² h)) * sin h
    //         = sin h / sin h
    //         = 1
    //
    // R_b = (0 * sin(0 - β) + 1 * cos(h) * cos(0 - β))/(0 * 0 + 1 * cos(h) * 1)
    //     = (cos(h)cos(-β)) / cos(H)
    //     = cos(-β), the angle from horizontal.
    //
    // NOTE: cos(-β) = cos(β).
    // float cos_sun = dot(norm, /*-sun_dir*/vec3(0, 0, 1));
    // float cos_moon = dot(norm, -moon_dir);
    //
    // Let ζ = diffuse reflectance of surrounding ground for solar radiation, then we have
    //
    // R_d = (1 + cos β) / 2
    // R_r = ζ (1 - cos β) / 2
    //
    // H_t = H_b R_b + H_d R_d + (H_b + H_d) R_r
    float sin_beta = dot(vec3(0, 1, 0), norm);
    float R_b = sqrt(max(0.0, 1.0 - sin_beta * sin_beta));
    // Rough estimate of diffuse reflectance of rest of ground.
    // NOTE: zeta should be close to 0.7 with snow cover, 0.2 normally?  Maybe?
    vec3 zeta = max(vec3(0.2), k_d * (1.0 - k_s));
    float R_d = (1 + R_b) * 0.5;
    vec3 R_r = zeta * (1.0 - R_b) * 0.5;
    //
    // We can break this down into:
    //      H_t_b = H_b * (R_b + R_r) = light_intensity * (R_b + R_r)
    //      H_t_r = H_d * (R_d + R_r) = light_intensity * (R_d + R_r)
    vec3 R_t_b = R_b + R_r;
    vec3 R_t_r = R_d + R_r;

    #ifdef EXPERIMENTAL_PHOTOREALISTIC
        vec3 lrf = light_reflection_factor(norm, dir, -norm, k_d, vec3(0.0), alpha, voxel_norm, voxel_lighting);
    #else
        // In practice, for gameplay purposes, we often want extra light at earlier and later times, so we use a
        // non-physical LRF to boost light during dawn and dusk.
        float lrf = pow(dot(norm, vec3(0, 0, 1)) + 1, 2) * 0.25;
    #endif
    vec3 light_frac = R_t_b * (sun_chroma * SUN_AMBIANCE + moon_chroma * MOON_AMBIANCE) * lrf;

    emitted_light = light_frac;

    vec3 emission = vec3(0);
    #if (CLOUD_MODE > CLOUD_MODE_FLAT)
        if (emission_strength() > 0.0) {
            emission = mix(vec3(0, 0.5, 1), vec3(1, 0, 0), emission_br()) * emission_strength() * 0.025;
        }
    #endif

    #ifdef FLASHING_LIGHTS_ENABLED
        vec3 lightning = lightning_at(wpos);
    #else
        vec3 lightning = vec3(0);
    #endif

    reflected_light = R_t_r * (
        (1.0 - SUN_AMBIANCE) * sun_chroma * sun_shadow * light_reflection_factor(norm, dir, sun_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting)
        + (1.0 - MOON_AMBIANCE) * moon_chroma * moon_shadow * light_reflection_factor(norm, dir, moon_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting)
        + emission
    ) + lightning;

    return rel_luminance(emitted_light + reflected_light);
}

// This has been extracted into a function to allow quick exit when detecting a star.
float is_star_at(vec3 dir) {
    float star_scale = 80.0;

    // Star positions
    vec3 pos = (floor(dir * star_scale) - 0.5) / star_scale;

    // Noisy offsets
    pos += (3.0 / star_scale) * (1.0 + hash(pos.yxzz) * 0.85);

    // Find distance to fragment
    float dist = length(pos - dir);

    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        const float power = 5.0;
    #else
        const float power = 50.0;
    #endif
    return power * max(sun_dir.z, 0.1) / (1.0 + pow(dist * 750, 8));
}

vec3 get_sky_light(vec3 dir, bool with_stars, float is_moon) {
    // Add white dots for stars. Note these flicker and jump due to FXAA
    float star = 0.0;
    if (with_stars) {
        vec3 star_dir = sun_dir.xyz * dir.z + cross(sun_dir.xyz, vec3(0, 1, 0)) * dir.x + vec3(0, 1, 0) * dir.y;
        star = is_star_at(star_dir) * (1.0 - is_moon);
    }

    vec3 sky_twilight_top = vec3(0.0, 0.0, 0.0);
    vec3 sky_twilight_mid = vec3(0.0, 0.0, 0.0);
    vec3 sky_twilight_bot = vec3(0.0, 0.0, 0.0);
    if (sun_dir.x > 0) {
      sky_twilight_top = SKY_DUSK_TOP;
      sky_twilight_mid = SKY_DUSK_MID;
      sky_twilight_bot = SKY_DUSK_BOT;
    } else {
      sky_twilight_top = SKY_DAWN_TOP;
      sky_twilight_mid = SKY_DAWN_MID;
      sky_twilight_bot = SKY_DAWN_BOT;
    }

    vec3 sky_top = mix(
        mix(
            sky_twilight_top * magnetosphere_tint(),
            SKY_NIGHT_TOP,
            pow(max(sun_dir.z, 0.0), 0.2)
        ) + star,
        SKY_DAY_TOP,
        max(-sun_dir.z, 0)
    );

    vec3 sky_mid = mix(
        mix(
            sky_twilight_mid * magnetosphere_tint(),
            SKY_NIGHT_MID,
            pow(max(sun_dir.z, 0.0), 0.1)
        ),
        SKY_DAY_MID,
        max(-sun_dir.z, 0)
    );

    vec3 sky_bot = mix(
        mix(
            sky_twilight_bot * magnetosphere_tint(),
            SKY_NIGHT_BOT,
            pow(max(sun_dir.z, 0.0), 0.2)
        ),
        SKY_DAY_BOT,
        max(-sun_dir.z, 0)
    );

    vec3 sky_color = mix(
        mix(
            sky_mid,
            sky_bot,
            max(-dir.z, 0)
        ),
        sky_top,
        max(dir.z, 0)
    );

    return sky_color * magnetosphere_tint();
}

vec3 get_sky_color(vec3 dir, vec3 origin, vec3 f_pos, float quality, bool with_features, float refractionIndex, bool fake_clouds, float sun_shade_frac) {
    // Sky color
    vec3 sun_dir = sun_dir.xyz;
    vec3 moon_dir = moon_dir.xyz;


    // Sun
    const vec3 SUN_SURF_COLOR = vec3(1.5, 0.9, 0.35) * 10.0;

    vec3 sun_halo_color = mix(
        (sun_dir.x > 0 ? SUN_HALO_DUSK : SUN_HALO_DAWN)* magnetosphere_tint(),
        SUN_HALO_DAY,
        pow(max(-sun_dir.z, 0.0), 0.5)
    );

    float sun_halo_power = 20.0;
    if (fake_clouds || medium.x == MEDIUM_WATER) {
        sun_halo_power = 30.0;
        sun_halo_color *= 0.01;
    }

    vec3 sun_halo = sun_halo_color * 25 * pow(max(dot(dir, -sun_dir), 0), sun_halo_power);
    vec3 sun_surf = vec3(0);
    if (with_features) {
        float angle = 0.00035;
        sun_surf = clamp((dot(dir, -sun_dir) - (1.0 - angle)) * 4 / angle, 0, 1)
            * SUN_SURF_COLOR
            * SUN_COLOR_FACTOR
            * sun_shade_frac;
    }
    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        sun_surf *= 0.1;
    }
    vec3 sun_light = sun_halo + sun_surf;

    // Moon
    const vec3 MOON_SURF_COLOR = vec3(0.7, 1.0, 1.5) * 250.0;
    const vec3 MOON_HALO_COLOR = vec3(0.015, 0.015, 0.05) * 250;

    vec3 moon_halo_color = MOON_HALO_COLOR;

    float moon_halo_power = 20.0;

    vec3 moon_surf = vec3(0);

    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        moon_halo_power = 50.0;
    }

    float is_moon = 0.0;

    if (with_features) {
        float moon_radius = 0.035;
        
        float tca = dot(-moon_dir, dir);

        float radius2 = moon_radius * moon_radius;
        float d2 = 1.0 - tca * tca;

        float diff = radius2 - d2;

        is_moon = clamp(tca * 2000.0, 0.0, 1.0) * clamp(diff * 4000.0, 0.0, 1.0);

        if (is_moon > 0.0) {
            float thc = sqrt(diff);

            float t0 = tca - thc;

            vec3 moon_normal = (t0 * dir + moon_dir) * (1.0 / moon_radius);

            float noise = snoise3(moon_normal * 8.2) + snoise3(moon_normal * 25.0) * 0.4;

            float direct_sunlight = max(dot(moon_normal, -sun_dir), 0.0);

            float planet_albedo = 0.12;
            float planet_reflected_light = (1.0 - abs(dot(moon_dir, sun_dir))) * max(dot(moon_normal, moon_dir), 0.0) * planet_albedo;
            float light = max(direct_sunlight + planet_reflected_light, 0.001);

            // ~sun is the same direction from the moon as it is to us.
            float surface_light = pow(light * (0.4 + 0.3 * noise), 2.0);

            moon_surf = MOON_SURF_COLOR * surface_light * is_moon;
        }
    }
    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        moon_halo_color *= 0.2;
        moon_surf *= 0.05;
    }
    vec3 moon_halo = moon_halo_color * pow(max(dot(dir, -moon_dir), 0) * max(dot(sun_dir, -moon_dir), 0), moon_halo_power);
    vec3 moon_light = moon_halo + moon_surf;

    // Replaced all clamp(sun_dir, 0, 1) with max(sun_dir, 0) because sun_dir is calculated from sin and cos, which are never > 1

    vec3 sky_color;
    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        sky_color = get_sky_light(dir, !fake_clouds, is_moon);
    } else {
        if (medium.x == MEDIUM_WATER) {
            sky_color = get_sky_light(dir, true, is_moon);
        } else {
            vec3 star_dir = normalize(sun_dir.xyz * dir.z + cross(sun_dir.xyz, vec3(0, 1, 0)) * dir.x + vec3(0, 1, 0) * dir.y);
            float star = is_star_at(star_dir) * (1.0 - is_moon);
            sky_color = vec3(0) + star;
        }
    }

    return sky_color + sun_light + moon_light;
}

float fog(vec3 f_pos, vec3 focus_pos, uint medium) {
    return max(1.0 - 5000.0 / (1.0 + distance(f_pos.xy, focus_pos.xy)), 0.0);
}

vec3 illuminate(float max_light, vec3 view_dir, vec3 emitted, vec3 reflected) {
    return emitted + reflected;
}

vec3 simple_lighting(vec3 pos, vec3 col, float shade) {
    // Bad fake lantern so we can see in caves
    vec3 d = pos.xyz - focus_pos.xyz;
    return col * clamp(2.5 / dot(d, d), shade * (get_sun_brightness() + 0.01), 1);
}

float wind_wave(float off, float scaling, float speed, float strength) {
    float aspeed = abs(speed);

    // TODO: Right now, the wind model is pretty simplistic. This means that there is frequently no wind at all, which
    // looks bad. For now, we add a lower bound on the wind speed to keep things looking nice.
    strength = max(strength, 6.0);
    aspeed = max(aspeed, 5.0);

    return (sin(tick_loop(2.0 * PI, 0.35 * scaling * floor(aspeed), off)) * (1.0 - fract(aspeed))
        + sin(tick_loop(2.0 * PI, 0.35 * scaling * ceil(aspeed), off)) * fract(aspeed)) * abs(strength) * 0.25;
}

#endif

#ifndef SRGB_GLSL
#define SRGB_GLSL

#extension GL_EXT_samplerless_texture_functions : enable

// Linear RGB, attenuation coefficients for water at roughly R, G, B wavelengths.
// See https://en.wikipedia.org/wiki/Electromagnetic_absorption_by_water
const vec3 MU_WATER = vec3(0.6, 0.04, 0.01);

//https://gamedev.stackexchange.com/questions/92015/optimized-linear-to-srgb-glsl
vec3 srgb_to_linear(vec3 srgb) {
    bvec3 cutoff = lessThan(srgb, vec3(0.04045));
    vec3 higher = pow((srgb + vec3(0.055))/vec3(1.055), vec3(2.4));
    vec3 lower = srgb/vec3(12.92);

    return mix(higher, lower, cutoff);
}

vec3 linear_to_srgb(vec3 col) {
    vec3 s1 = vec3(sqrt(col.r), sqrt(col.g), sqrt(col.b));
    vec3 s2 = vec3(sqrt(s1.r), sqrt(s1.g), sqrt(s1.b));
    vec3 s3 = vec3(sqrt(s2.r), sqrt(s2.g), sqrt(s2.b));
    return vec3(
            mix(11.500726 * col.r, (0.585122381 * s1.r + 0.783140355 * s2.r - 0.368262736 * s3.r), clamp((col.r - 0.0060) * 10000.0, 0.0, 1.0)),
            mix(11.500726 * col.g, (0.585122381 * s1.g + 0.783140355 * s2.g - 0.368262736 * s3.g), clamp((col.g - 0.0060) * 10000.0, 0.0, 1.0)),
            mix(11.500726 * col.b, (0.585122381 * s1.b + 0.783140355 * s2.b - 0.368262736 * s3.b), clamp((col.b - 0.0060) * 10000.0, 0.0, 1.0))
    );
}

float pow5(float x) {
    float x2 = x * x;
    return x2 * x2 * x;
}

vec4 pow5(vec4 x) {
    vec4 x2 = x * x;
    return x2 * x2 * x;
}

// Fresnel angle for perfectly specular dialectric materials.

// Schlick approximation
vec3 schlick_fresnel(vec3 Rs, float cosTheta) {
    return Rs + pow5(1.0 - cosTheta) * (1.0 - Rs);
}

// Beckmann Distribution
float BeckmannDistribution_D(float NdotH, float alpha) {
    const float PI = 3.1415926535897932384626433832795;
    float NdotH2 = NdotH * NdotH;
    float NdotH2m2 = NdotH2 * alpha * alpha;
    float k_spec = exp((NdotH2 - 1.0) / NdotH2m2) / (PI * NdotH2m2 * NdotH2);
    return mix(k_spec, 0.0, NdotH == 0.0);
}

// Voxel Distribution
float BeckmannDistribution_D_Voxel(vec3 wh, vec3 voxel_norm, float alpha) {
    vec3 sides = sign(voxel_norm);
    
    vec3 NdotH = wh * sides;

    const float PI = 3.1415926535897932384626433832795;
    vec3 NdotH2 = NdotH * NdotH;
    vec3 NdotH2m2 = NdotH2 * alpha * alpha;
    vec3 k_spec = exp((NdotH2 - 1.0) / NdotH2m2) / (PI * NdotH2m2 * NdotH2);
    return dot(mix(k_spec, vec3(0.0), equal(NdotH, vec3(0.0))), abs(voxel_norm));
}

float TrowbridgeReitzDistribution_D_Voxel(vec3 wh, vec3 voxel_norm, float alpha) {
    vec3 sides = sign(voxel_norm);

    vec3 NdotH = wh * sides;

    const float PI = 3.1415926535897932384626433832795;
    vec3 NdotH2 = NdotH * NdotH;
    vec3 NdotH2m2 = NdotH2 * alpha * alpha;
    vec3 e = (1.0 - NdotH2) / NdotH2m2;
    vec3 k_spec = 1.0 / (PI * NdotH2m2 * NdotH2 * (1.0 + e) * (1.0 + e));
    return dot(mix(k_spec, vec3(0.0), equal(NdotH, vec3(0.0))), abs(voxel_norm));
}

float BeckmannDistribution_Lambda(vec3 norm, vec3 dir, float alpha) {
    float CosTheta = dot(norm, dir);
    float SinTheta = sqrt(1.0 - CosTheta * CosTheta);
    float TanTheta = SinTheta / CosTheta;
    float absTanTheta = abs(TanTheta);
    float a = 1.0 / (alpha * absTanTheta);
    
    return mix(max(0.0, (1.0 - 1.259 * a + 0.396 * a * a) / (3.535 * a + 2.181 * a * a)), 0.0, isinf(absTanTheta) || a >= 1.6);
}

float BeckmannDistribution_G(vec3 norm, vec3 dir, vec3 light_dir, float alpha) {
    return 1.0 / (1.0 + BeckmannDistribution_Lambda(norm, dir, alpha) + BeckmannDistribution_Lambda(norm, -light_dir, alpha));
}

// Fresnel blending
//
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Microfacet_Models.html#fragment-MicrofacetDistributionPublicMethods-2
// and
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Fresnel_Incidence_Effects.html
vec3 FresnelBlend_f(vec3 norm, vec3 dir, vec3 light_dir, vec3 R_d, vec3 R_s, float alpha) {
    const float PI = 3.1415926535897932384626433832795;
    alpha = alpha * sqrt(2.0);
    float cos_wi = dot(-light_dir, norm);
    float cos_wo = dot(dir, norm);

    vec3 diffuse = (28.0 / (23.0 * PI)) * R_d *
        (1.0 - R_s) *
        (1.0 - pow5(1.0 - 0.5 * abs(cos_wi))) *
        (1.0 - pow5(1.0 - 0.5 * abs(cos_wo)));
    vec3 wh = -light_dir + dir;
#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    bool is_blocked = cos_wi == 0.0 || cos_wo == 0.0;
#else
    bool is_blocked = cos_wi <= 0.0 || cos_wo <= 0.0;
#endif
    if (is_blocked) {
        return vec3(0.0);
    }
    wh = normalize(wh);
    float dot_wi_wh = dot(-light_dir, wh);
    vec3 specular = dot(norm, dir) > 0.0 ? vec3(0.0) : (BeckmannDistribution_D(dot(wh, norm), alpha) /
        (4.0 * abs(dot_wi_wh) *
        max(abs(cos_wi), abs(cos_wo))) *
        schlick_fresnel(R_s, dot_wi_wh));
    return mix(diffuse + specular, vec3(0.0), bvec3(all(equal(light_dir, dir))));
}

// Fresnel blending
//
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Microfacet_Models.html#fragment-MicrofacetDistributionPublicMethods-2
// and
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Fresnel_Incidence_Effects.html
vec3 FresnelBlend_Voxel_f(vec3 norm, vec3 dir, vec3 light_dir, vec3 R_d, vec3 R_s, float alpha, vec3 voxel_norm, float dist) {
    const float PI = 3.1415926535897932384626433832795;
    alpha = alpha * sqrt(2.0);
    float cos_wi = dot(-light_dir, norm);
    float cos_wo = dot(dir, norm);

#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    vec4 AbsNdotL = abs(vec4(light_dir, cos_wi));
    vec4 AbsNdotV = abs(vec4(dir, cos_wo));
#else
    vec3 sides = sign(voxel_norm);
    vec4 AbsNdotL = vec4(max(-light_dir * sides, 0.0), abs(cos_wi));
    vec4 AbsNdotV = vec4(max(dir * sides, 0.0), abs(cos_wo));
#endif

    vec4 diffuse_factor = (1.0 - pow5(1.0 - 0.5 * AbsNdotL)) * (1.0 - pow5(1.0 - 0.5 * AbsNdotV));

    vec3 diffuse = (28.0 / (23.0 * PI)) * R_d * (1.0 - R_s) * dot(diffuse_factor, /*R_r * */vec4(abs(norm) * (1.0 - dist), dist));

    vec3 wh = -light_dir + dir;
#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    bool is_blocked = cos_wi == 0.0 || cos_wo == 0.0;
#else
    bool is_blocked = cos_wi <= 0.0 || cos_wo <= 0.0;
#endif
    if (is_blocked) {
        return vec3(0.0);
    }
    wh = normalize(wh);
    float dot_wi_wh = dot(-light_dir, wh);
    float distr = BeckmannDistribution_D_Voxel(wh, voxel_norm, alpha);
    vec3 specular = distr /
        (4.0 * abs(dot_wi_wh) *
        max(abs(cos_wi), abs(cos_wo))) *
        schlick_fresnel(R_s, dot_wi_wh);
    return mix(diffuse + specular, vec3(0.0), bvec3(all(equal(light_dir, dir))));
}

// Phong reflection.
//
// Note: norm, dir, light_dir must all be normalizd.
vec3 light_reflection_factor2(vec3 norm, vec3 dir, vec3 light_dir, vec3 k_d, vec3 k_s, float alpha) {
    // TODO: These are supposed to be the differential changes in the point location p, in tangent space.
    // That is, assuming we can parameterize a 2D surface by some function p : R² → R³, mapping from
    // points in a plane to 3D points on the surface, we can define
    // ∂p(u,v)/∂u and ∂p(u,v)/∂v representing the changes in the pont location as we move along these
    // coordinates.
    //
    // Then we can define the normal at a point, n(u,v) = ∂p(u,v)/∂u × ∂p(u,v)/∂v.
    //
    // Additionally, we can define the change in *normals* at each point using the
    // Weingarten equations (see http://www.pbr-book.org/3ed-2018/Shapes/Spheres.html):
    //
    // ∂n/∂u = (fF - eG) / (EG - F²) ∂p/∂u + (eF - fE) / (EG - F²) ∂p/∂v
    // ∂n/∂v = (gF - fG) / (EG - F²) ∂p/∂u + (fF - gE) / (EG - F²) ∂p/∂v
    //
    // where
    //
    // E = |∂p/∂u ⋅ ∂p/∂u|
    // F = ∂p/∂u ⋅ ∂p/∂u
    // G = |∂p/∂v ⋅ ∂p/∂v|
    //
    // and
    //
    // e = n ⋅ ∂²p/∂u²
    // f = n ⋅ ∂²p/(∂u∂v)
    // g = n ⋅ ∂²p/∂v²
    //
    // For planes (see http://www.pbr-book.org/3ed-2018/Shapes/Triangle_Meshes.html) we have
    // e = f = g = 0 (since the plane has no curvature of any sort) so we get:
    //
    // ∂n/∂u = (0, 0, 0)
    // ∂n/∂v = (0, 0, 0)
    //
    // To find ∂p/∂u and ∂p/∂v, we first write p and u parametrically:
    //    p(u, v) = p0 + u ∂p/∂u + v ∂p/∂v
    //
    // ( u₀ - u₂    v₀ - v₂
    //   u₁ - u₂    v₁ - v₂ )
    //
    // Basis: plane norm = norm = (0, 0, 1), x vector = any orthgonal vector on the plane.
    // vec3 w_i =
    // vec3 w_i = vec3(view_mat * vec4(-light_dir, 1.0));
    // vec3 w_o = vec3(view_mat * vec4(light_dir, 1.0));
    return FresnelBlend_f(norm, dir, light_dir, k_d, k_s, alpha);
}

vec3 light_reflection_factor(vec3 norm, vec3 dir, vec3 light_dir, vec3 k_d, vec3 k_s, float alpha, vec3 voxel_norm, float voxel_lighting) {
#if (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_LAMBERTIAN)
    const float PI = 3.141592;
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    vec4 AbsNdotL = abs(vec4(light_dir, dot(norm, light_dir)));
        #else
    vec3 sides = sign(voxel_norm);
    vec4 AbsNdotL = max(vec4(-light_dir * sides, dot(norm, -light_dir)), 0.0);
        #endif
    float diffuse = dot(AbsNdotL, vec4(abs(voxel_norm) * (1.0 - voxel_lighting), voxel_lighting));
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    float diffuse = abs(dot(norm, light_dir));
        #else
    float diffuse = max(dot(norm, -light_dir), 0.0);
        #endif
    #endif
    return k_d / PI * diffuse;
#elif (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_BLINN_PHONG)
    const float PI = 3.141592;
    alpha = alpha * sqrt(2.0);
    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    float ndotL = abs(dot(norm, light_dir));
    #else
    float ndotL = max(dot(norm, -light_dir), 0.0);
    #endif

    if (ndotL > 0.0) {
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        vec4 AbsNdotL = abs(vec4(light_dir, ndotL));
        #else
        vec3 sides = sign(voxel_norm);
        vec4 AbsNdotL = max(vec4(-light_dir * sides, ndotL), 0.0);
        #endif
        float diffuse = dot(AbsNdotL, vec4(abs(voxel_norm) * (1.0 - voxel_lighting), voxel_lighting));
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        float diffuse = ndotL;
    #endif
        vec3 H = normalize(-light_dir + dir);

    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        float NdotH = abs(dot(norm, H));
    #else
        float NdotH = max(dot(norm, H), 0.0);
    #endif
        return (1.0 - k_s) / PI * k_d * diffuse + k_s * pow(NdotH, alpha/* * 4.0*/);
    }

    return vec3(0.0);
#elif (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_ASHIKHMIN)
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        return FresnelBlend_Voxel_f(norm, dir, light_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting);
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        return FresnelBlend_f(norm, dir, light_dir, k_d, k_s, alpha);
    #endif
#endif
}

float rel_luminance(vec3 rgb)
{
    // https://en.wikipedia.org/wiki/Relative_luminance
    const vec3 W = vec3(0.2126, 0.7152, 0.0722);
    return dot(rgb, W);
}

// From https://discourse.vvvv.org/t/infinite-ray-intersects-with-infinite-plane/10537
// out of laziness.
bool IntersectRayPlane(vec3 rayOrigin, vec3 rayDirection, vec3 posOnPlane, vec3 planeNormal, inout vec3 intersectionPoint)
{
  float rDotn = dot(rayDirection, planeNormal);

  //parallel to plane or pointing away from plane?
  if (rDotn < 0.0000001 )
    return false;

  float s = dot(planeNormal, (posOnPlane - rayOrigin)) / rDotn;

  intersectionPoint = rayOrigin + s * rayDirection;

  return true;
}

// Compute uniform attenuation due to beam passing through a substance that fills an area below a horizontal plane
// (e.g. in most cases, water below the water surface depth) using the simplest form of the Beer-Lambert law
// (https://en.wikipedia.org/wiki/Beer%E2%80%93Lambert_law):
//
// I(z) = I₀ e^(-μz)
//
// We compute this value, except for the initial intensity which may be multiplied out later.
//
// wpos is the position of the point being hit.
// ray_dir is the reversed direction of the ray (going "out" of the point being hit).
// mu is the attenuation coefficient for R, G, and B wavelenghts.
// surface_alt is the estimated altitude of the horizontal surface separating the substance from air.
// defaultpos is the position to use in computing the distance along material at this point if there was a failure.
//
// Ideally, defaultpos is set so we can avoid branching on error.
vec3 compute_attenuation(vec3 wpos, vec3 ray_dir, vec3 mu, float surface_alt, vec3 defaultpos) {
#if (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_IMPORTANCE)
    return vec3(1.0);
#elif (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_RADIANCE)
    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        return vec3(1.0);
    #else
    ray_dir = faceforward(ray_dir, vec3(0.0, 0.0, -1.0), ray_dir);
    vec3 surface_dir = surface_alt < wpos.z ? vec3(0.0, 0.0, -1.0) : vec3(0.0, 0.0, 1.0);
    bool _intersects_surface = IntersectRayPlane(wpos, ray_dir, vec3(0.0, 0.0, surface_alt), surface_dir, defaultpos);
    float depth = length(defaultpos - wpos);
    return exp(-mu * depth);
    #endif
#endif
}

// Same as compute_attenuation but since both point are known, set a maximum to make sure we don't exceed the length
// from the default point.
vec3 compute_attenuation_point(vec3 wpos, vec3 ray_dir, vec3 mu, float surface_alt, vec3 defaultpos) {
#if (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_IMPORTANCE)
    return pow(1.0 - mu, vec3(3));
#elif (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_RADIANCE)
    return vec3(1.0);
#endif
}

vec3 greedy_extract_col_light_attr(texture2D t_col_light, sampler s_col_light, vec2 f_uv_pos, out float f_light, out float f_glow, out float f_ao, out uint f_attr, out float f_sky_exposure) {
    // TODO: Figure out how to use `texture` and modulation to avoid needing to do manual filtering
    // TODO: Use `texture` instead

    uvec4 tex_00 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(0, 0), 0) * 255.0);
    uvec4 tex_10 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(1, 0), 0) * 255.0);
    uvec4 tex_01 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(0, 1), 0) * 255.0);
    uvec4 tex_11 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(1, 1), 0) * 255.0);
    vec3 light_00 = vec3(tex_00.rg >> 3u, tex_00.a & 1u);
    vec3 light_10 = vec3(tex_10.rg >> 3u, tex_10.a & 1u);
    vec3 light_01 = vec3(tex_01.rg >> 3u, tex_01.a & 1u);
    vec3 light_11 = vec3(tex_11.rg >> 3u, tex_11.a & 1u);
    vec3 light_0 = mix(light_00, light_01, fract(f_uv_pos.y));
    vec3 light_1 = mix(light_10, light_11, fract(f_uv_pos.y));
    vec3 light = mix(light_0, light_1, fract(f_uv_pos.x));

    vec3 f_col = vec3(
        float(((tex_00.r & 0x7u) << 1u) | (tex_00.b & 0xF0u)),
        float(tex_00.a & 0xFEu),
        float(((tex_00.g & 0x7u) << 1u) | ((tex_00.b & 0x0Fu) << 4u))
    ) / 255.0;

    f_ao = light.z;
    f_light = light.x / 31.0;
    f_sky_exposure = light.x / 31.0 + (1.0 - f_ao) * 0.5;
    f_glow = light.y / 31.0;
    f_attr = tex_00.g >> 3u;
    return srgb_to_linear(f_col);
}

vec3 greedy_extract_col_light_kind_terrain(
    texture2D t_col_light, sampler s_col_light,
    utexture2D t_kind,
    vec2 f_uv_pos,
    out float f_light, out float f_glow, out float f_ao, out float f_sky_exposure, out uint f_kind
) {
    uint _f_attr;
    f_kind = uint(texelFetch(t_kind, ivec2(f_uv_pos), 0).r);
    return greedy_extract_col_light_attr(t_col_light, s_col_light, f_uv_pos, f_light, f_glow, f_ao, _f_attr, f_sky_exposure);
}

vec3 greedy_extract_col_light_figure(texture2D t_col_light, sampler s_col_light, vec2 f_uv_pos, out float f_light, out uint f_attr) {
    float _f_sky_exposure, _f_light, _f_glow, _f_ao;
    return greedy_extract_col_light_attr(t_col_light, s_col_light, f_uv_pos, f_light, _f_glow, _f_ao, f_attr, _f_sky_exposure);
}

#endif

#define t_horizon packed_input

#define s_horizon packed_sampler



const float MIN_SHADOW = 0.33;

vec2 pos_to_tex(vec2 pos) {
    // Want: (pixel + 0.5)
    vec2 uv_pos = (focus_off.xy + pos + 16) / 32.0;
    return vec2(uv_pos.x, uv_pos.y);
}

// textureBicubic from https://stackoverflow.com/a/42179924
vec4 cubic(float v) {
    vec4 n = vec4(1.0, 2.0, 3.0, 4.0) - v;
    vec4 s = n * n * n;
    float x = s.x;
    float y = s.y - 4.0 * s.x;
    float z = s.z - 4.0 * s.y + 6.0 * s.x;
    float w = 6.0 - x - y - z;
    return vec4(x, y, z, w) * (1.0/6.0);
}

// Computes atan(y, x), except with more stability when x is near 0.
float atan2(in float y, in float x) {
    bool s = (abs(x) > abs(y));
    return mix(PI/2.0 - atan(x,y), atan(y,x), s);
}

// NOTE: We assume the sampled coordinates are already in "texture pixels".
vec4 textureBicubic(texture2D tex, sampler sampl, vec2 texCoords) {
    // TODO: remove all textureSize calls and replace with constants
   vec2 texSize = textureSize(sampler2D(tex, sampl), 0);
   vec2 invTexSize = 1.0 / texSize;

   texCoords = texCoords/* * texSize */ - 0.5;


    vec2 fxy = fract(texCoords);
    texCoords -= fxy;

    vec4 xcubic = cubic(fxy.x);
    vec4 ycubic = cubic(fxy.y);

    vec4 c = texCoords.xxyy + vec2 (-0.5, +1.5).xyxy;

    vec4 s = vec4(xcubic.xz + xcubic.yw, ycubic.xz + ycubic.yw);
    vec4 offset = c + vec4 (xcubic.yw, ycubic.yw) / s;

    offset *= invTexSize.xxyy;

    vec4 sample0 = texture(sampler2D(tex, sampl), offset.xz);
    vec4 sample1 = texture(sampler2D(tex, sampl), offset.yz);
    vec4 sample2 = texture(sampler2D(tex, sampl), offset.xw);
    vec4 sample3 = texture(sampler2D(tex, sampl), offset.yw);

    float sx = s.x / (s.x + s.y);
    float sy = s.z / (s.z + s.w);

    return mix(
       mix(sample3, sample2, sx), mix(sample1, sample0, sx)
    , sy);
}

vec4 textureMaybeBicubic(texture2D tex, sampler sampl, vec2 texCoords) {
    // TODO: Allow regular `texture` to be used when cause of light leaking issues is found
    //#if (CLOUD_MODE >= CLOUD_MODE_HIGH)
        return textureBicubic(tex, sampl, texCoords);
    //#else
    //    vec2 offset = (texCoords + vec2(-1.0, 0.5)) / textureSize(sampler2D(tex, sampl), 0);
    //    return texture(sampler2D(tex, sampl), offset);
    //#endif
}

// 16 bit version (each of the 2 8-bit components are combined after bilinear sampling)
// NOTE: We assume the sampled coordinates are already in "texture pixels".
vec2 textureBicubic16(texture2D tex, sampler sampl, vec2 texCoords) {
   vec2 texSize = textureSize(sampler2D(tex, sampl), 0);
   vec2 invTexSize = 1.0 / texSize;

   texCoords = texCoords - 0.5;


    vec2 fxy = fract(texCoords);
    texCoords -= fxy;

    vec4 xcubic = cubic(fxy.x);
    vec4 ycubic = cubic(fxy.y);

    vec4 c = texCoords.xxyy + vec2 (-0.5, +1.5).xyxy;

    vec4 s = vec4(xcubic.xz + xcubic.yw, ycubic.xz + ycubic.yw);
    vec4 offset = c + vec4 (xcubic.yw, ycubic.yw) / s;

    offset *= invTexSize.xxyy;

    vec4 sample0_v4 = textureLod(sampler2D(tex, sampl), offset.xz, 0);
    vec4 sample1_v4 = textureLod(sampler2D(tex, sampl), offset.yz, 0);
    vec4 sample2_v4 = textureLod(sampler2D(tex, sampl), offset.xw, 0);
    vec4 sample3_v4 = textureLod(sampler2D(tex, sampl), offset.yw, 0);
    vec2 sample0 = sample0_v4.rb / 256.0 + sample0_v4.ga;
    vec2 sample1 = sample1_v4.rb / 256.0 + sample1_v4.ga;
    vec2 sample2 = sample2_v4.rb / 256.0 + sample2_v4.ga;
    vec2 sample3 = sample3_v4.rb / 256.0 + sample3_v4.ga;

    float sx = s.x / (s.x + s.y);
    float sy = s.z / (s.z + s.w);

    return mix(mix(sample3, sample2, sx), mix(sample1, sample0, sx), sy);
}

// Gets the altitude at a position relative to focus_off.
float alt_at(vec2 pos) {
    vec4 alt_sample = textureLod(sampler2D(t_alt, s_alt), wpos_to_uv(focus_off.xy + pos), 0);
    return (((alt_sample.r * (1.0 / 256.0) + alt_sample.g) * view_distance.w) + view_distance.z - focus_off.z);
}

float alt_at_real(vec2 pos) {
    return ((textureBicubic16(t_alt, s_alt, pos_to_tex(pos)).r * view_distance.w) + view_distance.z - focus_off.z);
}


float horizon_at2(vec4 f_horizons, float alt, vec3 pos, vec4 light_dir) {
    const float PI_2 = 3.1415926535897932384626433832795 / 2.0;
    const float MIN_LIGHT = 0.0;
    
    vec2 f_horizon = mix(f_horizons.rg, f_horizons.ba, bvec2(light_dir.x < 0.0));
    float angle = tan(f_horizon.x * PI_2);
    float height = f_horizon.y * view_distance.w + view_distance.z;
    const float w = 0.1;
    float deltah = height - alt - focus_off.z;
    float lighta = -light_dir.z / max(abs(light_dir.x), 0.0001);
    // NOTE: Ideally, deltah <= 0.0 is a sign we have an oblique horizon angle.
    float deltax = deltah / max(angle, 0.0001);
    float lighty = lighta * deltax;
    float deltay = lighty - deltah + max(pos.z - alt, 0.0);
    // NOTE: the "real" deltah should always be >= 0, so we know we're only handling the 0 case with max.
    float s = mix(max(min(max(deltay, 0.0) / max(deltax, 0.0001) / w, 1.0), 0.0), 1.0, deltah <= 0);
    return max(s * s * (3.0 - 2.0 * s), MIN_LIGHT);
}

vec2 splay(vec2 pos) {
    vec2 scale = textureSize(sampler2D(t_alt, s_alt), 0) * 32.0;
    float lod_dist = view_distance.x * 0.95 / max(scale.x, scale.y);
    float dist = abs(pos.x) + abs(pos.y);
    float stretch = (pow(dist, 5.5) * 0.75 + dist * 0.25) * (1.0 - lod_dist) + lod_dist;
    vec2 splayed = pos * stretch * scale;
    if (abs(pos.x) > 0.99 || abs(pos.y) > 0.99) {
        splayed *= 50.0;
    }
    return splayed;
}

vec3 lod_norm(vec2 f_pos/*vec3 pos*/, vec4 square) {
    float altx0 = alt_at(vec2(square.x, f_pos.y));
    float altx1 = alt_at(vec2(square.z, f_pos.y));
    float alty0 = alt_at(vec2(f_pos.x, square.y));
    float alty1 = alt_at(vec2(f_pos.x, square.w));
    float slope = abs(altx1 - altx0) + abs(alty0 - alty1);

    vec3 norm = normalize(vec3(
        (altx0 - altx1) / (square.z - square.x),
        (alty0 - alty1) / (square.w - square.y),
        1.0
    ));

    return faceforward(norm, vec3(0.0, 0.0, -1.0), norm);
}

vec3 lod_norm(vec2 f_pos) {
    const float SAMPLE_W = 32;
    return lod_norm(f_pos, vec4(f_pos - vec2(SAMPLE_W), f_pos + vec2(SAMPLE_W)));
}


vec3 lod_pos(vec2 pos, vec2 focus_pos) {
    // Remove spiking by "pushing" vertices towards local optima
    vec2 delta = splay(pos);
    vec2 hpos = focus_pos + delta;

    vec2 dir = normalize(pos);
    float shift = 150.0 * pow(length(pos), 3.0);
    for (int i = 1; i < 10; i ++) {
        hpos -= dir * dot(normalize(lod_norm(hpos)).xy, dir) * shift / float(i);
    }

    return vec3(hpos, alt_at_real(hpos));
}

#ifdef HAS_LOD_FULL_INFO
#define t_map packed_input

#define s_map packed_sampler


vec3 lod_col(vec2 pos) {
    #ifdef EXPERIMENTAL_PROCEDURALLODDETAIL
        vec2 wpos = pos + focus_off.xy;
        vec2 shift = vec2(
            textureLod(sampler2D(t_noise, s_noise), wpos / 200, 0).x - 0.5,
            textureLod(sampler2D(t_noise, s_noise), wpos / 200 + 0.5, 0).x - 0.5
        ) * 32 + vec2(
            textureLod(sampler2D(t_noise, s_noise), wpos / 50, 0).x - 0.5,
            textureLod(sampler2D(t_noise, s_noise), wpos / 50 + 0.5, 0).x - 0.5
        ) * 16;
        pos += shift;
        wpos += shift;
    #endif

    vec3 col = textureBicubic(t_map, s_map, pos_to_tex(pos)).rgb;

    return col;
}
#endif

vec3 water_diffuse(vec3 color, vec3 dir, float max_dist) {
    if (medium.x == 1) {
        float f_alt = alt_at(cam_pos.xy);
        float fluid_alt = max(cam_pos.z + 1, floor(f_alt + 1));

        float water_dist = clamp((fluid_alt - cam_pos.z) / pow(max(dir.z, 0), 2), 0, max_dist);

        float fade = pow(0.95, water_dist);

        return mix(vec3(0.0, 0.2, 0.5)
            * (get_sun_brightness() * get_sun_color() + get_moon_brightness() * get_moon_color())
            * pow(0.99, max((fluid_alt - cam_pos.z) * 12.0 - dir.z * 200, 0)), color.rgb * exp(-MU_WATER * water_dist * 0.1), fade);
    } else {
        return color;
    }
}

void lod_voxels(vec3 f_pos, vec3 f_norm, vec3 cam_dir, out vec3 voxel_pos, out vec3 voxel_norm, out float voxel_sz, out float f_ao) {
    voxel_pos = f_pos;
    voxel_norm = f_norm;
    voxel_sz = 1.0;
    f_ao = 1.0;
    
    #ifndef EXPERIMENTAL_NOLODVOXELS
        const float VOXEL_SCALE_FACTOR = 100000.0;
        vec3 wpos = f_pos + focus_off.xyz;
        
        voxel_sz = clamp(exp(floor(log(distance(cam_pos.xy, f_pos.xy) * 0.0001 + noise_2d(wpos.xy * 0.01) * 0.02) * 3) / 3) * VOXEL_SCALE_FACTOR / (internal_res.x + internal_res.y), 1.0, 128.0);
        
        #ifdef EXPERIMENTAL_PROCEDURALLODDETAIL
            const float MARCH_THRESHOLD = 4.0;
        #else
            const float MARCH_THRESHOLD = 2.0;
        #endif
        
        float t = -MARCH_THRESHOLD * voxel_sz;
        int i = 0;
        while (t < MARCH_THRESHOLD * voxel_sz && i++<40) {
            vec3 deltas = (fract((wpos + cam_dir * t) / voxel_sz) - step(vec3(0), cam_dir * voxel_sz)) / -cam_dir * voxel_sz;
            t += max(min(min(deltas.x, deltas.y), deltas.z), 0.001);

            voxel_pos = (floor((wpos + cam_dir * t) / voxel_sz) + 0.5) * voxel_sz;
            float surf_depth = 0.0;
            #ifdef EXPERIMENTAL_PROCEDURALLODDETAIL
                surf_depth = (noise_3d(voxel_pos / voxel_sz * 0.01) - 0.5)
                    * 10.0
                    * voxel_sz
                    * pow(mix(0.0, mix(1.0, 0.0, max(f_norm.z, 0.0)), max(f_norm.z, 0.0)), 0.5);
            #endif
            if (dot(voxel_pos - wpos, -f_norm) > surf_depth) {
                vec3 to_center = abs(voxel_pos - (wpos + cam_dir * t));
                voxel_norm = step(max(max(to_center.x, to_center.y), to_center.z), to_center) * sign(-cam_dir);
                float dist = dot(cam_dir * t, f_norm) + surf_depth;
                f_ao = clamp(dist / voxel_sz + max(f_norm.z, 0.5), 0.25, 1.0);
                voxel_pos -= focus_off.xyz;
                return;
            }
        }
        voxel_pos = f_pos;
        // Fallback, if we didn't hit any voxels
        voxel_norm = step(max(max(f_norm.x, f_norm.y), f_norm.z), f_norm) * sign(-cam_dir);
    #endif
}

#endif

#ifndef SKY_GLSL
#define SKY_GLSL

#ifndef RANDOM_GLSL
#define RANDOM_GLSL

#define t_noise packed_input

#define s_noise packed_sampler


float hash(vec4 p) {
    p = fract(p * 0.3183099 + 0.1) - fract(p + 23.22121);
    p *= 17.0;
    return (fract(p.x * p.y * (1.0 - p.z) * p.w * (p.x + p.y + p.z + p.w)) - 0.5) * 2.0;
}

#define M1 2047667443U
#define M2 3883706873U
#define M3 3961281721U

float hash_one(uint q) {
    uint n = ((M3 * q) ^ M2) * M1;

    return float(n) * (1.0 / float(0xffffffffU));
}

float hash_two(uvec2 q) {
    q *= uvec2(M1, M2);
    uint n = q.x ^ q.y;
    n = n * (n ^ (n >> 15));
    return float(n) * (1.0 / float(0xffffffffU));
}

vec3 hash_two_3(uvec2 q) {
    q *= uvec2(M1, M2);
    uvec3 n = uvec3(q.x ^ q.y ^ uvec3(M1, M2, M3));
    n = n * (n ^ (n >> 15));
    return vec3(n) * (1.0 / vec3(0xffffffffU));
}

float hash_three(uvec3 q) {
    q *= uvec3(M1, M2, M3);
    uint n = q.x ^ q.y ^ q.z;
    n = n * (n ^ (n >> 15));
    return float(n) * (1.0 / float(0xffffffffU));
}

float hash_fast(uvec3 q) {
    q *= uvec3(M1, M2, M3);

    uint n = (q.x ^ q.y ^ q.z) * M1;

    return float(n) * (1.0 / float(0xffffffffU));
}

// 2D, but using shifted 2D textures
float noise_2d(vec2 pos) {
    return noise_sample(pos);
}

// 3D, but using shifted 2D textures
float noise_3d(vec3 pos) {
    pos.z *= 15.0;
    uint z = uint(trunc(pos.z));
    vec2 offs0 = vec2(hash_one(z), hash_one(z + 73u));
    vec2 offs1 = vec2(hash_one(z + 1u), hash_one(z + 1u + 73u));
    return mix(noise_sample(pos.xy + offs0), noise_sample(pos.xy + offs1), fract(pos.z));
}

// 3D version of `snoise`
float snoise3(in vec3 x) {
    uvec3 p = uvec3(floor(x) + 10000.0);
    vec3 f = fract(x);
    //f = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(
            mix(hash_fast(p + uvec3(0, 0, 0)), hash_fast(p + uvec3(1, 0, 0)), f.x),
            mix(hash_fast(p + uvec3(0, 1, 0)), hash_fast(p + uvec3(1, 1, 0)), f.x),
            f.y),
        mix(
            mix(hash_fast(p + uvec3(0, 0, 1)), hash_fast(p + uvec3(1, 0, 1)), f.x),
            mix(hash_fast(p + uvec3(0, 1, 1)), hash_fast(p + uvec3(1, 1, 1)), f.x),
            f.y),
        f.z);
}

// 4D noise
float snoise(in vec4 x) {
    vec4 p = floor(x);
    vec4 f = fract(x);
    f = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(
            mix(
                mix(hash(p + vec4(0, 0, 0, 0)), hash(p + vec4(1, 0, 0, 0)), f.x),
                mix(hash(p + vec4(0, 1, 0, 0)), hash(p + vec4(1, 1, 0, 0)), f.x),
                f.y),
            mix(
                mix(hash(p + vec4(0, 0, 1, 0)), hash(p + vec4(1, 0, 1, 0)), f.x),
                mix(hash(p + vec4(0, 1, 1, 0)), hash(p + vec4(1, 1, 1, 0)), f.x),
                f.y),
            f.z),
        mix(
            mix(
                mix(hash(p + vec4(0, 0, 0, 1)), hash(p + vec4(1, 0, 0, 1)), f.x),
                mix(hash(p + vec4(0, 1, 0, 1)), hash(p + vec4(1, 1, 0, 1)), f.x),
                f.y),
            mix(
                mix(hash(p + vec4(0, 0, 1, 1)), hash(p + vec4(1, 0, 1, 1)), f.x),
                mix(hash(p + vec4(0, 1, 1, 1)), hash(p + vec4(1, 1, 1, 1)), f.x),
                f.y),
            f.z),
        f.w);
}

vec3 rand_perm_3(vec3 pos) {
    return abs(sin(pos * vec3(1473.7 * pos.z + 472.3, 8891.1 * pos.x + 723.1, 3813.3 * pos.y + 982.5)));
}

vec4 rand_perm_4(vec4 pos) {
    return sin(473.3 * pos * vec4(317.3 * pos.w + 917.7, 1473.7 * pos.z + 472.3, 8891.1 * pos.x + 723.1, 3813.3 * pos.y + 982.5) / pos.yxwz);
}

vec3 smooth_rand(vec3 pos, float lerp_axis) {
    return vec3(snoise(vec4(pos, lerp_axis)), snoise(vec4(pos + 400.0, lerp_axis)), snoise(vec4(pos + 1000.0, lerp_axis)));
}

// Transform normal distribution to triangle distribution.
float norm2tri(float n) {
   // TODO: compare perf with adding two normal noise distributions
   bool flip = n > 0.5;
   n = flip ? 1.0 - n : n;
   n = sqrt(n / 2.0);
   n = flip ? 1.0 - n : n;
   return n;
}

// Caustics, ported and modified from https://www.shadertoy.com/view/3tlfR7, originally David Hoskins.
// License Creative Commons Attribution-NonCommercial-ShareAlike 3.0 Unported License: https://creativecommons.org/licenses/by-nc-sa/3.0/legalcode.
// Modifying these three functions mean that you agree to release your changes under the above license, *not* under GPL 3 as with the rest of the project.

float hashvec2(vec2 p) {return fract(sin(p.x * 1e2 + p.y) * 1e5 + sin(p.y * 1e3) * 1e3 + sin(p.x * 735. + p.y * 11.1) * 1.5e2); }

float n12(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f *= f * (3.-2.*f);
    return mix(
        mix(hashvec2(i+vec2(0.,0.)),hashvec2(i+vec2(1.,0.)),f.x),
        mix(hashvec2(i+vec2(0.,1.)),hashvec2(i+vec2(1.,1.)),f.x),
        f.y
    );
}

float caustics(vec2 p, float t) {
    vec3 k = vec3(p,t);
    float l;
    mat3 m = mat3(-2.,-1.,2.,3.,-2.,1.,1.,2.,2.);
    float n = n12(p);
    k = k*m*.5;
    l = length(.5 - fract(k+n));
    k = k*m*.4;
    l = min(l, length(.5-fract(k+n)));
    k = k*m*.3;
    l = min(l, length(.5-fract(k+n)));
    return pow(l,3.)*5.5;
}

#ifdef EXPERIMENTAL_DISCARDTRANSPARENCY
bool dither(vec2 frag_coord, float a, uint id) {
    if (a < 1.0 / 17.0) {
        return true;
    }
    if (a > 16.0 / 17.0) {
        return false;
    }

    // Use the id to try to discard different pixels from different objects, causing
    // them to be visible behind eachother.
    float r0 = floor(hash_one(id) * 16.0);
    vec2 r1 = vec2(floor(r0 * 0.25), mod(r0, 4.0));

    // We sample the bayer multiple times to have a smoother gradient of dithering.
    // This could be achieved by having a larger bayer matrix, but would then have
    // to define a larger one and not sure it would be much more efficient.
    uvec2 pos0 = uvec2(frag_coord + r1) % 4;
    uvec2 pos1 = uvec2(frag_coord / 4.0 + r1) % 4;
    uvec2 pos2 = uvec2(frag_coord / 16.0 + r1) % 4;

    mat4 bayer = mat4(
        16.0, 4.0, 13.0, 1.0,
        8.0, 12.0, 5.0, 9.0,
        14.0, 2.0, 15.0, 3.0,
        6.0, 10.0, 7.0, 11.0
    );
    mat4 bayer0 = bayer / 17.0;
    mat4 bayer1 = bayer / (17.0 * 17.0);
    mat4 bayer2 = bayer / (17.0 * 17.0 * 17.0);

    return a < bayer0[pos0.x][pos0.y] + bayer1[pos1.x][pos1.y] + bayer2[pos2.x][pos2.y];
}
#endif

#endif

#ifndef SRGB_GLSL
#define SRGB_GLSL

#extension GL_EXT_samplerless_texture_functions : enable

// Linear RGB, attenuation coefficients for water at roughly R, G, B wavelengths.
// See https://en.wikipedia.org/wiki/Electromagnetic_absorption_by_water
const vec3 MU_WATER = vec3(0.6, 0.04, 0.01);

//https://gamedev.stackexchange.com/questions/92015/optimized-linear-to-srgb-glsl
vec3 srgb_to_linear(vec3 srgb) {
    bvec3 cutoff = lessThan(srgb, vec3(0.04045));
    vec3 higher = pow((srgb + vec3(0.055))/vec3(1.055), vec3(2.4));
    vec3 lower = srgb/vec3(12.92);

    return mix(higher, lower, cutoff);
}

vec3 linear_to_srgb(vec3 col) {
    vec3 s1 = vec3(sqrt(col.r), sqrt(col.g), sqrt(col.b));
    vec3 s2 = vec3(sqrt(s1.r), sqrt(s1.g), sqrt(s1.b));
    vec3 s3 = vec3(sqrt(s2.r), sqrt(s2.g), sqrt(s2.b));
    return vec3(
            mix(11.500726 * col.r, (0.585122381 * s1.r + 0.783140355 * s2.r - 0.368262736 * s3.r), clamp((col.r - 0.0060) * 10000.0, 0.0, 1.0)),
            mix(11.500726 * col.g, (0.585122381 * s1.g + 0.783140355 * s2.g - 0.368262736 * s3.g), clamp((col.g - 0.0060) * 10000.0, 0.0, 1.0)),
            mix(11.500726 * col.b, (0.585122381 * s1.b + 0.783140355 * s2.b - 0.368262736 * s3.b), clamp((col.b - 0.0060) * 10000.0, 0.0, 1.0))
    );
}

float pow5(float x) {
    float x2 = x * x;
    return x2 * x2 * x;
}

vec4 pow5(vec4 x) {
    vec4 x2 = x * x;
    return x2 * x2 * x;
}

// Fresnel angle for perfectly specular dialectric materials.

// Schlick approximation
vec3 schlick_fresnel(vec3 Rs, float cosTheta) {
    return Rs + pow5(1.0 - cosTheta) * (1.0 - Rs);
}

// Beckmann Distribution
float BeckmannDistribution_D(float NdotH, float alpha) {
    const float PI = 3.1415926535897932384626433832795;
    float NdotH2 = NdotH * NdotH;
    float NdotH2m2 = NdotH2 * alpha * alpha;
    float k_spec = exp((NdotH2 - 1.0) / NdotH2m2) / (PI * NdotH2m2 * NdotH2);
    return mix(k_spec, 0.0, NdotH == 0.0);
}

// Voxel Distribution
float BeckmannDistribution_D_Voxel(vec3 wh, vec3 voxel_norm, float alpha) {
    vec3 sides = sign(voxel_norm);
    
    vec3 NdotH = wh * sides;

    const float PI = 3.1415926535897932384626433832795;
    vec3 NdotH2 = NdotH * NdotH;
    vec3 NdotH2m2 = NdotH2 * alpha * alpha;
    vec3 k_spec = exp((NdotH2 - 1.0) / NdotH2m2) / (PI * NdotH2m2 * NdotH2);
    return dot(mix(k_spec, vec3(0.0), equal(NdotH, vec3(0.0))), abs(voxel_norm));
}

float TrowbridgeReitzDistribution_D_Voxel(vec3 wh, vec3 voxel_norm, float alpha) {
    vec3 sides = sign(voxel_norm);

    vec3 NdotH = wh * sides;

    const float PI = 3.1415926535897932384626433832795;
    vec3 NdotH2 = NdotH * NdotH;
    vec3 NdotH2m2 = NdotH2 * alpha * alpha;
    vec3 e = (1.0 - NdotH2) / NdotH2m2;
    vec3 k_spec = 1.0 / (PI * NdotH2m2 * NdotH2 * (1.0 + e) * (1.0 + e));
    return dot(mix(k_spec, vec3(0.0), equal(NdotH, vec3(0.0))), abs(voxel_norm));
}

float BeckmannDistribution_Lambda(vec3 norm, vec3 dir, float alpha) {
    float CosTheta = dot(norm, dir);
    float SinTheta = sqrt(1.0 - CosTheta * CosTheta);
    float TanTheta = SinTheta / CosTheta;
    float absTanTheta = abs(TanTheta);
    float a = 1.0 / (alpha * absTanTheta);
    
    return mix(max(0.0, (1.0 - 1.259 * a + 0.396 * a * a) / (3.535 * a + 2.181 * a * a)), 0.0, isinf(absTanTheta) || a >= 1.6);
}

float BeckmannDistribution_G(vec3 norm, vec3 dir, vec3 light_dir, float alpha) {
    return 1.0 / (1.0 + BeckmannDistribution_Lambda(norm, dir, alpha) + BeckmannDistribution_Lambda(norm, -light_dir, alpha));
}

// Fresnel blending
//
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Microfacet_Models.html#fragment-MicrofacetDistributionPublicMethods-2
// and
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Fresnel_Incidence_Effects.html
vec3 FresnelBlend_f(vec3 norm, vec3 dir, vec3 light_dir, vec3 R_d, vec3 R_s, float alpha) {
    const float PI = 3.1415926535897932384626433832795;
    alpha = alpha * sqrt(2.0);
    float cos_wi = dot(-light_dir, norm);
    float cos_wo = dot(dir, norm);

    vec3 diffuse = (28.0 / (23.0 * PI)) * R_d *
        (1.0 - R_s) *
        (1.0 - pow5(1.0 - 0.5 * abs(cos_wi))) *
        (1.0 - pow5(1.0 - 0.5 * abs(cos_wo)));
    vec3 wh = -light_dir + dir;
#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    bool is_blocked = cos_wi == 0.0 || cos_wo == 0.0;
#else
    bool is_blocked = cos_wi <= 0.0 || cos_wo <= 0.0;
#endif
    if (is_blocked) {
        return vec3(0.0);
    }
    wh = normalize(wh);
    float dot_wi_wh = dot(-light_dir, wh);
    vec3 specular = dot(norm, dir) > 0.0 ? vec3(0.0) : (BeckmannDistribution_D(dot(wh, norm), alpha) /
        (4.0 * abs(dot_wi_wh) *
        max(abs(cos_wi), abs(cos_wo))) *
        schlick_fresnel(R_s, dot_wi_wh));
    return mix(diffuse + specular, vec3(0.0), bvec3(all(equal(light_dir, dir))));
}

// Fresnel blending
//
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Microfacet_Models.html#fragment-MicrofacetDistributionPublicMethods-2
// and
// http://www.pbr-book.org/3ed-2018/Reflection_Models/Fresnel_Incidence_Effects.html
vec3 FresnelBlend_Voxel_f(vec3 norm, vec3 dir, vec3 light_dir, vec3 R_d, vec3 R_s, float alpha, vec3 voxel_norm, float dist) {
    const float PI = 3.1415926535897932384626433832795;
    alpha = alpha * sqrt(2.0);
    float cos_wi = dot(-light_dir, norm);
    float cos_wo = dot(dir, norm);

#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    vec4 AbsNdotL = abs(vec4(light_dir, cos_wi));
    vec4 AbsNdotV = abs(vec4(dir, cos_wo));
#else
    vec3 sides = sign(voxel_norm);
    vec4 AbsNdotL = vec4(max(-light_dir * sides, 0.0), abs(cos_wi));
    vec4 AbsNdotV = vec4(max(dir * sides, 0.0), abs(cos_wo));
#endif

    vec4 diffuse_factor = (1.0 - pow5(1.0 - 0.5 * AbsNdotL)) * (1.0 - pow5(1.0 - 0.5 * AbsNdotV));

    vec3 diffuse = (28.0 / (23.0 * PI)) * R_d * (1.0 - R_s) * dot(diffuse_factor, /*R_r * */vec4(abs(norm) * (1.0 - dist), dist));

    vec3 wh = -light_dir + dir;
#if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    bool is_blocked = cos_wi == 0.0 || cos_wo == 0.0;
#else
    bool is_blocked = cos_wi <= 0.0 || cos_wo <= 0.0;
#endif
    if (is_blocked) {
        return vec3(0.0);
    }
    wh = normalize(wh);
    float dot_wi_wh = dot(-light_dir, wh);
    float distr = BeckmannDistribution_D_Voxel(wh, voxel_norm, alpha);
    vec3 specular = distr /
        (4.0 * abs(dot_wi_wh) *
        max(abs(cos_wi), abs(cos_wo))) *
        schlick_fresnel(R_s, dot_wi_wh);
    return mix(diffuse + specular, vec3(0.0), bvec3(all(equal(light_dir, dir))));
}

// Phong reflection.
//
// Note: norm, dir, light_dir must all be normalizd.
vec3 light_reflection_factor2(vec3 norm, vec3 dir, vec3 light_dir, vec3 k_d, vec3 k_s, float alpha) {
    // TODO: These are supposed to be the differential changes in the point location p, in tangent space.
    // That is, assuming we can parameterize a 2D surface by some function p : R² → R³, mapping from
    // points in a plane to 3D points on the surface, we can define
    // ∂p(u,v)/∂u and ∂p(u,v)/∂v representing the changes in the pont location as we move along these
    // coordinates.
    //
    // Then we can define the normal at a point, n(u,v) = ∂p(u,v)/∂u × ∂p(u,v)/∂v.
    //
    // Additionally, we can define the change in *normals* at each point using the
    // Weingarten equations (see http://www.pbr-book.org/3ed-2018/Shapes/Spheres.html):
    //
    // ∂n/∂u = (fF - eG) / (EG - F²) ∂p/∂u + (eF - fE) / (EG - F²) ∂p/∂v
    // ∂n/∂v = (gF - fG) / (EG - F²) ∂p/∂u + (fF - gE) / (EG - F²) ∂p/∂v
    //
    // where
    //
    // E = |∂p/∂u ⋅ ∂p/∂u|
    // F = ∂p/∂u ⋅ ∂p/∂u
    // G = |∂p/∂v ⋅ ∂p/∂v|
    //
    // and
    //
    // e = n ⋅ ∂²p/∂u²
    // f = n ⋅ ∂²p/(∂u∂v)
    // g = n ⋅ ∂²p/∂v²
    //
    // For planes (see http://www.pbr-book.org/3ed-2018/Shapes/Triangle_Meshes.html) we have
    // e = f = g = 0 (since the plane has no curvature of any sort) so we get:
    //
    // ∂n/∂u = (0, 0, 0)
    // ∂n/∂v = (0, 0, 0)
    //
    // To find ∂p/∂u and ∂p/∂v, we first write p and u parametrically:
    //    p(u, v) = p0 + u ∂p/∂u + v ∂p/∂v
    //
    // ( u₀ - u₂    v₀ - v₂
    //   u₁ - u₂    v₁ - v₂ )
    //
    // Basis: plane norm = norm = (0, 0, 1), x vector = any orthgonal vector on the plane.
    // vec3 w_i =
    // vec3 w_i = vec3(view_mat * vec4(-light_dir, 1.0));
    // vec3 w_o = vec3(view_mat * vec4(light_dir, 1.0));
    return FresnelBlend_f(norm, dir, light_dir, k_d, k_s, alpha);
}

vec3 light_reflection_factor(vec3 norm, vec3 dir, vec3 light_dir, vec3 k_d, vec3 k_s, float alpha, vec3 voxel_norm, float voxel_lighting) {
#if (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_LAMBERTIAN)
    const float PI = 3.141592;
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    vec4 AbsNdotL = abs(vec4(light_dir, dot(norm, light_dir)));
        #else
    vec3 sides = sign(voxel_norm);
    vec4 AbsNdotL = max(vec4(-light_dir * sides, dot(norm, -light_dir)), 0.0);
        #endif
    float diffuse = dot(AbsNdotL, vec4(abs(voxel_norm) * (1.0 - voxel_lighting), voxel_lighting));
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    float diffuse = abs(dot(norm, light_dir));
        #else
    float diffuse = max(dot(norm, -light_dir), 0.0);
        #endif
    #endif
    return k_d / PI * diffuse;
#elif (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_BLINN_PHONG)
    const float PI = 3.141592;
    alpha = alpha * sqrt(2.0);
    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
    float ndotL = abs(dot(norm, light_dir));
    #else
    float ndotL = max(dot(norm, -light_dir), 0.0);
    #endif

    if (ndotL > 0.0) {
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        vec4 AbsNdotL = abs(vec4(light_dir, ndotL));
        #else
        vec3 sides = sign(voxel_norm);
        vec4 AbsNdotL = max(vec4(-light_dir * sides, ndotL), 0.0);
        #endif
        float diffuse = dot(AbsNdotL, vec4(abs(voxel_norm) * (1.0 - voxel_lighting), voxel_lighting));
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        float diffuse = ndotL;
    #endif
        vec3 H = normalize(-light_dir + dir);

    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        float NdotH = abs(dot(norm, H));
    #else
        float NdotH = max(dot(norm, H), 0.0);
    #endif
        return (1.0 - k_s) / PI * k_d * diffuse + k_s * pow(NdotH, alpha/* * 4.0*/);
    }

    return vec3(0.0);
#elif (LIGHTING_ALGORITHM == LIGHTING_ALGORITHM_ASHIKHMIN)
    #if (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_VOXEL)
        return FresnelBlend_Voxel_f(norm, dir, light_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting);
    #elif (LIGHTING_DISTRIBUTION_SCHEME == LIGHTING_DISTRIBUTION_SCHEME_MICROFACET)
        return FresnelBlend_f(norm, dir, light_dir, k_d, k_s, alpha);
    #endif
#endif
}

float rel_luminance(vec3 rgb)
{
    // https://en.wikipedia.org/wiki/Relative_luminance
    const vec3 W = vec3(0.2126, 0.7152, 0.0722);
    return dot(rgb, W);
}

// From https://discourse.vvvv.org/t/infinite-ray-intersects-with-infinite-plane/10537
// out of laziness.
bool IntersectRayPlane(vec3 rayOrigin, vec3 rayDirection, vec3 posOnPlane, vec3 planeNormal, inout vec3 intersectionPoint)
{
  float rDotn = dot(rayDirection, planeNormal);

  //parallel to plane or pointing away from plane?
  if (rDotn < 0.0000001 )
    return false;

  float s = dot(planeNormal, (posOnPlane - rayOrigin)) / rDotn;

  intersectionPoint = rayOrigin + s * rayDirection;

  return true;
}

// Compute uniform attenuation due to beam passing through a substance that fills an area below a horizontal plane
// (e.g. in most cases, water below the water surface depth) using the simplest form of the Beer-Lambert law
// (https://en.wikipedia.org/wiki/Beer%E2%80%93Lambert_law):
//
// I(z) = I₀ e^(-μz)
//
// We compute this value, except for the initial intensity which may be multiplied out later.
//
// wpos is the position of the point being hit.
// ray_dir is the reversed direction of the ray (going "out" of the point being hit).
// mu is the attenuation coefficient for R, G, and B wavelenghts.
// surface_alt is the estimated altitude of the horizontal surface separating the substance from air.
// defaultpos is the position to use in computing the distance along material at this point if there was a failure.
//
// Ideally, defaultpos is set so we can avoid branching on error.
vec3 compute_attenuation(vec3 wpos, vec3 ray_dir, vec3 mu, float surface_alt, vec3 defaultpos) {
#if (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_IMPORTANCE)
    return vec3(1.0);
#elif (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_RADIANCE)
    #if (LIGHTING_TYPE & LIGHTING_TYPE_TRANSMISSION) != 0
        return vec3(1.0);
    #else
    ray_dir = faceforward(ray_dir, vec3(0.0, 0.0, -1.0), ray_dir);
    vec3 surface_dir = surface_alt < wpos.z ? vec3(0.0, 0.0, -1.0) : vec3(0.0, 0.0, 1.0);
    bool _intersects_surface = IntersectRayPlane(wpos, ray_dir, vec3(0.0, 0.0, surface_alt), surface_dir, defaultpos);
    float depth = length(defaultpos - wpos);
    return exp(-mu * depth);
    #endif
#endif
}

// Same as compute_attenuation but since both point are known, set a maximum to make sure we don't exceed the length
// from the default point.
vec3 compute_attenuation_point(vec3 wpos, vec3 ray_dir, vec3 mu, float surface_alt, vec3 defaultpos) {
#if (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_IMPORTANCE)
    return pow(1.0 - mu, vec3(3));
#elif (LIGHTING_TRANSPORT_MODE == LIGHTING_TRANSPORT_MODE_RADIANCE)
    return vec3(1.0);
#endif
}

vec3 greedy_extract_col_light_attr(texture2D t_col_light, sampler s_col_light, vec2 f_uv_pos, out float f_light, out float f_glow, out float f_ao, out uint f_attr, out float f_sky_exposure) {
    // TODO: Figure out how to use `texture` and modulation to avoid needing to do manual filtering
    // TODO: Use `texture` instead

    uvec4 tex_00 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(0, 0), 0) * 255.0);
    uvec4 tex_10 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(1, 0), 0) * 255.0);
    uvec4 tex_01 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(0, 1), 0) * 255.0);
    uvec4 tex_11 = uvec4(texelFetch(sampler2D(t_col_light, s_col_light), ivec2(f_uv_pos) + ivec2(1, 1), 0) * 255.0);
    vec3 light_00 = vec3(tex_00.rg >> 3u, tex_00.a & 1u);
    vec3 light_10 = vec3(tex_10.rg >> 3u, tex_10.a & 1u);
    vec3 light_01 = vec3(tex_01.rg >> 3u, tex_01.a & 1u);
    vec3 light_11 = vec3(tex_11.rg >> 3u, tex_11.a & 1u);
    vec3 light_0 = mix(light_00, light_01, fract(f_uv_pos.y));
    vec3 light_1 = mix(light_10, light_11, fract(f_uv_pos.y));
    vec3 light = mix(light_0, light_1, fract(f_uv_pos.x));

    vec3 f_col = vec3(
        float(((tex_00.r & 0x7u) << 1u) | (tex_00.b & 0xF0u)),
        float(tex_00.a & 0xFEu),
        float(((tex_00.g & 0x7u) << 1u) | ((tex_00.b & 0x0Fu) << 4u))
    ) / 255.0;

    f_ao = light.z;
    f_light = light.x / 31.0;
    f_sky_exposure = light.x / 31.0 + (1.0 - f_ao) * 0.5;
    f_glow = light.y / 31.0;
    f_attr = tex_00.g >> 3u;
    return srgb_to_linear(f_col);
}

vec3 greedy_extract_col_light_kind_terrain(
    texture2D t_col_light, sampler s_col_light,
    utexture2D t_kind,
    vec2 f_uv_pos,
    out float f_light, out float f_glow, out float f_ao, out float f_sky_exposure, out uint f_kind
) {
    uint _f_attr;
    f_kind = uint(texelFetch(t_kind, ivec2(f_uv_pos), 0).r);
    return greedy_extract_col_light_attr(t_col_light, s_col_light, f_uv_pos, f_light, f_glow, f_ao, _f_attr, f_sky_exposure);
}

vec3 greedy_extract_col_light_figure(texture2D t_col_light, sampler s_col_light, vec2 f_uv_pos, out float f_light, out uint f_attr) {
    float _f_sky_exposure, _f_light, _f_glow, _f_ao;
    return greedy_extract_col_light_attr(t_col_light, s_col_light, f_uv_pos, f_light, _f_glow, _f_ao, f_attr, _f_sky_exposure);
}

#endif

#ifndef SHADOWS_GLSL
#define SHADOWS_GLSL

#ifdef HAS_SHADOW_MAPS
    #if (SHADOW_MODE == SHADOW_MODE_MAP)
        layout (std140, set = 0, binding = 9)
        uniform u_light_shadows {
            mat4 shadowMatrices;
            mat4 texture_mat;
        };
        
        // Use with sampler2DShadow
        #define t_directed_shadow_maps packed_input

        #define s_directed_shadow_maps packed_sampler

        
        // Use with samplerCubeShadow
        #define t_point_shadow_maps packed_input

        #define s_point_shadow_maps packed_sampler

        
        float VectorToDepth(vec3 Vec) {
            vec3 AbsVec = abs(Vec);
            float LocalZcomp = max(AbsVec.x, max(AbsVec.y, AbsVec.z));
        
            float NormZComp = shadow_proj_factors.x - shadow_proj_factors.y / LocalZcomp;
            return NormZComp;
        }
        
        const vec3 sampleOffsetDirections[20] = vec3[](
            vec3( 1,  1,  1), vec3( 1, -1,  1), vec3(-1, -1,  1), vec3(-1,  1,  1),
            vec3( 1,  1, -1), vec3( 1, -1, -1), vec3(-1, -1, -1), vec3(-1,  1, -1),
            vec3( 1,  1,  0), vec3( 1, -1,  0), vec3(-1, -1,  0), vec3(-1,  1,  0),
            vec3( 1,  0,  1), vec3(-1,  0,  1), vec3( 1,  0, -1), vec3(-1,  0, -1),
            vec3( 0,  1,  1), vec3( 0, -1,  1), vec3( 0, -1, -1), vec3( 0,  1, -1)
        );
        
        float ShadowCalculationPoint(uint lightIndex, vec3 fragToLight, vec3 fragNorm, vec3 fragPos) {
            if (lightIndex != 0u) {
                return 1.0;
            };
        
            float currentDepth = VectorToDepth(fragToLight);
        
            return textureGrad(samplerCubeShadow(t_point_shadow_maps, s_point_shadow_maps), vec4(fragToLight, currentDepth), vec3(0), vec3(0));
        }
        
        float ShadowCalculationDirected(in vec3 fragPos) {
            // Don't try to calculate directed shadows if there are no directed light sources
            // Applies, for example, in the char select menu
            if (light_shadow_count.z < 1) { return 1.0; }
        
            float bias = 0.0;
            float diskRadius = 0.01;
            vec4 sun_pos = texture_mat * vec4(fragPos, 1.0);
            return textureProj(sampler2DShadow(t_directed_shadow_maps, s_directed_shadow_maps), sun_pos);
        }
    #elif (SHADOW_MODE == SHADOW_MODE_NONE || SHADOW_MODE == SHADOW_MODE_CHEAP)
        float ShadowCalculationPoint(uint lightIndex, vec3 fragToLight, vec3 fragNorm, vec3 fragPos) {
            return 1.0;
        }
    #endif
#else
    float ShadowCalculationPoint(uint lightIndex, vec3 fragToLight, vec3 fragNorm, vec3 fragPos) {
        return 1.0;
    }
#endif

#endif

#ifndef GLOBALS_GLSL
#define GLOBALS_GLSL


    mat4 view_mat;
    mat4 proj_mat;
    mat4 all_mat;
    vec4 cam_pos;
    vec4 focus_off;
    vec4 focus_pos;
    vec4 view_distance;
    // .x = time of day, repeats every day.
    // .y = a continuous value for what day it is. Repeats every `tick_overflow` for precisions sake.
    vec4 time_of_day;
    vec4 sun_dir;
    vec4 moon_dir;
    // .x = The `Time` resource, repeated every `tick_overflow`
    // .y = a floored (`Time` / `tick_overflow`)
    // .z = Time local to client, not synced between clients.
    vec4 tick;
    vec4 screen_res;
    uvec4 light_shadow_count;
    vec4 shadow_proj_factors;
    uvec4 medium;
    ivec4 select_pos;
    vec4 gamma_exposure;
    vec4 last_lightning;
    vec2 wind_vel;
    vec2 internal_res;
    float ambiance;
    // 0 - FirstPerson
    // 1 - ThirdPerson
    uint cam_mode;
    float sprite_render_distance;
    float u_rotation;
    float screen_fade;


float distance_divider = 2.0;
float shadow_dithering = 0.5;

float tick_overflow = 300000.0;

// Get a scaled time with an offset that loops at a period.
float tick_loop(float period, float scale, float offset) {
    float loop = tick_overflow * scale;
    float rem = mod(loop, period);
    float rest = rem * tick.y;

    return mod(rest + tick.x * scale + offset, period);
}

float tick_loop(float period) {
    return tick_loop(period, 1.0, 0.0);
}

vec3 tick_loop(float period, vec3 scale, vec3 offset) {
    vec3 loop = tick_overflow * scale;
    vec3 rem = mod(loop, period);
    vec3 rest = rem * tick.y;

    return mod(rest + tick.x * scale + offset, period);
}

// Only works if t happened within tick_overflow
float time_since(float t) {
    return tick.x < t ? (tick_overflow - t + tick.x) : (tick.x - t);
}

#endif


#ifndef RAIN_OCCLUSION_GLSL
#define RAIN_OCCLUSION_GLSL

// Use with sampler2DShadow
#define t_directed_occlusion_maps packed_input

#define s_directed_occlusion_maps packed_sampler



    mat4 rain_occlusion_matrices;
    mat4 rain_occlusion_texture_mat;
    mat4 rain_dir_mat;
    float integrated_rain_vel;
    float rain_density;
    vec2 occlusion_dummy; // Fix alignment.


float rain_occlusion_at(in vec3 fragPos)
{
    vec4 rain_pos = rain_occlusion_texture_mat * vec4(fragPos, 1.0);

    float visibility = textureProj(sampler2DShadow(t_directed_occlusion_maps, s_directed_occlusion_maps), rain_pos);

    return visibility;
}
#endif

// Information about an approximately directional light, like the sun or moon.
struct DirectionalLight {
    float shadow;
    // Fully blocks all light, including ambience
    float block;
};

const float PI = 3.141592653;

const vec3 SKY_DAWN_TOP = vec3(0.10, 0.1, 0.10);
const vec3 SKY_DAWN_MID = vec3(1.2, 0.3, 0.2);
const vec3 SKY_DAWN_BOT = vec3(0.0, 0.1, 0.23);
const vec3 DAWN_LIGHT   = vec3(5.0, 2.0, 1.15);
const vec3 SUN_HALO_DAWN = vec3(3.0, 0.5, 0.1);

const vec3 SKY_DAY_TOP = vec3(0.1, 0.5, 0.9);
const vec3 SKY_DAY_MID = vec3(0.18, 0.28, 0.6);
const vec3 SKY_DAY_BOT = vec3(0.1, 0.2, 0.3);
const vec3 DAY_LIGHT   = vec3(3.8, 3.0, 1.8);
const vec3 SUN_HALO_DAY = vec3(0.15, 0.15, 0.001);

const vec3 SKY_DUSK_TOP = vec3(1.06, 0.1, 0.20);
const vec3 SKY_DUSK_MID = vec3(2.5, 0.3, 0.1);
const vec3 SKY_DUSK_BOT = vec3(0.0, 0.1, 0.23);
const vec3 DUSK_LIGHT   = vec3(8.0, 1.5, 0.15);
const vec3 SUN_HALO_DUSK = vec3(3.0, 0.5, 0.05);

const vec3 SKY_NIGHT_TOP = vec3(0.001, 0.001, 0.0025);
const vec3 SKY_NIGHT_MID = vec3(0.001, 0.005, 0.02);
const vec3 SKY_NIGHT_BOT = vec3(0.002, 0.004, 0.004);
const vec3 NIGHT_LIGHT   = vec3(5.0, 0.75, 0.2);

// Linear RGB, scattering coefficients for atmosphere at roughly R, G, B wavelengths.
//
// See https://en.wikipedia.org/wiki/Diffuse_sky_radiation
const vec3 MU_SCATTER = vec3(0.05, 0.10, 0.23);

const float SUN_COLOR_FACTOR = 5.0;
const float MOON_COLOR_FACTOR = 5.0;

const float UNDERWATER_MIST_DIST = 100.0;

const float PERSISTENT_AMBIANCE = 1.0 / 32.0;

// Glow from static light sources
// Allowed to be > 1 due to HDR
const vec3 GLOW_COLOR = vec3(0.89, 0.95, 0.52);

// Calculate glow from static light sources, + some noise for flickering.
// TODO: Optionally disable the flickering for performance?
vec3 glow_light(vec3 pos) {
    #if (SHADOW_MODE <= SHADOW_MODE_NONE)
        return GLOW_COLOR;
    #else
        return GLOW_COLOR * (1.0 + (noise_3d(vec3(pos.xy * 0.005, tick.x * 0.5)) - 0.5) * 0.5);
    #endif
}

float cloud_avg_alt() { return view_distance.z + (view_distance.w - view_distance.z) * 1.25; }

const float wind_speed = 0.25;
vec2 wind_offset() { return vec2(time_of_day.y * wind_speed * (3600.0 * 24.0)); }

float cloud_scale() { return view_distance.z / 150.0; }

#define t_alt packed_input

#define s_alt packed_sampler


// Transforms coordinate in the range 0..WORLD_SIZE to 0..1
vec2 wpos_to_uv(vec2 wpos) { return (wpos+16.0)/(32.0*vec2(param(24),param(25))); }

// Weather texture
#define t_weather packed_input

#define s_weather packed_sampler


vec4 sample_weather(vec2 wpos) { return weather_sample(wpos_to_uv(wpos)); }

float cloud_tendency_at(vec2 wpos) {
    return sample_weather(wpos).r;
}

float rain_density_at(vec2 wpos) {
    return sample_weather(wpos).g;
}

float cloud_shadow(vec3 pos, vec3 light_dir) {
    #if (CLOUD_MODE <= CLOUD_MODE_MINIMAL)
        return 1.0;
    #else
        vec2 xy_offset = light_dir.xy * ((cloud_avg_alt() - pos.z) / -light_dir.z);

        // Fade out shadow if the sun angle is too steep (simulates a widening penumbra with distance)
        const vec2 FADE_RANGE = vec2(1500, 10000);
        float fade = 1.0 - clamp((length(xy_offset) - FADE_RANGE.x) / (FADE_RANGE.y - FADE_RANGE.x), 0, 1);
        float cloud = cloud_tendency_at(pos.xy + focus_off.xy - xy_offset);

        return clamp(1 - fade * cloud * 16.0, 0, 1);
    #endif
}

float magnetosphere() { return sin(time_of_day.y); }

vec3 magnetosphere_tint() {
    #if (CLOUD_MODE <= CLOUD_MODE_LOW)
        return vec3(1);
    #else
        float magnetosphere = magnetosphere();
        float magnetosphere2 = pow(magnetosphere, 2) * 2 - 1;
        float magnetosphere3 = pow(magnetosphere2, 2) * 2 - 1;
        vec3 magnetosphere_change = vec3(1.0) + vec3(
            (magnetosphere + 1.0) * 2.0,
            (-magnetosphere2 + 1.0) * 2.0,
            (-magnetosphere3 + 1.0) * 1.0
        ) * 0.4;
        return normalize(magnetosphere_change);
    #endif
 }

#if (CLOUD_MODE > CLOUD_MODE_FLAT)
float emission_strength() {
    return clamp((magnetosphere() - 0.3) * 1.3, 0, 1) * max(sun_dir.z, 0);
}

float emission_br() {
    #if (CLOUD_MODE >= CLOUD_MODE_MEDIUM)
        return abs(pow(fract(time_of_day.y * 0.5) * 2 - 1, 2));
    #else
        return 0.5;
    #endif
}
#endif


float get_sun_brightness() {
    return max(-sun_dir.z + 0.5, 0.0);
}

float get_moon_brightness() {
    return max(sun_dir.z + 0.6, 0.0) * 0.1;
}

vec3 get_sun_color() {
    vec3 light = (sun_dir.x > 0) ? DUSK_LIGHT : DAWN_LIGHT;

    return mix(
        mix(
            light * magnetosphere_tint(),
            NIGHT_LIGHT,
            max(sun_dir.z, 0)
        ),
        DAY_LIGHT,
        max(-sun_dir.z, 0)
    );
}

// Average sky colour (i.e: perfectly scattered light from the sky)
vec3 get_sky_color() {
    return mix(
        mix(
            (SKY_DUSK_TOP + SKY_DUSK_MID) / 2 * magnetosphere_tint(),
            (SKY_NIGHT_TOP + SKY_NIGHT_MID) / 2,
            max(sun_dir.z, 0)
        ),
        (SKY_DAY_TOP + SKY_DAY_MID) / 2,
        max(-sun_dir.z, 0)
    );
}

vec3 get_moon_color() {
    return vec3(0.5, 0.5, 1.6);
}

DirectionalLight get_sun_info(vec4 _dir, float shade_frac, vec3 f_pos) {
    float shadow = shade_frac;
    float block = 1.0;
#ifdef HAS_SHADOW_MAPS
    #if (SHADOW_MODE == SHADOW_MODE_MAP)
        if (sun_dir.z < 0.0) {
            shadow = min(shadow, ShadowCalculationDirected(f_pos));
        }
    #endif
#endif
    return DirectionalLight(shadow, block);
}

DirectionalLight get_moon_info(vec4 _dir, float shade_frac) {
    float shadow = shade_frac;
    float block = 1.0;
    return DirectionalLight(shadow, block);
}

const float LIGHTNING_HEIGHT = 25.0;
const float MAX_LIGHTNING_PERIOD = 5.0;

float lightning_intensity() {
    float time_since_lightning = time_since(last_lightning.w);
    return
        // Strength
        1000000
        // Flash
        * max(0.0, 1.0 - time_since_lightning * 1.0)
        // Reverb
        * max(sin(time_of_day.x * 0.4), 0.0);
}

vec3 lightning_at(vec3 wpos) {
    float time_since_lightning = time_since(last_lightning.w);
    if (time_since_lightning < MAX_LIGHTNING_PERIOD) {
        vec3 diff = wpos + focus_off.xyz - (last_lightning.xyz + vec3(0, 0, LIGHTNING_HEIGHT));
        float dist = length(diff);
        return vec3(0.5, 0.8, 1.0)
            * lightning_intensity()
            // Attenuation
            / pow(50.0 + dist, 2);
    } else {
        return vec3(0.0);
    }
}

// Returns computed maximum intensity.
//
// wpos is the position of this fragment.
// mu is the attenuation coefficient for any substance on a horizontal plane.
// cam_attenuation is the total light attenuation due to the substance for beams between the point and the camera.
// surface_alt is the altitude of the attenuating surface.
float get_sun_diffuse2(
    DirectionalLight sun_info,
    DirectionalLight moon_info,
    vec3 norm,
    vec3 dir,
    vec3 wpos,
    vec3 mu,
    vec3 cam_attenuation,
    float surface_alt,
    vec3 k_a,
    vec3 k_d,
    vec3 k_s,
    float alpha,
    vec3 voxel_norm,
    float voxel_lighting,
    out vec3 emitted_light,
    out vec3 reflected_light
) {
    const vec3 SUN_AMBIANCE = MU_SCATTER;
    #ifdef EXPERIMENTAL_PHOTOREALISTIC
        const vec3 MOON_AMBIANCE = MU_SCATTER;
    #else
        // Boost ambiance, because we don't properly compensate for pupil dilation (which should occur *before* HDR,
        // not in the end user's eye). Also, real nights are too dark to be fun.
        const vec3 MOON_AMBIANCE = vec3(0.15, 0.25, 0.23) * 5;
    #endif

    vec3 sun_dir = sun_dir.xyz;
    // TODO: Use real moon dir here and have other ways to light up night.
    // So this is a hack to just pretend the moon is still opposite to the sun
    // for this and `get_moon_brightness`.
    vec3 moon_dir = -sun_dir.xyz;

    float sun_light = get_sun_brightness() * sun_info.block;
    float moon_light = get_moon_brightness() * moon_info.block * ambiance;

    vec3 sun_color = get_sun_color() * SUN_COLOR_FACTOR;
    vec3 moon_color = get_moon_color() * MOON_COLOR_FACTOR;

    // If the sun is facing the wrong way, we currently just want zero light, hence default point is wpos.
    vec3 sun_attenuation = compute_attenuation(wpos, -sun_dir, mu, surface_alt, wpos);
    vec3 moon_attenuation = compute_attenuation(wpos, -moon_dir, mu, surface_alt, wpos);

    vec3 sun_chroma = sun_color * sun_light * cam_attenuation * sun_attenuation;
    vec3 moon_chroma = moon_color * moon_light * cam_attenuation * moon_attenuation;

    float sun_shadow = sun_info.shadow * cloud_shadow(wpos, sun_dir);
    float moon_shadow = moon_info.shadow * cloud_shadow(wpos, moon_dir);

    // https://en.m.wikipedia.org/wiki/Diffuse_sky_radiation
    //
    // HdRd radiation should come in at angle normal to us.
    // const float H_d = 0.23;
    //
    // Let β be the angle from horizontal
    // (for objects exposed to the sky, where positive when sloping towards south and negative when sloping towards north):
    //
    //     sin β = (north ⋅ norm) / |north||norm|
    //           = dot(vec3(0, 1, 0), norm)
    //
    //     cos β = sqrt(1.0 - dot(vec3(0, 1, 0), norm))
    //
    // Let h be the hour angle (180/0.0 at midnight, 90/1.0 at dawn, 0/0.0 at noon, -90/-1.0 at dusk, -180 at midnight/0.0):
    //     cos h = (midnight ⋅ -light_dir) / |midnight||-light_dir|
    //           = (noon ⋅ light_dir) / |noon||light_dir|
    //           = dot(vec3(0, 0, 1), light_dir)
    //
    // Let φ be the latitude at this point. 0 at equator, -90 at south pole / 90 at north pole.
    //
    // Let δ be the solar declination (angular distance of the sun's rays north [or south[]
    // of the equator), i.e. the angle made by the line joining the centers of the sun and Earth with its projection on the
    // equatorial plane.  Caused by axial tilt, and 0 at equinoxes.  Normally varies between -23.45 and 23.45 degrees.
    //
    // Let α (the solar altitude / altitud3 angle) be the vertical angle between the projection of the sun's rays on the
    // horizontal plane and the direction of the sun's rays (passing through a point).
    //
    // Let Θ_z be the vertical angle between sun's rays and a line perpendicular to the horizontal plane through a point,
    // i.e.
    //
    // Θ_z = (π/2) - α
    //
    // i.e. cos Θ_z = sin α and
    //      cos α = sin Θ_z
    //
    // Let γ_s be the horizontal angle measured from north to the horizontal projection of the sun's rays (positive when
    // measured westwise).
    //
    // cos Θ_z = cos φ cos h cos δ + sin φ sin δ
    // cos γ_s = sec α (cos φ sin δ - cos δ sin φ cos h)
    //         = (1  / √(1 - cos² Θ_z)) (cos φ sin δ - cos δ sin φ cos h)
    // sin γ_s = sec α cos δ sin h
    //         = (1 / cos α) cos δ sin h
    //         = (1 / sin Θ_z) cos δ sin h
    //         = (1  / √(1 - cos² Θ_z)) cos δ sin h
    //
    // R_b = (sin(δ)sin(φ - β) + cos(δ)cos(h)cos(φ - β))/(sin(δ)sin(φ) + cos(δ)cos(h)cos(φ))
    //
    // Assuming we are on the equator (i.e. φ = 0), and there is no axial tilt or we are at an equinox (i.e. δ = 0):
    //
    // cos Θ_z = 1 * cos h * 1 + 0 * 0 = cos h
    // cos γ_s = (1  / √(1 - cos² h)) (1 * 0 - 1 * 0 * cos h)
    //         = (1  / √(1 - cos² h)) * 0
    //         = 0
    // sin γ_s = (1  / √(1 - cos² h)) * sin h
    //         = sin h / sin h
    //         = 1
    //
    // R_b = (0 * sin(0 - β) + 1 * cos(h) * cos(0 - β))/(0 * 0 + 1 * cos(h) * 1)
    //     = (cos(h)cos(-β)) / cos(H)
    //     = cos(-β), the angle from horizontal.
    //
    // NOTE: cos(-β) = cos(β).
    // float cos_sun = dot(norm, /*-sun_dir*/vec3(0, 0, 1));
    // float cos_moon = dot(norm, -moon_dir);
    //
    // Let ζ = diffuse reflectance of surrounding ground for solar radiation, then we have
    //
    // R_d = (1 + cos β) / 2
    // R_r = ζ (1 - cos β) / 2
    //
    // H_t = H_b R_b + H_d R_d + (H_b + H_d) R_r
    float sin_beta = dot(vec3(0, 1, 0), norm);
    float R_b = sqrt(max(0.0, 1.0 - sin_beta * sin_beta));
    // Rough estimate of diffuse reflectance of rest of ground.
    // NOTE: zeta should be close to 0.7 with snow cover, 0.2 normally?  Maybe?
    vec3 zeta = max(vec3(0.2), k_d * (1.0 - k_s));
    float R_d = (1 + R_b) * 0.5;
    vec3 R_r = zeta * (1.0 - R_b) * 0.5;
    //
    // We can break this down into:
    //      H_t_b = H_b * (R_b + R_r) = light_intensity * (R_b + R_r)
    //      H_t_r = H_d * (R_d + R_r) = light_intensity * (R_d + R_r)
    vec3 R_t_b = R_b + R_r;
    vec3 R_t_r = R_d + R_r;

    #ifdef EXPERIMENTAL_PHOTOREALISTIC
        vec3 lrf = light_reflection_factor(norm, dir, -norm, k_d, vec3(0.0), alpha, voxel_norm, voxel_lighting);
    #else
        // In practice, for gameplay purposes, we often want extra light at earlier and later times, so we use a
        // non-physical LRF to boost light during dawn and dusk.
        float lrf = pow(dot(norm, vec3(0, 0, 1)) + 1, 2) * 0.25;
    #endif
    vec3 light_frac = R_t_b * (sun_chroma * SUN_AMBIANCE + moon_chroma * MOON_AMBIANCE) * lrf;

    emitted_light = light_frac;

    vec3 emission = vec3(0);
    #if (CLOUD_MODE > CLOUD_MODE_FLAT)
        if (emission_strength() > 0.0) {
            emission = mix(vec3(0, 0.5, 1), vec3(1, 0, 0), emission_br()) * emission_strength() * 0.025;
        }
    #endif

    #ifdef FLASHING_LIGHTS_ENABLED
        vec3 lightning = lightning_at(wpos);
    #else
        vec3 lightning = vec3(0);
    #endif

    reflected_light = R_t_r * (
        (1.0 - SUN_AMBIANCE) * sun_chroma * sun_shadow * light_reflection_factor(norm, dir, sun_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting)
        + (1.0 - MOON_AMBIANCE) * moon_chroma * moon_shadow * light_reflection_factor(norm, dir, moon_dir, k_d, k_s, alpha, voxel_norm, voxel_lighting)
        + emission
    ) + lightning;

    return rel_luminance(emitted_light + reflected_light);
}

// This has been extracted into a function to allow quick exit when detecting a star.
float is_star_at(vec3 dir) {
    float star_scale = 80.0;

    // Star positions
    vec3 pos = (floor(dir * star_scale) - 0.5) / star_scale;

    // Noisy offsets
    pos += (3.0 / star_scale) * (1.0 + hash(pos.yxzz) * 0.85);

    // Find distance to fragment
    float dist = length(pos - dir);

    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        const float power = 5.0;
    #else
        const float power = 50.0;
    #endif
    return power * max(sun_dir.z, 0.1) / (1.0 + pow(dist * 750, 8));
}

vec3 get_sky_light(vec3 dir, bool with_stars, float is_moon) {
    // Add white dots for stars. Note these flicker and jump due to FXAA
    float star = 0.0;
    if (with_stars) {
        vec3 star_dir = sun_dir.xyz * dir.z + cross(sun_dir.xyz, vec3(0, 1, 0)) * dir.x + vec3(0, 1, 0) * dir.y;
        star = is_star_at(star_dir) * (1.0 - is_moon);
    }

    vec3 sky_twilight_top = vec3(0.0, 0.0, 0.0);
    vec3 sky_twilight_mid = vec3(0.0, 0.0, 0.0);
    vec3 sky_twilight_bot = vec3(0.0, 0.0, 0.0);
    if (sun_dir.x > 0) {
      sky_twilight_top = SKY_DUSK_TOP;
      sky_twilight_mid = SKY_DUSK_MID;
      sky_twilight_bot = SKY_DUSK_BOT;
    } else {
      sky_twilight_top = SKY_DAWN_TOP;
      sky_twilight_mid = SKY_DAWN_MID;
      sky_twilight_bot = SKY_DAWN_BOT;
    }

    vec3 sky_top = mix(
        mix(
            sky_twilight_top * magnetosphere_tint(),
            SKY_NIGHT_TOP,
            pow(max(sun_dir.z, 0.0), 0.2)
        ) + star,
        SKY_DAY_TOP,
        max(-sun_dir.z, 0)
    );

    vec3 sky_mid = mix(
        mix(
            sky_twilight_mid * magnetosphere_tint(),
            SKY_NIGHT_MID,
            pow(max(sun_dir.z, 0.0), 0.1)
        ),
        SKY_DAY_MID,
        max(-sun_dir.z, 0)
    );

    vec3 sky_bot = mix(
        mix(
            sky_twilight_bot * magnetosphere_tint(),
            SKY_NIGHT_BOT,
            pow(max(sun_dir.z, 0.0), 0.2)
        ),
        SKY_DAY_BOT,
        max(-sun_dir.z, 0)
    );

    vec3 sky_color = mix(
        mix(
            sky_mid,
            sky_bot,
            max(-dir.z, 0)
        ),
        sky_top,
        max(dir.z, 0)
    );

    return sky_color * magnetosphere_tint();
}

vec3 get_sky_color(vec3 dir, vec3 origin, vec3 f_pos, float quality, bool with_features, float refractionIndex, bool fake_clouds, float sun_shade_frac) {
    // Sky color
    vec3 sun_dir = sun_dir.xyz;
    vec3 moon_dir = moon_dir.xyz;


    // Sun
    const vec3 SUN_SURF_COLOR = vec3(1.5, 0.9, 0.35) * 10.0;

    vec3 sun_halo_color = mix(
        (sun_dir.x > 0 ? SUN_HALO_DUSK : SUN_HALO_DAWN)* magnetosphere_tint(),
        SUN_HALO_DAY,
        pow(max(-sun_dir.z, 0.0), 0.5)
    );

    float sun_halo_power = 20.0;
    if (fake_clouds || medium.x == MEDIUM_WATER) {
        sun_halo_power = 30.0;
        sun_halo_color *= 0.01;
    }

    vec3 sun_halo = sun_halo_color * 25 * pow(max(dot(dir, -sun_dir), 0), sun_halo_power);
    vec3 sun_surf = vec3(0);
    if (with_features) {
        float angle = 0.00035;
        sun_surf = clamp((dot(dir, -sun_dir) - (1.0 - angle)) * 4 / angle, 0, 1)
            * SUN_SURF_COLOR
            * SUN_COLOR_FACTOR
            * sun_shade_frac;
    }
    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        sun_surf *= 0.1;
    }
    vec3 sun_light = sun_halo + sun_surf;

    // Moon
    const vec3 MOON_SURF_COLOR = vec3(0.7, 1.0, 1.5) * 250.0;
    const vec3 MOON_HALO_COLOR = vec3(0.015, 0.015, 0.05) * 250;

    vec3 moon_halo_color = MOON_HALO_COLOR;

    float moon_halo_power = 20.0;

    vec3 moon_surf = vec3(0);

    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        moon_halo_power = 50.0;
    }

    float is_moon = 0.0;

    if (with_features) {
        float moon_radius = 0.035;
        
        float tca = dot(-moon_dir, dir);

        float radius2 = moon_radius * moon_radius;
        float d2 = 1.0 - tca * tca;

        float diff = radius2 - d2;

        is_moon = clamp(tca * 2000.0, 0.0, 1.0) * clamp(diff * 4000.0, 0.0, 1.0);

        if (is_moon > 0.0) {
            float thc = sqrt(diff);

            float t0 = tca - thc;

            vec3 moon_normal = (t0 * dir + moon_dir) * (1.0 / moon_radius);

            float noise = snoise3(moon_normal * 8.2) + snoise3(moon_normal * 25.0) * 0.4;

            float direct_sunlight = max(dot(moon_normal, -sun_dir), 0.0);

            float planet_albedo = 0.12;
            float planet_reflected_light = (1.0 - abs(dot(moon_dir, sun_dir))) * max(dot(moon_normal, moon_dir), 0.0) * planet_albedo;
            float light = max(direct_sunlight + planet_reflected_light, 0.001);

            // ~sun is the same direction from the moon as it is to us.
            float surface_light = pow(light * (0.4 + 0.3 * noise), 2.0);

            moon_surf = MOON_SURF_COLOR * surface_light * is_moon;
        }
    }
    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        moon_halo_color *= 0.2;
        moon_surf *= 0.05;
    }
    vec3 moon_halo = moon_halo_color * pow(max(dot(dir, -moon_dir), 0) * max(dot(sun_dir, -moon_dir), 0), moon_halo_power);
    vec3 moon_light = moon_halo + moon_surf;

    // Replaced all clamp(sun_dir, 0, 1) with max(sun_dir, 0) because sun_dir is calculated from sin and cos, which are never > 1

    vec3 sky_color;
    #if (CLOUD_MODE == CLOUD_MODE_FLAT)
        if (true) {
    #else
        if (fake_clouds || medium.x == MEDIUM_WATER) {
    #endif
        sky_color = get_sky_light(dir, !fake_clouds, is_moon);
    } else {
        if (medium.x == MEDIUM_WATER) {
            sky_color = get_sky_light(dir, true, is_moon);
        } else {
            vec3 star_dir = normalize(sun_dir.xyz * dir.z + cross(sun_dir.xyz, vec3(0, 1, 0)) * dir.x + vec3(0, 1, 0) * dir.y);
            float star = is_star_at(star_dir) * (1.0 - is_moon);
            sky_color = vec3(0) + star;
        }
    }

    return sky_color + sun_light + moon_light;
}

float fog(vec3 f_pos, vec3 focus_pos, uint medium) {
    return max(1.0 - 5000.0 / (1.0 + distance(f_pos.xy, focus_pos.xy)), 0.0);
}

vec3 illuminate(float max_light, vec3 view_dir, vec3 emitted, vec3 reflected) {
    return emitted + reflected;
}

vec3 simple_lighting(vec3 pos, vec3 col, float shade) {
    // Bad fake lantern so we can see in caves
    vec3 d = pos.xyz - focus_pos.xyz;
    return col * clamp(2.5 / dot(d, d), shade * (get_sun_brightness() + 0.01), 1);
}

float wind_wave(float off, float scaling, float speed, float strength) {
    float aspeed = abs(speed);

    // TODO: Right now, the wind model is pretty simplistic. This means that there is frequently no wind at all, which
    // looks bad. For now, we add a lower bound on the wind speed to keep things looking nice.
    strength = max(strength, 6.0);
    aspeed = max(aspeed, 5.0);

    return (sin(tick_loop(2.0 * PI, 0.35 * scaling * floor(aspeed), off)) * (1.0 - fract(aspeed))
        + sin(tick_loop(2.0 * PI, 0.35 * scaling * ceil(aspeed), off)) * fract(aspeed)) * abs(strength) * 0.25;
}

#endif

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

layout(location=0) in vec2 texture_uv;
layout(location=0) out vec4 color;
void main() {
    cam_pos=vec4(param(0),param(1),param(2),0);
    focus_off=vec4(0);
    view_distance=vec4(0,0,param(22),param(23));
    time_of_day=vec4(param(17),param(18),0,0);
    sun_dir=vec4(param(19),param(20),param(21),0);
    moon_dir=-sun_dir;
    ambiance=param(27);
    rain_density=param(26);
    vec3 right=vec3(param(4),param(5),param(6));
    vec3 up=vec3(param(8),param(9),param(10));
    vec3 forward=vec3(param(12),param(13),param(14));
    vec2 clip=texture_uv*2.0-1.0;
    vec3 ray=normalize(forward+right*clip.x*param(16)/param(15)+up*clip.y/param(15));
    vec4 cloud=get_flat_cloud_layer(ray,cam_pos.xyz,524288.0);
    color=vec4(clamp(cloud.rgb,vec3(0),vec3(cloud.a)),cloud.a);
}
