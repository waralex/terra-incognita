pub const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>terra incognita</title>
<style>
:root {
  --bg: #0d1117;
  --bg2: #161b22;
  --bg3: #21262d;
  --border: #30363d;
  --text: #c9d1d9;
  --text2: #8b949e;
  --accent: #58a6ff;
  --green: #3fb950;
  --yellow: #d29922;
  --purple: #bc8cff;
  --red: #f85149;
  --orange: #d18616;
}
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif;
  background: var(--bg);
  color: var(--text);
  font-size: 14px;
  line-height: 1.5;
}

/* Top bar */
.topbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 16px;
  background: var(--bg2);
  border-bottom: 1px solid var(--border);
}
.topbar-title {
  font-weight: 600;
  font-size: 14px;
  color: var(--text2);
  letter-spacing: 0.02em;
}
.session-select {
  background: var(--bg3);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  padding: 4px 8px;
  font-size: 13px;
  font-family: inherit;
  cursor: pointer;
  min-width: 140px;
}
.session-select:focus { outline: 1px solid var(--accent); }
.session-select option { background: var(--bg2); color: var(--text); }
.topbar-refresh {
  margin-left: auto;
  padding: 4px 12px;
  background: var(--bg3);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--accent);
  cursor: pointer;
  font-size: 13px;
  font-family: inherit;
}
.topbar-refresh:hover { background: var(--border); }
.topbar-status {
  font-size: 12px;
  color: var(--text2);
}

/* Layout */
.layout {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 1px;
  height: calc(100vh - 41px);
  background: var(--border);
}
.panel {
  background: var(--bg);
  overflow-y: auto;
  padding: 16px;
}
.panel h2 {
  font-size: 13px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text2);
  margin-bottom: 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border);
  position: sticky;
  top: 0;
  background: var(--bg);
  z-index: 1;
}
.tabs {
  display: flex;
  gap: 0;
  margin-bottom: 0;
  border-bottom: 1px solid var(--border);
  position: sticky;
  top: 0;
  background: var(--bg);
  z-index: 2;
}
.tab {
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text2);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  background: none;
  border-top: none;
  border-left: none;
  border-right: none;
  font-family: inherit;
}
.tab:hover { color: var(--text); }
.tab.active { color: var(--accent); border-bottom-color: var(--accent); }
.tab-content { display: none; padding-top: 12px; }
.tab-content.active { display: block; }

/* Transaction card */
.tx {
  border: 1px solid var(--border);
  border-radius: 6px;
  margin-bottom: 8px;
  overflow: hidden;
}
.tx.selected { border-color: var(--accent); }
.tx-header {
  padding: 10px 12px;
  background: var(--bg2);
  display: flex;
  align-items: baseline;
  gap: 8px;
  cursor: pointer;
}
.tx-header:hover { background: var(--bg3); }
.tx.selected .tx-header { background: rgba(88,166,255,0.08); }
.tx-time {
  font-size: 12px;
  color: var(--text2);
  white-space: nowrap;
}
.tx-question {
  font-weight: 500;
  color: var(--text);
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tx-body {
  display: none;
  padding: 12px;
  border-top: 1px solid var(--border);
}
.tx.open .tx-body { display: block; }
.tx-label {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text2);
  margin-top: 10px;
  margin-bottom: 4px;
}
.tx-label:first-child { margin-top: 0; }
.tx-answer {
  color: var(--text);
  white-space: pre-wrap;
  word-break: break-word;
}
.tx-reasoning {
  color: var(--text2);
  font-style: italic;
  white-space: pre-wrap;
  word-break: break-word;
}

/* Commands */
.cmd {
  background: var(--bg2);
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 8px 10px;
  margin-top: 4px;
  font-size: 13px;
}
.cmd-type {
  color: var(--orange);
  font-weight: 600;
  font-size: 12px;
}
.cmd-query {
  color: var(--accent);
  font-family: 'SF Mono', SFMono-Regular, Consolas, monospace;
  font-size: 12px;
  margin-top: 4px;
  white-space: pre-wrap;
  word-break: break-word;
}
.cmd-stats {
  color: var(--text2);
  font-size: 11px;
  margin-top: 2px;
}
.cmd-error {
  color: var(--red);
  font-size: 12px;
  margin-top: 2px;
}

/* Entity card */
.entity {
  border: 1px solid var(--border);
  border-radius: 6px;
  margin-bottom: 8px;
  overflow: hidden;
}
.entity-header {
  padding: 8px 12px;
  background: var(--bg2);
  font-weight: 500;
  cursor: pointer;
}
.entity-header:hover { background: var(--bg3); }
.entity-slug { color: var(--green); }
.entity-desc { color: var(--text2); font-weight: 400; margin-left: 8px; font-size: 13px; }
.entity-body {
  display: none;
  padding: 10px 12px;
  border-top: 1px solid var(--border);
}
.entity.open .entity-body { display: block; }

/* Property row */
.prop {
  display: flex;
  gap: 8px;
  padding: 4px 0;
  border-bottom: 1px solid var(--border);
  align-items: baseline;
}
.prop:last-child { border-bottom: none; }
.prop-slug { color: var(--purple); font-weight: 500; min-width: 120px; }
.prop-value {
  font-family: 'SF Mono', SFMono-Regular, Consolas, monospace;
  font-size: 13px;
  color: var(--text);
  flex: 1;
  word-break: break-word;
}
.prop-reasoning {
  color: var(--text2);
  font-size: 12px;
  font-style: italic;
  max-width: 300px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.prop.highlight { background: rgba(88,166,255,0.06); }

/* Hypothesis */
.hyp {
  background: var(--bg2);
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 8px 10px;
  margin-bottom: 6px;
}
.hyp-prop { color: var(--purple); }
.hyp-value {
  font-family: 'SF Mono', SFMono-Regular, Consolas, monospace;
  font-size: 13px;
}
.hyp-reasoning {
  color: var(--text2);
  font-size: 12px;
  font-style: italic;
  margin-top: 2px;
}

/* Assertion (in tx tab) */
.assertion {
  background: var(--bg2);
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 8px 10px;
  margin-bottom: 6px;
}
.assertion-entity { color: var(--green); font-weight: 500; }
.assertion-et { color: var(--text2); font-size: 12px; margin-left: 6px; }

/* Loading / error */
.status {
  text-align: center;
  padding: 40px;
  color: var(--text2);
}
.error { color: var(--red); }
.badge {
  display: inline-block;
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 10px;
  font-weight: 500;
}
.badge-fact { background: rgba(63,185,80,0.15); color: var(--green); }
.badge-hyp { background: rgba(188,140,255,0.15); color: var(--purple); }
.badge-new { background: rgba(88,166,255,0.15); color: var(--accent); }
.no-data { color: var(--text2); font-style: italic; padding: 12px 0; }
.tx-hint {
  color: var(--text2);
  font-size: 12px;
  font-style: italic;
  padding: 20px 0;
  text-align: center;
}
</style>
</head>
<body>

<div class="topbar">
  <span class="topbar-title">terra incognita</span>
  <select class="session-select" id="session-select" onchange="switchSession(this.value)">
    <option value="main">main</option>
  </select>
  <span class="topbar-status" id="topbar-status"></span>
  <button class="topbar-refresh" onclick="loadData()">Refresh</button>
</div>

<div class="layout">
  <div class="panel" id="left-panel">
    <h2>Transactions</h2>
    <div id="transactions"><div class="status">Loading...</div></div>
  </div>
  <div class="panel" id="right-panel">
    <div class="tabs">
      <button class="tab active" data-tab="entities">Entities</button>
      <button class="tab" data-tab="hypotheses">Hypotheses</button>
      <button class="tab" data-tab="assertions">Assertions</button>
    </div>
    <div id="tab-entities" class="tab-content active"></div>
    <div id="tab-hypotheses" class="tab-content"></div>
    <div id="tab-assertions" class="tab-content">
      <div class="tx-hint">Click a transaction to see its assertions</div>
    </div>
  </div>
</div>

<script>
let DATA = null;
let TX_STATE = null;  // state at selected transaction
let selectedTxId = null;
let currentSlug = 'main';

async function loadBranches() {
  try {
    const res = await fetch('/api/branches');
    if (!res.ok) return;
    const branches = await res.json();
    const sel = document.getElementById('session-select');
    const options = ['<option value="main">main</option>'];
    if (Array.isArray(branches)) {
      for (const b of branches) {
        if (b.slug && b.slug !== 'main') {
          const selected = b.slug === currentSlug ? ' selected' : '';
          options.push(`<option value="${esc(b.slug)}"${selected}>${esc(b.slug)}</option>`);
        }
      }
    }
    sel.innerHTML = options.join('');
    sel.value = currentSlug;
  } catch (e) { /* ignore */ }
}

function switchSession(slug) {
  currentSlug = slug;
  selectedTxId = null;
  TX_STATE = null;
  loadData();
}

async function loadData() {
  const statusEl = document.getElementById('topbar-status');
  statusEl.textContent = 'loading...';
  try {
    const res = await fetch(`/api/state?slug=${encodeURIComponent(currentSlug)}`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    DATA = await res.json();
    const txCount = (DATA.recent_transactions || []).length;
    const entCount = (DATA.entities || []).length;
    statusEl.textContent = `${txCount} tx, ${entCount} entities`;
    render();
  } catch (e) {
    statusEl.textContent = '';
    document.getElementById('transactions').innerHTML =
      `<div class="status error">Failed to load: ${esc(e.message)}</div>`;
  }
}

async function selectTransaction(txId) {
  if (selectedTxId === txId) {
    selectedTxId = null;
    TX_STATE = null;
    renderTransactions();
    renderEntities();
    renderHypotheses();
    renderAssertions();
    return;
  }
  selectedTxId = txId;
  renderTransactions();

  const el = document.getElementById('tab-assertions');
  el.innerHTML = '<div class="status">Loading state at transaction...</div>';
  switchToTab('assertions');

  try {
    const res = await fetch(`/api/state?slug=${encodeURIComponent(currentSlug)}&at_tx=${encodeURIComponent(txId)}`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    TX_STATE = await res.json();
    renderEntities();
    renderHypotheses();
    renderAssertions();
  } catch (e) {
    el.innerHTML = `<div class="status error">Failed: ${esc(e.message)}</div>`;
  }
}

function render() {
  renderTransactions();
  renderEntities();
  renderHypotheses();
  renderAssertions();
}

function esc(s) {
  if (s == null) return '';
  const d = document.createElement('div');
  d.textContent = String(s);
  return d.innerHTML;
}

function fmtTime(ts) {
  try {
    const d = new Date(ts);
    return d.toLocaleString('ru-RU', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' });
  } catch { return ts; }
}

function fmtVal(v) {
  if (v === null || v === undefined) return 'null';
  if (typeof v === 'object') return JSON.stringify(v, null, 2);
  return String(v);
}

function fmtReasoning(r) {
  if (r === null || r === undefined) return '';
  if (typeof r === 'string') return r;
  return JSON.stringify(r);
}

// --- Transactions ---
function renderTransactions() {
  const el = document.getElementById('transactions');
  const txns = DATA.recent_transactions || [];
  if (!txns.length) {
    el.innerHTML = '<div class="no-data">No transactions yet</div>';
    return;
  }
  el.innerHTML = txns.map((tx, i) => {
    const q = tx.question || '';
    const a = tx.answer || '';
    const r = fmtReasoning(tx.reasoning);
    const cmds = tx.commands || [];
    const isSelected = tx.id === selectedTxId;
    const isOpen = i === 0 || isSelected;

    return `<div class="tx${isOpen ? ' open' : ''}${isSelected ? ' selected' : ''}" data-txid="${esc(tx.id)}">
      <div class="tx-header" onclick="handleTxClick('${esc(tx.id)}', this, event)">
        <span class="tx-time">${esc(fmtTime(tx.timestamp))}</span>
        <span class="tx-question">${q ? esc(q) : '<span style="color:var(--text2)">no question</span>'}</span>
      </div>
      <div class="tx-body">
        ${a ? `<div class="tx-label">Answer</div><div class="tx-answer">${esc(a)}</div>` : ''}
        ${r ? `<div class="tx-label">Reasoning</div><div class="tx-reasoning">${esc(r)}</div>` : ''}
        ${cmds.length ? `<div class="tx-label">Commands (${cmds.length})</div>${renderCommands(cmds)}` : ''}
      </div>
    </div>`;
  }).join('');
}

function handleTxClick(txId, headerEl, event) {
  const txEl = headerEl.parentElement;
  txEl.classList.toggle('open');
  selectTransaction(txId);
}

function renderCommands(cmds) {
  return cmds.map(c => {
    const stats = c.stats || {};
    const statsStr = [
      stats.row_count != null ? `${stats.row_count} rows` : '',
      stats.elapsed_ms != null ? `${stats.elapsed_ms}ms` : '',
      stats.truncated ? 'truncated' : '',
    ].filter(Boolean).join(', ');

    return `<div class="cmd">
      <span class="cmd-type">${esc(c.type || c.command_type || '?')}</span>
      ${c.query ? `<div class="cmd-query">${esc(c.query)}</div>` : ''}
      ${statsStr ? `<div class="cmd-stats">${esc(statsStr)}</div>` : ''}
      ${c.error ? `<div class="cmd-error">${esc(c.error)}</div>` : ''}
    </div>`;
  }).join('');
}

// --- Entities (current state) ---
function renderEntities() {
  const el = document.getElementById('tab-entities');
  const source = (selectedTxId && TX_STATE) ? TX_STATE : DATA;
  renderEntityList(el, source.entities || [], selectedTxId);
}

function renderEntityList(el, entities, highlightTxId) {
  if (!entities.length) {
    el.innerHTML = '<div class="no-data">No entities</div>';
    return;
  }
  const html = entities.map(e => {
    const hasData = e.types && e.types.some(t => t.properties && t.properties.some(p => p.fact || p.hypotheses?.length));
    if (!hasData) return '';

    return `<div class="entity open">
      <div class="entity-header" onclick="this.parentElement.classList.toggle('open')">
        <span class="entity-slug">${esc(e.slug)}</span>
        ${e.description ? `<span class="entity-desc">${esc(e.description)}</span>` : ''}
      </div>
      <div class="entity-body">
        ${e.types.map(t => renderEntityTypeDetail(t, highlightTxId)).join('')}
      </div>
    </div>`;
  }).filter(Boolean).join('');
  el.innerHTML = html || '<div class="no-data">No entities with data</div>';
}

function renderEntityTypeDetail(t, highlightTxId) {
  const withData = t.properties.filter(p => p.fact || p.hypotheses?.length);
  if (!withData.length) return '';
  return `<div style="margin-bottom:8px">
    <div style="font-size:11px;color:var(--text2);margin-bottom:4px">${esc(t.entity_type)}</div>
    ${withData.map(p => {
      const items = [];
      if (p.fact) {
        const hl = highlightTxId && p.fact.tx_id === highlightTxId;
        items.push(`<div class="prop${hl ? ' highlight' : ''}">
          <span class="prop-slug">${esc(p.slug)}</span>
          <span class="badge badge-fact">fact</span>
          ${hl ? '<span class="badge badge-new">this tx</span>' : ''}
          <span class="prop-value">${esc(fmtVal(p.fact.value))}</span>
          <span class="prop-reasoning" title="${esc(fmtReasoning(p.fact.reasoning))}">${esc(fmtReasoning(p.fact.reasoning))}</span>
        </div>`);
      }
      for (const h of (p.hypotheses || [])) {
        const hl = highlightTxId && h.tx_id === highlightTxId;
        items.push(`<div class="prop${hl ? ' highlight' : ''}">
          <span class="prop-slug">${esc(p.slug)}</span>
          <span class="badge badge-hyp">hyp</span>
          ${hl ? '<span class="badge badge-new">this tx</span>' : ''}
          <span class="prop-value">${esc(fmtVal(h.value))}</span>
          <span class="prop-reasoning" title="${esc(fmtReasoning(h.reasoning))}">${esc(fmtReasoning(h.reasoning))}</span>
        </div>`);
      }
      return items.join('');
    }).join('')}
  </div>`;
}

// --- Hypotheses (current state) ---
function renderHypotheses() {
  const el = document.getElementById('tab-hypotheses');
  const source = (selectedTxId && TX_STATE) ? TX_STATE : DATA;
  renderHypothesesFromEntities(el, source.entities || []);
}

function renderHypothesesFromEntities(el, entities) {
  const hyps = [];
  for (const e of entities) {
    for (const t of (e.types || [])) {
      for (const p of (t.properties || [])) {
        for (const h of (p.hypotheses || [])) {
          hyps.push({ entity: e.slug, entityType: t.entity_type, prop: p.slug, ...h });
        }
      }
    }
  }

  if (!hyps.length) {
    el.innerHTML = '<div class="no-data">No active hypotheses</div>';
    return;
  }

  const grouped = {};
  for (const h of hyps) {
    if (!grouped[h.entity]) grouped[h.entity] = [];
    grouped[h.entity].push(h);
  }

  el.innerHTML = Object.entries(grouped).map(([entity, items]) =>
    `<div style="margin-bottom:12px">
      <div style="font-weight:500;color:var(--green);margin-bottom:6px">${esc(entity)}</div>
      ${items.map(h => `<div class="hyp">
        <span class="hyp-prop">${esc(h.prop)}</span>
        <span class="badge badge-hyp">hypothesis</span>
        <div class="hyp-value">${esc(fmtVal(h.value))}</div>
        ${h.reasoning ? `<div class="hyp-reasoning">${esc(fmtReasoning(h.reasoning))}</div>` : ''}
      </div>`).join('')}
    </div>`
  ).join('');
}

// --- Assertions (facts & hypotheses from selected tx) ---
function renderAssertions() {
  const el = document.getElementById('tab-assertions');
  if (!selectedTxId || !TX_STATE) {
    el.innerHTML = '<div class="tx-hint">Click a transaction to see its assertions</div>';
    return;
  }

  const tx = (DATA.recent_transactions || []).find(t => t.id === selectedTxId);
  const txTime = tx ? fmtTime(tx.timestamp) : '';
  const txQ = tx?.question || 'no question';

  // Collect facts and hypotheses from this transaction
  const items = [];
  for (const e of (TX_STATE.entities || [])) {
    for (const t of (e.types || [])) {
      for (const p of (t.properties || [])) {
        if (p.fact && p.fact.tx_id === selectedTxId) {
          items.push({ entity: e.slug, entityType: t.entity_type, prop: p.slug, kind: 'fact', value: p.fact.value, reasoning: p.fact.reasoning });
        }
        for (const h of (p.hypotheses || [])) {
          if (h.tx_id === selectedTxId) {
            items.push({ entity: e.slug, entityType: t.entity_type, prop: p.slug, kind: 'hypothesis', value: h.value, reasoning: h.reasoning });
          }
        }
      }
    }
  }

  let html = `<div style="margin-bottom:12px;padding-bottom:8px;border-bottom:1px solid var(--border)">
    <div style="font-size:12px;color:var(--text2)">Assertions from transaction</div>
    <div style="font-weight:500">${esc(txQ)} <span style="color:var(--text2);font-weight:400">${esc(txTime)}</span></div>
  </div>`;

  if (!items.length) {
    html += '<div class="no-data">No facts or hypotheses in this transaction (schema-only or empty)</div>';
  } else {
    // Group by entity
    const grouped = {};
    for (const it of items) {
      if (!grouped[it.entity]) grouped[it.entity] = [];
      grouped[it.entity].push(it);
    }

    html += Object.entries(grouped).map(([entity, assertions]) =>
      `<div class="entity open">
        <div class="entity-header" onclick="this.parentElement.classList.toggle('open')">
          <span class="entity-slug">${esc(entity)}</span>
          <span class="entity-desc">${assertions.length} assertion${assertions.length > 1 ? 's' : ''}</span>
        </div>
        <div class="entity-body">
          ${assertions.map(a => `<div class="assertion">
            <span class="badge ${a.kind === 'fact' ? 'badge-fact' : 'badge-hyp'}">${esc(a.kind)}</span>
            <span class="hyp-prop" style="margin-left:6px">${esc(a.prop)}</span>
            <span class="assertion-et">${esc(a.entityType)}</span>
            <div class="hyp-value">${esc(fmtVal(a.value))}</div>
            ${a.reasoning ? `<div class="hyp-reasoning">${esc(fmtReasoning(a.reasoning))}</div>` : ''}
          </div>`).join('')}
        </div>
      </div>`
    ).join('');
  }

  el.innerHTML = html;
}

// --- Tabs ---
function switchToTab(tabName) {
  document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
  document.querySelectorAll('.tab-content').forEach(t => t.classList.remove('active'));
  const tab = document.querySelector(`.tab[data-tab="${tabName}"]`);
  if (tab) tab.classList.add('active');
  const content = document.getElementById('tab-' + tabName);
  if (content) content.classList.add('active');
}

document.querySelectorAll('.tab').forEach(tab => {
  tab.addEventListener('click', () => switchToTab(tab.dataset.tab));
});

// Init
loadBranches().then(() => loadData());
</script>
</body>
</html>
"##;
