use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, net::Ipv4Addr, path::Path};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub ip_address: String,
    pub subnet_mask: String,
    pub gateway: String,
    pub dns1: String,
    pub dns2: String,
}

impl Profile {
    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.ip_address = self.ip_address.trim().to_string();
        self.subnet_mask = self.subnet_mask.trim().to_string();
        self.gateway = self.gateway.trim().to_string();
        self.dns1 = self.dns1.trim().to_string();
        self.dns2 = self.dns2.trim().to_string();
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_name(&self.name)?;
        let ip = parse_address(&self.ip_address, "IP地址")?;
        let gateway = parse_address(&self.gateway, "网关")?;
        let prefix = mask_prefix(&self.subnet_mask)?;
        let mask = u32::MAX << (32 - prefix);
        let network = u32::from(ip) & mask;
        let broadcast = network | !mask;
        if u32::from(ip) == network || u32::from(ip) == broadcast {
            return Err("IP地址不能是子网的网络地址或广播地址".into());
        }
        if u32::from(gateway) & mask != network
            || u32::from(gateway) == network
            || u32::from(gateway) == broadcast
            || gateway == ip
        {
            return Err("网关必须是同一子网内的另一台设备地址".into());
        }
        if self.dns1.is_empty() && self.dns2.is_empty() {
            return Err("DNS1 和 DNS2 至少填写一个".into());
        }
        for (label, value) in [("DNS1", &self.dns1), ("DNS2", &self.dns2)] {
            if !value.is_empty() {
                parse_address(value, label)?;
            }
        }
        Ok(())
    }

    pub fn dns_servers(&self) -> Vec<String> {
        let mut servers = Vec::new();
        for value in [&self.dns1, &self.dns2] {
            if !value.is_empty() && !servers.contains(value) {
                servers.push(value.clone());
            }
        }
        servers
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.chars().count() > 80
        || name.chars().any(|c| c.is_control() || c == '[' || c == ']')
    {
        return Err("配置名称不能为空，最多 80 个字符，且不能包含方括号或换行".into());
    }
    Ok(())
}

fn parse_address(value: &str, label: &str) -> Result<Ipv4Addr, String> {
    let ip = value
        .parse::<Ipv4Addr>()
        .map_err(|_| format!("{label}不是有效的 IPv4 地址"))?;
    if ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.octets()[0] == 0
        || ip.octets()[0] >= 240
    {
        return Err(format!("{label}不能使用未指定、环回、组播或保留地址"));
    }
    Ok(ip)
}

pub fn mask_prefix(mask: &str) -> Result<u32, String> {
    let value = u32::from(
        mask.parse::<Ipv4Addr>()
            .map_err(|_| "子网掩码不是有效的 IPv4 地址")?,
    );
    let prefix = value.leading_ones();
    if !(1..=30).contains(&prefix) || value != u32::MAX << (32 - prefix) {
        return Err("子网掩码必须连续，范围为 /1 到 /30（例如 255.255.255.0）".into());
    }
    Ok(prefix)
}

pub fn validate_profiles(profiles: &[Profile]) -> Result<(), String> {
    if profiles.len() > 100 {
        return Err("最多保存 100 个配置".into());
    }
    let mut names = HashSet::new();
    for profile in profiles {
        profile
            .validate()
            .map_err(|error| format!("配置“{}”：{error}", profile.name))?;
        if !names.insert(&profile.name) {
            return Err(format!("配置名称“{}”重复", profile.name));
        }
    }
    Ok(())
}

pub fn parse(raw: &str) -> Result<Vec<Profile>, String> {
    if raw.len() > 512 * 1024 {
        return Err("配置文件不能超过 512 KiB".into());
    }
    let mut profiles = Vec::<Profile>::new();
    let mut keys = HashSet::<String>::new();
    for (index, line) in raw.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let fail = |message: &str| format!("第 {} 行：{message}", index + 1);
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].trim().to_string();
            validate_name(&name).map_err(|error| fail(&error))?;
            profiles.push(Profile {
                name,
                ..Profile::default()
            });
            keys.clear();
            continue;
        }
        let profile = profiles
            .last_mut()
            .ok_or_else(|| fail("请先写 [配置名称]"))?;
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| fail("字段格式应为 字段名=值"))?;
        let key = key.trim();
        if !keys.insert(key.to_string()) {
            return Err(fail("同一配置中不能重复填写字段"));
        }
        let target = match key {
            "IP地址" => &mut profile.ip_address,
            "子网掩码" => &mut profile.subnet_mask,
            "网关" => &mut profile.gateway,
            "DNS1" => &mut profile.dns1,
            "DNS2" => &mut profile.dns2,
            _ => return Err(fail("未知字段，只支持 IP地址、子网掩码、网关、DNS1、DNS2")),
        };
        *target = value.trim().to_string();
    }
    validate_profiles(&profiles)?;
    Ok(profiles)
}

pub fn load(path: &Path) -> Result<Vec<Profile>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => parse(&raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("读取配置文件失败：{error}")),
    }
}

pub fn serialize(profiles: &[Profile]) -> Result<String, String> {
    validate_profiles(profiles)?;
    let mut raw = String::new();
    for profile in profiles {
        raw.push_str(&format!(
            "[{}]\nIP地址={}\n子网掩码={}\n网关={}\nDNS1={}\nDNS2={}\n\n",
            profile.name,
            profile.ip_address,
            profile.subnet_mask,
            profile.gateway,
            profile.dns1,
            profile.dns2
        ));
    }
    Ok(raw)
}

pub fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, content).map_err(|error| format!("写入配置失败：{error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("保存配置失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Profile {
        Profile {
            name: "家里*本地连接".into(),
            ip_address: "192.168.1.10".into(),
            subnet_mask: "255.255.255.0".into(),
            gateway: "192.168.1.1".into(),
            dns1: "192.168.1.1".into(),
            dns2: String::new(),
        }
    }
    #[test]
    fn chinese_ini_roundtrips_and_preserves_order() {
        let direct = profile();
        let mut proxy = direct.clone();
        proxy.name = "家里*代理".into();
        proxy.gateway = "192.168.1.3".into();
        proxy.dns1.clear();
        proxy.dns2 = "192.168.1.3".into();
        let profiles = vec![direct, proxy];
        let raw = serialize(&profiles).unwrap();
        assert_eq!(parse(&raw).unwrap(), profiles);
        assert_eq!(
            parse(&format!("\u{feff}; 注释\r\n{}", raw.replace('\n', "\r\n"))).unwrap(),
            profiles
        );
        assert_eq!(profiles[1].dns_servers(), vec!["192.168.1.3"]);
    }
    #[test]
    fn rejects_ambiguous_sections_and_fields() {
        let raw = serialize(&[profile()]).unwrap();
        assert!(parse(&(raw.clone() + &raw)).unwrap_err().contains("重复"));
        assert!(parse(&raw.replace("网关=", "网关=192.168.1.1\n网关=")).is_err());
        assert!(parse(&raw.replace("DNS2=", "密码=")).is_err());
        assert!(parse("IP地址=192.168.1.10").is_err());
    }
    #[test]
    fn requires_dns_and_valid_subnet_relationship() {
        let mut value = profile();
        value.dns1.clear();
        assert!(value.validate().unwrap_err().contains("至少"));
        value.dns2 = "1.1.1.1".into();
        assert!(value.validate().is_ok());
        value.gateway = "192.168.2.1".into();
        assert!(value.validate().is_err());
        value.gateway = "192.168.1.1".into();
        value.ip_address = "192.168.1.255".into();
        assert!(value.validate().is_err());
        assert_eq!(mask_prefix("255.255.255.0").unwrap(), 24);
        assert!(mask_prefix("255.0.255.0").is_err());
        assert!(mask_prefix("0.0.0.0").is_err());
    }
    #[test]
    fn rejects_script_values_and_section_injection() {
        let mut value = profile();
        value.ip_address = "192.168.1.10'; Stop-Process".into();
        assert!(value.validate().is_err());
        value = profile();
        value.name = "家里]\n[其他".into();
        assert!(serialize(&[value]).is_err());
    }

    #[test]
    fn atomic_write_replaces_existing_ini_with_complete_new_content() {
        let folder =
            std::env::temp_dir().join(format!("switchcat-ini-test-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("config.ini");
        fs::write(&path, "old content").unwrap();
        let raw = serialize(&[profile()]).unwrap();
        write_atomic(&path, &raw).unwrap();
        assert_eq!(load(&path).unwrap(), vec![profile()]);
        assert!(!path.with_extension("tmp").exists());
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&folder).unwrap();
    }
}
