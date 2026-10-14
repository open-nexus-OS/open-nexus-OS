#version 450
layout(set = 0, binding = 0) uniform sampler2D tex;
layout(push_constant) uniform Push { vec4 tint; } push;
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 color;
void main() {
    color = texture(tex, uv) * push.tint;
}
