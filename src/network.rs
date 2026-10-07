use std::fs;
use std::path::PathBuf;

fn get_sys_class_net() -> PathBuf {
    if let Ok(mock) = std::env::var("BITRATE_MOCK_SYSFS") {
        PathBuf::from(mock).join("sys/class/net")
    } else {
        PathBuf::from("/sys/class/net")
    }
}

fn get_proc_net_route() -> PathBuf {
    if let Ok(mock) = std::env::var("BITRATE_MOCK_SYSFS") {
        PathBuf::from(mock).join("proc/net/route")
    } else {
        PathBuf::from("/proc/net/route")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InterfaceType {
    Wired = 0,
    Wireless = 1,
    Other = 2,
}

pub fn interface_type(iface: &str) -> InterfaceType {
    if iface.starts_with("en") || iface.starts_with("eth") || iface.starts_with("em") {
        InterfaceType::Wired
    } else if iface.starts_with("wl") {
        InterfaceType::Wireless
    } else {
        InterfaceType::Other
    }
}

pub fn is_docker_or_bridge(iface: &str) -> bool {
    // Check known Docker and container virtual prefixes
    if iface.starts_with("docker")
        || iface.starts_with("veth")
        || iface.starts_with("br-")
        || iface.starts_with("br_")
        || iface.starts_with("virbr")
    {
        return true;
    }

    // Check if sysfs recognizes this interface as a bridge
    if get_sys_class_net().join(iface).join("bridge").exists() {
        return true;
    }

    // Check any bridge interface starting with "br" located in virtual devices
    if iface.starts_with("br") {
        let path = get_sys_class_net().join(iface);
        if fs::read_link(&path)
            .is_ok_and(|target| target.to_string_lossy().contains("/devices/virtual/"))
        {
            return true;
        }
    }

    false
}

pub fn get_default_route_interface() -> Option<String> {
    let content = fs::read_to_string(get_proc_net_route()).ok()?;
    let mut best_iface = None;
    let mut best_metric = u32::MAX;

    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 7 {
            let iface = fields[0];
            let dest = fields[1];
            let flags = u32::from_str_radix(fields[3], 16).unwrap_or(0);
            let metric = fields[6].parse::<u32>().unwrap_or(0);

            // Destination 00000000 with RTF_GATEWAY (0x2)
            if dest == "00000000" && (flags & 0x0002) != 0 && metric < best_metric {
                best_metric = metric;
                best_iface = Some(iface.to_string());
            }
        }
    }

    best_iface
}

pub fn get_network_interfaces(ignore_docker: bool) -> Vec<String> {
    let mut interfaces: Vec<String> = Vec::new();

    let Ok(paths) = fs::read_dir(get_sys_class_net()) else {
        return interfaces;
    };

    for entry in paths.flatten() {
        let Ok(iface) = entry.file_name().into_string() else {
            continue;
        };

        // 1. Skip loopback
        if iface == "lo" {
            continue;
        }

        // 2. Skip Docker, veth, and bridge interfaces if configured
        if ignore_docker && is_docker_or_bridge(&iface) {
            continue;
        }

        let path = entry.path();

        // 3. Check if the interface is 'up'
        let operstate = fs::read_to_string(path.join("operstate")).unwrap_or_default();
        if !operstate.contains("up") {
            continue;
        }

        // 4. Check for carrier (physical connection detected)
        let carrier = fs::read_to_string(path.join("carrier")).unwrap_or_default();
        if carrier.trim() == "1" {
            interfaces.push(iface);
        }
    }

    // Sort interfaces: default route first, then wired, then wifi, then others
    let default_route = get_default_route_interface();
    interfaces.sort_by(|a, b| {
        let a_is_default = default_route.as_deref() == Some(a);
        let b_is_default = default_route.as_deref() == Some(b);
        if a_is_default != b_is_default {
            return b_is_default.cmp(&a_is_default);
        }

        let a_type = interface_type(a);
        let b_type = interface_type(b);
        if a_type != b_type {
            return a_type.cmp(&b_type);
        }

        a.cmp(b)
    });

    interfaces
}

pub fn find_preferred_interface(interfaces: &[String], saved: Option<&str>) -> Option<usize> {
    if interfaces.is_empty() {
        return None;
    }

    // 1. If any wired interface is connected, prefer wired
    let wired_interfaces: Vec<(usize, &String)> = interfaces
        .iter()
        .enumerate()
        .filter(|(_, iface)| interface_type(iface) == InterfaceType::Wired)
        .collect();

    if !wired_interfaces.is_empty() {
        if let Some(saved_name) = saved
            && let Some(&(idx, _)) = wired_interfaces.iter().find(|(_, iface)| *iface == saved_name)
        {
            return Some(idx);
        }
        return Some(wired_interfaces[0].0);
    }

    // 2. If no wired interface is connected, prefer wireless
    let wireless_interfaces: Vec<(usize, &String)> = interfaces
        .iter()
        .enumerate()
        .filter(|(_, iface)| interface_type(iface) == InterfaceType::Wireless)
        .collect();

    if !wireless_interfaces.is_empty() {
        if let Some(saved_name) = saved
            && let Some(&(idx, _)) = wireless_interfaces.iter().find(|(_, iface)| *iface == saved_name)
        {
            return Some(idx);
        }
        return Some(wireless_interfaces[0].0);
    }

    // 3. Fallback to saved preference or first interface
    if let Some(saved_name) = saved
        && let Some(idx) = interfaces.iter().position(|iface| iface == saved_name)
    {
        return Some(idx);
    }

    Some(0)
}

pub fn get_received_bytes(network_interface: &str) -> Option<u64> {
    let rx_bytes_path = get_sys_class_net().join(network_interface).join("statistics/rx_bytes");
    if let Ok(received_bytes_str) = fs::read_to_string(rx_bytes_path) {
        return received_bytes_str.trim_end().parse::<u64>().ok();
    }
    None
}

pub fn get_sent_bytes(network_interface: &str) -> Option<u64> {
    let tx_bytes_path = get_sys_class_net().join(network_interface).join("statistics/tx_bytes");
    if let Ok(sent_bytes_str) = fs::read_to_string(tx_bytes_path) {
        return sent_bytes_str.trim_end().parse::<u64>().ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_docker_or_bridge() {
        assert!(is_docker_or_bridge("docker0"));
        assert!(is_docker_or_bridge("docker1"));
        assert!(is_docker_or_bridge("veth1234567"));
        assert!(is_docker_or_bridge("veth8bc1e04"));
        assert!(is_docker_or_bridge("br-e02939798c38"));
        assert!(is_docker_or_bridge("br-68e8fae61392"));
        assert!(is_docker_or_bridge("virbr0"));

        assert!(!is_docker_or_bridge("eno1"));
        assert!(!is_docker_or_bridge("eth0"));
        assert!(!is_docker_or_bridge("enp3s0"));
        assert!(!is_docker_or_bridge("wlo1"));
        assert!(!is_docker_or_bridge("wlan0"));
    }

    #[test]
    fn test_interface_type() {
        assert_eq!(interface_type("eno1"), InterfaceType::Wired);
        assert_eq!(interface_type("eth0"), InterfaceType::Wired);
        assert_eq!(interface_type("enp3s0"), InterfaceType::Wired);
        assert_eq!(interface_type("em1"), InterfaceType::Wired);

        assert_eq!(interface_type("wlo1"), InterfaceType::Wireless);
        assert_eq!(interface_type("wlan0"), InterfaceType::Wireless);
        assert_eq!(interface_type("wlp2s0"), InterfaceType::Wireless);

        assert_eq!(interface_type("wwan0"), InterfaceType::Other);
        assert_eq!(interface_type("tun0"), InterfaceType::Other);
    }

    #[test]
    fn test_find_preferred_interface() {
        let empty: Vec<String> = Vec::new();
        assert_eq!(find_preferred_interface(&empty, None), None);

        // Wired and wireless both present: wired is selected
        let mixed = vec!["eno1".to_string(), "wlo1".to_string()];
        assert_eq!(find_preferred_interface(&mixed, None), Some(0));

        // Even if wireless was saved, wired takes precedence when plugged in
        assert_eq!(find_preferred_interface(&mixed, Some("wlo1")), Some(0));

        // Only wireless present: wireless is selected
        let wifi_only = vec!["wlo1".to_string()];
        assert_eq!(find_preferred_interface(&wifi_only, None), Some(0));
        assert_eq!(find_preferred_interface(&wifi_only, Some("wlo1")), Some(0));

        // Multiple wired interfaces: saved wired interface is respected
        let multi_wired = vec!["eno1".to_string(), "enp5s0".to_string(), "wlo1".to_string()];
        assert_eq!(find_preferred_interface(&multi_wired, Some("enp5s0")), Some(1));
    }

    #[test]
    fn test_get_network_interfaces_on_host() {
        let filtered = get_network_interfaces(true);
        // None of the filtered interfaces should be docker, veth, or br
        for iface in &filtered {
            assert!(!is_docker_or_bridge(iface), "Found docker/bridge interface: {}", iface);
        }
    }

    #[test]
    fn test_real_life_simulation_scenario() {
        let temp_dir = std::env::temp_dir().join("bitrate_test_simulation");
        let _ = fs::remove_dir_all(&temp_dir);
        let net_dir = temp_dir.join("sys/class/net");
        let proc_dir = temp_dir.join("proc/net");
        fs::create_dir_all(&net_dir).unwrap();
        fs::create_dir_all(&proc_dir).unwrap();

        let set_interface = |name: &str, up: bool, carrier: bool, is_bridge: bool, rx: u64, tx: u64| {
            let iface_dir = net_dir.join(name);
            let stats_dir = iface_dir.join("statistics");
            fs::create_dir_all(&stats_dir).unwrap();
            fs::write(iface_dir.join("operstate"), if up { "up\n" } else { "down\n" }).unwrap();
            fs::write(iface_dir.join("carrier"), if carrier { "1\n" } else { "0\n" }).unwrap();
            fs::write(stats_dir.join("rx_bytes"), format!("{}\n", rx)).unwrap();
            fs::write(stats_dir.join("tx_bytes"), format!("{}\n", tx)).unwrap();
            if is_bridge {
                fs::create_dir_all(iface_dir.join("bridge")).unwrap();
            } else {
                let _ = fs::remove_dir_all(iface_dir.join("bridge"));
            }
        };

        let set_routes = |routes: &[(&str, &str, u32)]| {
            let mut content = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n".to_string();
            for (iface, dest, metric) in routes {
                content.push_str(&format!("{}\t{}\t0101A8C0\t0003\t0\t0\t{}\t00000000\t0\t0\t0\n", iface, dest, metric));
            }
            fs::write(proc_dir.join("route"), content).unwrap();
        };

        // Point BITRATE_MOCK_SYSFS to temporary mock directory
        unsafe {
            std::env::set_var("BITRATE_MOCK_SYSFS", &temp_dir);
        }

        println!("\n=== SIMULATION STEP 1: Wi-Fi on, Ethernet unplugged, Docker running ===");
        set_interface("wlo1", true, true, false, 1_000_000, 200_000);
        set_interface("eno1", false, false, false, 0, 0);
        set_interface("docker0", true, true, true, 50_000, 10_000);
        set_interface("br-e02939798c38", true, true, true, 80_000, 15_000);
        set_interface("veth8bc1e04", true, true, false, 40_000, 8_000);
        set_routes(&[("wlo1", "00000000", 600)]);

        let interfaces_step1 = get_network_interfaces(true);
        println!("  Detected interfaces (Docker ignored): {:?}", interfaces_step1);
        assert_eq!(interfaces_step1, vec!["wlo1".to_string()]);

        let pref_idx_1 = find_preferred_interface(&interfaces_step1, None).unwrap();
        let selected_step1 = &interfaces_step1[pref_idx_1];
        println!("  Selected interface: {} (Wi-Fi is working)", selected_step1);
        assert_eq!(selected_step1, "wlo1");

        let rx_1 = get_received_bytes(selected_step1).unwrap();
        let tx_1 = get_sent_bytes(selected_step1).unwrap();
        println!("  Read bandwidth stats: rx={} bytes, tx={} bytes", rx_1, tx_1);
        assert_eq!(rx_1, 1_000_000);
        assert_eq!(tx_1, 200_000);

        println!("\n=== SIMULATION STEP 2: Ethernet cable is plugged in ===");
        set_interface("eno1", true, true, false, 5_000_000, 1_000_000);
        set_routes(&[("eno1", "00000000", 100), ("wlo1", "00000000", 600)]);

        let interfaces_step2 = get_network_interfaces(true);
        println!("  Detected interfaces (Docker ignored): {:?}", interfaces_step2);
        assert_eq!(interfaces_step2, vec!["eno1".to_string(), "wlo1".to_string()]);

        let pref_idx_2 = find_preferred_interface(&interfaces_step2, None).unwrap();
        let selected_step2 = &interfaces_step2[pref_idx_2];
        println!("  Selected interface: {} (Wired Ethernet takes priority)", selected_step2);
        assert_eq!(selected_step2, "eno1");

        let rx_2 = get_received_bytes(selected_step2).unwrap();
        let tx_2 = get_sent_bytes(selected_step2).unwrap();
        println!("  Read bandwidth stats: rx={} bytes, tx={} bytes", rx_2, tx_2);
        assert_eq!(rx_2, 5_000_000);
        assert_eq!(tx_2, 1_000_000);

        println!("\n=== SIMULATION STEP 3: Ethernet cable is unplugged ===");
        set_interface("eno1", false, false, false, 5_000_000, 1_000_000);
        set_routes(&[("wlo1", "00000000", 600)]);

        let interfaces_step3 = get_network_interfaces(true);
        println!("  Detected interfaces (Docker ignored): {:?}", interfaces_step3);
        assert_eq!(interfaces_step3, vec!["wlo1".to_string()]);

        let pref_idx_3 = find_preferred_interface(&interfaces_step3, None).unwrap();
        let selected_step3 = &interfaces_step3[pref_idx_3];
        println!("  Selected interface: {} (Automatically fell back to Wi-Fi)", selected_step3);
        assert_eq!(selected_step3, "wlo1");

        println!("\n=== SIMULATION STEP 4: User disables Docker filter in settings ===");
        let interfaces_step4 = get_network_interfaces(false);
        println!("  Detected interfaces (Docker included): {:?}", interfaces_step4);
        assert!(interfaces_step4.contains(&"docker0".to_string()));
        assert!(interfaces_step4.contains(&"br-e02939798c38".to_string()));
        assert!(interfaces_step4.contains(&"veth8bc1e04".to_string()));
        assert!(interfaces_step4.contains(&"wlo1".to_string()));

        let pref_idx_4 = find_preferred_interface(&interfaces_step4, None).unwrap();
        let selected_step4 = &interfaces_step4[pref_idx_4];
        println!("  Selected interface: {} (Physical Wi-Fi still prioritized over Docker bridges)", selected_step4);
        assert_eq!(selected_step4, "wlo1");

        println!("\n=== SIMULATION STEP 5: User re-enables Docker filter in settings ===");
        let interfaces_step5 = get_network_interfaces(true);
        println!("  Detected interfaces (Docker ignored): {:?}", interfaces_step5);
        assert_eq!(interfaces_step5, vec!["wlo1".to_string()]);
        assert!(!interfaces_step5.contains(&"docker0".to_string()));
        assert!(!interfaces_step5.contains(&"br-e02939798c38".to_string()));
        assert!(!interfaces_step5.contains(&"veth8bc1e04".to_string()));

        println!("\n=== ALL SIMULATION STEPS VERIFIED SUCCESSFULLY ===");

        // Clean up mock environment variable and temp directory
        unsafe {
            std::env::remove_var("BITRATE_MOCK_SYSFS");
        }
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
