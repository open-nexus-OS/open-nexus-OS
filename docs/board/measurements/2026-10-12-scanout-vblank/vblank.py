#!/usr/bin/env python3
# Read-only measurement of the display controller's frame clock and flip on the stock system:
# (a) the vertical blank's period through the kernel's own vblank wait (no master needed),
# (b) the controller interrupt's rate over the same window, (c) a high-rate sample of the
# desktop channel's plane address, the output line counter, the raw interrupt word and the
# config-ready latch, so a flip's timing against the scan is visible, (d) the interrupt and
# control words. Nothing is written.
import ctypes, fcntl, mmap, os, struct, sys, time
DPU = 0xc0440000; LEN = 0x2a000
RDMA1_ADDR_LO = 0xba0; LINE = 0x18000 + 0xc8; INT_RAW = 0x960; CFG_RDY = 0x56c
def irqs():
    for l in open('/proc/interrupts'):
        if l.rstrip().endswith('DPU_ONLINE'): return sum(int(v) for v in l.split()[1:] if v.isdigit())
# (a) vblank period via DRM_IOCTL_WAIT_VBLANK (relative, 1) on the KMS card
fd = os.open('/dev/dri/card1', os.O_RDWR)
IOC = 0xC018643A
t = []
for i in range(240):
    buf = bytearray(struct.pack('IIqq', 1, 1, 0, 0))
    try:
        fcntl.ioctl(fd, IOC, buf)
    except OSError as e:
        print('vblank ioctl failed:', e); break
    _, seq, sec, usec = struct.unpack('IIqq', buf)
    t.append((seq, sec * 1_000_000 + usec))
if len(t) > 2:
    d = sorted(b[1] - a[1] for a, b in zip(t, t[1:]))
    print('vblank: n=%d median_us=%d min_us=%d max_us=%d seq %d..%d' % (len(d), d[len(d)//2], d[0], d[-1], t[0][0], t[-1][0]))
# (b) interrupt rate
a = irqs(); t0 = time.time(); time.sleep(5); b = irqs()
print('dpu online irq: %.1f per second over %.2fs' % ((b - a) / (time.time() - t0), time.time() - t0))
# (c) high-rate sample
mfd = os.open('/dev/mem', os.O_RDWR | os.O_SYNC)
m = mmap.mmap(mfd, LEN, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE, offset=DPU)
rd = lambda off: ctypes.c_uint32.from_buffer(m, off).value
samples = []; t0 = time.perf_counter_ns(); n = 0
while time.perf_counter_ns() - t0 < 3_000_000_000:
    samples.append((time.perf_counter_ns() - t0, rd(RDMA1_ADDR_LO), rd(LINE), rd(INT_RAW), rd(CFG_RDY))); n += 1
print('samples: %d in 3 s (%.0f per second)' % (n, n / 3))
addrs = sorted({s[1] for s in samples}); print('plane addresses seen:', ['0x%x' % x for x in addrs])
lines = [s[2] & 0xffff for s in samples]; print('line counter min=%d max=%d distinct=%d' % (min(lines), max(lines), len(set(lines))))
raws = sorted({s[3] for s in samples}); print('int raw values:', ['0x%x' % x for x in raws][:12])
cfg = sorted({s[4] for s in samples}); print('cfg-ready values:', ['0x%x' % x for x in cfg])
flips = [(s[0] // 1000, s[1], s[2] & 0xffff, s[3]) for p, s in zip(samples, samples[1:]) if s[1] != p[1]]
print('flips (address changes): %d' % len(flips))
for f in flips[:12]: print('  at %d us addr=0x%x line=%d raw=0x%x' % f)
# vsync bit timing: where in the scan does raw bit0 appear
vs = [s[2] & 0xffff for s in samples if s[3] & 1]
if vs: print('vsync raw bit set at lines: min=%d max=%d n=%d' % (min(vs), max(vs), len(vs)))
# (d) words
def words(base, length, label):
    print(label + ':', ' '.join('%x=%x' % (i, rd(base + i)) for i in range(0, length, 4) if rd(base + i)))
words(0x900, 0xc0, 'int block 0x900')
words(0x500, 0x100, 'ctl 0x500')
words(0xb80, 0x100, 'rdma1 0xb80')
words(0x18000, 0x100, 'outctrl2 0x18000')
