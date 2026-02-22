// SPDX-License-Identifier: GPL-3.0-only
// Clipping technique based on niri's clipped_surface.frag (GPL-3.0)
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
uniform float radius;
uniform mat3  input_to_geo;

float rounding_alpha(vec2 coords, vec2 sz, float r) {
    vec2 center;
    if (coords.x < r && coords.y < r) {
        center = vec2(r, r);
    } else if (sz.x - r < coords.x && coords.y < r) {
        center = vec2(sz.x - r, r);
    } else if (sz.x - r < coords.x
            && sz.y - r < coords.y) {
        center = vec2(sz.x - r, sz.y - r);
    } else if (coords.x < r && sz.y - r < coords.y) {
        center = vec2(r, sz.y - r);
    } else {
        return 1.0;
    }
    float dist = distance(coords, center);
    return 1.0 - smoothstep(r - 0.5, r + 0.5, dist);
}

void main() {
    vec3 geo = input_to_geo * vec3(v_coords, 1.0);

    vec4 color = texture2D(tex, v_coords);
    #if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
    #endif

    // Clip outside window geometry
    if (geo.x < 0.0 || 1.0 < geo.x
        || geo.y < 0.0 || 1.0 < geo.y)
    {
        color = vec4(0.0);
    } else {
        // Apply circle corner clipping
        color *= rounding_alpha(
            geo.xy * geo_size, geo_size, radius
        );
    }

    color = color * alpha;

    #if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
    #endif

    gl_FragColor = color;
}
