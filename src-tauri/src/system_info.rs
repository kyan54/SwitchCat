use crate::config::AppConfig;
use if_addrs::{get_if_addrs, IfAddr};
use serde::Serialize;
use std::{
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    process::Command,
};

#[derive(Debug, Clone, Default, Serialize)]
pub struct NetworkSnapshot {
    pub ipv4_addresses: Vec<String>,
    pub gateway: Option<String>,
    pub ssid: Option<String>,
}

pub fn snapshot() -> NetworkSnapshot {
    NetworkSnapshot {
        ipv4_addresses: local_ipv4_addresses(),
        gateway: default_gateway(),
        ssid: current_ssid(),
    }
}

pub fn best_local_ip_for(host: &str, port: u16) -> Option<String> {
    let destination = format!("{host}:{port}");
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(destination).ok()?;
    match socket.local_addr().ok()? {
        SocketAddr::V4(address) if !address.ip().is_loopback() => Some(address.ip().to_string()),
        _ => None,
    }
}

pub fn detect_profile(config: &AppConfig, network: &NetworkSnapshot) -> Option<String> {
    let mut matches: Vec<(i32, String)> = Vec::new();

    for (id, profile) in config.profiles.iter().filter(|(_, profile)| profile.enabled) {
        let mut score = 0;
        let mut has_rule = false;

        if !profile.detect.ssids.is_empty() {
            has_rule = true;
            if let Some(ssid) = &network.ssid {
                if profile
                    .detect
                    .ssids
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(ssid))
                {
                    score += 100;
                }
            }
        }

        if !profile.detect.gateways.is_empty() {
            has_rule = true;
            if let Some(gateway) = &network.gateway {
                if profile.detect.gateways.iter().any(|candidate| candidate == gateway) {
                    score += 50;
                }
            }
        }

        if !profile.detect.local_cidrs.is_empty() {
            has_rule = true;
            if network.ipv4_addresses.iter().any(|address| {
                profile
                    .detect
                    .local_cidrs
                    .iter()
                    .any(|cidr| ipv4_in_cidr(address, cidr))
            }) {
                score += 10;
            }
        }

        if has_rule && score > 0 {
            matches.push((score, id.clone()));
        }
    }

    matches.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let best = matches.first()?;
    if matches.get(1).is_some_and(|next| next.0 == best.0) {
        return None;
    }
    Some(best.1.clone())
}

fn local_ipv4_addresses() -> Vec<String> {
    let mut addresses: Vec<String> = get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| match interface.addr {
            IfAddr::V4(address) if !address.ip.is_loopback() => Some(address.ip.to_string()),
            _ => None,
        })
        .collect();
    addresses.sort();
    addresses.dedup();
    addresses
}

#[cfg(target_os = "windows")]
fn default_gateway() -> Option<String> {
    command_output(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-NetRoute -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' | Sort-Object RouteMetric,InterfaceMetric | Select-Object -First 1 -ExpandProperty NextHop)",
        ],
    )
    .and_then(first_non_empty_line)
}

#[cfg(target_os = "macos")]
fn default_gateway() -> Option<String> {
    let output = command_output("/sbin/route", &["-n", "get", "default"])?;
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("gateway:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn default_gateway() -> Option<String> {
    let output = command_output("ip", &["-4", "route", "show", "default"])?;
    let tokens: Vec<&str> = output.split_whitespace().collect();
    tokens
        .windows(2)
        .find(|pair| pair[0] == "via")
        .map(|pair| pair[1].to_string())
}

#[cfg(target_os = "windows")]
fn current_ssid() -> Option<String> {
    let output = command_output("netsh", &["wlan", "show", "interfaces"])?;
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with("SSID") && !trimmed.starts_with("BSSID") {
            trimmed
                .split_once(':')
                .map(|(_, value)| value.trim())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        } else {
            None
        }
    })
}

#[cfg(target_os = "macos")]
fn current_ssid() -> Option<String> {
    for interface in ["en0", "en1"] {
        if let Some(output) = command_output(
            "/usr/sbin/networksetup",
            &["-getairportnetwork", interface],
        ) {
            if let Some((_, value)) = output.split_once(':') {
                let value = value.trim();
                if !value.is_empty() && !value.contains("not associated") {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

#[cfg(all(unix, not(target_os = "macos")))]
fn current_ssid() -> Option<String> {
    let output = command_output("nmcli", &["-t", "-f", "ACTIVE,SSID", "device", "wifi"])?;
    output.lines().find_map(|line| {
        line.strip_prefix("yes:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.replace("\\:", ":"))
    })
}

fn command_output(program: &str, arguments: &[&str]) -> Option<String> {
    let output = system_command(program).args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "windows")]
fn system_command(program: &str) -> Command {
    use std::os::windows::process::CommandExt;

    // Network discovery runs on startup and at each refresh. Prevent PowerShell/netsh from
    // briefly creating a console window in the foreground of the desktop application.
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(target_os = "windows"))]
fn system_command(program: &str) -> Command {
    Command::new(program)
}

fn first_non_empty_line(value: String) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn ipv4_in_cidr(address: &str, cidr: &str) -> bool {
    let Ok(address) = address.parse::<Ipv4Addr>() else {
        return false;
    };
    let Some((network, prefix)) = cidr.split_once('/') else {
        return false;
    };
    let Ok(network) = network.parse::<Ipv4Addr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u32>() else {
        return false;
    };
    if prefix > 32 {
        return false;
    }
    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    u32::from(address) & mask == u32::from(network) & mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_matching_is_exact() {
        assert!(ipv4_in_cidr("192.0.2.10", "192.0.2.0/24"));
        assert!(!ipv4_in_cidr("198.51.100.10", "192.0.2.0/24"));
        assert!(!ipv4_in_cidr("invalid", "192.0.2.0/24"));
    }
}
