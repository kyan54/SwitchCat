# SwitchCat

一只常驻系统托盘、一直在奔跑的小猫，用来快速切换 OpenWrt PassWall2 的本地直连和代理节点。

- Windows：常驻任务栏右下角通知区域
- macOS：常驻右上角菜单栏
- Linux：支持 AppIndicator 的桌面环境托盘
- 右键（可配置为左键）直接看到“本地直连 + OpenWrt 当前全部节点”
- 节点由 SSH 实时读取，OpenWrt 增删或改名后自动同步
- 支持多个环境，例如“家里”和“公司”，每个环境使用各自的 OpenWrt、客户端 IP、ACL 和 SSH 主机指纹
- 可按 Wi-Fi SSID、默认网关或本地 CIDR 自动识别环境
- 保存配置后仍不会自动连接；每个环境必须手动完成一次“测试并启用 SSH”

## 工作方式

SwitchCat 不在电脑上启动代理内核，也不保存 OpenWrt 密码。它使用系统 OpenSSH 连接 OpenWrt，通过 UCI 找到当前电脑的 PassWall2 ACL：

- 选择“本地直连”：将该 ACL 设置为 `mode=0`（No Proxy）
- 选择代理节点：将该 ACL 设置为 `mode=1` 并更新 `node`
- 每次更改后提交 UCI 并重载 PassWall2，然后重新读取配置确认结果

Windows 版还会同步修改拥有 `client_ip` 的网卡：选择代理时使用 `proxy_gateway/proxy_dns` 并关闭该网卡 IPv6；选择本地直连时使用 `direct_gateway/direct_dns` 并恢复 IPv6。Windows 版因此需要以管理员身份运行，本机固定 IPv4 保持不变。

四项参数显示在每个环境主配置中的“Windows 网络切换”区域。DNS 可留空并自动使用同组网关；本地网关必须明确填写，缺少参数时对应线路按钮会保持禁用。

macOS/Linux 当前只切换 ACL，建议网关和 DNS 始终指向 OpenWrt。若使用过 v2rayN、Clash 等其他代理软件，请先关闭其系统代理/PAC，避免形成两层代理。

## 安装

前往 [Releases](https://github.com/kyan54/SwitchCat/releases) 下载对应平台安装包：

| 平台 | 安装包 |
| --- | --- |
| Windows x64 | `.msi` 或 NSIS `.exe` |
| macOS Apple Silicon | `.dmg`（aarch64） |
| macOS Intel | `.dmg`（x86_64） |
| Linux x64 | `.AppImage`、`.deb` 或 `.rpm` |

当前自动构建未使用商业代码签名证书。Windows SmartScreen 或 macOS Gatekeeper 可能显示“未知开发者”；请只从本仓库 Releases 下载并核对来源。

## OpenWrt 前提

1. 已安装并启用 PassWall2。
2. 为每台电脑建立一条单独的 ACL，`Source` 使用该电脑固定 IPv4。
3. ACL 应启用，TCP/UDP 代理端口按照你的 PassWall2 方案设置。
4. OpenWrt 已启用 Dropbear SSH，电脑能访问其 SSH 端口。
5. Windows 环境正确填写本地路由器/OpenWrt 两组网关和 DNS；macOS/Linux 的网关和 DNS 指向 OpenWrt。

SwitchCat 优先按 `client_ip` 精确匹配 ACL；`acl_remarks` 只作为可选兜底。若匹配到多条 ACL，会拒绝切换以避免改错规则。

## 首次配置

首次启动会自动打开设置页：

1. 填写环境名称、OpenWrt 地址、SSH 用户/端口、本机固定 IPv4。
2. 保存 `config.toml`。
3. SwitchCat 会自动弹出当前平台的 SSH 页面；如果系统 SSH 已经免密，直接点击“测试现有 SSH”。
4. 只有测试失败时，才按顺序复制三段命令：生成密钥、安装公钥、验证连接。
5. 测试成功后才会读取全部节点，并启动该环境的定时同步。

在 SSH 验证成功之前，该环境的“本地直连”、代理节点、刷新和环境切换均不可操作。修改 OpenWrt 地址、SSH 用户/端口、私钥路径、本机 IP 或 ACL 备注后，验证状态会自动失效，需要重新测试；这可以避免程序在配置尚未完成时连接错误的设备。

私钥路径留空时，SwitchCat 优先复用系统 OpenSSH 配置、ssh-agent 和默认密钥，并在尚未创建环境专用指纹时复用系统 `known_hosts`。若通过应用内引导配置，则会为环境维护独立的 `known_hosts`，便于家里和公司使用相同 LAN 地址但不同主机指纹。若指纹发生变化，连接会被拒绝。

### Windows 手动免密配置

打开 PowerShell。Windows 10/11 通常已内置 OpenSSH；若 `ssh` 不存在，请用管理员 PowerShell 安装：

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Client~~~~0.0.1.0
```

以下是普通 OpenSSH 示例；`192.0.2.3` 是文档专用地址，必须替换。应用内引导会按照你的环境生成包含独立 `known_hosts` 的完整命令：

```powershell
$Key = "$env:USERPROFILE\.ssh\id_ed25519"
New-Item -ItemType Directory -Force (Split-Path $Key) | Out-Null
if (!(Test-Path $Key)) { ssh-keygen -t ed25519 -f $Key }

Get-Content "$Key.pub" | ssh -p 22 root@192.0.2.3 `
  "umask 077; mkdir -p /etc/dropbear; cat >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys"

ssh -p 22 -i $Key -o BatchMode=yes root@192.0.2.3 "echo SSH_OK"
```

安装公钥时会要求一次 OpenWrt `root` 密码。密钥若设置了口令，后台刷新时还需要系统 `ssh-agent` 已加载该密钥；追求完全静默运行时可在生成密钥时将口令留空。

### macOS 手动免密配置

打开“应用程序 → 实用工具 → 终端”。macOS 已内置 OpenSSH：

```bash
KEY="$HOME/.ssh/id_ed25519"
mkdir -p "$(dirname "$KEY")"
[ -f "$KEY" ] || ssh-keygen -t ed25519 -f "$KEY"

cat "$KEY.pub" | ssh -p 22 -i "$KEY" root@192.0.2.3 \
  'umask 077; mkdir -p /etc/dropbear; cat >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys'

ssh -p 22 -i "$KEY" -o BatchMode=yes root@192.0.2.3 'echo SSH_OK'
```

同样只在安装公钥时输入一次 OpenWrt 密码。应用内命令还会单独保存并校验该环境的 SSH 主机指纹。

## 多环境

在设置页点击“添加环境”，可分别配置。表中的地址来自 IETF 文档专用网段，不能直接使用：

| 字段 | 家里示例 | 公司示例 |
| --- | --- | --- |
| OpenWrt | `192.0.2.3` | `198.51.100.3` |
| 本机固定 IP / ACL Source | `192.0.2.10` | `198.51.100.42` |
| 自动识别 SSID | `HomeWiFi` | `OfficeWiFi` |
| 自动识别 CIDR | `192.0.2.0/24` | `198.51.100.0/24` |

托盘的“切换环境”子菜单可手动选择。只有已启用并完成 SSH 验证的环境可以切换；自动识别也只考虑这些环境，并会在切换前再次确认 OpenWrt 可连接。

完整格式见 [`config.example.toml`](config.example.toml)。实际位置可以从设置页查看：

- Windows：`%APPDATA%\com.kyan54.switchcat\config.toml`（具体路径以应用显示为准）
- macOS：`~/Library/Application Support/com.kyan54.switchcat/config.toml`
- Linux：`~/.config/com.kyan54.switchcat/config.toml`

## 开发

需要 Node.js 20+、Rust stable 和 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
npm install
npm run check
npm run tauri dev
```

构建安装包：

```bash
npm run tauri build
```

Rust 单元测试覆盖 UCI 节点/ACL 解析、CIDR 环境匹配和动态猫图标帧：

```bash
cd src-tauri
cargo test
```

## 自动发布

`.github/workflows/build.yml` 会在提交和 Pull Request 时构建 Windows、Linux、macOS Intel、macOS Apple Silicon。合并到 `main` 后，工作流会读取应用版本并自动创建对应的 `v*` GitHub Release，同时上传全部安装包；同一版本已发布时不会重复创建。

## 安全说明

- 不保存 OpenWrt 密码或私钥内容，只保存私钥路径。
- SSH 使用 `BatchMode=yes`，托盘后台任务不会弹出密码输入框。
- 首次主机指纹只能通过明确的安装步骤接受；日常连接使用严格校验。
- UCI section/node ID 经过白名单校验后才会进入远程命令。
- 建议只允许可信 LAN 访问 OpenWrt SSH，不要将 Dropbear 直接暴露到公网。

## License

[MIT](LICENSE)
