#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
"""(sweep2.py shares its header with sweep.py — TASK-0335 P2: the per-pixel questions.)

The gate scene's frame time through a system's own GLES stack, off-screen.

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


# ---- TASK-0335 P2 (2026-10-21): the three questions cycle 50 left for the vendor stack ----
def irqs():
    for line in open("/proc/interrupts"):
        if line.rstrip().endswith(sys.argv[1]):
            return sum(int(v) for v in line.split()[1:] if v.isdigit())
    return 0

def run(label, fn, frames=int(sys.argv[2]) if len(sys.argv) > 2 else 150):
    for i in range(20):
        fn(i); gl.glFinish()
    b = irqs(); times = []
    for i in range(frames):
        t0 = time.perf_counter_ns(); fn(i); gl.glFinish()
        times.append((time.perf_counter_ns() - t0) // 1000)
    times.sort()
    print("sweep2: %-34s median_us=%d p95_us=%d irqs/frame=%.1f" % (label, times[len(times)//2], times[int(len(times)*0.95)], (irqs()-b)/frames))

print("sweep2: renderer=%r version=%r" % (renderer, version))
# Q1: a blended straight layer and a rounded one at 512² and 1024² — the slope is the ns a pixel.
for side in (() if len(sys.argv) > 3 and sys.argv[3] == "q23" else (512, 1024)):
    gl.glUseProgram(p_tex); gl.glUniform1f(u(p_tex, b"a"), 0.78)
    run("straight blended %dx%d" % (side, side), lambda i, s=side: draw(p_tex, (i % 2, 0, s, s), content, scene, True))
    gl.glUseProgram(p_sdf); gl.glUniform4f(u(p_sdf, b"b"), 0.0, 0.0, float(side), float(side))
    gl.glUniform2f(u(p_sdf, b"k"), 20.0, 0.0); gl.glUniform1f(u(p_sdf, b"a"), 0.78)
    run("rounded blended %dx%d" % (side, side), lambda i, s=side: draw(p_sdf, (i % 2, 0, s, s), content, scene, True))
    gl.glUseProgram(p_tex); gl.glUniform1f(u(p_tex, b"a"), 1.0)
    run("opaque %dx%d" % (side, side), lambda i, s=side: draw(p_tex, (i % 2, 0, s, s), content, scene, False))

# Q2: a LINEAR (strided) texture — a dma-buf image of a linear gbm buffer — against the driver's own layout.
gbm.gbm_bo_create.restype = c_void_p; gbm.gbm_bo_create.argtypes = [c_void_p, c_uint, c_uint, c_uint, c_uint]
gbm.gbm_bo_get_fd.restype = c_int; gbm.gbm_bo_get_fd.argtypes = [c_void_p]
gbm.gbm_bo_get_stride.restype = c_uint; gbm.gbm_bo_get_stride.argtypes = [c_void_p]
egl.eglGetProcAddress.restype = c_void_p; egl.eglGetProcAddress.argtypes = [c_char_p]
CreateImage = ctypes.CFUNCTYPE(c_void_p, c_void_p, c_void_p, c_uint, c_void_p, POINTER(c_int))(egl.eglGetProcAddress(b"eglCreateImageKHR"))
ImageTarget = ctypes.CFUNCTYPE(None, c_uint, c_void_p)(egl.eglGetProcAddress(b"glEGLImageTargetTexture2DOES"))
def linear_texture():
    bo = None
    for flags in ((1 << 4), (1 << 4) | (1 << 2), (1 << 4) | (1 << 0), (1 << 0) | (1 << 2)):
        bo = gbm.gbm_bo_create(dev, W, H, 0x34325241, flags)
        if bo:
            print("sweep2: gbm bo flags=%#x" % flags); break
    if not bo:
        raise RuntimeError("no gbm bo for any flag set")
    fd_ = gbm.gbm_bo_get_fd(bo); stride = gbm.gbm_bo_get_stride(bo)
    attrs = (c_int * 15)(0x3057, W, 0x3056, H, 0x3271, 0x34325241, 0x3272, fd_, 0x3273, 0, 0x3274, stride, 0x3443, 0, 0x3444, 0, 0x3038) if False else \
            (c_int * 13)(0x3057, W, 0x3056, H, 0x3271, 0x34325241, 0x3272, fd_, 0x3273, 0, 0x3274, stride, 0x3038)
    img = CreateImage(dpy, None, 0x3270, None, attrs) or die("no dma-buf image %#x" % egl.eglGetError())
    t = c_uint(); gl.glGenTextures(1, byref(t)); gl.glBindTexture(0x0DE1, t); ImageTarget(0x0DE1, img)
    for pname, v in ((0x2801, 0x2601), (0x2800, 0x2601), (0x2802, 0x812F), (0x2803, 0x812F)):
        gl.glTexParameteri(0x0DE1, pname, v)
    return t.value, stride
try:
    lin, stride = linear_texture()
    lin_fbo = target(lin)
    gl.glUseProgram(p_tex); gl.glUniform1f(u(p_tex, b"a"), 1.0)
    draw(p_tex, (0, 0, W, H), wall, lin_fbo); gl.glFinish()
    print("sweep2: linear texture stride=%d" % stride)
    run("opaque 1920x1080 from LINEAR", lambda i: draw(p_tex, (i % 2, 0, W, H), lin, scene, False))
    run("opaque 1920x1080 from driver layout", lambda i: draw(p_tex, (i % 2, 0, W, H), wall, scene, False))
    gl.glUniform1f(u(p_tex, b"a"), 0.78)
    run("blended 1920x1080 from LINEAR", lambda i: draw(p_tex, (i % 2, 0, W, H), lin, scene, True))
    run("blended 1920x1080 from driver layout", lambda i: draw(p_tex, (i % 2, 0, W, H), wall, scene, True))
except SystemExit:
    raise
except Exception as e:
    print("sweep2: linear texture skipped: %r" % (e,))

# Q3: renders a frame — eight small draws each into its own target against one.
small = [target(texture(None, 64, 64)) for _ in range(8)]
gl.glUseProgram(p_tex); gl.glUniform1f(u(p_tex, b"a"), 1.0)
def one(i): draw(p_tex, (i % 2, 0, 16, 16), wall, small[0])
def eight(i):
    for f in small: draw(p_tex, (i % 2, 0, 16, 16), wall, f)
run("1 render of 16x16", one)
run("8 renders of 16x16 (8 targets)", eight)
def eight_same(i):
    for k in range(8): draw(p_tex, (k, 0, 16, 16), wall, small[0])
run("8 draws of 16x16 (1 target)", eight_same)
