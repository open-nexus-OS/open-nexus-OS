/* SPDX-License-Identifier: Apache-2.0 */
/*
 * Copyright 2026 Open Nexus OS Contributors
 *
 * Power-domain ids of the K1 power controller, as the hardware numbers them
 * (measured on the reference board's stock system, 2026-09-22; the mainline
 * binding declares `#power-domain-cells = <1>` on the MPMU/APMU syscons but
 * ships no id header and no driver). Only the domains Open Nexus OS names are
 * listed; each one's register protocol is in the `nexus-soc` table, measured in
 * docs/board/measurements/2026-09-30-power-domains/ (TASK-0245B P3).
 */
#ifndef _DT_BINDINGS_SPACEMIT_K1_PMU_H_
#define _DT_BINDINGS_SPACEMIT_K1_PMU_H_

#define K1_PD_BUS	0	/* always on: SD hosts, USB, EMAC live here */
#define K1_PD_VPU	1
#define K1_PD_GPU	2
#define K1_PD_HDMI	7

#endif
