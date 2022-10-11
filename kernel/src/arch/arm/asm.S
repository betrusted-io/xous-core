/*
 * SPDX-FileCopyrightText: 2022 Foundation Devices, Inc <hello@foundationdevices.com>
 * SPDX-License-Identifier: Apache-2.0
 */

.global _start
_start:
    cpsid       ifa
    b           kmain