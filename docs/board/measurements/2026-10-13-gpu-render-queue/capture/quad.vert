#version 450
layout(location = 0) in vec2 pos_px;
layout(location = 1) in vec2 uv_in;
layout(location = 0) out vec2 uv;
void main() {
    gl_Position = vec4(pos_px / vec2(960.0, 540.0) - vec2(1.0), 0.0, 1.0);
    uv = uv_in;
}
