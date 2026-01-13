#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Minimal PCAP decoder for OS2VM debugging (no external deps).
# Focus: Ethernet + ARP + IPv4 + TCP/UDP summary.
#
# Usage:
#   python3 tools/pcap_summary.py os2vm-A.pcap --tcp-ports 34567,34568 --udp-ports 37020
#
import argparse
import struct
from dataclasses import dataclass
from typing import Iterator, Optional, Tuple


def mac_str(b: bytes) -> str:
    return ":".join(f"{x:02x}" for x in b)


def ip_str(b: bytes) -> str:
    return ".".join(str(x) for x in b)


@dataclass
class PcapPkt:
    ts_sec: int
    ts_usec: int
    data: bytes


def iter_pcap(path: str) -> Iterator[PcapPkt]:
    with open(path, "rb") as f:
        gh = f.read(24)
        if len(gh) != 24:
            raise SystemExit("pcap: short global header")
        magic = gh[:4]
        if magic == b"\xd4\xc3\xb2\xa1":
            endian = "<"
        elif magic == b"\xa1\xb2\xc3\xd4":
            endian = ">"
        else:
            raise SystemExit(f"pcap: unsupported magic {magic!r} (only classic pcap)")
        _, _, _, _, _, _, network = struct.unpack(endian + "IHHIIII", gh)
        if network != 1:
            raise SystemExit(f"pcap: unsupported linktype {network} (expected Ethernet=1)")
        ph_fmt = endian + "IIII"
        while True:
            ph = f.read(16)
            if not ph:
                break
            if len(ph) != 16:
                raise SystemExit("pcap: short packet header")
            ts_sec, ts_usec, incl_len, _orig_len = struct.unpack(ph_fmt, ph)
            data = f.read(incl_len)
            if len(data) != incl_len:
                raise SystemExit("pcap: short packet data")
            yield PcapPkt(ts_sec, ts_usec, data)


def parse_eth(frame: bytes) -> Optional[Tuple[bytes, bytes, int, bytes]]:
    if len(frame) < 14:
        return None
    dst = frame[0:6]
    src = frame[6:12]
    ethertype = struct.unpack("!H", frame[12:14])[0]
    payload = frame[14:]
    return dst, src, ethertype, payload


def parse_arp(p: bytes) -> Optional[Tuple[int, int, bytes, bytes, bytes, bytes]]:
    if len(p) < 28:
        return None
    htype, ptype, hlen, plen, oper = struct.unpack("!HHBBH", p[:8])
    if htype != 1 or ptype != 0x0800 or hlen != 6 or plen != 4:
        return None
    sha = p[8:14]
    spa = p[14:18]
    tha = p[18:24]
    tpa = p[24:28]
    return oper, htype, sha, spa, tha, tpa


def parse_ipv4(p: bytes) -> Optional[Tuple[int, bytes, bytes, bytes]]:
    if len(p) < 20:
        return None
    ver_ihl = p[0]
    ver = ver_ihl >> 4
    ihl = (ver_ihl & 0x0F) * 4
    if ver != 4 or ihl < 20 or len(p) < ihl:
        return None
    proto = p[9]
    src = p[12:16]
    dst = p[16:20]
    return proto, src, dst, p[ihl:]


def parse_udp(p: bytes) -> Optional[Tuple[int, int]]:
    if len(p) < 8:
        return None
    sport, dport = struct.unpack("!HH", p[:4])
    return sport, dport


def parse_tcp(p: bytes) -> Optional[Tuple[int, int, int]]:
    if len(p) < 20:
        return None
    sport, dport = struct.unpack("!HH", p[:4])
    off = (p[12] >> 4) * 4
    if off < 20 or len(p) < off:
        return None
    flags = p[13]
    return sport, dport, flags


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("pcap")
    ap.add_argument("--tcp-ports", default="", help="comma-separated TCP ports to highlight")
    ap.add_argument("--udp-ports", default="", help="comma-separated UDP ports to highlight")
    ap.add_argument("--limit", type=int, default=120, help="max lines to print")
    args = ap.parse_args()

    tcp_ports = {int(x) for x in args.tcp_ports.split(",") if x.strip()} if args.tcp_ports else set()
    udp_ports = {int(x) for x in args.udp_ports.split(",") if x.strip()} if args.udp_ports else set()

    counts = {
        "eth": 0,
        "arp_req": 0,
        "arp_rep": 0,
        "ipv4": 0,
        "udp": 0,
        "tcp": 0,
        "tcp_syn": 0,
        "tcp_synack": 0,
        "tcp_rst": 0,
    }

    printed = 0
    for pkt in iter_pcap(args.pcap):
        eth = parse_eth(pkt.data)
        if not eth:
            continue
        counts["eth"] += 1
        _dst, src, etype, pay = eth
        if etype == 0x0806:
            a = parse_arp(pay)
            if not a:
                continue
            oper, _ht, sha, spa, _tha, tpa = a
            if oper == 1:
                counts["arp_req"] += 1
                if printed < args.limit:
                    print(f"ARP who-has {ip_str(tpa)} tell {ip_str(spa)} ({mac_str(sha)})")
                    printed += 1
            elif oper == 2:
                counts["arp_rep"] += 1
                if printed < args.limit:
                    print(f"ARP reply {ip_str(spa)} is-at {mac_str(sha)}")
                    printed += 1
            continue

        if etype != 0x0800:
            continue
        ip = parse_ipv4(pay)
        if not ip:
            continue
        counts["ipv4"] += 1
        proto, src_ip, dst_ip, l4 = ip
        if proto == 17:
            u = parse_udp(l4)
            if not u:
                continue
            counts["udp"] += 1
            sport, dport = u
            if udp_ports and (sport not in udp_ports and dport not in udp_ports):
                continue
            if printed < args.limit:
                print(f"UDP {ip_str(src_ip)}:{sport} -> {ip_str(dst_ip)}:{dport} len={len(l4)}")
                printed += 1
        elif proto == 6:
            t = parse_tcp(l4)
            if not t:
                continue
            counts["tcp"] += 1
            sport, dport, flags = t
            syn = bool(flags & 0x02)
            ack = bool(flags & 0x10)
            rst = bool(flags & 0x04)
            if syn and not ack:
                counts["tcp_syn"] += 1
            if syn and ack:
                counts["tcp_synack"] += 1
            if rst:
                counts["tcp_rst"] += 1
            if tcp_ports and (sport not in tcp_ports and dport not in tcp_ports):
                continue
            if printed < args.limit:
                f = []
                if syn:
                    f.append("SYN")
                if ack:
                    f.append("ACK")
                if rst:
                    f.append("RST")
                fl = ",".join(f) if f else f"flags=0x{flags:02x}"
                print(f"TCP {ip_str(src_ip)}:{sport} -> {ip_str(dst_ip)}:{dport} {fl}")
                printed += 1

    print("---")
    for k in ["eth", "arp_req", "arp_rep", "ipv4", "udp", "tcp", "tcp_syn", "tcp_synack", "tcp_rst"]:
        print(f"{k}: {counts[k]}")


if __name__ == "__main__":
    main()
