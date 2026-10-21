"use strict";
(() => {
  const token = document.querySelector('meta[name="meowlink-access"]').content;
  const $ = (id) => document.getElementById(id);
  const notice = (message) => { $("notice").textContent = message; };
  let busy = false, initial = true, latest = null;
  const targetFields = ["host", "port", "serverName", "authorityId", "realmId", "saveProfileId", "caCertificate"];
  const pathFields = ["playerTargetFile", "hostExecutable", "clientDirectory"];
  async function api(path, body) {
    const response = await fetch(`/api/${path}`, { method: body === undefined ? "GET" : "POST", headers: { "X-MeowLink-Token": token, "Content-Type": "application/json" }, cache: "no-store", credentials: "omit", ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    const result = await response.json();
    if (!response.ok) throw new Error(result.message || "请求失败");
    return result;
  }
  function paint(state) {
    latest = state;
    const locked = busy || state.running || state.cleanupFailed;
    document.querySelectorAll("#settings input").forEach((input) => { input.disabled = locked; });
    ["save", "check", "login", "register"].forEach((id) => { $(id).disabled = locked; });
    $("start").disabled = locked || !state.identity;
    $("stop").disabled = busy || !(state.running || state.cleanupFailed);
    $("logout").disabled = busy || !(state.identity || state.running || state.cleanupFailed);
    $("quit").disabled = busy;
    $("identity").textContent = state.identity ? `已认证：${state.identity.username} · Account ID ${state.identity.nativeAccountId}` : "未登录";
    $("instance").textContent = state.cleanupFailed ? "清理未确认，禁止切换账号或重新启动。" : state.running ? "原生实例运行中（进服结果请查看游戏）" : "尚未启动游戏";
    if (initial) {
      if (state.config) {
        targetFields.forEach((key) => { $(key).value = state.config.target[key] ?? ""; });
        pathFields.forEach((key) => { $(key).value = state.config[key] ?? ""; });
      } else if (state.defaults) {
        $("hostExecutable").value = state.defaults.hostExecutable ?? "";
        $("playerTargetFile").value = state.defaults.playerTargetFile ?? "";
      }
      initial = false;
    }
  }
  async function refresh() { if (!busy) paint(await api("status")); }
  async function action(path, body) {
    if (busy) return;
    busy = true; if (latest) paint(latest);
    notice("正在处理…");
    try {
      const result = await api(path, body);
      notice(result.message || "操作完成。");
      if (path === "shutdown") { document.querySelectorAll("button").forEach((button) => { button.disabled = true; }); return; }
    } catch (error) { notice(error.message); }
    finally { busy = false; }
    try { await refresh(); } catch (error) { notice(error.message); }
  }
  $("save").addEventListener("click", () => {
    const target = Object.fromEntries(targetFields.map((key) => [key, $(key).value.trim()]));
    const port = Number(target.port);
    if (!Number.isInteger(port) || port < 1 || port > 65535) { notice("端口必须是 1–65535 的整数。"); return; }
    target.port = port; target.caCertificate ||= null;
    action("config", { schemaVersion: 1, target, ...Object.fromEntries(pathFields.map((key) => [key, $(key).value.trim()])) });
  });
  for (const path of ["register", "login"]) $(path).addEventListener("click", () => {
    const credentials = { username: $("username").value, password: $("password").value };
    $("password").value = "";
    if (!credentials.username || !credentials.password) { notice("请输入用户名和密码。"); return; }
    const request = action(path, credentials); credentials.password = ""; return request;
  });
  for (const [id, path] of [["check", "check"], ["logout", "logout"], ["start", "start"], ["stop", "stop"], ["quit", "shutdown"]]) $(id).addEventListener("click", () => action(path, {}));
  if (!/^[0-9a-f]{64}$/.test(token)) { notice("本机页面无效，请使用配置程序控制台给出的地址重新打开。"); document.querySelectorAll("button").forEach((button) => { button.disabled = true; }); return; }
  refresh().then(() => notice(latest.configurationError || (latest.config ? "配置已加载。" : "首次使用，请填写服主提供的服务器配置。"))).catch((error) => notice(error.message));
  setInterval(() => { refresh().catch(() => {}); }, 2500);
})();
