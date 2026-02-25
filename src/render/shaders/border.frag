// SPDX-License-Identifier: GPL-3.0-only
// Border ring shader — renders one of 8 pieces (4 corners + 4 edges)
// Rounding technique based on niri (GPL-3.0) https://github.com/YaLTeR/niri

precision highp float;

uniform float alpha;
uniform vec2 size;
varying vec2 v_coords;

#ifdef DEBUG_FLAGS
uniform float tint;
#endif

uniform vec2  border_size;
uniform vec2  piece_offset;
uniform float border_width;
uniform vec4  border_color;
uniform float outer_radius;
uniform float scale;

float rounding_alpha(vec2 p, vec2 sz, float r,
                     float half_px) {
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
    vec2 px = v_coords * size + piece_offset;

    // outer shape
    float outer = rounding_alpha(px, border_size, outer_radius, half_px);

    // inner shape
    float inner_r = max(0.0, outer_radius - border_width);
    vec2 ip = px - vec2(border_width);
    vec2 isz = border_size - 2.0 * border_width;
    float inner = 0.0;

    if (ip.x >= 0.0 && ip.x <= isz.x && ip.y >= 0.0 && ip.y <= isz.y)
        inner = rounding_alpha(ip, isz, inner_r, half_px);

    float ring = outer * (1.0 - inner);
    vec4 color = border_color * ring;
    color *= alpha;

#ifdef DEBUG_FLAGS
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
