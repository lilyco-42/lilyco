// 桌面端只有这一条链：挑文件 → 选问题 → invoke 命令层 → 把 lbin 那本账原样铺开。
// 页面自己不解释任何字段：说法由 lbin 与 CI 上那本对账负责，这里只负责看得见。

const { invoke } = window.__TAURI.core;
const { open } = window.__TAURI.dialog;

const pickBtn = document.getElementById("pick");
const picked = document.getElementById("picked");
const runBtn = document.getElementById("run");
const box = document.getElementById("commands");
const hint = document.getElementById("hint");
const state = document.getElementById("state");
const empty = document.getElementById("empty");
const table = document.getElementById("book");
const body = table.querySelector("tbody");
const rawbox = document.getElementById("rawbox");
const raw = document.getElementById("raw");

const chosen = { path: null, command: null };

function setState(kind, text) {
  state.dataset.kind = kind;
  state.textContent = text;
}

function syncRun() {
  runBtn.disabled = !(chosen.path && chosen.command);
}

function cell(value) {
  if (value === null) return "null（文件没写这一格）";
  if (typeof value === "string") return value === "" ? "（空串）" : value;
  return JSON.stringify(value, null, 1);
}

function renderBook(json) {
  body.textContent = "";
  const entries =
    json && typeof json === "object" && !Array.isArray(json)
      ? Object.entries(json)
      : [["结果", json]];
  for (const [key, value] of entries) {
    const row = document.createElement("tr");
    const k = document.createElement("th");
    k.scope = "row";
    k.textContent = key;
    const v = document.createElement("td");
    v.textContent = cell(value);
    row.append(k, v);
    body.append(row);
  }
  empty.hidden = true;
  table.hidden = false;
  rawbox.hidden = false;
  raw.textContent = JSON.stringify(json, null, 2);
}

async function loadCommands() {
  const list = await invoke("office_commands");
  for (const one of list) {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "chip";
    chip.dataset.name = one.name;
    chip.dataset.hint = one.hint;
    chip.setAttribute("aria-pressed", "false");
    chip.textContent = one.name;
    chip.addEventListener("click", () => {
      chosen.command = one.name;
      hint.textContent = one.hint;
      for (const other of box.querySelectorAll(".chip")) {
        other.setAttribute("aria-pressed", String(other === chip));
      }
      syncRun();
    });
    box.append(chip);
  }
}

pickBtn.addEventListener("click", async () => {
  const pickedPath = await open({
    multiple: false,
    title: "挑一份要看的文件",
    filters: [
      {
        name: "办公文件",
        extensions: [
          "docx", "docm", "dotx", "odt", "rtf", "doc",
          "xlsx", "xlsm", "ods", "xls", "csv",
          "pptx", "odp", "ppt",
          "pdf",
        ],
      },
      { name: "全部文件", extensions: ["*"] },
    ],
  });
  if (typeof pickedPath !== "string") return;
  chosen.path = pickedPath;
  const name = pickedPath.split(/[\\/]/).pop();
  picked.textContent = name + "　—　" + pickedPath;
  syncRun();
});

runBtn.addEventListener("click", async () => {
  if (!chosen.path || !chosen.command) return;
  runBtn.disabled = true;
  setState("loading", "在读：" + chosen.command + " " + chosen.path);
  try {
    const json = await invoke("office_run", {
      command: chosen.command,
      path: chosen.path,
    });
    renderBook(json);
    setState("ok", "读完了");
  } catch (error) {
    empty.hidden = false;
    table.hidden = true;
    rawbox.hidden = true;
    empty.textContent = String(error);
    setState("error", "没读出来");
  } finally {
    syncRun();
  }
});

setState("idle", "");
loadCommands().catch((error) => setState("error", String(error)));
