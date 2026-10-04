/// <reference types="vite/client" />
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface Profile { name: string; ip_address: string; subnet_mask: string; gateway: string; dns1: string; dns2: string }
interface Adapter { id: string; name: string; status: string }
interface RuntimeState { busy: boolean; last_applied: { adapter_id: string; profile: Profile } | null; last_error: string | null }
interface Bootstrap { config_path: string; profiles: Profile[]; adapter_id: string; config_error: string | null; runtime: RuntimeState; version: string }
const app = document.querySelector<HTMLDivElement>("#app")!;
const preview = import.meta.env.DEV && new URLSearchParams(location.search).has("preview");
let bootstrap: Bootstrap;
let profiles: Profile[] = [];
let adapters: Adapter[] = [];
let adapterId = "";
let selected = 0;
let dirty = false;
let working = "";
let adapterError = "";
let message = "";
let messageIsError = false;

const h = (value: string): string => value.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const busy = (): boolean => Boolean(working || bootstrap?.runtime.busy);
const disabled = (value: boolean): string => value ? "disabled" : "";
const emptyProfile = (name: string): Profile => ({ name, ip_address: "", subnet_mask: "255.255.255.0", gateway: "", dns1: "", dns2: "" });
const equal = (left: Profile, right: Profile): boolean => (Object.keys(left) as (keyof Profile)[]).every((key) => left[key] === right[key]);

function feedback(text: string, error = false): void {
  message = text; messageIsError = error;
  const output = document.querySelector<HTMLElement>("#feedback");
  if (output) { output.textContent = text; output.className = error ? "feedback error" : "feedback"; }
}
function markDirty(): void {
  dirty = true; feedback("");
  const save = document.querySelector<HTMLButtonElement>("#save");
  if (save) { save.disabled = busy() || !adapterId || Boolean(bootstrap.config_error); save.textContent = "保存配置"; }
  document.querySelectorAll<HTMLButtonElement>('[data-action="apply"]').forEach((button) => { button.disabled = true; });
  const indicator = document.querySelector<HTMLElement>("#draft-indicator");
  if (indicator) indicator.textContent = "有未保存的修改";
}
function field(label: string, key: keyof Profile, value: string, placeholder: string, optional = false): string {
  return '<label class="field"><span>' + label + (optional ? '<small>可选</small>' : "") +
    '</span><input type="text" data-field="' + key + '" value="' + h(value) + '" placeholder="' +
    placeholder + '" autocomplete="off" spellcheck="false" maxlength="' + (key === "name" ? "80" : "15") + '"></label>';
}
function render(): void {
  const profile = profiles[selected];
  const applied = bootstrap.runtime.last_applied;
  const status = bootstrap.runtime.busy ? "正在应用网络配置…" : applied ? "最近应用：" + applied.profile.name : "从右键菜单选择配置，即可切换";
  const adapterMissing = Boolean(adapterId && !adapters.some((adapter) => adapter.id === adapterId));
  app.innerHTML = '<div class="shell">' +
    '<header class="header"><div class="brand"><span class="cat-mark" aria-hidden="true">🐈</span><div><h1>SwitchCat</h1><p>配置好，点一下，就切换。</p></div></div><span class="version">v' + h(bootstrap.version) + '</span></header>' +
    (preview ? '<div class="notice">界面预览 · 不会修改网卡</div>' : "") +
    '<section class="adapter-bar"><label for="adapter">目标网卡</label><select id="adapter" ' + disabled(busy()) + '><option value="">请选择要修改的网卡</option>' +
    (adapterMissing ? '<option value="' + h(adapterId) + '" selected>已保存的网卡暂不可用，请重新选择</option>' : "") +
    adapters.map((adapter) => '<option value="' + h(adapter.id) + '" ' + (adapter.id === adapterId ? "selected" : "") + '>' + h(adapter.name) + ' · ' + h(adapter.status === "Up" ? "已连接" : adapter.status === "Disconnected" ? "未连接" : adapter.status) + '</option>').join("") +
    '</select><button class="text-button" data-action="adapters" ' + disabled(busy()) + '>刷新网卡</button></section>' +
    (adapterError ? '<div class="notice error">' + h(adapterError) + '</div>' : "") +
    (bootstrap.config_error ? '<div class="notice error">配置文件读取失败：' + h(bootstrap.config_error) + '。请打开配置目录修正后重新读取。</div>' : "") +
    '<div class="workspace"><aside class="sidebar"><div class="section-title"><h2>我的配置</h2><span>' + profiles.length + '</span></div><div class="profile-list">' +
    profiles.map((item, index) => {
      const checked = applied && applied.adapter_id === adapterId && equal(applied.profile, item);
      return '<button class="profile-choice ' + (index === selected ? "selected" : "") + '" data-action="select" data-index="' + index + '" ' + disabled(busy()) + '><span class="profile-dot ' + (checked ? "applied" : "") + '"></span><span>' + h(item.name || "未命名配置") + '</span>' + (checked ? '<small aria-label="最近成功应用">✓</small>' : "") + '</button>';
    }).join("") +
    '</div><button class="add-button" data-action="add" ' + disabled(busy() || profiles.length >= 100) + '>＋ 添加配置</button><p class="sidebar-help">配置名会原样显示在托盘右键菜单中。</p></aside><main class="editor">' +
    (profile ? '<div class="editor-heading"><div><span class="eyebrow">网络配置</span><h2>' + h(profile.name || "新配置") + '</h2></div><div class="editor-tools"><button class="text-button" data-action="duplicate" ' + disabled(busy() || profiles.length >= 100) + '>复制</button><button class="text-button danger" data-action="delete" ' + disabled(busy()) + '>删除</button></div></div>' +
      '<fieldset ' + disabled(busy()) + '><div class="name-field">' + field("配置名称", "name", profile.name, "例如：家里*本地连接") + '</div><div class="field-grid">' +
      field("IP 地址", "ip_address", profile.ip_address, "192.168.1.10") + field("子网掩码", "subnet_mask", profile.subnet_mask, "255.255.255.0") +
      field("网关", "gateway", profile.gateway, "192.168.1.1") + '<div class="field-hint">应用时，会将这些参数写入上方选中的网卡。</div>' +
      field("DNS1", "dns1", profile.dns1, "192.168.1.1", true) + field("DNS2", "dns2", profile.dns2, "例如：1.1.1.1", true) +
      '</div></fieldset><p class="dns-help">DNS1 和 DNS2 至少填写一个。</p><div class="apply-row"><button class="secondary" data-action="apply" ' +
      disabled(busy() || dirty || !adapterId || adapterMissing || Boolean(bootstrap.config_error)) + '>' + (bootstrap.runtime.busy ? "正在切换…" : "应用此配置") +
      '</button><span>' + (dirty ? "先保存修改，再应用" : "也可以从托盘右键菜单直接切换") + '</span></div>'
      : '<div class="empty-state"><span aria-hidden="true">＋</span><h2>添加你的第一个配置</h2><p>填好 IP、掩码、网关和 DNS，保存后就能从托盘切换。</p><button class="secondary" data-action="add" ' + disabled(busy()) + '>添加配置</button></div>') +
    '</main></div><div class="status-line"><span class="status-dot ' + (bootstrap.runtime.busy ? "busy" : "") + '"></span><span>' + h(status) + '</span></div>' +
    (bootstrap.runtime.last_error ? '<div class="notice error">' + h(bootstrap.runtime.last_error) + '</div>' : "") +
    '<footer class="footer"><div class="file-info"><button class="text-button" data-action="folder">config.ini ↗</button><span title="' + h(bootstrap.config_path) + '">' + h(bootstrap.config_path) +
    '</span></div><div class="footer-actions"><button class="secondary small" data-action="reload" ' + disabled(busy()) + '>重新读取</button><button id="save" class="primary" data-action="save" ' +
    disabled(busy() || !dirty || !adapterId || adapterMissing || Boolean(bootstrap.config_error)) + '>' + (working === "save" ? "保存中…" : dirty ? "保存配置" : "已保存") +
    '</button></div></footer><div class="bottom-line"><span id="draft-indicator">' + (dirty ? "有未保存的修改" : "关闭窗口后继续在托盘运行") +
    '</span><p id="feedback" class="feedback ' + (messageIsError ? "error" : "") + '" role="status">' + h(message) + '</p></div></div>';
}
async function load(): Promise<void> {
  const next = preview ? demoBootstrap() : await invoke<Bootstrap>("get_bootstrap");
  bootstrap = next; profiles = structuredClone(next.profiles); adapterId = next.adapter_id;
  selected = Math.max(0, Math.min(selected, profiles.length - 1)); dirty = false;
}
async function loadAdapters(): Promise<void> {
  try {
    adapters = preview ? [
      { id: "7f0335c4-508b-40bc-bb78-a019059d50ce", name: "以太网", status: "Up" },
      { id: "487cb7df-37d8-44ea-abfe-2de420dca701", name: "WLAN", status: "Disconnected" },
    ] : await invoke<Adapter[]>("list_adapters");
    adapterError = "";
  } catch (error) { adapterError = String(error); }
}
function uniqueName(base: string): string {
  let name = base; let number = 2;
  while (profiles.some((profile) => profile.name === name)) name = base + " " + number++;
  return name;
}
async function save(): Promise<void> {
  for (let index = 0; index < profiles.length; index++) {
    const profile = profiles[index];
    for (const key of Object.keys(profile) as (keyof Profile)[]) profile[key] = profile[key].trim();
    if (!profile.name || !profile.ip_address || !profile.subnet_mask || !profile.gateway || (!profile.dns1 && !profile.dns2)) {
      selected = index; render();
      throw new Error("请填写配置名称、IP、子网掩码、网关，以及至少一个 DNS");
    }
  }
  if (preview) { bootstrap.profiles = structuredClone(profiles); bootstrap.adapter_id = adapterId; dirty = false; }
  else { await invoke("save_config", { profiles, adapterId }); await load(); }
  feedback("配置已保存，托盘菜单已可使用");
}
app.addEventListener("input", (event) => {
  const input = event.target as HTMLInputElement;
  if (input.dataset.field && profiles[selected]) { profiles[selected][input.dataset.field as keyof Profile] = input.value; markDirty(); }
});
app.addEventListener("change", (event) => {
  const target = event.target as HTMLSelectElement;
  if (target.id === "adapter") { adapterId = target.value; dirty = true; render(); }
});
app.addEventListener("click", async (event) => {
  const button = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-action]");
  if (!button || button.disabled) return;
  const action = button.dataset.action!;
  if (busy() && action !== "folder") return;
  if (action === "select") { selected = Number(button.dataset.index); render(); return; }
  if (action === "add" || action === "duplicate") {
    const name = uniqueName(action === "duplicate" && profiles[selected] ? profiles[selected].name + " 副本" : profiles.length ? "新配置" : "家里*本地连接");
    profiles.push(action === "duplicate" && profiles[selected] ? { ...profiles[selected], name } : emptyProfile(name));
    selected = profiles.length - 1; dirty = true; feedback(""); render(); return;
  }
  if (action === "delete") {
    if (!confirm("删除配置“" + profiles[selected].name + "”？保存后生效。")) return;
    profiles.splice(selected, 1); selected = Math.max(0, selected - 1); dirty = true; feedback(""); render(); return;
  }
  if (action === "reload" && dirty && !confirm("重新读取会放弃未保存的修改，继续吗？")) return;
  working = action; feedback(""); render();
  try {
    if (action === "save") await save();
    else if (action === "reload") { await load(); feedback("已重新读取配置文件"); }
    else if (action === "adapters") await loadAdapters();
    else if (action === "folder") { if (!preview) await invoke("open_config_folder"); }
    else if (action === "apply") {
      if (preview) feedback("预览模式不会修改网卡");
      else { await invoke("apply_profile", { name: profiles[selected].name }); await load(); feedback("已应用配置"); }
    }
  } catch (error) { feedback(String(error).replace(/^Error: /, ""), true); }
  finally { working = ""; render(); }
});
function demoBootstrap(): Bootstrap {
  return {
    config_path: "C:\\Users\\你\\AppData\\Roaming\\com.kyan54.switchcat\\config.ini",
    profiles: [
      { name: "家里*本地连接", ip_address: "192.168.1.10", subnet_mask: "255.255.255.0", gateway: "192.168.1.1", dns1: "192.168.1.1", dns2: "" },
      { name: "家里*代理", ip_address: "192.168.1.10", subnet_mask: "255.255.255.0", gateway: "192.168.1.3", dns1: "192.168.1.3", dns2: "" },
    ], adapter_id: "7f0335c4-508b-40bc-bb78-a019059d50ce", config_error: null,
    runtime: { busy: false, last_applied: null, last_error: null }, version: "0.2.0",
  };
}
async function start(): Promise<void> {
  try {
    if (!preview) {
      await listen<RuntimeState>("switchcat-state", (event) => { if (bootstrap) { bootstrap.runtime = event.payload; render(); } });
      await listen("config-saved", () => {
        if (!dirty && !working) void load().then(render).catch((error) => feedback(String(error), true));
      });
      window.addEventListener("focus", () => {
        if (bootstrap && !dirty && !busy()) void load().then(render).catch((error) => feedback(String(error), true));
      });
    }
    await load(); render(); await loadAdapters(); render();
  } catch (error) {
    app.innerHTML = '<div class="boot-error"><h1>配置页暂时无法打开</h1><p>' + h(String(error)) + '</p><button class="secondary" id="retry">重试</button></div>';
    document.querySelector("#retry")?.addEventListener("click", () => location.reload());
  }
}
void start();
