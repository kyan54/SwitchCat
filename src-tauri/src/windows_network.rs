use crate::{
    config::{mask_prefix, Profile},
    network::{self, Adapter},
};
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const CREATE_NO_WINDOW: u32 = 0x08000000;

struct ProcessOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

fn hidden_command(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// Keep pipe reads off the waiting thread and put a deadline around the whole command.
fn run_powershell(script: &str, timeout: Duration) -> Result<ProcessOutput, String> {
    let script = format!(
        "$ErrorActionPreference='Stop'; $utf8=[System.Text.UTF8Encoding]::new($false); [Console]::OutputEncoding=$utf8; $OutputEncoding=$utf8;\n{script}"
    );
    let mut child = hidden_command("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动 Windows PowerShell：{error}"))?;
    let (sender, receiver) = mpsc::channel();
    for (stderr, stream) in [
        (
            false,
            Box::new(child.stdout.take().ok_or("无法读取命令输出")?) as Box<dyn Read + Send>,
        ),
        (
            true,
            Box::new(child.stderr.take().ok_or("无法读取命令错误")?) as Box<dyn Read + Send>,
        ),
    ] {
        let sender = sender.clone();
        thread::spawn(move || {
            let mut stream = stream;
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let remaining = (128 * 1024usize).saturating_sub(bytes.len());
                        bytes.extend_from_slice(&buffer[..count.min(remaining)]);
                    }
                }
            }
            let _ = sender.send((stderr, String::from_utf8_lossy(&bytes).into_owned()));
        });
    }
    drop(sender);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                stop_tree(&mut child);
                return Err(format!("读取网络命令状态失败：{error}"));
            }
        }
        if started.elapsed() >= timeout {
            stop_tree(&mut child);
            return Err(format!(
                "Windows 网络命令超过 {} 秒，已停止。可能已有部分参数更新，请检查网卡后重新应用配置",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(40));
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    for _ in 0..2 {
        let (is_stderr, value) = receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "网络命令输出未能正常结束".to_string())?;
        if is_stderr {
            stderr = value;
        } else {
            stdout = value;
        }
    }
    Ok(ProcessOutput {
        success: status.success(),
        stdout,
        stderr,
    })
}

fn stop_tree(child: &mut std::process::Child) {
    // Kill only the process created for this operation and its netsh descendants.
    if let Ok(mut killer) = hidden_command("taskkill.exe")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) {
            if killer.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(40));
        }
        let _ = killer.kill();
        let _ = killer.wait();
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn list_adapters() -> Result<Vec<Adapter>, String> {
    let output = run_powershell(
        "$items = @(Get-NetAdapter | Sort-Object Name | ForEach-Object { [pscustomobject]@{ id=([guid]$_.InterfaceGuid).ToString(); name=$_.Name; status=$_.Status.ToString() } }); ConvertTo-Json -InputObject $items -Compress",
        Duration::from_secs(15),
    )?;
    if !output.success {
        return Err(localized_error(&output.stderr));
    }
    serde_json::from_str(output.stdout.trim_start_matches('\u{feff}').trim())
        .map_err(|error| format!("读取网卡列表失败：{error}"))
}

fn apply_script(adapter_id: &str, profile: &Profile) -> Result<String, String> {
    network::validate_adapter_id(adapter_id)?;
    profile.validate()?;
    let prefix = mask_prefix(&profile.subnet_mask)?;
    let dns = profile
        .dns_servers()
        .iter()
        .map(|ip| format!("'{ip}'"))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        r#"
function Log-Step([string]$name, $watch, [bool]$changed) {{
    Write-Output ("SWITCHCAT_STEP|{{0}}|{{1}}|{{2}}" -f $name,$watch.ElapsedMilliseconds,$changed)
}}
$watch = [System.Diagnostics.Stopwatch]::StartNew()
$adapter = @(Get-NetAdapter | Where-Object {{ ([guid]$_.InterfaceGuid).ToString() -eq '{adapter_id}' }})
if ($adapter.Count -ne 1) {{ throw 'SWITCHCAT_ADAPTER_NOT_FOUND' }}
$index = [int]$adapter[0].ifIndex
Log-Step 'adapter_read' $watch $false
$ip = '{ip}'
$mask = '{mask}'
$prefix = {prefix}
$gateway = '{gateway}'
$dns = @({dns})
$watch.Restart()
$addresses = @(Get-NetIPAddress -InterfaceIndex $index -AddressFamily IPv4 -ErrorAction SilentlyContinue)
$interface = Get-NetIPInterface -InterfaceIndex $index -AddressFamily IPv4
$routes = @(Get-NetRoute -InterfaceIndex $index -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue)
$currentDns = @((Get-DnsClientServerAddress -InterfaceIndex $index -AddressFamily IPv4).ServerAddresses)
$addressMatches = $addresses.Count -eq 1 -and $addresses[0].IPAddress -eq $ip -and $addresses[0].PrefixLength -eq $prefix -and $addresses[0].AddressState -ne 'Duplicate'
$routeMatches = $routes.Count -eq 1 -and $routes[0].NextHop -eq $gateway
$changeAddress = (-not $addressMatches) -or (-not $routeMatches) -or ($interface.Dhcp -ne 'Disabled')
$changeDns = ($currentDns -join ',') -ne ($dns -join ',')
Log-Step 'read_before' $watch $false
$watch.Restart()
if ($changeAddress) {{
    & netsh.exe interface ipv4 set address "name=$index" source=static "address=$ip" "mask=$mask" "gateway=$gateway" gwmetric=1 store=persistent | Out-Null
    if ($LASTEXITCODE -ne 0) {{ throw 'SWITCHCAT_ADDRESS_FAILED' }}
}}
Log-Step 'address_gateway' $watch $changeAddress
$watch.Restart()
if ($changeDns) {{
    Set-DnsClientServerAddress -InterfaceIndex $index -ServerAddresses $dns
}}
Log-Step 'dns' $watch $changeDns
$watch.Restart()
$checkIp = @(Get-NetIPAddress -InterfaceIndex $index -AddressFamily IPv4 -IPAddress $ip -ErrorAction SilentlyContinue | Where-Object {{ $_.PrefixLength -eq $prefix -and $_.AddressState -ne 'Duplicate' }})
$checkRoutes = @(Get-NetRoute -InterfaceIndex $index -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue)
$checkDns = @((Get-DnsClientServerAddress -InterfaceIndex $index -AddressFamily IPv4).ServerAddresses)
if ($checkIp.Count -eq 0) {{ throw 'SWITCHCAT_IP_VERIFY_FAILED' }}
if ($checkRoutes.Count -ne 1 -or $checkRoutes[0].NextHop -ne $gateway) {{ throw 'SWITCHCAT_GATEWAY_VERIFY_FAILED' }}
if (($checkDns -join ',') -ne ($dns -join ',')) {{ throw 'SWITCHCAT_DNS_VERIFY_FAILED' }}
Log-Step 'verify' $watch $false
Write-Output 'SWITCHCAT_NETWORK_OK'
"#,
        ip = profile.ip_address,
        mask = profile.subnet_mask,
        gateway = profile.gateway
    ))
}

pub fn apply(adapter_id: &str, profile: &Profile) -> Result<(), String> {
    let output = run_powershell(&apply_script(adapter_id, profile)?, Duration::from_secs(45))?;
    for line in output.stdout.lines() {
        if let Some(value) = line.strip_prefix("SWITCHCAT_STEP|") {
            let parts: Vec<_> = value.split('|').collect();
            if parts.len() == 3 {
                log::info!(
                    "network profile={} step={} elapsed_ms={} changed={}",
                    profile.name,
                    parts[0],
                    parts[1],
                    parts[2]
                );
            }
        }
    }
    if output.success
        && output
            .stdout
            .lines()
            .any(|line| line.trim() == "SWITCHCAT_NETWORK_OK")
    {
        return Ok(());
    }
    log::error!("Windows network operation failed: {}", output.stderr);
    Err(format!(
        "{}。可能已有部分参数更新，请检查网卡后重新应用配置",
        localized_error(&output.stderr)
    ))
}

fn localized_error(detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    if detail.contains("SWITCHCAT_ADAPTER_NOT_FOUND") {
        "找不到已选择的网卡，请在配置页重新选择并保存".into()
    } else if detail.contains("SWITCHCAT_ADDRESS_FAILED") {
        "设置 IP、子网掩码或网关失败，请以管理员身份运行并检查网卡".into()
    } else if detail.contains("SWITCHCAT_IP_VERIFY_FAILED") {
        "应用后 IP 或子网掩码校验失败，请检查是否存在地址冲突".into()
    } else if detail.contains("SWITCHCAT_GATEWAY_VERIFY_FAILED") {
        "应用后网关校验失败，请检查是否存在冲突的默认路由".into()
    } else if detail.contains("SWITCHCAT_DNS_VERIFY_FAILED") {
        "应用后 DNS 校验失败".into()
    } else if lower.contains("access is denied")
        || lower.contains("access denied")
        || lower.contains("requires elevation")
        || detail.contains("拒绝访问")
        || detail.contains("需要提升")
    {
        "请退出后以管理员身份运行 SwitchCat".into()
    } else {
        "Windows 网络操作失败，请检查网卡状态和管理员权限，详情见本地日志".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dns2_only_and_explicit_target_generate_valid_script() {
        let profile = Profile {
            name: "家里*代理".into(),
            ip_address: "192.168.1.10".into(),
            subnet_mask: "255.255.255.0".into(),
            gateway: "192.168.1.3".into(),
            dns1: String::new(),
            dns2: "1.1.1.1".into(),
        };
        let script = apply_script("7f0335c4-508b-40bc-bb78-a019059d50ce", &profile).unwrap();
        assert!(script.contains("$dns = @('1.1.1.1')"));
        assert!(script.contains("$prefix = 24"));
        assert!(script.contains("if ($changeAddress)"));
        assert!(script.contains("if ($changeDns)"));
        assert!(!script.contains("AdapterBinding"));
        assert!(apply_script("'; exit", &profile).is_err());
    }
    #[test]
    fn command_timeout_returns_instead_of_holding_the_action_lock() {
        let started = Instant::now();
        let result = run_powershell("Start-Sleep -Seconds 10", Duration::from_millis(200));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(6));
    }
    #[test]
    fn adapter_list_uses_json_even_for_a_single_result() {
        let output = run_powershell("ConvertTo-Json -InputObject @([pscustomobject]@{ id='7f0335c4-508b-40bc-bb78-a019059d50ce'; name='Ethernet'; status='Up' }) -Compress", Duration::from_secs(15)).unwrap();
        assert!(output.success);
        let adapters: Vec<Adapter> = serde_json::from_str(output.stdout.trim()).unwrap();
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].name, "Ethernet");
    }

    #[test]
    fn generated_apply_script_parses_without_executing_changes() {
        let profile = Profile {
            name: "家里*本地连接".into(),
            ip_address: "192.168.1.10".into(),
            subnet_mask: "255.255.255.0".into(),
            gateway: "192.168.1.1".into(),
            dns1: "1.1.1.1".into(),
            dns2: "8.8.8.8".into(),
        };
        let script = apply_script("7f0335c4-508b-40bc-bb78-a019059d50ce", &profile).unwrap();
        let validation = format!(
            "$tokens=$null; $errors=$null; [System.Management.Automation.Language.Parser]::ParseInput('{}',[ref]$tokens,[ref]$errors) | Out-Null; if ($errors.Count) {{ throw ($errors | Out-String) }}; Write-Output 'PARSE_OK'",
            script.replace('\'', "''")
        );
        let output = run_powershell(&validation, Duration::from_secs(15)).unwrap();
        assert!(output.success, "{}", output.stderr);
        assert!(output.stdout.contains("PARSE_OK"));
    }

    #[test]
    fn discovered_adapter_guids_can_be_saved_and_reused() {
        for adapter in list_adapters().unwrap() {
            network::validate_adapter_id(&adapter.id).unwrap();
        }
    }
}
