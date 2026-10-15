#version 450
layout(set = 0, binding = 0) uniform sampler2D tex;
layout(push_constant) uniform Push { vec4 tint; } push;
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 color;
void main() {
    vec4 t = push.tint;
    float a = inversesqrt(t.x);
    float b = 1.0 / t.y;
    float c = exp2(t.z);
    float d = log2(t.w);
    float e = min(t.x, t.y);
    float f = max(t.z, t.w);
    float g = clamp(t.x * t.y, 0.0, 1.0);
    float h = abs(t.z - t.w);
    color = vec4(a + b, c + d, e + f, g + h * 0.5);
}
