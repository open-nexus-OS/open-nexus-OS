#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
"""The gate scene's frame time through a system's own GLES stack, off-screen.

Scene (1920x1080 RGBA8 target, every frame from scratch, glFinish per frame):
  wall : one full-screen textured quad.
  glass-small: `glass` with the bar and the dock only.
  glass-quarter: `glass`, but the backdrop is ONE quarter-size copy of the frame, blurred once
         (11 taps a pass, the same reach); each layer samples it instead of blurring itself.
  glass: `wall`, then three glass layers — a bar 1920x40 at the top, a dock 800x90 at the
         bottom centre, a window 960x1044 at the right — each: a separable gaussian backdrop
         blur of its rectangle (two passes, 21 bilinear taps, reach 20 px), a soft rounded
         drop shadow, and a translucent textured rounded-rect composited over it.
Prints one line per scene: frames, median / p95 / p99 / max in microseconds, and the GPU's
interrupt count over the run when /proc/interrupts names one (argv[1], default none).
Usage: gate_scene.py [irq-name] [frames]"""
import ctypes, os, sys, time
from ctypes import c_void_p, c_int, c_uint, c_char_p, c_float, POINTER, byref

W, H = 1920, 1080
egl = ctypes.CDLL("libEGL.so.1"); gbm = ctypes.CDLL("libgbm.so.1"); gl = ctypes.CDLL("libGLESv2.so.2")
gbm.gbm_create_device.restype = c_void_p; gbm.gbm_create_device.argtypes = [c_int]
egl.eglGetPlatformDisplay.restype = c_void_p; egl.eglGetPlatformDisplay.argtypes = [c_uint, c_void_p, c_void_p]
egl.eglInitialize.argtypes = [c_void_p, POINTER(c_int), POINTER(c_int)]
egl.eglCreateContext.restype = c_void_p; egl.eglCreateContext.argtypes = [c_void_p, c_void_p, c_void_p, POINTER(c_int)]
egl.eglMakeCurrent.argtypes = [c_void_p, c_void_p, c_void_p, c_void_p]
egl.eglBindAPI.argtypes = [c_uint]
gl.glGetString.restype = c_char_p; gl.glGetString.argtypes = [c_uint]
gl.glGetUniformLocation.argtypes = [c_uint, c_char_p]
gl.glGetAttribLocation.argtypes = [c_uint, c_char_p]
gl.glUniform4f.argtypes = [c_int, c_float, c_float, c_float, c_float]
gl.glUniform2f.argtypes = [c_int, c_float, c_float]
gl.glUniform1f.argtypes = [c_int, c_float]
gl.glClearColor.argtypes = [c_float] * 4
gl.glVertexAttribPointer.argtypes = [c_uint, c_int, c_uint, c_uint, c_int, c_void_p]
gl.glTexImage2D.argtypes = [c_uint, c_int, c_int, c_int, c_int, c_int, c_uint, c_uint, c_void_p]

def die(msg):
    print("gate-scene: FAIL", msg); sys.exit(1)

fd = os.open("/dev/dri/renderD128", os.O_RDWR)
dev = gbm.gbm_create_device(fd) or die("no gbm device")
dpy = egl.eglGetPlatformDisplay(0x31D7, dev, None)
egl.eglInitialize(dpy, byref(c_int()), byref(c_int())) or die("eglInitialize")
egl.eglBindAPI(0x30A0)
attrs = (c_int * 3)(0x3098, 2, 0x3038)
ctx = egl.eglCreateContext(dpy, None, None, attrs) or die("no context %#x" % egl.eglGetError())
egl.eglMakeCurrent(dpy, None, None, ctx) or die("surfaceless make-current %#x" % egl.eglGetError())
renderer = gl.glGetString(0x1F01).decode(); version = gl.glGetString(0x1F02).decode()

VS = b"""attribute vec2 p; uniform vec4 r; varying vec2 uv; varying vec2 px;
void main(){ vec2 q = r.xy + p * r.zw; px = q; uv = q / vec2(1920.0, 1080.0);
 gl_Position = vec4(uv * 2.0 - 1.0, 0.0, 1.0); }"""
FS_TEX = b"""precision mediump float; varying vec2 uv; uniform sampler2D t; uniform float a;
void main(){ gl_FragColor = vec4(texture2D(t, uv).rgb, a); }"""
FS_BLUR = b"""precision mediump float; varying vec2 uv; uniform sampler2D t; uniform vec2 d;
void main(){ vec4 s = vec4(0.0); float n = 0.0;
 for (int i = -10; i <= 10; i++) { float x = float(i); float w = exp(-x * x / 50.0);
  s += texture2D(t, uv + d * x * 2.0) * w; n += w; }
 gl_FragColor = s / n; }"""
FS_BLURQ = b"""precision mediump float; varying vec2 uv; uniform sampler2D t; uniform vec2 d;
void main(){ vec4 s = vec4(0.0); float n = 0.0;
 for (int i = -5; i <= 5; i++) { float x = float(i); float w = exp(-x * x / 12.5);
  s += texture2D(t, uv + d * x) * w; n += w; }
 gl_FragColor = s / n; }"""
FS_DOWN = b"""precision mediump float; varying vec2 uv; uniform sampler2D t;
void main(){ vec2 o = vec2(1.0 / 1920.0, 1.0 / 1080.0);
 gl_FragColor = 0.25 * (texture2D(t, uv + o * vec2(-1.0, -1.0)) + texture2D(t, uv + o * vec2(1.0, -1.0))
  + texture2D(t, uv + o * vec2(-1.0, 1.0)) + texture2D(t, uv + o * vec2(1.0, 1.0))); }"""
FS_SDF = b"""precision mediump float; varying vec2 px; varying vec2 uv; uniform sampler2D t;
uniform vec4 b; uniform vec2 k; uniform float a;
float sd(vec2 q, vec2 h, float r){ vec2 e = abs(q) - h + r; return length(max(e, 0.0)) + min(max(e.x, e.y), 0.0) - r; }
void main(){ float dist = sd(px - (b.xy + b.zw * 0.5), b.zw * 0.5, k.x);
 if (k.y > 0.0) { gl_FragColor = vec4(0.0, 0.0, 0.0, a * (1.0 - smoothstep(-k.y, k.y, dist))); }
 else { gl_FragColor = vec4(texture2D(t, uv).rgb, a * (1.0 - smoothstep(-0.5, 0.5, dist))); } }"""

def shader(kind, src):
    s = gl.glCreateShader(kind); p = c_char_p(src)
    gl.glShaderSource(s, 1, byref(p), None); gl.glCompileShader(s)
    ok = c_int(); gl.glGetShaderiv(s, 0x8B81, byref(ok))
    if not ok.value:
        log = ctypes.create_string_buffer(2048); gl.glGetShaderInfoLog(s, 2048, None, log); die(log.value.decode())
    return s

def program(fs):
    p = gl.glCreateProgram(); gl.glAttachShader(p, shader(0x8B31, VS)); gl.glAttachShader(p, shader(0x8B30, fs))
    gl.glBindAttribLocation(p, 0, b"p"); gl.glLinkProgram(p)
    ok = c_int(); gl.glGetProgramiv(p, 0x8B82, byref(ok)); ok.value or die("link")
    return p

def texture(pixels=None, w=W, h=H):
    t = c_uint(); gl.glGenTextures(1, byref(t)); gl.glBindTexture(0x0DE1, t)
    gl.glTexImage2D(0x0DE1, 0, 0x1908, w, h, 0, 0x1908, 0x1401, pixels)
    for pname, v in ((0x2801, 0x2601), (0x2800, 0x2601), (0x2802, 0x812F), (0x2803, 0x812F)):
        gl.glTexParameteri(0x0DE1, pname, v)
    return t.value

def target(tex):
    f = c_uint(); gl.glGenFramebuffers(1, byref(f)); gl.glBindFramebuffer(0x8D40, f)
    gl.glFramebufferTexture2D(0x8D40, 0x8CE0, 0x0DE1, tex, 0)
    gl.glCheckFramebufferStatus(0x8D40) == 0x8CD5 or die("framebuffer incomplete")
    return f.value

def pattern(seed):
    row = bytearray(W * 4)
    buf = bytearray(W * H * 4)
    for y in range(H):
        for x in range(0, W, 8):
            v = ((x * 7 + y * 13 + seed) ^ (x >> 3) * (y >> 3)) & 255
            row[x * 4:(x + 8) * 4] = bytes((v, (v * 3) & 255, 255 - v, 255)) * 8
        buf[y * W * 4:(y + 1) * W * 4] = row
    return (ctypes.c_ubyte * len(buf)).from_buffer(buf)

wall = texture(pattern(1)); content = texture(pattern(97))
scene_tex = texture(); tmp_tex = texture()
scene = target(scene_tex); tmp = target(tmp_tex)
QW, QH = W // 4, H // 4
q1_tex = texture(None, QW, QH); q2_tex = texture(None, QW, QH)
q1 = target(q1_tex); q2 = target(q2_tex)
p_tex, p_blur, p_sdf = program(FS_TEX), program(FS_BLUR), program(FS_SDF)
p_blurq, p_down = program(FS_BLURQ), program(FS_DOWN)
quad = (c_float * 8)(0, 0, 1, 0, 0, 1, 1, 1)
gl.glEnableVertexAttribArray(0); gl.glVertexAttribPointer(0, 2, 0x1406, 0, 0, quad)
gl.glViewport(0, 0, W, H); gl.glBlendFunc(0x0302, 0x0303)

def u(p, name): return gl.glGetUniformLocation(p, name)

def draw(p, rect, tex, fbo, blend=False):
    gl.glBindFramebuffer(0x8D40, fbo); gl.glBindTexture(0x0DE1, tex)
    (gl.glEnable if blend else gl.glDisable)(0x0BE2)
    gl.glUniform4f(u(p, b"r"), *map(float, rect)); gl.glDrawArrays(0x0005, 0, 4)

LAYERS = ((0, 1040, 1920, 40, 0), (560, 20, 800, 90, 24), (940, 18, 960, 1044, 16))

def frame(glass, phase):
    gl.glUseProgram(p_tex); gl.glUniform1f(u(p_tex, b"a"), 1.0)
    draw(p_tex, (phase % 2, 0, W, H), wall, scene)
    if not glass:
        return
    layers = LAYERS[:2] if glass == "small" else LAYERS
    if glass == "quarter":
        # One quarter-size copy of the whole frame, blurred once; every layer samples it.
        gl.glViewport(0, 0, QW, QH)
        gl.glUseProgram(p_down); draw(p_down, (0, 0, W, H), scene_tex, q1)
        gl.glUseProgram(p_blurq)
        gl.glUniform2f(u(p_blurq, b"d"), 1.0 / QW, 0.0); draw(p_blurq, (0, 0, W, H), q1_tex, q2)
        gl.glUniform2f(u(p_blurq, b"d"), 0.0, 1.0 / QH); draw(p_blurq, (0, 0, W, H), q2_tex, q1)
        gl.glViewport(0, 0, W, H)
    for x, y, w, h, radius in layers:
        if glass == "quarter":
            gl.glUseProgram(p_tex); gl.glUniform1f(u(p_tex, b"a"), 1.0); draw(p_tex, (x, y, w, h), q1_tex, scene)
        else:
            gl.glUseProgram(p_blur)
            gl.glUniform2f(u(p_blur, b"d"), 1.0 / W, 0.0); draw(p_blur, (x, y, w, h), scene_tex, tmp)
            gl.glUniform2f(u(p_blur, b"d"), 0.0, 1.0 / H); draw(p_blur, (x, y, w, h), tmp_tex, scene)
        gl.glUseProgram(p_sdf); gl.glUniform4f(u(p_sdf, b"b"), float(x), float(y - 6), float(w), float(h))
        gl.glUniform2f(u(p_sdf, b"k"), float(radius), 24.0); gl.glUniform1f(u(p_sdf, b"a"), 0.35)
        draw(p_sdf, (x - 32, y - 38, w + 64, h + 64), content, scene, True)
        gl.glUniform4f(u(p_sdf, b"b"), float(x), float(y), float(w), float(h))
        gl.glUniform2f(u(p_sdf, b"k"), float(radius), 0.0); gl.glUniform1f(u(p_sdf, b"a"), 0.78)
        draw(p_sdf, (x, y, w, h), content, scene, True)

def irqs(name):
    if not name:
        return None
    for line in open("/proc/interrupts"):
        if line.rstrip().endswith(name):
            return sum(int(v) for v in line.split()[1:] if v.isdigit())
    return None

irq_name = sys.argv[1] if len(sys.argv) > 1 else ""
frames = int(sys.argv[2]) if len(sys.argv) > 2 else 300
print("gate-scene: renderer=%r version=%r target=%dx%d" % (renderer, version, W, H))
for label, glass in (("wall", False), ("glass", True), ("glass-small", "small"), ("glass-quarter", "quarter")):
    for i in range(30):
        frame(glass, i); gl.glFinish()
    before = irqs(irq_name); times = []
    for i in range(frames):
        t0 = time.perf_counter_ns(); frame(glass, i); gl.glFinish()
        times.append((time.perf_counter_ns() - t0) // 1000)
    after = irqs(irq_name); times.sort()
    pick = lambda q: times[min(len(times) - 1, int(len(times) * q))]
    err = gl.glGetError()
    print("gate-scene: %s frames=%d median_us=%d p95_us=%d p99_us=%d max_us=%d irqs=%s glerror=%#x" % (
        label, frames, pick(0.5), pick(0.95), pick(0.99), times[-1],
        "n/a" if before is None else after - before, err))
