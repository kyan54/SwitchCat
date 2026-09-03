use crate::config::Profile;
use log::error;
use std::{net::Ipv4Addr, process::Command};

#[derive(Debug)]
struct NetworkTarget {
    gateway: Ipv4Addr,
    dns: Ipv4Addr,
    proxy: bool,
}

pub fn apply_route(profile: &Profile, proxy: bool) -> Result<(), String> {
    let client_ip = parse_ipv4(&profile.device.client_ip, "本机固定 IPv4")?;
    let target = target_for(profile, proxy)?;
    let ipv6_command = if target.proxy {
        "Disable-NetAdapterBinding"
    } else {
        "Enable-NetAdapterBinding"
    };

    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$clientIp = '{client_ip}'
$gateway = '{}'
$dns = '{}'
$ipInfo = @(Get-NetIPAddress -AddressFamily IPv4 -IPAddress $clientIp -ErrorAction SilentlyContinue | Where-Object {{ $_.AddressState -ne 'Duplicate' }}) | Select-Object -First 1
if (-not $ipInfo) {{ throw 'SWITCHCAT_INTERFACE_NOT_FOUND' }}
$prefix = [int]$ipInfo.PrefixLength
$fullBytes = [math]::Floor($prefix / 8)
$remainingBits = $prefix % 8
$octets = for ($i = 0; $i -lt 4; $i++) {{
  if ($i -lt $fullBytes) {{ 255 }}
  elseif (($i -eq $fullBytes) -and ($remainingBits -gt 0)) {{ [int](256 - [math]::Pow(2, 8 - $remainingBits)) }}
  else {{ 0 }}
}}
$mask = $octets -join '.'
& netsh.exe interface ipv4 set address "name=$($ipInfo.InterfaceAlias)" source=static "address=$clientIp" "mask=$mask" "gateway=$gateway" gwmetric=1 | Out-Null
if ($LASTEXITCODE -ne 0) {{ throw 'SWITCHCAT_ADDRESS_FAILED' }}
Set-DnsClientServerAddress -InterfaceIndex $ipInfo.InterfaceIndex -ServerAddresses @($dns)
{ipv6_command} -Name $ipInfo.InterfaceAlias -ComponentID ms_tcpip6 -Confirm:$false -ErrorAction Stop | Out-Null
Clear-DnsClientCache -ErrorAction SilentlyContinue
$currentRoute = Get-NetRoute -AddressFamily IPv4 -InterfaceIndex $ipInfo.InterfaceIndex -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | Sort-Object RouteMetric | Select-Object -First 1
if ((-not $currentRoute) -or ($currentRoute.NextHop -ne $gateway)) {{ throw 'SWITCHCAT_GATEWAY_VERIFY_FAILED' }}
$currentDns = @(Get-DnsClientServerAddress -InterfaceIndex $ipInfo.InterfaceIndex -AddressFamily IPv4).ServerAddresses
if ($currentDns -notcontains $dns) {{ throw 'SWITCHCAT_DNS_VERIFY_FAILED' }}
Write-Output 'SWITCHCAT_NETWORK_OK'"#,
        target.gateway, target.dns
    );

    let output = powershell_command()
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|_| "无法启动 Windows PowerShell，请确认系统组件完整".to_string())?;

    if output.status.success()
        && String::from_utf8_lossy(&output.stdout).contains("SWITCHCAT_NETWORK_OK")
    {
        return Ok(());
    }

    let detail = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    error!("Windows network switch failed: {detail}");
    Err(localized_error(&detail))
}

pub fn route_configured(profile: &Profile, proxy: bool) -> bool {
    target_for(profile, proxy).is_ok()
}

fn target_for(profile: &Profile, proxy: bool) -> Result<NetworkTarget, String> {
    let (gateway, dns) = if proxy {
        let gateway = value_or(&profile.device.proxy_gateway, &profile.openwrt.host);
        let dns = value_or(&profile.device.proxy_dns, gateway);
        (gateway, dns)
    } else {
        let gateway = profile.device.direct_gateway.trim();
        let dns = value_or(&profile.device.direct_dns, gateway);
        (gateway, dns)
    };

    Ok(NetworkTarget {
        gateway: parse_ipv4(gateway, if proxy { "OpenWrt 网关" } else { "本地路由器" })?,
        dns: parse_ipv4(dns, if proxy { "OpenWrt DNS" } else { "本地 DNS" })?,
        proxy,
    })
}

fn value_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim()
    } else {
        value
    }
}

fn parse_ipv4(value: &str, label: &str) -> Result<Ipv4Addr, String> {
    value
        .trim()
        .parse::<Ipv4Addr>()
        .map_err(|_| format!("{label}“{value}”不是有效的 IPv4 地址，请先在环境配置中修正"))
}

fn localized_error(detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    if detail.contains("SWITCHCAT_INTERFACE_NOT_FOUND") {
        "找不到拥有配置中固定 IPv4 的网卡，请检查本机 IP 是否填写正确".to_string()
    } else if detail.contains("SWITCHCAT_ADDRESS_FAILED") {
        "设置固定 IPv4 和默认网关失败，请确认 SwitchCat 已以管理员身份运行".to_string()
    } else if detail.contains("SWITCHCAT_GATEWAY_VERIFY_FAILED") {
        "默认网关设置后校验失败，请检查网卡中是否存在冲突的默认路由".to_string()
    } else if detail.contains("SWITCHCAT_DNS_VERIFY_FAILED") {
        "DNS 设置后校验失败，请检查网卡或安全软件限制".to_string()
    } else if lower.contains("requires elevation")
        || lower.contains("access is denied")
        || detail.contains("拒绝访问")
        || detail.contains("需要提升")
    {
        "没有修改网卡的权限，请退出后右键选择“以管理员身份运行”".to_string()
    } else {
        "修改 Windows 网关、DNS 或 IPv6 状态失败，请确认程序以管理员身份运行且网卡未被其他软件接管".to_string()
    }
}

fn powershell_command() -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut command = Command::new("powershell.exe");
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_defaults_to_openwrt_address() {
        let mut config = crate::config::AppConfig::with_local_ip("192.0.2.10".to_string());
        let profile = config.profiles.get_mut("home").unwrap();
        profile.openwrt.host = "192.0.2.3".to_string();
        let target = target_for(profile, true).unwrap();
        assert_eq!(target.gateway, "192.0.2.3".parse::<Ipv4Addr>().unwrap());
        assert_eq!(target.dns, "192.0.2.3".parse::<Ipv4Addr>().unwrap());
    }

    #[test]
    fn direct_requires_a_valid_gateway() {
        let config = crate::config::AppConfig::with_local_ip("192.0.2.10".to_string());
        let error = target_for(&config.profiles["home"], false).unwrap_err();
        assert!(error.contains("本地路由器"));
    }
}
