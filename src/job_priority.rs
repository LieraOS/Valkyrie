// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Priority lowering for long-running job threads. Copy/move/delete
// workers can keep the disk and the CPU busy for minutes; without
// deprioritisation they compete head-on with the compositor, the
// terminal and everything interactive. `background()` drops the
// calling thread to a low I/O class and a positive nice level so
// interactive latency survives a heavy batch, while the job still
// runs at full speed on an otherwise idle system.

/// Apply background scheduling to the current thread: Linux ioprio
/// "best effort" level 7 plus nice +5. Idle-class I/O would risk
/// starving a job the user is actively watching; BE 7 keeps full
/// bandwidth whenever nothing else needs the disk. All failures are
/// ignored on purpose — on kernels or filesystems that cannot honour
/// ioprio the job simply runs unprioritised, never denied.
pub fn background() {
    const IOPRIO_WHO_PROCESS: libc::c_int = 1;
    const IOPRIO_CLASS_SHIFT: libc::c_int = 13;
    const IOPRIO_CLASS_BE: libc::c_int = 2;
    let ioprio = ((IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 7) as libc::c_long;
    unsafe {
        libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0, ioprio);
        libc::setpriority(libc::PRIO_PROCESS as _, 0, 5);
    }
}
