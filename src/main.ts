import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type StringMap<T> = Record<string, T>;

interface AppSettings {
  active_profile: string;
  auto_detect_profile: boolean;
  refresh_seconds: number;
  start_at_login: boolean;
  animate_cat: boolean;
  menu_on_left_click: boolean;
}

interface OpenWrtSettings {
  host: string;
  port: number;
  user: string;
  identity_file: string;
  connect_timeout_seconds: number;
}

interface DeviceSettings {
  client_ip: string;
  acl_remarks: string;
  direct_gateway: string;
  proxy_gateway: string;
  direct_dns: string;
  proxy_dns: string;
}

interface DetectSettings {
  ssids: string[];
  gateways: string[];
  local_cidrs: string[];
}

interface Profile {
  name: string;
  enabled: boolean;
  ssh_verified: boolean;
  openwrt: OpenWrtSettings;
  device: DeviceSettings;
  detect: DetectSettings;
}

interface AppConfig {
  version: number;
  app: AppSettings;
  profiles: StringMap<Profile>;
}

interface ProxyNode {
  id: string;
  name: string;
  core: string;
  protocol: string;
  address: string;
  port: string;
}

type RouteSelection =
  | { kind: "direct" }
  | { kind: "proxy"; node_id: string; node_name: string }
  | { kind: "global"; node_id: string; node_name: string }
  | { kind: "unknown" };

interface Inventory {
  nodes: ProxyNode[];
  acl_section: string;
  acl_remarks: string;
  selection: RouteSelection;
}

interface ProfileRuntime {
  inventory: Inventory | null;
  last_error: string | null;
  last_refreshed_unix: number | null;
  busy: boolean;
}

interface NetworkSnapshot {
  ipv4_addresses: string[];
  gateway: string | null;
  ssid: string | null;
}

interface Bootstrap {
  config_exists: boolean;
  config_path: string;
  config_dir: string;
  platform: "windows" | "macos" | "linux" | string;
  config: AppConfig;
  runtime: StringMap<ProfileRuntime>;
  network: NetworkSnapshot;
}

interface SshStatus {
  ok: boolean;
  kind: string;
  message: string;
}

interface SshInstructions {
  platform: string;
  keygen_command: string;
  install_command: string;
  verify_command: string;
  known_hosts_file: string;
  identity_file: string;
}

interface StateChanged {
  profile_id: string;
  operation: string;
  ok: boolean;
  message: string;
}

const appRoot = document.querySelector<HTMLDivElement>("#app")!;
const modalRoot = document.querySelector<HTMLDivElement>("#modal-root")!;
const toastRoot = document.querySelector<HTMLDivElement>("#toast-root")!;

let bootstrap: Bootstrap;
let draft: AppConfig;
let selectedProfileId = "";
let dirty = false;
let busyAction = "";
let firstRun = false;
let refreshTimer: number | undefined;

const h = (value: unknown): string =>
  String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");

const clone = <T>(value: T): T => structuredClone(value);
const checked = (value: boolean): string => (value ? "checked" : "");
const disabled = (value: boolean): string => (value ? "disabled" : "");

function routeName(selection?: RouteSelection): string {
  if (!selection || selection.kind === "unknown") return "状态未知";
  if (selection.kind === "direct") return "本地直连";
  if (selection.kind === "global") return `全局：${selection.node_name}`;
  return selection.node_name;
}

function nodeMeta(node: ProxyNode): string {
  const technology = [node.core, node.protocol].filter(Boolean).join("/") || "未知协议";
  const endpoint = [node.address, node.port].filter(Boolean).join(":");
  return `[${technology}]${endpoint ? ` ${endpoint}` : ""}`;
}

function splitList(value: string): string[] {
  return value
    .split(/[\n,，]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

function platformName(platform: string): string {
  if (platform === "windows") return "Windows PowerShell";
  if (platform === "macos") return "macOS 终端";
  return "Linux 终端";
}

function formatTime(unix: number | null): string {
  if (!unix) return "尚未刷新";
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(unix * 1000));
}

function selectionNodeId(selection?: RouteSelection): string | null {
  if (selection?.kind === "proxy" || selection?.kind === "global") return selection.node_id;
  return null;
}

function render(): void {
  const profile = draft.profiles[selectedProfileId];
  if (!profile) {
    selectedProfileId = Object.keys(draft.profiles)[0] ?? "";
  }
  const selected = draft.profiles[selectedProfileId];
  const activeId = draft.app.active_profile;
  const activeProfile = draft.profiles[activeId];
  const runtime = bootstrap.runtime[activeId];
  const activeReady = Boolean(activeProfile?.enabled && activeProfile?.ssh_verified);
  const inventory = activeReady ? runtime?.inventory : undefined;
  const selection = inventory?.selection;
  const isActiveSelection = selectedProfileId === activeId;

  appRoot.innerHTML = `
    <div class="app-shell">
      <aside class="sidebar">
        <div class="brand">
          <div class="cat-stage" aria-label="一只正在奔跑的小猫">
            <div class="cat-emoji">🐈</div><i></i><i></i><i></i>
          </div>
          <div><strong>SwitchCat</strong><span>PassWall2 tray switcher</span></div>
        </div>

        <div class="sidebar-label">环境</div>
        <nav class="profile-list" aria-label="环境列表">
          ${Object.entries(draft.profiles)
            .map(([id, item]) => {
              const itemRuntime = bootstrap.runtime[id];
              const itemRoute = !item.enabled
                ? "环境未启用"
                : !item.ssh_verified
                  ? "等待 SSH 验证"
                  : routeName(itemRuntime?.inventory?.selection);
              return `<button class="profile-item ${id === selectedProfileId ? "selected" : ""}" data-action="select-profile" data-profile-id="${h(id)}">
                <span class="profile-dot ${itemRuntime?.last_error ? "error" : itemRuntime?.inventory ? "online" : ""}"></span>
                <span class="profile-copy"><strong>${h(item.name)}</strong><small>${h(itemRoute)}</small></span>
                ${id === activeId ? '<span class="active-pill">当前</span>' : ""}
              </button>`;
            })
            .join("")}
        </nav>
        <button class="add-profile" data-action="add-profile"><span>＋</span> 添加环境</button>

        <div class="sidebar-foot">
          <div class="network-mini"><span>本机网络</span><strong>${h(bootstrap.network.ssid || bootstrap.network.gateway || "未识别")}</strong></div>
          <button class="quiet-button" data-action="hide">隐藏到托盘</button>
        </div>
      </aside>

      <main class="workspace">
        <header class="topbar">
          <div>
            <div class="eyebrow">当前环境 · ${h(activeProfile?.name || "未配置")}</div>
            <h1>${h(!activeProfile?.enabled ? "环境未启用" : !activeProfile?.ssh_verified ? "等待 SSH 验证" : routeName(selection))}</h1>
          </div>
          <div class="topbar-actions">
            <button class="secondary" data-action="refresh" ${disabled(!activeReady || Boolean(runtime?.busy))}>
              ${runtime?.busy ? '<span class="spinner"></span> 正在读取' : "↻ 刷新节点"}
            </button>
            <button class="primary" data-action="save" ${disabled(!dirty || Boolean(busyAction))}>${dirty ? "保存配置" : "已保存"}</button>
          </div>
        </header>

        <div class="content-scroll">
          ${firstRun ? onboardingCard() : ""}
          ${selected ? profileEditor(selectedProfileId, selected, isActiveSelection) : emptyState()}
          ${activeProfile ? routePanel(activeId, activeProfile, runtime) : ""}
          ${generalPanel()}
        </div>
      </main>
    </div>`;
}

function onboardingCard(): string {
  return `<section class="onboarding-card">
    <div class="onboarding-number">1</div>
    <div>
      <span class="kicker">首次配置</span>
      <h2>先告诉小猫要连接哪台 OpenWrt</h2>
      <p>填写环境地址、本机固定 IPv4 和 PassWall2 ACL 信息，保存后按照 ${h(platformName(bootstrap.platform))} 引导完成一次 SSH 免密配置。</p>
      <div class="step-row"><span class="current">1 配置环境</span><i></i><span>2 安装 SSH 公钥</span><i></i><span>3 读取节点</span></div>
    </div>
  </section>`;
}

function profileEditor(profileId: string, profile: Profile, isActive: boolean): string {
  const networkIps = bootstrap.network.ipv4_addresses;
  return `<section class="panel profile-panel">
    <div class="panel-heading">
      <div><span class="kicker">环境配置</span><h2>${h(profile.name)}</h2></div>
      <div class="heading-actions">
        ${isActive ? '<span class="status-chip success">✓ 当前环境</span>' : `<button class="secondary compact" data-action="activate" data-profile-id="${h(profileId)}" ${disabled(!profile.enabled || !profile.ssh_verified || dirty)}>切换到此环境</button>`}
        ${Object.keys(draft.profiles).length > 1 ? `<button class="danger-link" data-action="delete-profile" data-profile-id="${h(profileId)}">删除</button>` : ""}
      </div>
    </div>

    <div class="form-grid two">
      ${field("环境名称", "profile.name", profile.name, "例如：家里、公司")}
      ${field("环境 ID", "profile-id", profileId, "仅用于配置文件", "text", true)}
      ${field("OpenWrt 地址", "profile.openwrt.host", profile.openwrt.host, "OpenWrt 的 LAN IPv4 或主机名")}
      ${field("SSH 用户", "profile.openwrt.user", profile.openwrt.user, "通常为 root")}
      ${field("SSH 端口", "profile.openwrt.port", profile.openwrt.port, "22", "number")}
      ${field("私钥路径（留空自动使用）", "profile.openwrt.identity_file", profile.openwrt.identity_file, defaultKeyHint())}
      ${field("本机固定 IPv4", "profile.device.client_ip", profile.device.client_ip, networkIps[0] || "此电脑的固定 IPv4")}
      ${field("ACL 备注（可选兜底）", "profile.device.acl_remarks", profile.device.acl_remarks, "例如 Windows / macbook")}
    </div>

    <div class="callout info">
      <strong>网络前提</strong>
      <span>此环境下，本机网关和 DNS 应固定指向 <code>${h(profile.openwrt.host)}</code>。菜单里的“本地直连”会把对应 ACL 切到 No Proxy，不会反复修改系统网卡。</span>
    </div>

    <details class="advanced">
      <summary>环境自动识别与网络参考</summary>
      <div class="form-grid two detail-fields">
        ${textareaField("Wi-Fi SSID（每行一个）", "profile.detect.ssids", profile.detect.ssids.join("\n"), "HomeWiFi\nOfficeWiFi")}
        ${textareaField("本地网段 CIDR（每行一个）", "profile.detect.local_cidrs", profile.detect.local_cidrs.join("\n"), "例如：192.0.2.0/24（请替换）")}
        ${textareaField("默认网关（每行一个）", "profile.detect.gateways", profile.detect.gateways.join("\n"), "此环境实际网关")}
        <div class="mini-grid">
          ${field("本地路由器", "profile.device.direct_gateway", profile.device.direct_gateway, "主路由 LAN IPv4")}
          ${field("OpenWrt 网关", "profile.device.proxy_gateway", profile.device.proxy_gateway, profile.openwrt.host)}
          ${field("本地 DNS", "profile.device.direct_dns", profile.device.direct_dns, "本地 DNS IPv4")}
          ${field("OpenWrt DNS", "profile.device.proxy_dns", profile.device.proxy_dns, profile.openwrt.host)}
        </div>
      </div>
    </details>

    <div class="panel-footer">
      <div><label class="switch-label"><input type="checkbox" data-field="profile.enabled" ${checked(profile.enabled)}><span class="switch"></span>启用此环境</label><span class="status-chip ${profile.ssh_verified ? "success" : "warning"}">${profile.ssh_verified ? "✓ SSH 已验证" : "SSH 未验证"}</span></div>
      <div>
        <button class="secondary" data-action="ssh-guide" data-profile-id="${h(profileId)}">SSH 免密引导</button>
        <button class="secondary" data-action="check-ssh" data-profile-id="${h(profileId)}" ${disabled(!profile.enabled || Boolean(busyAction))}>${profile.ssh_verified ? "重新验证 SSH" : "测试并启用 SSH"}</button>
      </div>
    </div>
  </section>`;
}

function routePanel(activeId: string, profile: Profile, runtime?: ProfileRuntime): string {
  const routeReady = profile.enabled && profile.ssh_verified;
  const inventory = routeReady ? runtime?.inventory : undefined;
  const currentNode = selectionNodeId(inventory?.selection);
  return `<section class="panel route-panel">
    <div class="panel-heading">
      <div><span class="kicker">托盘菜单预览</span><h2>${h(profile.name)} 的线路</h2></div>
      <span class="refresh-time">${h(formatTime(runtime?.last_refreshed_unix ?? null))}</span>
    </div>
    ${!profile.enabled ? '<div class="callout info"><strong>环境未启用</strong><span>启用并保存此环境后，才能验证 SSH 和切换线路。</span></div>' : !profile.ssh_verified ? `<div class="callout info"><strong>等待 SSH 验证</strong><span>完成免密配置并点击“测试并启用 SSH”后，才会读取节点和开放线路切换。</span></div>` : runtime?.last_error ? `<div class="callout error"><strong>读取失败</strong><span>${h(runtime.last_error)}</span><button class="text-button" data-action="ssh-guide" data-profile-id="${h(activeId)}">查看 SSH 配置</button></div>` : ""}
    <div class="route-list">
      <button class="route-item ${inventory?.selection.kind === "direct" ? "active" : ""}" data-action="switch-direct" ${disabled(!routeReady || !inventory || Boolean(runtime?.busy) || Boolean(busyAction) || dirty)}>
        <span class="route-icon direct">⌂</span><span><strong>本地直连</strong><small>PassWall2 ACL · No Proxy</small></span>${inventory?.selection.kind === "direct" ? "<b>✓</b>" : ""}
      </button>
      ${(inventory?.nodes ?? [])
        .map(
          (node) => `<button class="route-item ${currentNode === node.id ? "active" : ""}" data-action="switch-node" data-node-id="${h(node.id)}" ${disabled(!routeReady || Boolean(runtime?.busy) || Boolean(busyAction) || dirty)}>
            <span class="route-icon proxy">↗</span><span><strong>${h(node.name)}</strong><small>${h(nodeMeta(node))}</small></span>${currentNode === node.id ? "<b>✓</b>" : ""}
          </button>`,
        )
        .join("")}
      ${!routeReady ? '<div class="route-empty">线路切换尚未启用</div>' : !inventory && !runtime?.last_error ? '<div class="route-empty"><span class="spinner dark"></span> 正在等待首次读取…</div>' : ""}
      ${inventory && inventory.nodes.length === 0 ? '<div class="route-empty">OpenWrt 中没有可用的 PassWall2 节点</div>' : ""}
    </div>
  </section>`;
}

function generalPanel(): string {
  return `<section class="panel general-panel">
    <div class="panel-heading"><div><span class="kicker">SwitchCat</span><h2>通用设置</h2></div><code class="path" title="${h(bootstrap.config_path)}">config.toml</code></div>
    <div class="settings-list">
      ${toggleSetting("开机自动运行", "app.start_at_login", draft.app.start_at_login, "启动后安静待在任务栏 / 菜单栏")}
      ${toggleSetting("奔跑动画", "app.animate_cat", draft.app.animate_cat, "让托盘里的小猫一直跑")}
      ${toggleSetting("左键也打开菜单", "app.menu_on_left_click", draft.app.menu_on_left_click, "关闭后仅右键显示线路菜单")}
      ${toggleSetting("自动识别家里 / 公司", "app.auto_detect_profile", draft.app.auto_detect_profile, "按照 SSID、默认网关和 CIDR 自动选择环境")}
      <label class="setting-row"><span><strong>刷新间隔</strong><small>定期同步 OpenWrt 新增或删除的节点</small></span><span class="inline-number"><input type="number" min="10" max="3600" data-field="app.refresh_seconds" value="${h(draft.app.refresh_seconds)}"><em>秒</em></span></label>
    </div>
  </section>`;
}

function field(label: string, path: string, value: string | number, placeholder = "", type = "text", readonly = false): string {
  return `<label class="field"><span>${h(label)}</span><input type="${h(type)}" data-field="${h(path)}" value="${h(value)}" placeholder="${h(placeholder)}" ${readonly ? "readonly" : ""}></label>`;
}

function textareaField(label: string, path: string, value: string, placeholder = ""): string {
  return `<label class="field"><span>${h(label)}</span><textarea data-field="${h(path)}" placeholder="${h(placeholder)}">${h(value)}</textarea></label>`;
}

function toggleSetting(label: string, path: string, value: boolean, description: string): string {
  return `<label class="setting-row"><span class="setting-copy"><strong>${h(label)}</strong><small>${h(description)}</small></span><span class="toggle-control"><input class="toggle-input" type="checkbox" data-field="${h(path)}" aria-label="${h(label)}" ${checked(value)}><span class="switch" aria-hidden="true"></span></span></label>`;
}

function defaultKeyHint(): string {
  return bootstrap.platform === "windows" ? "%USERPROFILE%\\.ssh\\id_ed25519" : "~/.ssh/id_ed25519";
}

function emptyState(): string {
  return `<section class="panel empty"><h2>还没有环境</h2><button class="primary" data-action="add-profile">添加环境</button></section>`;
}

function setDraftField(path: string, input: HTMLInputElement | HTMLTextAreaElement): void {
  const value = input instanceof HTMLInputElement && input.type === "checkbox" ? input.checked : input.value;
  if (path.startsWith("app.")) {
    const key = path.slice(4) as keyof AppSettings;
    if (key === "refresh_seconds") draft.app[key] = Number(value) || 0;
    else (draft.app[key] as string | boolean) = value as string | boolean;
  } else if (path.startsWith("profile.")) {
    const profile = draft.profiles[selectedProfileId];
    if (!profile) return;
    const keys = path.slice(8).split(".");
    let target = profile as unknown as Record<string, unknown>;
    for (const key of keys.slice(0, -1)) target = target[key] as Record<string, unknown>;
    const key = keys.at(-1)!;
    if (path.endsWith(".port") || path.endsWith("connect_timeout_seconds")) target[key] = Number(value) || 0;
    else if (path.includes(".detect.")) target[key] = splitList(String(value));
    else target[key] = value;
  }
  dirty = true;
  updateSaveButton();
}

function updateSaveButton(): void {
  const button = document.querySelector<HTMLButtonElement>('[data-action="save"]');
  if (!button) return;
  button.disabled = !dirty || Boolean(busyAction);
  button.textContent = dirty ? "保存配置" : "已保存";
}

async function loadBootstrap(preserveSelection = true): Promise<void> {
  const next = await invoke<Bootstrap>("get_bootstrap");
  const oldSelection = selectedProfileId;
  bootstrap = next;
  draft = clone(next.config);
  firstRun = !next.config_exists;
  selectedProfileId = preserveSelection && draft.profiles[oldSelection]
    ? oldSelection
    : draft.app.active_profile || Object.keys(draft.profiles)[0] || "";
  dirty = !next.config_exists;
  render();
}

async function saveConfig(showGuideAfter = false): Promise<void> {
  busyAction = "save";
  updateSaveButton();
  try {
    const shouldShowGuide = firstRun || showGuideAfter;
    const profileId = selectedProfileId;
    await invoke("save_config", { config: draft });
    await loadBootstrap();
    toast("配置已保存", "success");
    if (shouldShowGuide) {
      await showSshGuide(profileId);
    }
  } catch (error) {
    toast(errorText(error), "error", 6500);
  } finally {
    busyAction = "";
    render();
  }
}

async function perform<T>(name: string, action: () => Promise<T>, success?: string): Promise<T | undefined> {
  busyAction = name;
  render();
  try {
    const result = await action();
    if (success) toast(success, "success");
    await loadBootstrap();
    return result;
  } catch (error) {
    toast(errorText(error), "error", 6500);
    return undefined;
  } finally {
    busyAction = "";
    render();
  }
}

async function showSshGuide(profileId: string): Promise<void> {
  if (dirty) {
    toast("请先保存环境配置，再生成准确的 SSH 命令", "warning");
    return;
  }
  try {
    const instructions = await invoke<SshInstructions>("get_ssh_instructions", { profileId });
    const profile = draft.profiles[profileId];
    modalRoot.innerHTML = `<div class="modal-backdrop" data-action="close-modal">
      <section class="modal" role="dialog" aria-modal="true" aria-labelledby="ssh-title">
        <button class="modal-close" data-action="close-modal" aria-label="关闭">×</button>
        <span class="kicker">首次只需操作一次</span>
        <h2 id="ssh-title">${h(platformName(instructions.platform))} SSH 免密配置</h2>
        <p class="modal-lead">为环境“${h(profile?.name)}”创建专用连接。SwitchCat 不保存 OpenWrt 密码；安装公钥时会由系统 SSH 提示输入一次 root 密码。</p>
        ${sshPrerequisite(instructions.platform)}
        ${commandStep(1, "生成 SSH 密钥", instructions.keygen_command)}
        ${commandStep(2, "把公钥安装到 OpenWrt", instructions.install_command, "首次连接会显示主机指纹，请确认设备地址无误后输入 yes，再输入 OpenWrt root 密码。")}
        ${commandStep(3, "验证免密连接", instructions.verify_command)}
        <div class="ssh-paths"><span>私钥 <code>${h(instructions.identity_file)}</code></span><span>独立主机指纹 <code>${h(instructions.known_hosts_file)}</code></span></div>
        <div class="modal-actions"><button class="secondary" data-action="close-modal">稍后再做</button><button class="primary" data-action="check-ssh" data-profile-id="${h(profileId)}">我已完成，测试连接</button></div>
      </section>
    </div>`;
  } catch (error) {
    toast(errorText(error), "error");
  }
}

function sshPrerequisite(platform: string): string {
  if (platform === "windows") {
    return `<div class="prerequisite"><strong>打开 PowerShell</strong><span>开始菜单搜索 PowerShell 后打开。Windows 10/11 通常已内置 OpenSSH；若提示找不到 ssh，请用管理员 PowerShell 执行：</span><code>Add-WindowsCapability -Online -Name OpenSSH.Client~~~~0.0.1.0</code></div>`;
  }
  if (platform === "macos") {
    return '<div class="prerequisite"><strong>打开“终端”</strong><span>前往“应用程序 → 实用工具 → 终端”。macOS 已内置 OpenSSH，无需另装软件。</span></div>';
  }
  return '<div class="prerequisite"><strong>打开终端</strong><span>请先确认系统已安装 OpenSSH 客户端（ssh 与 ssh-keygen）。</span></div>';
}

function commandStep(number: number, title: string, command: string, note = ""): string {
  return `<div class="command-step"><div class="command-number">${number}</div><div><strong>${h(title)}</strong>${note ? `<p>${h(note)}</p>` : ""}<div class="command-box"><pre>${h(command)}</pre><button data-action="copy" data-copy="${h(command)}">复制</button></div></div></div>`;
}

async function checkSsh(profileId: string): Promise<void> {
  if (dirty) {
    toast("请先保存环境配置，再测试 SSH", "warning");
    return;
  }
  busyAction = "ssh";
  render();
  try {
    const status = await invoke<SshStatus>("check_ssh", { profileId });
    await loadBootstrap();
    toast(status.message, status.ok ? "success" : "error", 6500);
    if (status.ok) {
      modalRoot.innerHTML = "";
      await perform("refresh", () => invoke("refresh_profile", { profileId }), "SSH 正常，节点已同步");
    } else if (["host_key_missing", "auth", "ssh_missing"].includes(status.kind)) {
      await showSshGuide(profileId);
    }
  } catch (error) {
    toast(errorText(error), "error", 6500);
  } finally {
    busyAction = "";
    render();
  }
}

function addProfile(): void {
  let index = 1;
  let id = "office";
  while (draft.profiles[id]) id = `environment_${++index}`;
  const source = draft.profiles[selectedProfileId] ?? Object.values(draft.profiles)[0];
  draft.profiles[id] = {
    name: id === "office" ? "公司" : `环境 ${index}`,
    enabled: true,
    ssh_verified: false,
    openwrt: {
      host: "",
      port: 22,
      user: "root",
      identity_file: "",
      connect_timeout_seconds: 6,
    },
    device: {
      client_ip: bootstrap.network.ipv4_addresses[0] ?? source?.device.client_ip ?? "",
      acl_remarks: "",
      direct_gateway: bootstrap.network.gateway ?? "",
      proxy_gateway: "",
      direct_dns: bootstrap.network.gateway ?? "",
      proxy_dns: "",
    },
    detect: { ssids: [], gateways: [], local_cidrs: [] },
  };
  selectedProfileId = id;
  dirty = true;
  render();
  requestAnimationFrame(() => document.querySelector<HTMLInputElement>('[data-field="profile.name"]')?.select());
}

function deleteProfile(profileId: string): void {
  const profile = draft.profiles[profileId];
  if (!profile || !window.confirm(`确定删除环境“${profile.name}”吗？保存配置后生效。`)) return;
  delete draft.profiles[profileId];
  if (draft.app.active_profile === profileId) draft.app.active_profile = Object.keys(draft.profiles)[0] ?? "";
  selectedProfileId = draft.app.active_profile;
  dirty = true;
  render();
}

function toast(message: string, type: "success" | "error" | "warning" = "success", duration = 3500): void {
  const element = document.createElement("div");
  element.className = `toast ${type}`;
  element.textContent = message;
  toastRoot.append(element);
  window.setTimeout(() => element.classList.add("leaving"), duration - 250);
  window.setTimeout(() => element.remove(), duration);
}

appRoot.addEventListener("input", (event) => {
  const input = (event.target as HTMLElement).closest<HTMLInputElement | HTMLTextAreaElement>("[data-field]");
  if (input?.dataset.field) setDraftField(input.dataset.field, input);
});

appRoot.addEventListener("change", (event) => {
  const input = (event.target as HTMLElement).closest<HTMLInputElement>("input[type=checkbox][data-field]");
  if (input?.dataset.field) {
    setDraftField(input.dataset.field, input);
    render();
  }
});

appRoot.addEventListener("click", async (event) => {
  const button = (event.target as HTMLElement).closest<HTMLElement>("[data-action]");
  if (!button) return;
  const profileId = button.dataset.profileId ?? selectedProfileId;
  switch (button.dataset.action) {
    case "select-profile": selectedProfileId = profileId; render(); break;
    case "add-profile": addProfile(); break;
    case "delete-profile": deleteProfile(profileId); break;
    case "save": await saveConfig(); break;
    case "ssh-guide": await showSshGuide(profileId); break;
    case "check-ssh": await checkSsh(profileId); break;
    case "activate":
      if (dirty) toast("请先保存配置", "warning");
      else await perform("activate", () => invoke("activate_profile", { profileId }), "环境已切换");
      break;
    case "refresh":
      if (dirty) toast("请先保存配置", "warning");
      else await perform("refresh", () => invoke("refresh_profile", { profileId: draft.app.active_profile }), "节点已刷新");
      break;
    case "switch-direct":
      if (dirty) toast("请先保存配置", "warning");
      else await perform("switch", () => invoke("switch_direct", { profileId: draft.app.active_profile }));
      break;
    case "switch-node":
      if (dirty) toast("请先保存配置", "warning");
      else await perform("switch", () => invoke("switch_node", { profileId: draft.app.active_profile, nodeId: button.dataset.nodeId }));
      break;
    case "hide": await invoke("hide_settings"); break;
  }
});

modalRoot.addEventListener("click", async (event) => {
  const target = event.target as HTMLElement;
  const action = target.closest<HTMLElement>("[data-action]");
  if (!action) return;
  if (action.dataset.action === "close-modal" && (target === action || action.tagName === "BUTTON")) modalRoot.innerHTML = "";
  if (action.dataset.action === "copy") {
    await navigator.clipboard.writeText(action.dataset.copy ?? "");
    toast("命令已复制", "success");
  }
  if (action.dataset.action === "check-ssh") await checkSsh(action.dataset.profileId ?? selectedProfileId);
});

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && modalRoot.innerHTML) modalRoot.innerHTML = "";
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
    event.preventDefault();
    if (dirty) void saveConfig();
  }
});

async function start(): Promise<void> {
  try {
    await loadBootstrap(false);
    await listen<StateChanged>("switchcat-state", (event) => {
      window.clearTimeout(refreshTimer);
      if (!event.payload.ok) toast(event.payload.message, "error", 5500);
      refreshTimer = window.setTimeout(() => {
        if (!dirty) void loadBootstrap();
      }, 140);
    });
  } catch (error) {
    appRoot.innerHTML = `<div class="fatal"><div>🙀</div><h1>SwitchCat 启动失败</h1><p>${h(errorText(error))}</p></div>`;
  }
}

void start();
