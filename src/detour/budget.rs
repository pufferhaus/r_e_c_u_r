//! Detour ring byte-budget resolution. Per-target defaults + 50% free-RAM ceiling.

use tracing::info;

pub fn default_budget_mb_for_build() -> u64 {
    #[cfg(feature = "pi3")]
    {
        return 128;
    }
    #[cfg(feature = "pi5")]
    {
        return 256;
    }
    #[cfg(not(any(feature = "pi3", feature = "pi5")))]
    {
        512
    }
}

pub fn resolved_budget_bytes(config_mb: Option<u64>) -> usize {
    let free_mb = read_free_ram_mb();
    let bytes = resolved_budget_bytes_for_test(config_mb, free_mb);
    info!(
        "detour: ring budget = {} MB (requested {:?}, free RAM = {} MB)",
        bytes / (1024 * 1024),
        config_mb,
        free_mb,
    );
    bytes
}

pub fn resolved_budget_bytes_for_test(config_mb: Option<u64>, free_ram_mb: u64) -> usize {
    let requested = config_mb.unwrap_or_else(default_budget_mb_for_build);
    let ceiling = free_ram_mb / 2;
    let chosen = requested.min(ceiling);
    (chosen as usize) * 1024 * 1024
}

fn read_free_ram_mb() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.available_memory() / (1024 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_budget_is_compiled_in_per_target() {
        let mb = default_budget_mb_for_build();
        assert!(mb == 128 || mb == 256 || mb == 512, "got {mb}");
    }

    #[test]
    fn resolved_budget_clamps_to_free_ram_ceiling() {
        let bytes = resolved_budget_bytes_for_test(Some(256), 200);
        assert_eq!(bytes, 100 * 1024 * 1024);
    }

    #[test]
    fn resolved_budget_uses_request_when_under_ceiling() {
        let bytes = resolved_budget_bytes_for_test(Some(64), 1024);
        assert_eq!(bytes, 64 * 1024 * 1024);
    }

    #[test]
    fn none_request_uses_default_for_build() {
        let default_mb = default_budget_mb_for_build();
        let bytes = resolved_budget_bytes_for_test(None, 10_000);
        assert_eq!(bytes, default_mb as usize * 1024 * 1024);
    }
}
