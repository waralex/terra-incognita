const messagesEl = document.getElementById("messages");
const form = document.getElementById("chat-form");
const input = document.getElementById("input");
const sendBtn = document.getElementById("send-btn");
const branchEl = document.getElementById("branch");
const inspectorEmpty = document.getElementById("inspector-empty");
const inspectorContent = document.getElementById("inspector-content");
const inspectorTitle = document.getElementById("inspector-title");
const beforeContent = document.getElementById("before-content");
const responseContent = document.getElementById("response-content");

const branch = new URLSearchParams(location.search).get("branch") || "main";
branchEl.textContent = branch;

let busy = false;
let selectedEl = null;
const snapshotCache = new Map();
// Ordered list of tx_ids (oldest first) — used to find "previous tx"
const txList = [];

form.addEventListener("submit", (e) => {
  e.preventDefault();
  const text = input.value.trim();
  if (!text || busy) return;
  sendMessage(text);
});

input.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    form.dispatchEvent(new Event("submit"));
  }
});

input.addEventListener("input", () => {
  input.style.height = "auto";
  input.style.height = Math.min(input.scrollHeight, 120) + "px";
});

// Load chat history from last 100 transactions
async function loadHistory() {
  try {
    const res = await fetch(`/api/snapshot?branch=${encodeURIComponent(branch)}&transactions=100`);
    if (!res.ok) return;
    const data = await res.json();
    const txs = data.transactions || [];
    // txs come newest-first from terra, reverse to oldest-first
    txs.reverse();
    for (const tx of txs) {
      const txId = tx.context?.tx_id;
      if (txId) txList.push(txId);
      const q = tx.meta?.question;
      const a = tx.meta?.answer;
      if (q) addMessage("user", String(q));
      if (a) {
        const el = addMessage("assistant", String(a));
        if (txId) el.dataset.txId = txId;
      }
    }
  } catch (e) {
    console.error("Failed to load history:", e);
  }
}

loadHistory();

function processLine(line, assistantEl, setAnswer) {
  if (!line.startsWith("data: ")) return;
  let data;
  try { data = JSON.parse(line.slice(6)); } catch { return; }

  switch (data.type) {
    case "delta":
      assistantEl.textContent += data.text;
      messagesEl.scrollTop = messagesEl.scrollHeight;
      break;
    case "answer":
      setAnswer(data.text);
      assistantEl.textContent = data.text;
      if (data.mutations) {
        assistantEl.dataset.mutations = JSON.stringify(data.mutations);
      }
      break;
    case "transaction": {
      const txId = data.result?.context?.tx_id;
      if (txId) {
        assistantEl.dataset.txId = txId;
        txList.push(txId);
      }
      addMessage("system", `tx ${txId?.slice(0, 8) || "ok"}`);
      break;
    }
    case "error":
      addMessage("error", data.error);
      break;
  }
}

function addMessage(role, text) {
  const el = document.createElement("div");
  el.className = `message ${role}`;
  el.textContent = text;
  messagesEl.appendChild(el);
  messagesEl.scrollTop = messagesEl.scrollHeight;

  if (role === "assistant") {
    el.addEventListener("click", () => selectMessage(el));
  }

  return el;
}

function findPrevTx(txId) {
  const idx = txList.indexOf(txId);
  if (idx <= 0) return null;
  return txList[idx - 1];
}

async function selectMessage(el) {
  if (selectedEl) selectedEl.classList.remove("selected");
  selectedEl = el;
  el.classList.add("selected");

  inspectorEmpty.hidden = true;
  inspectorContent.hidden = false;

  const msgText = el.textContent;
  inspectorTitle.textContent = msgText.length > 60 ? msgText.slice(0, 60) + "..." : msgText;

  // Render mutations (only available for live messages)
  const mutations = el.dataset.mutations ? JSON.parse(el.dataset.mutations) : {};
  renderMutations(mutations);

  // Find previous tx for "state before"
  const txId = el.dataset.txId;
  const prevTx = txId ? findPrevTx(txId) : null;

  if (!prevTx) {
    beforeContent.innerHTML = '<div class="no-data">No prior state</div>';
    return;
  }

  if (snapshotCache.has(prevTx)) {
    renderSnapshot(snapshotCache.get(prevTx));
    return;
  }

  beforeContent.innerHTML = '<div class="no-data">Loading...</div>';
  try {
    const res = await fetch(`/api/snapshot?branch=${encodeURIComponent(branch)}&at_tx=${encodeURIComponent(prevTx)}`);
    const snapshot = await res.json();
    snapshotCache.set(prevTx, snapshot);
    if (selectedEl === el) renderSnapshot(snapshot);
  } catch (e) {
    if (selectedEl === el) {
      beforeContent.innerHTML = `<div class="no-data">Failed to load: ${e.message}</div>`;
    }
  }
}

function renderSnapshot(snapshot) {
  const parts = [];

  if (snapshot.entities?.length > 0) {
    parts.push('<div style="margin-bottom:8px;font-size:12px;color:var(--text-dim)">Entities</div>');
    for (const e of snapshot.entities) {
      parts.push(renderEntityDetails(e));
    }
  }

  if (snapshot.transactions?.length > 0) {
    parts.push('<div style="margin-top:8px;margin-bottom:8px;font-size:12px;color:var(--text-dim)">Transactions</div>');
    for (const tx of snapshot.transactions) {
      parts.push(renderTxItem(tx));
    }
  }

  if (parts.length === 0) {
    parts.push('<div class="no-data">Empty state</div>');
  }

  beforeContent.innerHTML = parts.join("");
}

function renderEntityDetails(e) {
  const propsHtml = (e.properties || []).map(p =>
    `<div class="prop-row"><span class="prop-key">${esc(p.property)}:</span> <span class="prop-val">${esc(JSON.stringify(p.value))}</span></div>`
  ).join("");

  const descHtml = e.description != null
    ? `<div class="prop-row"><span class="prop-key">description:</span> <span class="prop-val">${esc(JSON.stringify(e.description))}</span></div>`
    : "";

  return `<details><summary>${esc(e.slug)}</summary><div class="detail-body">${descHtml}${propsHtml || '<span class="no-data">no properties</span>'}</div></details>`;
}

function renderTxItem(tx) {
  const time = tx.context?.time ? formatTime(tx.context.time) : "?";
  const q = tx.meta?.question ? ` — ${esc(String(tx.meta.question).slice(0, 80))}` : "";
  return `<div class="tx-item"><span class="tx-time">${esc(time)}</span>${q}</div>`;
}

function renderMutations(mutations) {
  const parts = [];

  if (mutations.create?.length) {
    for (const item of mutations.create) {
      parts.push(renderMutationItem("create", item.slug, item));
    }
  }
  if (mutations.update?.length) {
    for (const item of mutations.update) {
      parts.push(renderMutationItem("update", item.slug, item));
    }
  }
  if (mutations.touch?.length) {
    for (const item of mutations.touch) {
      parts.push(renderMutationItem("touch", item.entity, item));
    }
  }

  if (parts.length === 0) {
    responseContent.innerHTML = '<div class="no-data">No mutations</div>';
    return;
  }

  responseContent.innerHTML = parts.join("");
}

function renderMutationItem(type, label, item) {
  const badge = `<span class="mutation-badge ${type}">${type}</span>`;
  let body = "";

  if (type === "touch") {
    body = `<div class="detail-body"><div class="prop-row"><span class="prop-key">reasoning:</span> <span class="prop-val">${esc(item.reasoning || "")}</span></div></div>`;
  } else {
    const lines = [];
    if (item.description != null) {
      lines.push(`<div class="prop-row"><span class="prop-key">description:</span> <span class="prop-val">${esc(JSON.stringify(item.description))}</span></div>`);
    }
    if (item.properties?.length) {
      for (const p of item.properties) {
        lines.push(`<div class="prop-row"><span class="prop-key">${esc(p.property)}:</span> <span class="prop-val">${esc(JSON.stringify(p.value))}</span></div>`);
      }
    }
    body = `<div class="detail-body">${lines.join("") || '<span class="no-data">no data</span>'}</div>`;
  }

  return `<details open><summary>${badge}${esc(label)}</summary>${body}</details>`;
}

function formatTime(iso) {
  try {
    const d = new Date(iso);
    return d.toLocaleString("en-GB", {
      month: "short", day: "numeric",
      hour: "2-digit", minute: "2-digit",
      hour12: false,
    });
  } catch { return iso; }
}

function esc(s) {
  const d = document.createElement("div");
  d.textContent = s;
  return d.innerHTML;
}

async function sendMessage(text) {
  busy = true;
  sendBtn.disabled = true;
  input.value = "";
  input.style.height = "auto";

  addMessage("user", text);
  const assistantEl = addMessage("assistant", "");

  try {
    const res = await fetch("/api/chat", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message: text, branch }),
    });

    if (!res.ok) {
      const err = await res.json();
      addMessage("error", err.error || "Request failed");
      return;
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let fullAnswer = "";

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";

      for (const line of lines) {
        processLine(line, assistantEl, (v) => { fullAnswer = v; });
      }
    }

    if (buffer.trim()) {
      processLine(buffer, assistantEl, (v) => { fullAnswer = v; });
    }
  } catch (e) {
    addMessage("error", e.message);
  } finally {
    busy = false;
    sendBtn.disabled = false;
    input.focus();
  }
}
