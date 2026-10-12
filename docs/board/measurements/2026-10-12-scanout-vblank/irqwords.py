#!/usr/bin/env python3
# Read-only: which words of the controller's interrupt block pulse per frame (set by hardware,
# cleared by the stock driver), and at which scan line — the vertical-blank source and its
# status/clear layout, measured instead of read from a header.
import ctypes, mmap, os, time
DPU = 0xc0440000; LEN = 0x2a000; LINE = 0x18000 + 0xc8
mfd = os.open('/dev/mem', os.O_RDWR | os.O_SYNC)
m = mmap.mmap(mfd, LEN, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE, offset=DPU)
rd = lambda off: ctypes.c_uint32.from_buffer(m, off).value
words = list(range(0x900, 0x9c0, 4))
seen = {w: {} for w in words}; trans = {w: 0 for w in words}; last = {w: rd(w) for w in words}
lines_at = {w: [] for w in words}
t0 = time.perf_counter_ns(); n = 0
while time.perf_counter_ns() - t0 < 4_000_000_000:
    line = rd(LINE) & 0xffff
    for w in words:
        v = rd(w)
        if v != last[w]:
            trans[w] += 1
            if len(lines_at[w]) < 40: lines_at[w].append((v, line))
            last[w] = v
        seen[w][v] = seen[w].get(v, 0) + 1
    n += 1
print('passes', n)
for w in words:
    if trans[w] or len(seen[w]) > 1:
        vals = sorted(seen[w].items(), key=lambda kv: -kv[1])[:6]
        print('0x%x: transitions=%d values=%s first=%s' % (w, trans[w], ['%x:%d' % kv for kv in vals], ['%x@%d' % x for x in lines_at[w][:10]]))
print('static:', ' '.join('%x=%x' % (w, rd(w)) for w in words if not trans[w] and rd(w)))
