use crate::config::Profile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Adapter {
    pub id: String,
    pub name: String,
    pub status: String,
}

pub fn validate_adapter_id(id: &str) -> Result<(), String> {
    let parts: Vec<_> = id.split('-').collect();
    if parts.len() != 5
        || parts.iter().zip([8, 4, 4, 4, 12]).any(|(part, length)| {
            part.len() != length || !part.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err("请在配置页选择要修改的网卡并保存".into());
    }
    Ok(())
}
pub fn list_adapters() -> Result<Vec<Adapter>, String> {
    #[cfg(target_os = "windows")]
    {
        crate::windows_network::list_adapters()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("此版本仅支持 Windows 网卡切换".into())
    }
}
pub fn apply(adapter_id: &str, profile: &Profile) -> Result<(), String> {
    validate_adapter_id(adapter_id)?;
    profile.validate()?;
    #[cfg(target_os = "windows")]
    {
        crate::windows_network::apply(adapter_id, profile)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("此版本仅支持 Windows 网卡切换".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapter_identifiers_cannot_change_the_command() {
        assert!(validate_adapter_id("7f0335c4-508b-40bc-bb78-a019059d50ce").is_ok());
        assert!(validate_adapter_id("").is_err());
        assert!(validate_adapter_id("7f0335c4-508b-40bc-bb78-a019059d50ce'; exit").is_err());
        assert!(validate_adapter_id("7f0335c4-508b-40bc-bb78").is_err());
    }
}
