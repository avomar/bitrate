use std::{fs, path::PathBuf};

/// Paths are injectable so tests never depend on the host's network state.
struct NetworkPaths {
    interfaces: PathBuf,
    routes: PathBuf,
}

impl NetworkPaths {
    fn system() -> Self {
        let root = std::env::var_os("BITRATE_MOCK_SYSFS")
            .map_or_else(|| PathBuf::from("/"), PathBuf::from);
        Self {
            interfaces: root.join("sys/class/net"),
            routes: root.join("proc/net/route"),
        }
    }

    fn is_container_or_bridge(&self, name: &str) -> bool {
        ["docker", "veth", "br-", "br_", "virbr"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
            || self.interfaces.join(name).join("bridge").exists()
    }

    fn connected_interfaces(&self, ignore_docker: bool) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.interfaces) else {
            return Vec::new();
        };
        let mut interfaces = Vec::new();
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if name == "lo" || (ignore_docker && self.is_container_or_bridge(&name)) {
                continue;
            }
            let path = entry.path();
            let state = fs::read_to_string(path.join("operstate")).unwrap_or_default();
            let carrier = fs::read_to_string(path.join("carrier")).unwrap_or_default();
            if state.trim() == "up" && carrier.trim() == "1" {
                interfaces.push(name);
            }
        }

        let routes = fs::read_to_string(&self.routes).unwrap_or_default();
        let default = default_route_interface(&routes, &interfaces);
        // Stable ordering also gives a predictable fallback without a default route.
        interfaces.sort_by_cached_key(|name| {
            (
                default.as_deref() != Some(name.as_str()),
                interface_priority(name),
                name.clone(),
            )
        });
        interfaces
    }

    fn counter(&self, interface: &str, counter: &str) -> Option<u64> {
        fs::read_to_string(
            self.interfaces
                .join(interface)
                .join("statistics")
                .join(counter),
        )
        .ok()?
        .trim()
        .parse()
        .ok()
    }
}

fn interface_priority(name: &str) -> u8 {
    if ["en", "eth", "em"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        0
    } else if name.starts_with("wl") {
        1
    } else {
        2
    }
}

/// Pick the lowest-metric active IPv4 default route among visible interfaces.
fn default_route_interface(routes: &str, interfaces: &[String]) -> Option<String> {
    routes
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 8 || fields[1] != "00000000" || fields[7] != "00000000" {
                return None;
            }
            let flags = u32::from_str_radix(fields[3], 16).ok()?;
            let metric = fields[6].parse::<u32>().ok()?;
            // RTF_UP is required; a default route need not have a gateway.
            if flags & 1 == 0 || !interfaces.iter().any(|name| name == fields[0]) {
                return None;
            }
            Some((metric, fields[0]))
        })
        .min()
        .map(|(_, name)| name.to_owned())
}

pub fn get_network_interfaces(ignore_docker: bool) -> Vec<String> {
    NetworkPaths::system().connected_interfaces(ignore_docker)
}

/// Respect an explicit choice while available, otherwise use discovery's first choice.
pub fn find_preferred_interface(interfaces: &[String], saved: Option<&str>) -> Option<usize> {
    saved
        .and_then(|name| interfaces.iter().position(|interface| interface == name))
        .or_else(|| (!interfaces.is_empty()).then_some(0))
}

pub fn get_received_bytes(interface: &str) -> Option<u64> {
    NetworkPaths::system().counter(interface, "rx_bytes")
}

pub fn get_sent_bytes(interface: &str) -> Option<u64> {
    NetworkPaths::system().counter(interface, "tx_bytes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture {
        root: PathBuf,
        network: NetworkPaths,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "bitrate-network-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let network = NetworkPaths {
                interfaces: root.join("net"),
                routes: root.join("route"),
            };
            fs::create_dir(&network.interfaces).unwrap();
            Self { root, network }
        }

        fn interface(&self, name: &str, state: &str, carrier: bool) {
            let path = self.network.interfaces.join(name);
            fs::create_dir_all(path.join("statistics")).unwrap();
            fs::write(path.join("operstate"), state).unwrap();
            fs::write(path.join("carrier"), if carrier { "1\n" } else { "0\n" }).unwrap();
            fs::write(path.join("statistics/rx_bytes"), "1000000\n").unwrap();
            fs::write(path.join("statistics/tx_bytes"), "500000\n").unwrap();
        }

        fn routes(&self, routes: &[(&str, u32)]) {
            let mut contents =
                "Iface Destination Gateway Flags RefCnt Use Metric Mask\n".to_owned();
            for (name, metric) in routes {
                contents.push_str(&format!(
                    "{name} 00000000 0101A8C0 0003 0 0 {metric} 00000000\n"
                ));
            }
            fs::write(&self.network.routes, contents).unwrap();
        }

        fn selected(&self, ignore_docker: bool, saved: Option<&str>) -> Option<String> {
            let interfaces = self.network.connected_interfaces(ignore_docker);
            find_preferred_interface(&interfaces, saved).map(|index| interfaces[index].clone())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn filters_containers_and_bridges_but_keeps_physical_interfaces() {
        let fixture = Fixture::new();
        for name in [
            "lo",
            "docker0",
            "veth123",
            "br-test",
            "br_test",
            "virbr0",
            "custom-bridge",
            "eno1",
            "wlo1",
        ] {
            fixture.interface(name, "up\n", true);
        }
        fs::create_dir(fixture.network.interfaces.join("custom-bridge/bridge")).unwrap();
        assert_eq!(fixture.network.connected_interfaces(true), ["eno1", "wlo1"]);
        let unfiltered = fixture.network.connected_interfaces(false);
        assert_eq!(unfiltered.len(), 8);
        assert!(!unfiltered.iter().any(|name| name == "lo"));
    }

    #[test]
    fn ignores_disconnected_interfaces_and_missing_sysfs() {
        let fixture = Fixture::new();
        fixture.interface("eno1", "up", false);
        fixture.interface("wlo1", "down", true);
        assert!(fixture.network.connected_interfaces(false).is_empty());
        fs::remove_dir_all(&fixture.network.interfaces).unwrap();
        assert!(fixture.network.connected_interfaces(false).is_empty());
    }

    #[test]
    fn follows_route_changes_and_falls_back_after_disconnect() {
        let fixture = Fixture::new();
        fixture.interface("wlo1", "up", true);
        fixture.interface("eno1", "down", false);
        fixture.routes(&[("wlo1", 600)]);
        assert_eq!(fixture.selected(true, None).as_deref(), Some("wlo1"));

        fixture.interface("eno1", "up", true);
        fixture.routes(&[("wlo1", 600), ("eno1", 100)]);
        assert_eq!(fixture.selected(true, None).as_deref(), Some("eno1"));
        assert_eq!(
            fixture.selected(true, Some("wlo1")).as_deref(),
            Some("wlo1")
        );

        fixture.routes(&[("wlo1", 100), ("eno1", 600)]);
        assert_eq!(fixture.selected(true, None).as_deref(), Some("wlo1"));
        fixture.interface("eno1", "down", false);
        assert_eq!(
            fixture.selected(true, Some("eno1")).as_deref(),
            Some("wlo1")
        );
    }

    #[test]
    fn preserves_selection_by_name_when_filter_changes_indices() {
        let fixture = Fixture::new();
        fixture.interface("docker0", "up", true);
        fixture.interface("eno1", "up", true);
        fixture.interface("wlo1", "up", true);
        fixture.routes(&[("docker0", 10), ("wlo1", 100), ("eno1", 600)]);
        assert_eq!(
            fixture.network.connected_interfaces(false),
            ["docker0", "eno1", "wlo1"]
        );
        assert_eq!(fixture.network.connected_interfaces(true), ["wlo1", "eno1"]);
        for ignore in [false, true, false] {
            assert_eq!(
                fixture.selected(ignore, Some("wlo1")).as_deref(),
                Some("wlo1")
            );
        }
        assert_eq!(
            fixture.selected(true, Some("docker0")).as_deref(),
            Some("wlo1")
        );
        assert_eq!(
            fixture.selected(false, Some("docker0")).as_deref(),
            Some("docker0")
        );
    }

    #[test]
    fn route_parser_rejects_invalid_routes_and_accepts_direct_defaults() {
        let interfaces = vec!["eno1".to_owned(), "wlo1".to_owned()];
        let routes = "Iface Destination Gateway Flags RefCnt Use Metric Mask
missing 00000000 00000000 0001 0 0 0 00000000
wlo1 00000000 00000000 0000 0 0 1 00000000
wlo1 00000000 00000000 0001 0 0 bad 00000000
wlo1 00000000 00000000 0001 0 0 2 00FFFFFF
malformed
wlo1 00000000 00000000 0001 0 0 100 00000000
eno1 00000000 00000000 0003 0 0 600 00000000";
        assert_eq!(
            default_route_interface(routes, &interfaces).as_deref(),
            Some("wlo1")
        );
    }

    #[test]
    fn falls_back_predictably_without_routes() {
        let fixture = Fixture::new();
        for name in ["wlo1", "eno2", "eno1"] {
            fixture.interface(name, "up", true);
        }
        assert_eq!(
            fixture.network.connected_interfaces(true),
            ["eno1", "eno2", "wlo1"]
        );
        assert_eq!(
            fixture.selected(true, Some("missing")).as_deref(),
            Some("eno1")
        );
        assert_eq!(find_preferred_interface(&[], None), None);
    }

    #[test]
    fn reads_counters_and_handles_missing_or_invalid_values() {
        let fixture = Fixture::new();
        fixture.interface("eno1", "up", true);
        assert_eq!(fixture.network.counter("eno1", "rx_bytes"), Some(1_000_000));
        assert_eq!(fixture.network.counter("eno1", "tx_bytes"), Some(500_000));
        assert_eq!(fixture.network.counter("missing", "rx_bytes"), None);
        fs::write(
            fixture.network.interfaces.join("eno1/statistics/rx_bytes"),
            "invalid",
        )
        .unwrap();
        assert_eq!(fixture.network.counter("eno1", "rx_bytes"), None);
    }
}
