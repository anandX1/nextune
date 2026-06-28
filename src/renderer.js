const invoke = window.__TAURI__.core.invoke;
const listen = window.__TAURI__.event.listen;

window.NexTune = {
  getState: () => invoke('get_state'),
  getSettings: () => invoke('get_settings'),
  saveSettings: (s) => invoke('save_settings', { s }),
  getHistory: () => invoke('get_history'),
  getVersion: () => invoke('get_version'),
  minimize: () => invoke('window_minimize'),
  maximize: () => invoke('window_maximize'),
  close: () => invoke('window_close'),
  getRamHistory: () => invoke('get_ram_history'),
  scanProcesses: () => invoke('scan_processes'),
  killProcess: (data) => invoke('kill_process', data),
  killAll: () => invoke('kill_all_bloat'),
  getStartupItems: () => invoke('get_startup_items'),
  toggleStartup: (data) => invoke('toggle_startup', data),
  getServices: () => invoke('get_services'),
  toggleService: (data) => invoke('toggle_service', data),
  scanJunk: () => invoke('scan_junk'),
  cleanJunk: (data) => invoke('clean_junk', data),
  streamOn: () => invoke('stream_on'),
  streamOff: () => invoke('stream_off'),
  installAutoBot: () => invoke('install_autobot'),
  uninstallAutoBot: () => invoke('uninstall_autobot'),
  createRestorePoint: (data) => invoke('create_restore_point', data),
  undoAction: (data) => invoke('undo_action', data),
  openExternal: (url) => invoke('open_external', { url }),
  installUpdate: () => invoke('install_update'),
  on: async (channel, cb) => { await listen(channel, (event) => cb(event.payload)); }
};

/* ============================================================
   NexTune — renderer.js
   UI logic, connects to Electron main via NexTune API (preload)
   Author: Anand <anand@picfomo.com>
   ============================================================ */

'use strict';

// ── State ─────────────────────────────────────────────────────
let state    = {};
let settings = {};
let streamOn = false;
let scanData = [];
let junkData = null;
let cleanerSelected = new Set();

// ── Init ──────────────────────────────────────────────────────
window.addEventListener('DOMContentLoaded', async () => {
  // Window controls
  document.getElementById('minimizeBtn').onclick = () => NexTune.minimize();
  document.getElementById('maximizeBtn').onclick = () => NexTune.maximize();
  document.getElementById('closeBtn').onclick    = () => NexTune.close();

  // App version
  const v = await NexTune.getVersion();
  document.getElementById('appVersion').textContent = `v${v}`;
  document.getElementById('aboutVersion').textContent = `v${v}`;

  // Load state
  state    = await NexTune.getState();
  settings = await NexTune.getSettings();
  applySettings(settings);

  if (state.streamMode) setStreamModeUI(true);
  if (state.autobotInstalled) applyAutobotUI(true);
  if (state.cleanedBytes) {
    document.getElementById('totalCleaned').textContent = fmtBytes(state.cleanedBytes);
  }

  // Auto-load startup + history in background
  loadStartupItems();
  loadHistory();
  renderServices();
  
  // Render RAM Graph
  renderRamGraph();
  
  // Update graph periodically (every 60s)
  setInterval(renderRamGraph, 60000);

  // Live stats listener
  NexTune.on('stats-update', updateStats);
  NexTune.on('stream-mode-changed', setStreamModeUI);
  NexTune.on('update-available', (info) => {
    document.getElementById('updateBadge').style.display = 'flex';
    toast(`🆕 NexTune ${info.version} is available!`, 'info', 6000);
  });
  NexTune.on('update-downloaded', () => {
    toast('✅ Update downloaded — will install on next restart', 'success', 5000);
  });
});

// ── Tab Switching ─────────────────────────────────────────────
function switchTab(id, el) {
  document.querySelectorAll('.tab-panel').forEach(p => p.classList.remove('active'));
  document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
  document.getElementById(`panel-${id}`).classList.add('active');
  if (el) el.classList.add('active');
}

// ── Live Stats ────────────────────────────────────────────────
function updateStats(s) {
  // CPU
  setGauge('cpuRing', s.cpuPct, '#7c3aed');
  document.getElementById('cpuPct').innerHTML = `${s.cpuPct}<span>%</span>`;
  document.getElementById('cpuTemp').textContent = s.cpuTemp ? `${s.cpuTemp} °C` : '';
  document.getElementById('cpuTempChip').textContent = s.cpuTemp ? `${s.cpuTemp} °C` : '--';
  colorGaugeByLoad('gaugeCpu', s.cpuPct);

  // RAM
  setGauge('ramRing', s.ramPct, '#06b6d4');
  document.getElementById('ramPct').innerHTML = `${s.ramPct}<span>%</span>`;
  document.getElementById('ramDetail').textContent = `${s.ramUsed} / ${s.ramTotal} GB`;
  document.getElementById('ramFree').textContent = `${s.ramFree} GB`;
  colorGaugeByLoad('gaugeRam', s.ramPct);

  // GPU
  setGauge('gpuRing', s.gpuPct, '#10b981');
  document.getElementById('gpuPct').innerHTML = `${s.gpuPct}<span>%</span>`;

  // Disk
  document.getElementById('diskRead').textContent = `${s.diskRead}`;
  document.getElementById('diskDetail').textContent = `R: ${s.diskRead} | W: ${s.diskWrite} MB/s`;
}

// ── RAM Timeline Graph ────────────────────────────────────────
async function renderRamGraph() {
  const canvas = document.getElementById('ramTimelineGraph');
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  
  // Fix blurriness on high-DPI displays
  const dpr = window.devicePixelRatio || 1;
  const rect = canvas.parentElement.getBoundingClientRect();
  canvas.width = rect.width * dpr;
  canvas.height = rect.height * dpr;
  ctx.scale(dpr, dpr);
  
  const width = rect.width;
  const height = rect.height;
  
  const history = await NexTune.getRamHistory();
  
  ctx.clearRect(0, 0, width, height);
  if (!history || history.length < 2) {
    ctx.fillStyle = 'rgba(255, 255, 255, 0.4)';
    ctx.font = '12px Inter';
    ctx.textAlign = 'center';
    ctx.fillText('Gathering RAM usage data...', width / 2, height / 2);
    return;
  }
  
  // Determine min/max for Y axis mapping
  const padding = 10;
  const graphWidth = width;
  const graphHeight = height - padding * 2;
  
  const maxRam = Math.max(...history.map(d => d.ram_used_mb)) * 1.1; // 10% headroom
  const minRam = Math.max(0, Math.min(...history.map(d => d.ram_used_mb)) * 0.9);
  
  const range = maxRam - minRam || 1;
  
  const xStep = graphWidth / (history.length - 1);
  
  // Draw Area Fill
  ctx.beginPath();
  ctx.moveTo(0, height);
  history.forEach((point, i) => {
    const x = i * xStep;
    const y = padding + graphHeight - ((point.ram_used_mb - minRam) / range) * graphHeight;
    ctx.lineTo(x, y);
  });
  ctx.lineTo(width, height);
  ctx.closePath();
  
  const gradient = ctx.createLinearGradient(0, 0, 0, height);
  gradient.addColorStop(0, 'rgba(6, 182, 212, 0.3)'); // Cyan top
  gradient.addColorStop(1, 'rgba(6, 182, 212, 0.0)'); // Transparent bottom
  ctx.fillStyle = gradient;
  ctx.fill();
  
  // Draw Line
  ctx.beginPath();
  history.forEach((point, i) => {
    const x = i * xStep;
    const y = padding + graphHeight - ((point.ram_used_mb - minRam) / range) * graphHeight;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  });
  ctx.strokeStyle = '#06b6d4'; // Cyan
  ctx.lineWidth = 2;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  ctx.stroke();
}

function setGauge(id, pct, color) {
  const ring = document.getElementById(id);
  if (!ring) return;
  const circ = 314;
  ring.style.strokeDashoffset = circ - (circ * Math.min(pct, 100) / 100);
  ring.style.stroke = color;
  ring.style.filter = `drop-shadow(0 0 6px ${color})`;
}

function colorGaugeByLoad(cardId, pct) {
  const card = document.getElementById(cardId);
  if (!card) return;
  card.style.borderColor = pct > 85 ? 'rgba(239,68,68,0.4)' : pct > 60 ? 'rgba(245,158,11,0.3)' : 'rgba(255,255,255,0.06)';
}

// ── Quick Clean ───────────────────────────────────────────────
async function quickClean() {
  const btn = document.querySelector('.btn-primary');
  setLoading(btn, true);
  const r = await NexTune.killAll();
  setLoading(btn, false);
  if (r) {
    toast(`💀 All background bloat terminated!`, 'success');
    runScan(); // refresh scan results
  }
}

// ── Smart Scan ────────────────────────────────────────────────
async function runScan() {
  const state = document.getElementById('scanState');
  const grid  = document.getElementById('processGrid');
  state.innerHTML = '<div class="loading-state"><div class="spinner-lg"></div><p>Scanning your PC for background bloat...</p></div>';
  state.style.display = 'block';
  grid.style.display = 'none';

  const procs = await NexTune.scanProcesses();
  scanData = procs;

  if (!procs || procs.length === 0) {
    state.innerHTML = '<div class="scan-idle"><div class="scan-idle-icon">✅</div><h3>No Bloat Detected!</h3><p>Your PC is clean. No known background bloat is currently running.</p></div>';
    document.getElementById('bloatCount').textContent = '0 found';
    document.getElementById('killAllBtn').style.display = 'none';
    return;
  }

  // Show results
  state.style.display = 'none';
  grid.style.display = 'grid';

  const killable = procs.filter(p => p.safe_to_kill);
  document.getElementById('bloatCount').textContent = `${procs.length} detected`;
  document.getElementById('scanBadge').textContent  = procs.length;
  document.getElementById('scanBadge').style.display = 'flex';
  document.getElementById('killAllBtn').style.display = killable.length > 0 ? 'inline-flex' : 'none';

  const catIcons = {
    'Android Emulator': '📱', 'Communication': '💬', 'Cloud Sync': '☁️',
    'Design Tool': '🎨', 'IoT': '🔌', 'Screen Recorder': '🎬',
    'Browser': '🌐', 'Game Launcher': '🎮', 'Media': '🎵',
    'Suspicious': '⚠️', 'Windows': '🪟', 'GPU Driver': '🖥️',
    'Antivirus': '🛡️', 'Streaming': '📡', 'Development': '💻', 'Device Sync': '📱'
  };

  grid.innerHTML = procs.map((p, i) => {
    const icon    = catIcons[p.category] || '⚙️';
    const killed  = state?.killedProcesses?.includes(p.exe);
    const actionBtn = !p.safe_to_kill
      ? `<span class="btn-protected">⚠️ Protected</span>`
      : killed
        ? `<span class="btn-killed">✓ Killed</span>`
        : `<button class="btn-kill" id="kill-${i}" onclick="killOne(${i}, '${p.exe}', '${p.name.replace(/'/g, "\\'")}', this)">💀 Kill</button>`;
    return `
      <div class="proc-card ${killed ? 'killed' : ''}" id="pcard-${i}">
        <div class="proc-emoji">${icon}</div>
        <div class="proc-info">
          <div class="proc-name">${p.name}</div>
          <div class="proc-vendor">${p.vendor || ''} · ${p.exe}</div>
          <div class="proc-ram">RAM: ~${p.memMB > 0 ? p.memMB + ' MB' : (p.ramRange || '?') + ' MB'} (live)</div>
          <div class="proc-desc">${p.description}</div>
          <span class="impact-badge impact-${p.impact}">${p.impact}</span>
        </div>
        ${actionBtn}
      </div>`;
  }).join('');
}

async function killOne(index, exe, name, btn) {
  setLoading(btn, true);
  const r = await NexTune.killProcess({ exe, name });
  if (r && r.ok) {
    const card = document.getElementById(`pcard-${index}`);
    card.classList.add('killed');
    btn.outerHTML = `<span class="btn-killed">✓ Killed</span>`;
    toast(`💀 ${name} terminated. RAM freed!`, 'success');
  } else {
    setLoading(btn, false);
    toast(`Process ${name} may already be closed`, 'info');
  }
}

async function killAllBloat() {
  const btn = document.getElementById('killAllBtn');
  setLoading(btn, true);
  const r = await NexTune.killAll();
  setLoading(btn, false);
  if (r) {
    toast(`💀 ${r.count || scanData.length} bloat processes terminated!`, 'success');
    document.querySelectorAll('.proc-card').forEach(c => c.classList.add('killed'));
    document.querySelectorAll('.btn-kill').forEach(b => { b.outerHTML = '<span class="btn-killed">✓ Killed</span>'; });
  }
}

// ── Startup Manager ───────────────────────────────────────────
async function loadStartupItems() {
  const state = document.getElementById('startupState');
  const list  = document.getElementById('startupList');
  if (state) { state.innerHTML = '<div class="loading-state"><div class="spinner-lg"></div><p>Reading Windows Registry...</p></div>'; state.style.display = 'block'; }
  if (list)  list.style.display = 'none';

  const items = await NexTune.getStartupItems();
  if (!items || items.length === 0) {
    if (state) state.innerHTML = '<div class="scan-idle"><div class="scan-idle-icon">🚀</div><h3>No startup items found</h3></div>';
    return;
  }
  if (state) state.style.display = 'none';
  if (list)  list.style.display = 'flex';

  list.innerHTML = items.map((item, i) => `
    <div class="startup-item" id="su-${i}">
      <div class="su-icon">💻</div>
      <div class="su-info">
        <div class="su-name">${item.name}</div>
        <div class="su-path">${item.path}</div>
        <div class="su-hive">${item.hive}</div>
      </div>
      <div class="su-toggle">
        <label class="toggle" title="Toggle startup">
          <input type="checkbox" checked onchange="toggleStartup(${i}, this)" />
          <span class="toggle-track"></span>
        </label>
        <span class="toggle-label" id="su-lbl-${i}">ON</span>
      </div>
    </div>
  `).join('');

  // Store items for toggle
  window._startupItems = items;
}

async function toggleStartup(index, checkbox) {
  const item = window._startupItems[index];
  const enable = checkbox.checked;
  checkbox.disabled = true;

  const r = await NexTune.toggleStartup({ name: item.name, hive: item.hive, key: item.key, enable, regPath: item.path });

  checkbox.disabled = false;
  const row = document.getElementById(`su-${index}`);
  const lbl = document.getElementById(`su-lbl-${index}`);
  if (r && r.ok) {
    lbl.textContent = enable ? 'ON' : 'OFF';
    row.classList.toggle('disabled', !enable);
    toast(enable ? `✅ ${item.name} re-enabled at startup` : `🚫 ${item.name} removed from startup`, 'success');
  } else {
    checkbox.checked = !enable; // revert
    toast('Failed to change startup entry', 'error');
  }
}

// ── Deep Cleaner ──────────────────────────────────────────────
async function scanJunk() {
  const state   = document.getElementById('cleanerState');
  const results = document.getElementById('cleanerResults');
  state.innerHTML = '<div class="loading-state"><div class="spinner-lg"></div><p>Scanning for junk files...</p></div>';
  state.style.display = 'block';
  results.style.display = 'none';

  junkData = await NexTune.scanJunk();

  state.style.display = 'none';
  results.style.display = 'block';

  const catIcons = {
    system: '🪟', user: '👤', logs: '📋', updates: '🔄', cache: '💾', browser: '🌐'
  };

  document.getElementById('cleanerSummary').innerHTML = `
    <div class="cleaner-total">${junkData.totalMB} MB</div>
    <div class="cleaner-total-label">Total junk found across ${junkData.items.length} categories</div>
  `;

  cleanerSelected = new Set(junkData.items.map(i => i.category));

  document.getElementById('cleanerCategories').innerHTML = junkData.items.map((item, i) => `
    <div class="cleaner-cat selected" id="cat-${i}" onclick="toggleCatSelect(${i}, this)">
      <input type="checkbox" class="cat-check" checked id="catCheck-${i}" onclick="event.stopPropagation(); toggleCatSelect(${i}, document.getElementById('cat-${i}'))" />
      <div class="cat-icon">${catIcons[item.category] || '📁'}</div>
      <div class="cat-info">
        <div class="cat-name">${item.label}</div>
        <div class="cat-path">${item.path}</div>
      </div>
      <div class="cat-size">${item.sizeMB} MB</div>
    </div>
  `).join('');
}

function toggleCatSelect(index, el) {
  const item = junkData.items[index];
  const chk  = document.getElementById(`catCheck-${index}`);
  const selected = !chk.checked;
  chk.checked = selected;
  el.classList.toggle('selected', selected);
  if (selected) cleanerSelected.add(item.category);
  else cleanerSelected.delete(item.category);
}

function selectAllCategories(on) {
  junkData.items.forEach((item, i) => {
    const el  = document.getElementById(`cat-${i}`);
    const chk = document.getElementById(`catCheck-${i}`);
    chk.checked = on;
    el.classList.toggle('selected', on);
    if (on) cleanerSelected.add(item.category); else cleanerSelected.delete(item.category);
  });
}

async function cleanSelected() {
  if (cleanerSelected.size === 0) { toast('Select at least one category', 'error'); return; }
  const btn = document.querySelector('.cleaner-footer .btn-primary');
  setLoading(btn, true);

  const r = await NexTune.cleanJunk({ categories: [...cleanerSelected] });
  setLoading(btn, false);

  if (r && r.ok) {
    toast(`🧹 Cleaned ${r.cleanedMB} MB of junk files!`, 'success', 5000);
    document.getElementById('totalCleaned').textContent = `${r.cleanedMB} MB`;
    // Reset cleaner UI
    document.getElementById('cleanerResults').style.display = 'none';
    document.getElementById('cleanerState').innerHTML = `
      <div class="scan-idle">
        <div class="scan-idle-icon">✅</div>
        <h3>Cleaned ${r.cleanedMB} MB!</h3>
        <p>All selected junk files removed successfully.</p>
        <button class="btn-primary" onclick="scanJunk()" style="margin-top:20px;">Scan Again</button>
      </div>`;
    document.getElementById('cleanerState').style.display = 'block';
  } else {
    toast('Some files could not be deleted (may be in use)', 'error');
  }
}

// ── Services ──────────────────────────────────────────────────
const SERVICES = [
  { name: 'SysMain (Superfetch)', key: 'SysMain', desc: 'Pre-loads apps into RAM — competes with games/stream for memory. Disable for best gaming performance.', note: 'SSD users: definite disable. HDD users: keep enabled.', action: 'manual', risk: 'safe' },
  { name: 'Print Spooler', key: 'Spooler', desc: 'Manages print queue. No printer connected? This wastes memory sitting idle every boot.', note: 'Re-enables automatically if printer is connected', action: 'manual', risk: 'safe' },
  { name: 'Windows Search Indexing', key: 'WSearch', desc: 'Constantly indexes files in background — causes random disk I/O spikes that hurt recording & gaming.', note: 'Pause during stream; search still works, just no real-time re-indexing', action: 'stop', risk: 'safe' },
  { name: 'Intel XTU Service', key: 'XTU3SERVICE', desc: 'Intel Extreme Tuning Utility background service — only useful if actively overclocking.', note: 'Zero value if not overclocking', action: 'manual', risk: 'safe' },
  { name: 'Xbox Live Auth Manager', key: 'XblAuthManager', desc: 'Xbox authentication daemon — only needed if using Xbox Game Bar or the Xbox app.', note: 'Game Bar still works — this is just the background auth service', action: 'manual', risk: 'safe' },
  { name: 'WSL Service', key: 'WSLService', desc: 'Windows Subsystem for Linux — development tools and MCP servers may depend on this.', note: 'Keep running if you use WSL or Antigravity', action: 'keep', risk: 'protect' },
  { name: 'NVIDIA Display Container', key: 'NVDisplay.ContainerLocalSystem', desc: 'NVIDIA GPU driver service — NVENC encoder (OBS) and GPU rendering depend on this completely.', note: 'Never disable — kills GPU functionality and OBS encoding', action: 'keep', risk: 'protect' }
];

function renderServices() {
  const list = document.getElementById('servicesList');
  if (!list) return;
  list.innerHTML = SERVICES.map((svc, i) => `
    <div class="svc-item">
      <div class="svc-info">
        <div class="svc-name">${svc.name}</div>
        <div class="svc-key">${svc.key}</div>
        <div class="svc-desc">${svc.desc}</div>
        ${svc.note ? `<div class="svc-note">💡 ${svc.note}</div>` : ''}
      </div>
      <div class="svc-right">
        ${svc.risk === 'protect'
          ? `<span class="badge badge-protect">PROTECTED</span>`
          : `<span class="badge badge-safe">SAFE</span>
             <button class="btn-secondary" id="svcBtn-${i}" style="padding:7px 14px;font-size:12px;"
               onclick="applyService('${svc.key}', '${svc.name}', '${svc.action}', ${i})">Apply</button>`
        }
      </div>
    </div>
  `).join('');
}

async function applyService(key, name, action, index) {
  const btn = document.getElementById(`svcBtn-${index}`);
  setLoading(btn, true);
  const r = await NexTune.toggleService({ key, action });
  setLoading(btn, false);
  toast(r && r.ok ? `⚙️ ${name} — updated!` : `Failed to update ${name}`, r && r.ok ? 'success' : 'error');
}

// ── Stream Mode ───────────────────────────────────────────────
async function toggleStreamMode() {
  const btn = document.getElementById('streamToggle');
  setLoading(btn, true);

  if (!streamOn) {
    const r = await NexTune.streamOn();
    setLoading(btn, false);
    if (r && r.ok) { setStreamModeUI(true); toast('🔴 STREAM MODE ON — PC fully optimized!', 'success', 5000); }
  } else {
    const r = await NexTune.streamOff();
    setLoading(btn, false);
    if (r && r.ok) { setStreamModeUI(false); toast('⏹ Stream Mode OFF — System restored.', 'info'); }
  }
}

function setStreamModeUI(on) {
  streamOn = on;
  const orb   = document.getElementById('streamOrb');
  const title = document.getElementById('streamTitle');
  const desc  = document.getElementById('streamDesc');
  const icon  = document.getElementById('streamBtnIcon');
  const text  = document.getElementById('streamBtnText');
  const pill  = document.getElementById('streamPill');
  const disp  = document.getElementById('streamDisplay');

  if (on) {
    orb.className   = 'stream-orb live';
    orb.textContent = '🔴';
    title.textContent = 'PC in STREAM MODE';
    desc.textContent  = 'All background bloat killed. CPU at max. OBS prioritized. Click to end stream.';
    icon.textContent  = '⏹';
    text.textContent  = 'END STREAM';
    pill.style.display = 'flex';
    disp.classList.add('live-mode');
  } else {
    orb.className   = 'stream-orb idle';
    orb.textContent = '⚡';
    title.textContent = 'PC in Idle Mode';
    desc.textContent  = 'Enable Stream Mode to kill background bloat, max CPU performance, and prioritize your game/stream.';
    icon.textContent  = '🔴';
    text.textContent  = 'GO LIVE';
    pill.style.display = 'none';
    disp.classList.remove('live-mode');
  }
}

// ── History ───────────────────────────────────────────────────
async function loadHistory() {
  const list = document.getElementById('historyList');
  if (!list) return;

  const h = await NexTune.getHistory();
  if (!h || h.length === 0) {
    list.innerHTML = '<div class="loading-state"><p>No history yet. Actions you take will appear here.</p></div>';
    return;
  }

  list.innerHTML = h.map(item => {
    const dt  = new Date(item.timestamp);
    const fmt = `${dt.toLocaleDateString()} ${dt.toLocaleTimeString([], { hour:'2-digit', minute:'2-digit' })}`;
    return `
      <div class="history-item ${item.undone ? 'undone' : ''}" id="hist-${item.id}">
        <div class="hist-time">${fmt}</div>
        <div class="hist-action">${item.action}</div>
        <div class="hist-detail">${item.detail}</div>
        ${item.undoCmd && !item.undone
          ? `<button class="btn-undo" onclick="undoAction(${item.id}, this)">↩ Undo</button>`
          : item.undone ? `<span style="font-size:11px;color:var(--text-3);">Undone</span>` : ''
        }
      </div>`;
  }).join('');
}

async function undoAction(id, btn) {
  setLoading(btn, true);
  const r = await NexTune.undoAction({ id });
  if (r && r.ok) {
    toast('↩ Action undone!', 'success');
    document.getElementById(`hist-${id}`)?.classList.add('undone');
    btn.outerHTML = `<span style="font-size:11px;color:var(--text-3);">Undone</span>`;
  } else {
    setLoading(btn, false);
    toast('Could not undo this action', 'error');
  }
}

// ── AutoBot ───────────────────────────────────────────────────
async function installAutoBot() {
  const btn = document.getElementById('autobotInstallBtn');
  setLoading(btn, true);
  const r = await NexTune.installAutoBot();
  setLoading(btn, false);
  if (r && r.ok) { applyAutobotUI(true); toast('🤖 AutoBot installed! Runs at every login + every 30 min.', 'success', 5000); }
}

async function uninstallAutoBot() {
  const btn = document.getElementById('autobotRemoveBtn');
  setLoading(btn, true);
  const r = await NexTune.uninstallAutoBot();
  setLoading(btn, false);
  if (r && r.ok) { applyAutobotUI(false); toast('🗑️ AutoBot removed.', 'info'); }
}

function applyAutobotUI(on) {
  document.getElementById('autobotInstallBtn').style.display = on ? 'none' : 'inline-flex';
  document.getElementById('autobotRemoveBtn').style.display  = on ? 'inline-flex' : 'none';
}

// ── Settings ──────────────────────────────────────────────────
function applySettings(s) {
  const get = id => document.getElementById(id);
  if (get('settingStartMin'))  get('settingStartMin').checked  = s.startMinimized || false;
  if (get('settingTrayClose')) get('settingTrayClose').checked = s.trayOnClose !== false;
  if (get('settingNotifs'))    get('settingNotifs').checked    = s.notifications !== false;
  if (get('settingInterval'))  get('settingInterval').value    = String(s.autoCleanInterval || 30);
  if (get('settingRestore'))   get('settingRestore').checked   = s.createRestorePoint !== false;
  if (get('settingSafeMode'))  get('settingSafeMode').checked  = s.safeMode !== false;
}

async function saveSettings() {
  const get = id => document.getElementById(id);
  const s = {
    startMinimized:     get('settingStartMin')?.checked  || false,
    trayOnClose:        get('settingTrayClose')?.checked !== false,
    notifications:      get('settingNotifs')?.checked    !== false,
    autoCleanInterval:  parseInt(get('settingInterval')?.value) || 30,
    createRestorePoint: get('settingRestore')?.checked   !== false,
    safeMode:           get('settingSafeMode')?.checked  !== false
  };
  await NexTune.saveSettings(s);
  toast('💾 Settings saved!', 'success');
}

// ── Utility ───────────────────────────────────────────────────
function fmtBytes(bytes) {
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`;
  return `${Math.round(bytes / 1048576)} MB`;
}

function setLoading(btn, loading) {
  if (!btn) return;
  if (loading) {
    btn._orig = btn.innerHTML;
    btn.innerHTML = '<span class="spinner-sm"></span>';
    btn.disabled = true;
  } else {
    btn.innerHTML = btn._orig || btn.innerHTML;
    btn.disabled = false;
  }
}

let _toastId = 0;
function toast(msg, type = 'info', duration = 3500) {
  const container = document.getElementById('toastContainer');
  const id  = ++_toastId;
  const div = document.createElement('div');
  div.className = `toast ${type}`;
  div.id = `toast-${id}`;
  div.textContent = msg;
  container.appendChild(div);
  requestAnimationFrame(() => { requestAnimationFrame(() => div.classList.add('show')); });
  setTimeout(() => {
    div.classList.remove('show');
    setTimeout(() => div.remove(), 400);
  }, duration);
}
