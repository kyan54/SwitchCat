# SwitchCat

一个简单的 Windows 托盘网络配置切换器：保存几组 IP、子网掩码、网关和 DNS，手动点选即可应用到指定网卡。

## 使用

1. 启动 SwitchCat，确认 Windows 的管理员授权提示（修改网卡所需）。
2. 在配置页手动选择目标网卡。软件会记住它，不会自动换到其他网卡。
3. 添加配置并填写名称、IP 地址、子网掩码、网关及至少一个 DNS，点击“保存配置”。
4. 右键托盘图标，点击配置名称即可切换；左键打开配置页。
5. 配置页也可以点击“应用此配置”。关闭窗口后，软件继续在托盘运行；退出请使用托盘菜单。

保存配置只写文件；只有点击配置名或“应用此配置”时才修改网卡。菜单上的勾表示本次运行中最近成功应用且内容未改变的配置，不代表自动检测到的当前网络状态。

切换完成后发送 Windows 通知：成功显示配置名称，失败显示原因。请安装发行版使用；通知的显示位置和是否弹出由 Windows 通知设置控制，勿扰模式可能将提示收进通知中心。

## 配置文件

配置页显示实际文件路径，通常为 %APPDATA%\com.kyan54.switchcat\config.ini 。

使用 UTF-8 INI 格式，示例见 [config.example.ini](config.example.ini)：

~~~ini
[家里*本地连接]
IP地址=192.168.1.10
子网掩码=255.255.255.0
网关=192.168.1.1
DNS1=192.168.1.1
DNS2=

[家里*代理]
IP地址=192.168.1.10
子网掩码=255.255.255.0
网关=192.168.1.3
DNS1=
DNS2=192.168.1.3
~~~

DNS1 和 DNS2 至少配置一个，DNS2 单独填写也可以。配置名不能重复，字段名按示例填写，IP、掩码、网关必须完整。网关需位于 IP 所在子网，支持连续的 /1 到 /30 子网掩码。配置的顺序就是菜单顺序。允许空行及以分号或井号开头的注释。

右键打开菜单及应用时均重新读取文件，因此也可以用文本编辑器修改配置。在配置页点击“重新读取”会显示最新内容。格式错误会明确提示，不会应用旧的缓存。

目标网卡的 GUID 单独保存在同目录的 settings.json，网卡改名不影响选择；设备被替换或删除时需要重新选择。没有自动识别环境、SSH、OpenWrt/PassWall2 读写、节点同步或定时刷新。

## 切换范围

每次只对手动选择的网卡应用配置中的 IPv4 地址、子网掩码、默认网关及 DNS。这些是静态 IPv4 配置：原先使用 DHCP 的目标网卡会改为静态配置。IPv6、其他网卡、远端路由器配置不变。参数已符合目标时跳过对应的写入。

切换完成后核验 IP、掩码、网关和 DNS；错误或超过 45 秒会提示失败并结束命令。失败可能发生在部分参数已更新之后，请检查网卡后重新应用配置。

版本 0.2.0 起为 Windows 手动切换版，旧版 config.toml 和 known_hosts 不再读取，也不会删除。升级后请在新配置页选择网卡并重新保存配置。

## 下载与构建

从 [Releases](https://github.com/kyan54/SwitchCat/releases) 下载 Windows x64 EXE 或 MSI 安装包。

开发需要 Node.js 22、Rust stable MSVC 及 Tauri 2 Windows 构建依赖：

~~~powershell
npm ci
npm run check
npm run tauri dev
~~~

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
~~~

main 分支提交会由 GitHub Actions 执行前端检查、Rust 测试，并构建和发布当前版本的 Windows 安装包。

## 日志

日志通常位于 %LOCALAPPDATA%\com.kyan54.switchcat\logs\SwitchCat.log 。

记录应用总耗时、Windows 命令耗时，以及网卡读取、参数读取、地址与网关写入、DNS 写入、结果核验各阶段的耗时和是否发生修改。日志时间使用 UTC，台北时间加 8 小时。

## License

[MIT](LICENSE)
