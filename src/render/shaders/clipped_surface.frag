// SPDX-License-Identifier: GPL-3.0-only
// Shader technique based on niri's clipped_surface.frag (GPL-3.0)
// https://github.com/YaLTeR/niri

#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision highp float;

#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

uniform vec2  geo_size;
uniform float inner_radius;
uniform float scale;
uniform mat3  input_to_geo;

float rounding_alpha(vec2 p, vec2 sz, float r, float half_px) {
    if (r <= 0.0) return 1.0;

    vec2 c;
    if (p.x < r && p.y < r)
        c = vec2(r, r);
    else if (sz.x - r < p.x && p.y < r)
        c = vec2(sz.x - r, r);
    else if (sz.x - r < p.x && sz.y - r < p.y)
        c = vec2(sz.x - r, sz.y - r);
    else if (p.x < r && sz.y - r < p.y)
        c = vec2(r, sz.y - r);
    else
        return 1.0;
    
    return 1.0 - smoothstep(r - half_px, r + half_px, distance(p, c));
}

void main() {
    float half_px = 0.5 / scale;
    vec3 geo = input_to_geo * vec3(v_coords, 1.0);

    vec4 color = texture2D(tex, v_coords);
    #if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
    #endif

    // discard pixel outside the window geometry
    if (geo.x < 0.0 || 1.0 < geo.x || geo.y < 0.0 || 1.0 < geo.y) {
        color = vec4(0.0);
    } else {
        color *= rounding_alpha(
            geo.xy * geo_size, geo_size, inner_radius, half_px
        );
    }

    color = color * alpha;

    #if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
    #endif

    gl_FragColor = color;
}
