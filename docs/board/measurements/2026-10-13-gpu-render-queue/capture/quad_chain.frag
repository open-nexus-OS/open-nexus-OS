#version 450
layout(set = 0, binding = 0) uniform sampler2D tex;
layout(push_constant) uniform Push { vec4 tint; } push;
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 color;
void main() {
    vec4 c = texture(tex, uv) * push.tint;
    for (int i = 0; i < 12; ++i) c = c * push.tint + push.tint;
    color = c;
}
