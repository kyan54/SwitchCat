use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, net::Ipv4Addr, path::Path};

pub const CONFIG_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub app: AppSettings,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_active_profile")]
    pub active_profile: String,
    #[serde(default = "default_true")]
    pub auto_detect_profile: bool,
    #[serde(default = "default_refresh_seconds")]
    pub refresh_seconds: u64,
    #[serde(default = "default_true")]
    pub start_at_login: bool,
    #[serde(default = "default_true")]
    pub animate_cat: bool,
    #[serde(default = "default_true")]
    pub menu_on_left_click: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub ssh_verified: bool,
    #[serde(default)]
    pub openwrt: OpenWrtSettings,
    #[serde(default)]
    pub device: DeviceSettings,
    #[serde(default)]
    pub detect: DetectSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenWrtSettings {
    #[serde(default = "default_openwrt_host")]
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default = "default_ssh_user")]
    pub user: String,
    #[serde(default)]
    pub identity_file: String,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSettings {
    #[serde(default)]
    pub client_ip: String,
    #[serde(default)]
    pub acl_remarks: String,
    #[serde(default = "default_direct_gateway")]
    pub direct_gateway: String,
    #[serde(default = "default_openwrt_host")]
    pub proxy_gateway: String,
    #[serde(default = "default_direct_gateway")]
    pub direct_dns: String,
    #[serde(default = "default_openwrt_host")]
    pub proxy_dns: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetectSettings {
    #[serde(default)]
    pub ssids: Vec<String>,
    #[serde(default)]
    pub gateways: Vec<String>,
    #[serde(default)]
    pub local_cidrs: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::with_local_ip(String::new())
    }
}

impl AppConfig {
    pub fn with_local_ip(local_ip: String) -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "home".to_string(),
            Profile {
                name: "家里".to_string(),
                enabled: true,
                ssh_verified: false,
                openwrt: OpenWrtSettings::default(),
                device: DeviceSettings {
                    client_ip: local_ip,
                    ..DeviceSettings::default()
                },
                detect: DetectSettings {
                    ..DetectSettings::default()
                },
            },
        );

        Self {
            version: CONFIG_VERSION,
            app: AppSettings::default(),
            profiles,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != CONFIG_VERSION {
            return Err(format!(
                "不支持的配置版本 {}，当前版本需要 {}",
                self.version, CONFIG_VERSION
            ));
        }
        if self.profiles.is_empty() {
            return Err("至少需要配置一个环境".to_string());
        }
        if !self.profiles.contains_key(&self.app.active_profile) {
            return Err("当前环境不存在，请重新选择".to_string());
        }
        if !self
            .profiles
            .get(&self.app.active_profile)
            .is_some_and(|profile| profile.enabled)
        {
            return Err("当前环境已禁用，请先选择一个启用的环境".to_string());
        }
        if !(10..=3600).contains(&self.app.refresh_seconds) {
            return Err("节点刷新间隔必须在 10 到 3600 秒之间".to_string());
        }

        for (id, profile) in &self.profiles {
            if !valid_profile_id(id) {
                return Err(format!(
                    "环境 ID“{id}”无效，只能包含英文、数字、短横线和下划线"
                ));
            }
            if profile.name.trim().is_empty() || profile.name.chars().any(char::is_control) {
                return Err(format!("环境“{id}”缺少显示名称"));
            }
            if !valid_ssh_host(&profile.openwrt.host) {
                return Err(format!("环境“{}”的 OpenWrt 地址无效", profile.name));
            }
            if !valid_ssh_user(&profile.openwrt.user) {
                return Err(format!("环境“{}”的 SSH 用户无效", profile.name));
            }
            if profile.openwrt.port == 0 {
                return Err(format!("环境“{}”的 SSH 端口无效", profile.name));
            }
            if !(2..=30).contains(&profile.openwrt.connect_timeout_seconds) {
                return Err(format!("环境“{}”的 SSH 超时必须在 2 到 30 秒之间", profile.name));
            }
            if profile.openwrt.identity_file.chars().any(char::is_control) {
                return Err(format!("环境“{}”的私钥路径无效", profile.name));
            }
            if profile.device.client_ip.parse::<Ipv4Addr>().is_err() {
                return Err(format!(
                    "环境“{}”的本机 IPv4 地址无效：{}",
                    profile.name, profile.device.client_ip
                ));
            }
            for gateway in &profile.detect.gateways {
                if gateway.parse::<Ipv4Addr>().is_err() {
                    return Err(format!("环境“{}”的自动识别网关无效：{gateway}", profile.name));
                }
            }
            for cidr in &profile.detect.local_cidrs {
                if !valid_ipv4_cidr(cidr) {
                    return Err(format!("环境“{}”的 CIDR 无效：{cidr}", profile.name));
                }
            }
        }
        Ok(())
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            active_profile: default_active_profile(),
            auto_detect_profile: true,
            refresh_seconds: default_refresh_seconds(),
            start_at_login: true,
            animate_cat: true,
            menu_on_left_click: true,
        }
    }
}

impl Default for OpenWrtSettings {
    fn default() -> Self {
        Self {
            host: default_openwrt_host(),
            port: default_ssh_port(),
            user: default_ssh_user(),
            identity_file: String::new(),
            connect_timeout_seconds: default_connect_timeout(),
        }
    }
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            client_ip: String::new(),
            acl_remarks: String::new(),
            direct_gateway: default_direct_gateway(),
            proxy_gateway: default_openwrt_host(),
            direct_dns: default_direct_gateway(),
            proxy_dns: default_openwrt_host(),
        }
    }
}

pub fn load(path: &Path) -> Result<Option<AppConfig>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("读取配置文件失败（{}）：{error}", path.display()))?;
    let config: AppConfig =
        toml::from_str(&raw).map_err(|error| format!("配置文件格式错误：{error}"))?;
    config.validate()?;
    Ok(Some(config))
}

pub fn save(path: &Path, config: &AppConfig) -> Result<(), String> {
    config.validate()?;
    let parent = path
        .parent()
        .ok_or_else(|| "无法确定配置目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建配置目录失败：{error}"))?;

    let raw = toml::to_string_pretty(config).map_err(|error| format!("序列化配置失败：{error}"))?;
    fs::write(path, raw).map_err(|error| format!("保存配置失败：{error}"))?;
    Ok(())
}

fn valid_profile_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn valid_ssh_host(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
}

fn valid_ssh_user(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn valid_ipv4_cidr(value: &str) -> bool {
    let Some((network, prefix)) = value.split_once('/') else {
        return false;
    };
    network.parse::<Ipv4Addr>().is_ok()
        && prefix.parse::<u8>().is_ok_and(|prefix| prefix <= 32)
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

fn default_true() -> bool {
    true
}

fn default_active_profile() -> String {
    "home".to_string()
}

fn default_refresh_seconds() -> u64 {
    60
}

fn default_openwrt_host() -> String {
    String::new()
}

fn default_direct_gateway() -> String {
    String::new()
}

fn default_ssh_port() -> u16 {
    22
}

fn default_ssh_user() -> String {
    "root".to_string()
}

fn default_connect_timeout() -> u64 {
    6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_profile_is_valid() {
        let mut config = AppConfig::with_local_ip("192.0.2.10".to_string());
        let profile = config.profiles.get_mut("home").unwrap();
        profile.openwrt.host = "192.0.2.3".to_string();
        profile.device.proxy_gateway = "192.0.2.3".to_string();
        profile.device.proxy_dns = "192.0.2.3".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn profile_ids_are_restricted() {
        assert!(valid_profile_id("home_2"));
        assert!(!valid_profile_id("home/work"));
        assert!(!valid_profile_id("公司"));
    }

    #[test]
    fn existing_config_requires_explicit_ssh_verification() {
        let raw = r#"
version = 2

[app]
active_profile = "home"

[profiles.home]
name = "家里"
enabled = true

[profiles.home.openwrt]
host = "192.0.2.3"

[profiles.home.device]
client_ip = "192.0.2.10"
"#;
        let config: AppConfig = toml::from_str(raw).unwrap();
        assert!(!config.profiles["home"].ssh_verified);
        assert!(config.validate().is_ok());
    }
}
