// hidpilot UI: Logi Tune style screens over the Rust backend (see gui/src/main.rs).
const { invoke } = window.__TAURI__.core;

const state = {
  page: "home",
  devices: null,
  id: null,
  snap: null,
  sheet: null,
  busy: false,
};

const app = document.getElementById("app");
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

// ---------- device pictures ----------
// Official renders come from the user's Logitech install when present (lgasset://);
// otherwise these line drawings stand in.
const ICONS = {
  headset: `<svg viewBox="0 0 48 48" width="100%" height="100%" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M8 30v-6a16 16 0 0 1 32 0v6"/><rect x="6" y="28" width="9" height="13" rx="4" fill="currentColor"/><rect x="33" y="28" width="9" height="13" rx="4" fill="currentColor"/><path d="M15 38c3 5 8 6 11 6" stroke-width="2"/></svg>`,
  mouse: `<svg viewBox="0 0 48 48" width="100%" height="100%" fill="none" stroke="currentColor" stroke-width="3"><rect x="12" y="5" width="24" height="38" rx="12"/><path d="M24 5v13M12 18h24"/></svg>`,
  keyboard: `<svg viewBox="0 0 48 48" width="100%" height="100%" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><rect x="3" y="12" width="42" height="24" rx="4"/><path d="M10 19h2M17 19h2M24 19h2M31 19h2M38 19h0M10 25h2M17 25h2M24 25h2M31 25h2M15 30h18"/></svg>`,
};
const picture = (kind, name) => {
  const slug = String(name || kind).toLowerCase().replace(/[^a-z0-9]+/g, "-");
  return `<img src="lgasset://localhost/devices/${slug}.webp" alt="" style="width:100%;height:100%;object-fit:contain" onerror="this.replaceWith(Object.assign(document.createElement('span'),{innerHTML:ICONS['${kind}']||'',style:'width:70%;height:70%;display:block'}))">`;
};
window.ICONS = ICONS;

function batteryHtml(b) {
  if (!b) return "";
  const p = b.percent;
  if (p == null) return `<span class="battery">${esc(b.level || "")}</span>`;
  const cls = p <= 10 ? "critical" : p <= 20 ? "low" : "";
  return `<span class="battery ${cls}"><i></i>${p}%${b.charging ? " ⚡" : ""}</span>`;
}

// ---------- backend calls ----------
async function loadDevices(rescan = false) {
  try {
    state.devices = await invoke("devices", { rescan });
  } catch (e) {
    toast(e);
    state.devices = [];
  }
  render();
}

async function openDevice(id) {
  const entry = state.devices?.find((d) => d.id === id);
  if (entry && !entry.online) return toast(t("toast.headsetOff"));
  state.id = id;
  state.page = "device";
  state.snap = null;
  state.op = null;
  OP.remap = null;
  render();
  // Over Bluetooth only the battery is known; the settings go through the USB receiver.
  if (entry?.kind === "headset" && entry.link === "bluetooth") {
    state.snap = { ...entry, btOnly: true };
    state.op = { tab: "audio" };
    return render();
  }
  try {
    state.snap = { ...(await invoke("device", { id })), link: entry?.link };
    if (state.snap.kind !== "headset") await loadMeta(depotOf(state.snap.name));
    enterTab(state.snap.kind === "keyboard" ? "keys" : state.snap.kind === "mouse" ? "buttons" : "audio");
  } catch (e) {
    toast(e);
    state.page = "home";
  }
  render();
}

async function set(key, value) {
  if (state.busy) return;
  state.busy = true;
  try {
    state.snap = await invoke("set", { id: state.id, key, value });
  } catch (e) {
    toast(e);
  } finally {
    state.busy = false;
    render();
  }
}

function toast(msg) {
  const t = document.createElement("div");
  t.className = "toast";
  t.textContent = String(msg);
  document.body.appendChild(t);
  setTimeout(() => t.remove(), 3500);
}

// ---------- screens ----------
function topbar() {
  return "";
}

function home() {
  const date = new Date().toLocaleDateString(LANG, { weekday: "long", day: "numeric", month: "long" });
  const list =
    state.devices == null
      ? `<div class="spinner"></div>`
      : state.devices.length === 0
        ? `<div class="empty">${t("home.noDevices")}<br>${t("home.noDevicesHint")}</div>`
        : state.devices
            .map(
              (d) => `<button class="device-row" data-open="${esc(d.id)}">
                <span class="thumb">${picture(d.kind, d.name)}</span>
                <span class="meta">${esc(d.name)}<br>${batteryHtml(d.battery)}</span>
              </button>`,
            )
            .join("");
  return `${topbar()}
    <div class="screen">
      <div class="date">${esc(date.charAt(0).toUpperCase() + date.slice(1))}</div>
      <div class="tabs"><div class="tab">${t("home.tabStart")}</div><div class="tab active">${t("home.tabDevices")}</div></div>
      ${list}
    </div>
    <div class="home-footer"><button data-act="rescan">${t("home.refreshDevices")}</button><span></span></div>`;
}

function deviceHeader(s) {
  return `<div class="dev-head">
    <button class="back" data-act="home">‹</button>
    <span class="thumb">${picture(s.kind, s.name)}</span>
    <div class="title"><div class="name">${esc(s.name)}</div>${batteryHtml(s.battery)}</div>
    <button class="info-btn" data-act="about">i</button>
  </div>`;
}

const AUTO_OFF = [5, 10, 15, 30, 60, 120, 240];
const minutesText = (m) => (m == null ? "—" : m < 60 ? t("time.minutes", { n: m }) : t("time.hours", { n: m / 60 }));
// Preset names are ids sent back to the headset; only their display is translated.
const presetName = (n) => (hasT(`eq.preset.${n}`) ? t(`eq.preset.${n}`) : n);
const toggle = (key, on) => `<button class="toggle ${on ? "on" : ""}" data-toggle="${key}" aria-pressed="${!!on}"></button>`;

function headsetPage(s) {
  const eq = s.equalizer;
  const pct = s.sidetone == null ? "—" : `${Math.round((s.sidetone / s.sidetoneMax) * 100)}%`;
  return `${topbar()}${deviceHeader(s)}
  <div class="screen"><div class="groups">
    <div class="card">
      <button class="row click" data-act="eq" style="width:100%"><span>${t("eq.title")}</span><span><span class="value">${eq ? esc(presetName(eq.preset)) : "—"}</span><span class="chev">›</span></span></button>
    </div>
    <div class="card">
      <button class="row click" data-sheet="sidetone" style="width:100%"><span>${t("headset.sidetone")}</span><span class="value">${pct}</span></button>
      <button class="row click" data-sheet="mic" style="width:100%"><span>${t("headset.micLevel")}</span><span class="value">${s.micLevel == null ? "—" : s.micLevel + "%"}</span></button>
      <div class="row"><span>${t("headset.micMuted")}</span>${toggle("micMuted", s.micMuted)}</div>
    </div>
    <div class="card">
      <button class="row click" data-sheet="name" style="width:100%"><span>${t("headset.name")}</span><span class="value">${esc(s.name)}</span></button>
      <div class="row"><span>${t("headset.rotateToMute")}</span>${toggle("rotateToMute", s.rotateToMute)}</div>
      <div class="row"><span>${t("headset.voicePrompts")}</span>${toggle("voicePrompts", s.voicePrompts)}</div>
      <button class="row click" data-act="connections" style="width:100%"><span>${t("headset.connections")}</span><span class="chev">›</span></button>
    </div>
    <div class="card">
      <button class="row click" data-sheet="autooff" style="width:100%"><span>${t("headset.autoOff")}</span><span class="value">${minutesText(s.autoSleep)}</span></button>
    </div>
    <div class="card">
      <div class="row"><span>${t("headset.diag")}</span><button class="pill" data-act="diag">${t("headset.runNow")}</button></div>
    </div>
  </div></div>`;
}

// Generic page for mice and keyboards until their Options+ screens land.
function hidppPage(s) {
  const rows = [];
  if (s.dpiChoices) {
    rows.push(`<button class="row click" data-sheet="dpi" style="width:100%"><span>${t("point.speedDpi")}</span><span class="value">${s.dpi ?? "—"}</span></button>`);
  }
  if (s.smartshift) {
    rows.push(`<div class="row"><span>SmartShift</span>${toggle("smartshift.ratchet", !s.smartshift.ratchet)}</div>`);
  }
  if (s.wheel) {
    rows.push(`<div class="row"><span>${t("point.smooth")}</span>${toggle("wheel.hires", s.wheel.hires)}</div>`);
    rows.push(`<div class="row"><span>${t("point.invertScroll")}</span>${toggle("wheel.invert", s.wheel.invert)}</div>`);
  }
  if (s.thumbwheelInvert != null) rows.push(`<div class="row"><span>${t("point.invertThumb")}</span>${toggle("thumbwheelInvert", s.thumbwheelInvert)}</div>`);
  if (s.backlight) rows.push(`<div class="row"><span>${t("backlight.title")}</span>${toggle("backlight.mode", s.backlight.mode !== "off")}</div>`);
  if (s.fnKeys != null) rows.push(`<div class="row"><span>${t("keys.fnKeys")}</span>${toggle("fnKeys", s.fnKeys)}</div>`);
  const hosts = s.hosts
    ? `<div class="card">${Array.from({ length: s.hosts.count }, (_, i) => {
        const n = s.hosts.names?.[i]?.[1] || t("es.channel", { n: i + 1 });
        return `<button class="row click" data-host="${i}" style="width:100%"><span>${i + 1}. ${esc(n)}</span><span class="radio ${i === s.hosts.current ? "on" : ""}"></span></button>`;
      }).join("")}</div>`
    : "";
  return `${topbar()}${deviceHeader(s)}
  <div class="screen"><div class="groups">
    <div class="card">${rows.join("")}</div>
    ${hosts ? `<div class="section-title">${t("tab.easyswitch")}</div>${hosts}` : ""}
  </div></div>`;
}

const EQ_LABELS = ["eq.bass", "", "eq.mid", "", "eq.treble"];
const EQ_MIN = -128;
const EQ_MAX = 127;
const eqFrac = (v) => (v - EQ_MIN) / (EQ_MAX - EQ_MIN);

function eqCurve(bands) {
  // Smooth curve through the knobs, filled to the bottom, as Logi Tune draws it.
  const xs = bands.map((_, i) => (i + 0.5) / bands.length);
  const ys = bands.map((v) => 1 - eqFrac(v));
  let d = `M0,${ys[0]}`;
  const pts = [[0, ys[0]], ...xs.map((x, i) => [x, ys[i]]), [1, ys[ys.length - 1]]];
  for (let i = 1; i < pts.length; i++) {
    const [x0, y0] = pts[i - 1];
    const [x1, y1] = pts[i];
    const mx = (x0 + x1) / 2;
    d += ` C${mx},${y0} ${mx},${y1} ${x1},${y1}`;
  }
  return `<svg viewBox="0 0 1 1" preserveAspectRatio="none"><path d="${d} L1,1 L0,1 Z" fill="var(--primary-light)" opacity=".45"/></svg>`;
}

function eqPage(s) {
  const eq = s.equalizer || { bands: [0, 0, 0, 0, 0], preset: "Default" };
  const bands = eq.bands;
  const knobs = bands
    .map(
      (v, i) => `<div class="band" data-band="${i}" role="slider" aria-label="${esc(t("eq.band", { n: i + 1 }))}" aria-valuemin="${EQ_MIN}" aria-valuemax="${EQ_MAX}" aria-valuenow="${v}">
        <div class="rail"></div>
        <div class="fill" style="height:${eqFrac(v) * 100}%"></div>
        <div class="knob" style="bottom:${eqFrac(v) * 100}%"></div>
      </div>`,
    )
    .join("");
  const current = eq.preset;
  const presets = [...s.presets.map((p) => p.name), "Custom"]
    .map((n) => `<button class="row click" data-preset="${esc(n)}" style="width:100%"><span>${esc(presetName(n))}</span><span class="radio ${n === current ? "on" : ""}"></span></button>`)
    .join("");
  return `${topbar()}
  <div class="page-head"><button class="back" data-act="device">←</button><h1>${t("eq.title")}</h1></div>
  <div class="screen">
    <div class="eq-box">
      <div class="eq">${eqCurve(bands)}${knobs}</div>
      <div class="eq-labels">${EQ_LABELS.map((l) => `<span>${l && t(l)}</span>`).join("")}</div>
      <div class="eq-caption">${t("eq.current")}</div>
    </div>
    <div class="section-title"><span>${t("eq.presets")}</span><button class="reset" data-preset="Default" title="${esc(t("eq.reset"))}">↺</button></div>
    <div class="card" style="margin:0 13px 24px">${presets}</div>
  </div>`;
}

function aboutPage(s) {
  return `${topbar()}
  <div class="page-head"><button class="back" data-act="device">←</button><h1>${t("about.title")}</h1></div>
  <div class="screen"><div class="about">
    <div class="hero">${picture(s.kind, s.name)}</div>
    <div class="name">${esc(s.name)}</div>
    ${batteryHtml(s.battery)}
    ${(s.firmware || []).length ? `<div class="info-card"><div class="k">${t("about.firmware")}</div>${s.firmware.map((f) => `<div class="v">${esc(f.kind)}: ${esc(f.version)}</div>`).join("")}</div>` : ""}
    ${s.serial ? `<div class="info-card"><div class="k">${t("about.serial")}</div><div class="v">${esc(s.serial)}<button class="copy" data-copy="${esc(s.serial)}" title="${esc(t("common.copy"))}">⧉</button></div></div>` : ""}
  </div></div>`;
}

function connectionsPage(s) {
  const n = s.bluetoothConnections;
  return `${topbar()}
  <div class="page-head"><button class="back" data-act="device">←</button><h1>${t("headset.connections")}</h1></div>
  <div class="screen"><div class="groups">
    <div class="card">
      <div class="row"><span>${t("conn.usbReceiver")}</span><span class="value">${t("conn.connected")}</span></div>
      <div class="row"><span>Bluetooth</span><span class="value">${n == null ? "—" : t("devices.count", { n })}</span></div>
    </div>
  </div></div>`;
}

// ---------- bottom sheets ----------
function sheet() {
  const s = state.snap;
  if (!state.sheet || !s) return "";
  const head = (t) => `<div class="sheet-head">${t}<button class="close" data-act="close">✕</button></div>`;
  let body = "";
  let foot = `<div class="sheet-foot"><button class="primary-btn" data-act="sheet-ok">${t("common.done")}</button></div>`;
  switch (state.sheet) {
    case "sidetone": {
      const v = state.draft ?? s.sidetone ?? 0;
      const pct = Math.round((v / s.sidetoneMax) * 100);
      body = `${head(t("headset.sidetone"))}<div class="sheet-body">
        <div class="label-row"><span>${t("headset.sidetoneLevel")}</span><span>${pct}%</span></div>
        <input class="line" type="range" min="0" max="${s.sidetoneMax}" value="${v}" data-draft style="--fill:${pct}%"></div>`;
      break;
    }
    case "mic": {
      const v = state.draft ?? s.micLevel ?? 0;
      body = `${head(t("headset.micLevel"))}<div class="sheet-body">
        <div class="label-row"><span>${t("headset.micLevelCaps")}</span><span>${v}%</span></div>
        <input class="line" type="range" min="0" max="100" step="5" value="${v}" data-draft style="--fill:${v}%"></div>`;
      break;
    }
    case "autooff": {
      const v = state.draft ?? s.autoSleep;
      body = `${head(t("headset.autoOff"))}<div class="sheet-body">${AUTO_OFF.map(
        (m) => `<button class="choice" data-pick="${m}" style="width:100%"><span>${minutesText(m)}</span><span class="radio ${m === v ? "on" : ""}"></span></button>`,
      ).join("")}</div>`;
      foot = `<div class="sheet-foot"><button class="primary-btn" data-act="sheet-ok">${t("common.save")}</button></div>`;
      break;
    }
    case "name": {
      body = `${head(t("headset.name"))}<div class="sheet-body">
        <input class="text-input" maxlength="30" value="${esc(state.draft ?? s.name)}" data-name>
        <div class="hint">${t("headset.nameHint")}</div></div>`;
      foot = `<div class="sheet-foot"><button class="primary-btn" data-act="sheet-ok">${t("common.save")}</button></div>`;
      break;
    }
    case "dpi": {
      const choices = s.dpiChoices;
      const v = state.draft ?? s.dpi;
      const i = Math.max(0, choices.indexOf(v));
      const pct = Math.round((i / (choices.length - 1)) * 100);
      body = `${head(t("point.speed"))}<div class="sheet-body">
        <div class="label-row"><span>DPI</span><span>${v}</span></div>
        <input class="line" type="range" min="0" max="${choices.length - 1}" value="${i}" data-dpi style="--fill:${pct}%"></div>`;
      break;
    }
    case "diag": {
      body = `${head(t("headset.diag"))}<div class="sheet-body">${(state.diag || [])
        .map((r) => `<div class="choice"><span>${esc(t(r.name))}</span><span class="value">${r.ok ? "✓ OK" : "✕ " + esc(r.detail ?? t("diag.noAnswer"))}</span></div>`)
        .join("")}</div>`;
      foot = `<div class="sheet-foot"><button class="primary-btn" data-act="close">${t("common.done")}</button></div>`;
      break;
    }
  }
  return `<div class="scrim" data-act="scrim"><div class="sheet">${body}${foot}</div></div>`;
}

async function sheetOk() {
  const which = state.sheet;
  const v = state.draft;
  state.sheet = null;
  state.draft = null;
  if (v == null) return render();
  if (which === "sidetone") await set("sidetone", Number(v));
  if (which === "mic") await set("micLevel", Number(v));
  if (which === "autooff") await set("autoSleep", Number(v));
  if (which === "name") await set("name", String(v).trim());
  if (which === "dpi") await set("dpi", Number(v));
}

async function diagnose() {
  // Re-reads every headset setting; anything that fails to answer is reported.
  // Names are i18n keys and a missing detail means "no response", so they follow the language.
  const s = await invoke("device", { id: state.id }).catch((e) => ({ error: String(e) }));
  const check = (name, v) => ({ name, ok: v != null && !s.error, detail: s.error || null });
  state.diag = [
    check("diag.receiver", s.name),
    check("common.battery", s.battery),
    check("eq.title", s.equalizer),
    check("diag.mic", s.micMuted),
    check("headset.sidetone", s.sidetone),
    check("diag.micSystem", s.micLevel),
  ];
  if (!s.error) state.snap = s;
  render();
}

// ---------- render & events ----------
function render() {
  const s = state.snap;
  let html;
  if (state.page === "flow") html = opFlowPage();
  else if (state.page === "settings") html = opSettings();
  else if (state.page === "home" || (!s && state.page !== "device")) html = opHome();
  else if (!s) html = `<div class="spinner"></div>`;
  else html = opDevice(s);
  // Keep the side panel's scroll position across live refreshes.
  const scroll = app.querySelector(".op-panel")?.scrollTop;
  app.innerHTML = html;
  const panel = app.querySelector(".op-panel");
  if (panel && scroll) panel.scrollTop = scroll;
}

function opFlowPage() {
  return `<div class="op">
    <header class="op-dev-header"><button class="op-back" data-act="home">←</button><div><div class="op-dev-name">Flow</div></div></header>
    <div class="op-body with-panel"><div class="op-stage op-flow-stage">${flowArrangement()}</div><aside class="op-panel">${flowPanel()}</aside></div>
  </div>`;
}

app.addEventListener("click", async (ev) => {
  const el = ev.target.closest(
    "[data-act],[data-open],[data-sheet],[data-toggle],[data-pick],[data-preset],[data-host],[data-copy],[data-tab],[data-slot],[data-bind],[data-thumb],[data-flow-toggle],[data-flow],[data-service],[data-layout],[data-platform],[data-setting]",
  );
  if (!el) return;
  const d = el.dataset;
  if (d.tab || d.slot || d.bind !== undefined || d.thumb || d.flowToggle || d.flow || d.service || d.layout || d.platform || d.setting) return opClick(el);
  if (d.open) return openDevice(d.open);
  if (d.sheet) {
    state.sheet = d.sheet;
    state.draft = null;
    return render();
  }
  if (d.toggle) {
    const cur = el.classList.contains("on");
    // SmartShift's switch shows "on" for free spin enabled automatically, i.e. not ratchet.
    if (d.toggle === "smartshift.ratchet") return set(d.toggle, cur);
    if (d.toggle === "backlight.mode") return set(d.toggle, cur ? "off" : "on");
    if (d.toggle === "backlight.auto") return set("backlight.mode", cur ? "on" : "auto");
    return set(d.toggle, !cur);
  }
  if (d.pick) {
    state.draft = Number(d.pick);
    return render();
  }
  if (d.preset) {
    const p = state.snap.presets.find((x) => x.name === d.preset);
    if (!p) return;
    return set("equalizer", p.bands);
  }
  if (d.host) return set("host", Number(d.host));
  if (d.copy) {
    await navigator.clipboard?.writeText(d.copy).catch(() => {});
    return toast(t("common.copied"));
  }
  switch (d.act) {
    case "home":
      state.page = "home";
      state.snap = null;
      state.op = null;
      return loadDevices();
    case "device":
      state.page = "device";
      return render();
    case "eq":
    case "about":
    case "connections":
      state.page = d.act;
      return render();
    case "rescan":
      state.devices = null;
      render();
      return loadDevices(true);
    case "diag":
      return diagnose();
    case "rename": {
      const v = app.querySelector("[data-rename]")?.value.trim();
      if (v && v !== state.snap.name) await set("name", v);
      return;
    }
    case "close":
      state.sheet = null;
      state.draft = null;
      return render();
    case "scrim":
      if (ev.target === el) {
        state.sheet = null;
        state.draft = null;
        render();
      }
      return;
    case "sheet-ok":
      return sheetOk();
    case "panel-close":
      state.op = { tab: state.op?.tab };
      return render();
    case "settings":
      state.page = "settings";
      render();
      return loadSettings();
    case "flow-home":
      state.page = "flow";
      render();
      return loadFlow();
  }
});

app.addEventListener("input", (ev) => {
  const t = ev.target;
  if (t.dataset.draft !== undefined) {
    state.draft = Number(t.value);
    render();
    app.querySelector("[data-draft]")?.focus();
  } else if (t.dataset.dpi !== undefined) {
    state.draft = state.snap.dpiChoices[Number(t.value)];
    render();
    app.querySelector("[data-dpi]")?.focus();
  } else if (t.dataset.name !== undefined) {
    state.draft = t.value;
  } else if (t.dataset.flowCode !== undefined) {
    flowCodeInput(t);
  } else if (t.dataset.flowAddr !== undefined) {
    flowAddrInput(t);
  }
});

app.addEventListener("change", (ev) => {
  const d = ev.target.dataset;
  if (
    d.gesture || d.flowSide || d.dpiLive !== undefined || d.threshold !== undefined || d.backlight !== undefined ||
    d.sidetone !== undefined || d.mic !== undefined || d.autooff !== undefined || d.language !== undefined
  )
    opChange(ev.target);
});

// EQ bands are dragged with the pointer. Only a press that starts on a band and is then
// released writes to the headset, so stray clicks or re-renders never change the EQ.
let eqDrag = null;
function eqValueAt(band, clientY) {
  const r = band.getBoundingClientRect();
  const frac = 1 - Math.min(1, Math.max(0, (clientY - r.top) / r.height));
  return Math.round(EQ_MIN + frac * (EQ_MAX - EQ_MIN));
}
function eqShow(i, bands) {
  const box = app.querySelector(".eq");
  if (!box) return;
  box.querySelector("svg").outerHTML = eqCurve(bands);
  const band = box.querySelector(`.band[data-band="${i}"]`);
  band.querySelector(".fill").style.height = `${eqFrac(bands[i]) * 100}%`;
  band.querySelector(".knob").style.bottom = `${eqFrac(bands[i]) * 100}%`;
}
app.addEventListener("pointerdown", (ev) => {
  const band = ev.target.closest(".eq .band");
  if (!band || ev.button !== 0) return;
  band.setPointerCapture(ev.pointerId);
  const i = Number(band.dataset.band);
  const bands = [...(state.snap.equalizer?.bands || [0, 0, 0, 0, 0])];
  eqDrag = { i, band, bands };
  bands[i] = eqValueAt(band, ev.clientY);
  eqShow(i, bands);
});
app.addEventListener("pointermove", (ev) => {
  if (!eqDrag) return;
  eqDrag.bands[eqDrag.i] = eqValueAt(eqDrag.band, ev.clientY);
  eqShow(eqDrag.i, eqDrag.bands);
});
app.addEventListener("pointerup", async () => {
  if (!eqDrag) return;
  const { bands } = eqDrag;
  eqDrag = null;
  await set("equalizer", bands);
});
app.addEventListener("pointercancel", () => {
  eqDrag = null;
  render();
});

// Live view of the hardware: re-read the open device and redraw when something changed
// on the device itself (boom flipped to mute, DPI button, backlight keys, battery...).
// Skipped while the window is hidden or the user is in the middle of an interaction.
let lastSnap = "";
async function refreshLive() {
  if (state.page !== "device" || !state.id || !state.snap || state.snap.btOnly || state.busy || eqDrag) return;
  if (document.visibilityState !== "visible") return;
  const focused = document.activeElement;
  if (focused && ["INPUT", "SELECT", "TEXTAREA"].includes(focused.tagName)) return;
  try {
    const fresh = { ...(await invoke("device", { id: state.id })), link: state.snap.link };
    const text = JSON.stringify(fresh);
    if (text !== lastSnap && text !== JSON.stringify(state.snap) && state.page === "device" && !state.busy) {
      state.snap = fresh;
      render();
    }
    lastSnap = text;
  } catch {}
}
// The headset answers in a few milliseconds over its USB receiver; wireless HID++ devices
// are asked less often to spare their batteries.
setInterval(() => {
  if (state.snap?.kind === "headset") refreshLive();
}, 1500);
setInterval(() => {
  if (state.snap && state.snap.kind !== "headset") refreshLive();
}, 4000);

// A device switched on or off (the headset re-enumerates its receiver) shows up on its own.
setInterval(async () => {
  try {
    if (await invoke("devices_changed")) await onDevicesChanged();
  } catch {}
}, 3000);

async function onDevicesChanged() {
  const was = state.page === "device" ? state.id : null;
  const link = state.snap?.link;
  await loadDevices();
  if (!was) return;
  const now = state.devices.find((d) => d.id === was);
  if (!now || !now.online) {
    state.page = "home";
    state.snap = null;
    render();
  } else if (now.link !== link) {
    // The headset moved between its receiver and Bluetooth: reopen it on the new path.
    await openDevice(was);
  }
}

// Keep battery levels fresh while the window is open.
setInterval(() => {
  if (state.page === "home" && !state.sheet) loadDevices();
}, 60_000);

// The saved language comes first so the first screen is already in it.
invoke("settings_get")
  .then((st) => setLanguage(st.language))
  .catch(() => setLanguage("auto"))
  .finally(() => loadDevices());
