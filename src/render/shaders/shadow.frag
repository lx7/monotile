// SPDX-License-Identifier: GPL-3.0-only
// Shadow based on: https://madebyevan.com/shaders/fast-rounded-rectangle-shadows/
// License: CC0 (http://creativecommons.org/publicdomain/zero/1.0/)
// Shader technique based on niri (GPL-3.0) https://github.com/YaLTeR/niri

precision highp float;

uniform float alpha;
uniform vec2 size;
varying vec2 v_coords;

#ifdef DEBUG_FLAGS
uniform float tint;
#endif

uniform vec2  win_size;
uniform vec2  win_offset;
uniform float outer_radius;
uniform vec2  shadow_box_size;
uniform vec2  shadow_box_offset;
uniform float shadow_sigma;
uniform vec4  shadow_color;
uniform float scale;

float gaussian(float x, float s) {
    return exp(-x * x / (2.0 * s * s)) / (sqrt(6.283185307) * s);
}

vec2 erf(vec2 x) {
    vec2 s = sign(x), a = abs(x);
    x = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    x *= x;
    return s - s / (x * x);
}

float shadow_x(float x, float y, float s, float r, vec2 hs) {
    float d = min(hs.y - r - abs(y), 0.0);
    float c = hs.x - r + sqrt(max(0.0, r*r - d*d));
    vec2 i = 0.5 + 0.5 * erf((x + vec2(-c, c)) * (sqrt(0.5) / s));
    return i.y - i.x;
}

float shadow(vec2 lo, vec2 hi, vec2 pt, float s, float r) {
    vec2 c = (lo + hi) * 0.5;
    vec2 hs = (hi - lo) * 0.5;
    pt -= c;
    float start = clamp(-3.0*s, pt.y - hs.y, pt.y + hs.y);
    float end   = clamp( 3.0*s, pt.y - hs.y, pt.y + hs.y);
    float step  = (end - start) / 4.0;
    float y = start + step * 0.5;
    float val = 0.0;

    for (int i = 0; i < 4; i++) {
        val += shadow_x(pt.x, pt.y - y, s, r, hs)
             * gaussian(y, s) * step;
        y += step;
    }

    return val;
}

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
    vec2 px = v_coords * size;

    // shadow intensity
    float sv = shadow(vec2(0.0), shadow_box_size,
                      px - shadow_box_offset,
                      shadow_sigma, outer_radius);
    vec4 color = shadow_color * sv;

    // cut out window + border interior
    vec2 wp = px - win_offset;
    if (0.0 <= wp.x && wp.x <= win_size.x && 0.0 <= wp.y && wp.y <= win_size.y)
        color *= (1.0 - rounding_alpha(wp, win_size, outer_radius, half_px));

    color *= alpha;

#ifdef DEBUG_FLAGS
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
