use crate::{config::Profile, system_info};
use log::error;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[derive(Debug, Clone, Serialize)]
pub struct ProxyNode {
    pub id: String,
    pub name: String,
    pub core: String,
    pub protocol: String,
    pub address: String,
    pub port: String,
}

impl ProxyNode {
    pub fn menu_label(&self) -> String {
        let technology = match (self.core.is_empty(), self.protocol.is_empty()) {
            (false, false) => format!("{}/{}", self.core, self.protocol),
            (false, true) => self.core.clone(),
            (true, false) => self.protocol.clone(),
            (true, true) => "未知协议".to_string(),
        };
        let endpoint = match (self.address.is_empty(), self.port.is_empty()) {
            (false, false) => format!("{}:{}", self.address, self.port),
            (false, true) => self.address.clone(),
            _ => String::new(),
        };
        if endpoint.is_empty() {
            format!("{}   [{}]", self.name, technology)
        } else {
            format!("{}   [{}] {}", self.name, technology, endpoint)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteSelection {
    Direct,
    Proxy { node_id: String, node_name: String },
    Global { node_id: String, node_name: String },
    Unknown,
}

impl RouteSelection {
    pub fn description(&self) -> String {
        match self {
            Self::Direct => "本地直连".to_string(),
            Self::Proxy { node_name, .. } => node_name.clone(),
            Self::Global { node_name, .. } => format!("全局节点：{node_name}"),
            Self::Unknown => "状态未知".to_string(),
        }
    }

    pub fn is_node(&self, node_id: &str) -> bool {
        matches!(
            self,
            Self::Proxy { node_id: current, .. } | Self::Global { node_id: current, .. }
                if current == node_id
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Inventory {
    pub nodes: Vec<ProxyNode>,
    pub acl_section: String,
    pub acl_remarks: String,
    pub selection: RouteSelection,
}

#[derive(Debug, Clone, Serialize)]
pub struct SshStatus {
    pub ok: bool,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SshInstructions {
    pub platform: String,
    pub keygen_command: String,
    pub install_command: String,
    pub verify_command: String,
    pub known_hosts_file: String,
    pub identity_file: String,
}

#[derive(Debug, Default)]
struct UciSection {
    kind: String,
    options: BTreeMap<String, Vec<String>>,
}

pub fn check_ssh(profile_id: &str, profile: &Profile, config_dir: &Path) -> SshStatus {
    match run_ssh(profile_id, profile, config_dir, "printf SWITCHCAT_SSH_OK") {
        Ok(output) if output.trim() == "SWITCHCAT_SSH_OK" => SshStatus {
            ok: true,
            kind: "ready".to_string(),
            message: "SSH 免密连接正常".to_string(),
        },
        Ok(_) => SshStatus {
            ok: false,
            kind: "unexpected_output".to_string(),
            message: "SSH 已连接，但 OpenWrt 返回内容不符合预期".to_string(),
        },
        Err(error) => classify_ssh_error(&error),
    }
}

pub fn fetch_inventory(
    profile_id: &str,
    profile: &Profile,
    config_dir: &Path,
) -> Result<Inventory, String> {
    let raw = run_ssh(profile_id, profile, config_dir, "uci -q show passwall2")
        .map_err(user_facing_ssh_error)?;
    parse_inventory(&raw, &profile.device.client_ip, &profile.device.acl_remarks)
}

pub fn switch_direct(
    profile_id: &str,
    profile: &Profile,
    config_dir: &Path,
) -> Result<Inventory, String> {
    ensure_expected_client_ip(profile)?;
    let inventory = fetch_inventory(profile_id, profile, config_dir)?;
    validate_uci_section(&inventory.acl_section)?;

    let assignments = [
        format!("passwall2.{}.enabled=1", inventory.acl_section),
        format!("passwall2.{}.mode=0", inventory.acl_section),
    ];
    let command = apply_command(&assignments);
    run_ssh(profile_id, profile, config_dir, &command).map_err(user_facing_ssh_error)?;

    let updated = fetch_inventory(profile_id, profile, config_dir)?;
    if !matches!(updated.selection, RouteSelection::Direct) {
        return Err("OpenWrt 已执行命令，但 ACL 没有切换到本地直连".to_string());
    }
    Ok(updated)
}

pub fn switch_node(
    profile_id: &str,
    profile: &Profile,
    config_dir: &Path,
    requested_node_id: &str,
) -> Result<Inventory, String> {
    ensure_expected_client_ip(profile)?;
    let inventory = fetch_inventory(profile_id, profile, config_dir)?;
    validate_uci_section(&inventory.acl_section)?;
    let node = inventory
        .nodes
        .iter()
        .find(|node| node.id == requested_node_id)
        .ok_or_else(|| "所选节点已不存在，请刷新节点列表后重试".to_string())?;
    validate_uci_section(&node.id)?;

    let assignments = [
        format!("passwall2.{}.enabled=1", inventory.acl_section),
        format!("passwall2.{}.mode=1", inventory.acl_section),
        format!("passwall2.{}.node={}", inventory.acl_section, node.id),
    ];
    let command = apply_command(&assignments);
    run_ssh(profile_id, profile, config_dir, &command).map_err(user_facing_ssh_error)?;

    let updated = fetch_inventory(profile_id, profile, config_dir)?;
    if !updated.selection.is_node(requested_node_id) {
        return Err("OpenWrt 已执行命令，但 ACL 当前节点与所选节点不一致".to_string());
    }
    Ok(updated)
}

pub fn ssh_instructions(
    profile_id: &str,
    profile: &Profile,
    config_dir: &Path,
) -> SshInstructions {
    let known_hosts = known_hosts_path(config_dir, profile_id);
    let identity = identity_file(profile);
    let target = format!("{}@{}", profile.openwrt.user, profile.openwrt.host);

    #[cfg(target_os = "windows")]
    {
        let identity_display = identity.display().to_string().replace('\'', "''");
        let known_display = known_hosts.display().to_string().replace('\'', "''");
        let keygen_command = format!(
            "$Key = '{identity_display}'\nNew-Item -ItemType Directory -Force (Split-Path $Key) | Out-Null\nif (!(Test-Path $Key)) {{ ssh-keygen -t ed25519 -f $Key }}"
        );
        let common = format!(
            "-p {} -i $Key -o IdentitiesOnly=yes -o UserKnownHostsFile=$Known",
            profile.openwrt.port
        );
        let install_command = format!(
            "$Key = '{identity_display}'\n$Known = '{known_display}'\nNew-Item -ItemType Directory -Force (Split-Path $Known) | Out-Null\nGet-Content \"$Key.pub\" | ssh {common} -o StrictHostKeyChecking=accept-new {target} \"umask 077; mkdir -p /etc/dropbear; cat >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys\""
        );
        let verify_command = format!(
            "$Key = '{identity_display}'\n$Known = '{known_display}'\nssh {common} -o BatchMode=yes -o StrictHostKeyChecking=yes {target} \"echo SSH_OK\""
        );
        return SshInstructions {
            platform: "windows".to_string(),
            keygen_command,
            install_command,
            verify_command,
            known_hosts_file: known_display,
            identity_file: identity_display,
        };
    }

    #[cfg(not(target_os = "windows"))]
    {
        let identity_display = identity.display().to_string();
        let known_display = known_hosts.display().to_string();
        let keygen_command = format!(
            "KEY={}\nmkdir -p \"$(dirname \"$KEY\")\"\n[ -f \"$KEY\" ] || ssh-keygen -t ed25519 -f \"$KEY\"",
            sh_quote(&identity_display)
        );
        let install_command = format!(
            "KEY={}\nKNOWN={}\nmkdir -p \"$(dirname \"$KNOWN\")\"\ncat \"$KEY.pub\" | ssh -p {} -i \"$KEY\" -o IdentitiesOnly=yes -o UserKnownHostsFile=\"$KNOWN\" -o StrictHostKeyChecking=accept-new {} 'umask 077; mkdir -p /etc/dropbear; cat >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys'",
            sh_quote(&identity_display),
            sh_quote(&known_display),
            profile.openwrt.port,
            sh_quote(&target)
        );
        let verify_command = format!(
            "KEY={}\nKNOWN={}\nssh -p {} -i \"$KEY\" -o IdentitiesOnly=yes -o UserKnownHostsFile=\"$KNOWN\" -o BatchMode=yes -o StrictHostKeyChecking=yes {} 'echo SSH_OK'",
            sh_quote(&identity_display),
            sh_quote(&known_display),
            profile.openwrt.port,
            sh_quote(&target)
        );
        SshInstructions {
            platform: if cfg!(target_os = "macos") {
                "macos".to_string()
            } else {
                "linux".to_string()
            },
            keygen_command,
            install_command,
            verify_command,
            known_hosts_file: known_display,
            identity_file: identity_display,
        }
    }
}

fn run_ssh(
    profile_id: &str,
    profile: &Profile,
    config_dir: &Path,
    remote_command: &str,
) -> Result<String, String> {
    let known_hosts = known_hosts_path(config_dir, profile_id);
    if let Some(parent) = known_hosts.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建 SSH 配置目录失败：{error}"))?;
    }

    let mut command = ssh_command();
    command
        .arg("-p")
        .arg(profile.openwrt.port.to_string())
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg(format!(
            "ConnectTimeout={}",
            profile.openwrt.connect_timeout_seconds.clamp(2, 30)
        ))
        .arg("-o")
        .arg("StrictHostKeyChecking=yes")
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-o")
        .arg(format!("UserKnownHostsFile={}", known_hosts.display()));

    let identity = identity_file(profile);
    if !identity.as_os_str().is_empty() {
        command.arg("-i").arg(identity);
    }

    command
        .arg(format!("{}@{}", profile.openwrt.user, profile.openwrt.host))
        .arg(remote_command);

    let output = command
        .output()
        .map_err(|error| format!("无法启动 ssh：{error}"))?;
    output_to_result(output)
}

fn output_to_result(output: Output) -> Result<String, String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    Err(if detail.is_empty() {
        format!("SSH 执行失败，退出码：{:?}", output.status.code())
    } else {
        detail
    })
}

fn ensure_expected_client_ip(profile: &Profile) -> Result<(), String> {
    if let Some(actual_ip) = system_info::best_local_ip_for(
        &profile.openwrt.host,
        profile.openwrt.port,
    ) {
        if actual_ip != profile.device.client_ip {
            return Err(format!(
                "当前连接 OpenWrt 使用的本机地址是 {actual_ip}，但环境配置为 {}。为避免修改其他设备的 ACL，SwitchCat 已取消切换",
                profile.device.client_ip
            ));
        }
    }
    Ok(())
}

fn classify_ssh_error(error: &str) -> SshStatus {
    let lower = error.to_ascii_lowercase();
    let (kind, message) = if lower.contains("could not resolve hostname") {
        ("dns", "无法解析 OpenWrt 地址，请检查地址填写和当前网络".to_string())
    } else if lower.contains("hostname contains invalid characters")
        || lower.contains("invalid hostname")
    {
        ("invalid_host", "OpenWrt 地址无效或尚未填写完整".to_string())
    } else if lower.contains("remote host identification has changed") {
        ("host_key_changed", "SSH 主机指纹发生变化。为防止连接到错误设备，SwitchCat 已拒绝连接".to_string())
    } else if lower.contains("host key verification failed")
        || lower.contains("no ed25519 host key is known")
        || lower.contains("no ecdsa host key is known")
    {
        ("host_key_missing", "尚未确认该环境的 SSH 主机指纹，请按首次配置命令操作".to_string())
    } else if lower.contains("permission denied") {
        ("auth", "SSH 免密认证失败，请重新安装公钥".to_string())
    } else if lower.contains("connection timed out") || lower.contains("operation timed out") {
        ("timeout", "连接 OpenWrt 超时，请检查地址、防火墙和网络连通性".to_string())
    } else if lower.contains("connection refused") {
        ("refused", "OpenWrt 拒绝了 SSH 连接，请检查 Dropbear 服务和 SSH 端口".to_string())
    } else if lower.contains("no route to host") || lower.contains("network is unreachable") {
        ("unreachable", "当前网络无法访问 OpenWrt，请确认所选环境和本机网络".to_string())
    } else if lower.contains("connection reset")
        || lower.contains("connection closed")
        || lower.contains("kex_exchange_identification")
    {
        ("disconnected", "SSH 连接在握手过程中断开，请检查 OpenWrt SSH 服务".to_string())
    } else if lower.contains("无法启动 ssh") || lower.contains("no such file or directory") {
        ("ssh_missing", "系统未安装 OpenSSH 客户端".to_string())
    } else {
        ("ssh_error", "SSH 操作失败，请检查 OpenWrt 地址、SSH 服务和免密配置".to_string())
    };
    SshStatus {
        ok: false,
        kind: kind.to_string(),
        message,
    }
}

fn user_facing_ssh_error(error_message: String) -> String {
    error!("SSH command failed: {error_message}");
    classify_ssh_error(&error_message).message
}

fn parse_inventory(raw: &str, client_ip: &str, acl_remarks: &str) -> Result<Inventory, String> {
    let sections = parse_uci(raw);
    let mut nodes: Vec<ProxyNode> = sections
        .iter()
        .filter(|(_, section)| section.kind == "nodes")
        .map(|(id, section)| ProxyNode {
            id: id.clone(),
            name: option(section, "remarks")
                .or_else(|| option(section, "remark"))
                .unwrap_or_else(|| id.clone()),
            core: option(section, "type").unwrap_or_default(),
            protocol: option(section, "protocol").unwrap_or_default(),
            address: option(section, "address").unwrap_or_default(),
            port: option(section, "port").unwrap_or_default(),
        })
        .collect();
    nodes.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut matching_acls: Vec<(&String, &UciSection)> = sections
        .iter()
        .filter(|(_, section)| section.kind == "acl_rule")
        .filter(|(_, section)| acl_matches(section, client_ip, acl_remarks))
        .collect();

    if matching_acls.is_empty() {
        return Err(format!(
            "PassWall2 中没有找到来源为 {} 的 ACL。请先在 OpenWrt 创建对应规则",
            client_ip
        ));
    }
    if matching_acls.len() > 1 {
        return Err(format!(
            "PassWall2 中找到多个匹配 {} 的 ACL，请确保每台设备只对应一条规则",
            client_ip
        ));
    }

    let (acl_id, acl) = matching_acls.remove(0);
    let mode = option(acl, "mode").unwrap_or_else(|| "0".to_string());
    let configured_node = option(acl, "node").unwrap_or_default();
    let global_node = sections
        .values()
        .find(|section| section.kind == "global")
        .and_then(|section| option(section, "node"))
        .unwrap_or_default();

    let selection = match mode.as_str() {
        "0" => RouteSelection::Direct,
        "1" => selection_for_node(&nodes, &configured_node, false),
        "2" => selection_for_node(&nodes, &global_node, true),
        _ => RouteSelection::Unknown,
    };

    Ok(Inventory {
        nodes,
        acl_section: acl_id.clone(),
        acl_remarks: option(acl, "remarks").unwrap_or_default(),
        selection,
    })
}

fn parse_uci(raw: &str) -> BTreeMap<String, UciSection> {
    let mut sections: BTreeMap<String, UciSection> = BTreeMap::new();
    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let Some(key) = key.strip_prefix("passwall2.") else {
            continue;
        };
        let values = shell_words::split(raw_value)
            .unwrap_or_else(|_| vec![raw_value.trim_matches('\'').to_string()]);

        if let Some((section_id, option_name)) = key.split_once('.') {
            let section = sections.entry(section_id.to_string()).or_default();
            section.options.insert(option_name.to_string(), values);
        } else {
            let section = sections.entry(key.to_string()).or_default();
            section.kind = values.first().cloned().unwrap_or_default();
        }
    }
    sections
}

fn acl_matches(section: &UciSection, client_ip: &str, acl_remarks: &str) -> bool {
    let source_match = section
        .options
        .get("sources")
        .is_some_and(|sources| sources.iter().any(|source| source == client_ip));
    if source_match {
        return true;
    }
    !acl_remarks.trim().is_empty()
        && option(section, "remarks").is_some_and(|remarks| remarks == acl_remarks)
}

fn option(section: &UciSection, name: &str) -> Option<String> {
    section.options.get(name).and_then(|values| values.first()).cloned()
}

fn selection_for_node(nodes: &[ProxyNode], node_id: &str, global: bool) -> RouteSelection {
    let node_name = nodes
        .iter()
        .find(|node| node.id == node_id)
        .map(|node| node.name.clone())
        .unwrap_or_else(|| {
            if node_id.is_empty() {
                "未选择节点".to_string()
            } else {
                node_id.to_string()
            }
        });
    if global {
        RouteSelection::Global {
            node_id: node_id.to_string(),
            node_name,
        }
    } else {
        RouteSelection::Proxy {
            node_id: node_id.to_string(),
            node_name,
        }
    }
}

fn apply_command(assignments: &[String]) -> String {
    let mut commands = vec!["set -eu".to_string()];
    commands.extend(
        assignments
            .iter()
            .map(|assignment| format!("uci set {}", sh_quote(assignment))),
    );
    commands.push("uci commit passwall2".to_string());
    commands.push(
        "if /etc/init.d/passwall2 reload >/dev/null 2>&1; then :; else /etc/init.d/passwall2 restart >/dev/null 2>&1; fi"
            .to_string(),
    );
    commands.push("printf SWITCHCAT_APPLIED".to_string());
    commands.join("; ")
}

fn validate_uci_section(value: &str) -> Result<(), String> {
    if !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '_' | '-' | '@' | '[' | ']')
        })
    {
        Ok(())
    } else {
        Err("OpenWrt 返回了不安全的 UCI 节点标识，已拒绝执行".to_string())
    }
}

fn known_hosts_path(config_dir: &Path, profile_id: &str) -> PathBuf {
    config_dir
        .join("known_hosts")
        .join(format!("{profile_id}.known_hosts"))
}

fn identity_file(profile: &Profile) -> PathBuf {
    if !profile.openwrt.identity_file.trim().is_empty() {
        return expand_home(&profile.openwrt.identity_file);
    }
    home_dir().join(".ssh").join("id_ed25519")
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return home_dir();
    }
    if let Some(rest) = value.strip_prefix("~/").or_else(|| value.strip_prefix("~\\")) {
        return home_dir().join(rest);
    }
    #[cfg(target_os = "windows")]
    if let Some(rest) = value
        .strip_prefix("%USERPROFILE%\\")
        .or_else(|| value.strip_prefix("%USERPROFILE%/"))
    {
        return home_dir().join(rest);
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(rest) = value.strip_prefix("$HOME/") {
        return home_dir().join(rest);
    }
    PathBuf::from(value)
}

fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
    .map(PathBuf::from)
    .unwrap_or_default()
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "windows")]
fn ssh_command() -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut command = Command::new("ssh.exe");
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(target_os = "windows"))]
fn ssh_command() -> Command {
    Command::new("ssh")
}

#[cfg(test)]
mod tests {
    use super::*;

    const UCI_SAMPLE: &str = r#"
passwall2.global=global
passwall2.global.node='node_us'
passwall2.node_us=nodes
passwall2.node_us.remarks='US Direct'
passwall2.node_us.type='Xray'
passwall2.node_us.protocol='vless'
passwall2.node_us.address='203.0.113.10'
passwall2.node_us.port='8443'
passwall2.node_jp=nodes
passwall2.node_jp.remarks='JP Vision'
passwall2.node_jp.type='sing-box'
passwall2.node_jp.protocol='hysteria2'
passwall2.node_jp.address='203.0.113.20'
passwall2.node_jp.port='443'
passwall2.acl_windows=acl_rule
passwall2.acl_windows.enabled='1'
passwall2.acl_windows.remarks='Windows'
passwall2.acl_windows.sources='192.0.2.10'
passwall2.acl_windows.mode='1'
passwall2.acl_windows.node='node_jp'
"#;

    #[test]
    fn parses_nodes_and_active_acl() {
        let inventory = parse_inventory(UCI_SAMPLE, "192.0.2.10", "").unwrap();
        assert_eq!(inventory.nodes.len(), 2);
        assert_eq!(inventory.acl_section, "acl_windows");
        assert!(matches!(
            inventory.selection,
            RouteSelection::Proxy { ref node_id, .. } if node_id == "node_jp"
        ));
    }

    #[test]
    fn parses_direct_mode() {
        let raw = UCI_SAMPLE.replace("mode='1'", "mode='0'");
        let inventory = parse_inventory(&raw, "192.0.2.10", "").unwrap();
        assert!(matches!(inventory.selection, RouteSelection::Direct));
    }

    #[test]
    fn shell_quotes_single_quotes() {
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn ssh_errors_are_localized_for_the_ui() {
        let cases = [
            (
                "Permission denied (publickey).",
                "SSH 免密认证失败，请重新安装公钥",
            ),
            (
                "ssh: connect to host 192.0.2.3 port 22: Connection timed out",
                "连接 OpenWrt 超时，请检查地址、防火墙和网络连通性",
            ),
            (
                "ssh: connect to host 192.0.2.3 port 22: Connection refused",
                "OpenWrt 拒绝了 SSH 连接，请检查 Dropbear 服务和 SSH 端口",
            ),
        ];

        for (raw, expected) in cases {
            let status = classify_ssh_error(raw);
            assert!(!status.ok);
            assert_eq!(status.message, expected);
            assert!(!status.message.contains(raw));
        }
    }
}
