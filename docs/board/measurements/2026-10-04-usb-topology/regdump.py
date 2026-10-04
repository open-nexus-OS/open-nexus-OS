#!/usr/bin/env python3
# Read-only MMIO word dump (one-off measurement, nothing is written): mmap /dev/mem and read each
# word with ONE 32-bit load (`c_uint32.from_buffer`) — slicing the mapping issues byte loads.
import ctypes, mmap, os, sys
def dump(base, length, label):
    page = base & ~0xfff
    off = base - page
    size = ((off + length + 0xfff) // 0x1000) * 0x1000
    fd = os.open("/dev/mem", os.O_RDWR | os.O_SYNC)
    try:
        m = mmap.mmap(fd, size, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE, offset=page)
    except Exception as e:
        print(f"{label} 0x{base:x}: mmap failed: {e}"); os.close(fd); return
    line = f"{label} 0x{base:x}+0x{length:x}:"
    for i in range(0, length, 4):
        v = ctypes.c_uint32.from_buffer(m, off + i).value
        if v:
            line += f" {i:x}={v:x}"
    print(line, flush=True)
    del v
    m.close(); os.close(fd)
for spec in sys.argv[1:]:
    label, base, length = spec.split(":")
    dump(int(base, 16), int(length, 16), label)
