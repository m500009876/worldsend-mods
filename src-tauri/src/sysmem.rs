// Detects how much RAM the player's PC actually has, so the launcher can
// suggest a sensible default allocation instead of a hardcoded number that
// might be more than the whole machine has (or too little for a good PC).

use sysinfo::System;

/// Total physical RAM installed on this machine, in whole GB (rounded).
/// Returns 0 if it couldn't be determined for some reason.
pub fn total_ram_gb() -> u32 {
    let mut sys = System::new();
    sys.refresh_memory();
    let total_bytes = sys.total_memory(); // bytes, on modern sysinfo versions
    if total_bytes == 0 {
        return 0;
    }
    ((total_bytes as f64) / 1024.0 / 1024.0 / 1024.0).round() as u32
}

/// Recommended RAM allocation for Minecraft given total system RAM.
/// Leaves headroom for the OS and other apps, and doesn't go overboard
/// even on machines with a lot of RAM.
pub fn recommended_ram_gb(total_gb: u32) -> u32 {
    if total_gb == 0 {
        return 4; // unknown system RAM — safe, conservative fallback
    }
    let half = total_gb / 2;
    // Always leave at least 2GB for the OS/background apps.
    let cap = total_gb.saturating_sub(2).max(2);
    half.clamp(2, cap).min(12)
}
