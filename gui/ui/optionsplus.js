// Logi Options+ style screens for mice and keyboards: home carousel, device page with
// tabs, hotspots placed from Logitech's own metadata, and a side panel per hotspot.
// Shares `state`, `invoke`, `esc`, `set`, `toast`, `render` with app.js.

const OP = {
  meta: {}, // depot name -> parsed metadata json
  remap: null,
  flow: null,
  pick: null, // nearby computer whose side is being chosen: {key, side}
  lastAsk: null, // last pairing asked for, to retry it
  codeDraft: "",
  codeSent: false,
  forget: null, // peer key waiting for the second "forget" click
  layout: null,
  services: {}, // service name -> {active, installed}
};

// Page errors go to ~/.local/state/hidpilot/gui.log.
window.addEventListener("error", (e) => invoke("log_js", { msg: `${e.message} @ ${e.filename}:${e.lineno}` }).catch(() => {}));
window.addEventListener("unhandledrejection", (e) => invoke("log_js", { msg: `unhandled: ${e.reason}` }).catch(() => {}));

// Actions a button or key can take, in menu order; labels are "action.<id>" in i18n.js.
const ACTIONS = [
  "none",
  "default",
  "click:left",
  "click:right",
  "click:middle",
  "click:back",
  "click:forward",
  "overview",
  "show-desktop",
  "workspace-left",
  "workspace-right",
  "app-switcher",
  "close-window",
  "lock",
  "screenshot",
  "emoji",
  "copy",
  "paste",
  "cut",
  "undo",
  "redo",
  "zoom-in",
  "zoom-out",
  "zoom-reset",
  "tab-next",
  "tab-previous",
  "media:play-pause",
  "media:next",
  "media:previous",
  "media:volume-up",
  "media:volume-down",
  "media:mute",
  "media:mic-mute",
  "smartshift",
];
const THUMB_PRESETS = ["scroll", "volume", "zoom", "workspaces", "tabs"];
// Hotspot names come from Logitech's SLOT_NAME_* ids; "slot.<id>" in i18n.js, with these
// sharing another entry's text. Key caps (Home, End...) keep their printed names.
const SLOT_ALIAS = {
  SLOT_NAME_GESTURE_LEFT_BUTTON: "gesture.left",
  SLOT_NAME_GESTURE_RIGHT_BUTTON: "gesture.right",
  SLOT_NAME_GESTURE_UP_BUTTON: "gesture.up",
  SLOT_NAME_GESTURE_DOWN_BUTTON: "gesture.down",
  SLOT_NAME_GESTURE_CLICK_BUTTON: "gesture.click",
  SLOT_NAME_THUMBWHEEL_SETTINGS: "slot.SLOT_NAME_THUMBWHEEL",
  SLOT_NAME_MIC_MUTE: "slot.SLOT_NAME_MUTE_UNMUTE_AUDIO",
  SLOT_NAME_MUTE_MICROPHONE: "slot.SLOT_NAME_MUTE_UNMUTE_AUDIO",
  SLOT_NAME_EMOJI: "action.emoji",
  SLOT_NAME_SCREENSHOT: "action.screenshot",
  SLOT_NAME_SCREEN_CAPTURE: "action.screenshot",
  SLOT_NAME_PLAY_PAUSE: "action.media:play-pause",
  SLOT_NAME_VOLUME_DOWN: "action.media:volume-down",
  SLOT_NAME_VOLUME_UP: "action.media:volume-up",
  SLOT_NAME_BACKLIGHT_SETTINGS: "backlight.title",
  SLOT_NAME_BACKLIGHT_EFFECTS: "backlight.title",
};
const SLOT_FIXED = { SLOT_NAME_HOME: "Home", SLOT_NAME_END: "End", SLOT_NAME_PAGE_UP: "Page Up", SLOT_NAME_PAGE_DOWN: "Page Down", SLOT_NAME_FUNCTION_KEY: "Fn" };
function slotLabel(n) {
  const easy = /^SLOT_NAME_EASYSWITCH_(\d)$/.exec(n);
  if (easy) return t("es.channel", { n: easy[1] });
  if (SLOT_FIXED[n]) return SLOT_FIXED[n];
  const key = SLOT_ALIAS[n] || `slot.${n}`;
  if (hasT(key)) return t(key);
  // Unknown slot names still read decently: SLOT_NAME_FOO_BAR -> "Foo bar".
  return String(n || t("slot.default")).replace(/^SLOT_NAME_/, "").replace(/_/g, " ").toLowerCase().replace(/^./, (c) => c.toUpperCase());
}
const GESTURE_DIRS = ["click", "up", "down", "left", "right"];

const depotOf = (name) => String(name).toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
const slugOf = (name) => String(name).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
const cidHex = (dec) => "0x" + Number(dec).toString(16).padStart(4, "0");

async function loadMeta(depot) {
  if (OP.meta[depot]) return OP.meta[depot];
  for (const f of ["core_metadata.json", "metadata.json"]) {
    try {
      OP.meta[depot] = await invoke("asset_json", { path: `optionsplus/${depot}/${f}` });
      return OP.meta[depot];
    } catch {}
  }
  invoke("log_js", { msg: `no hotspot metadata for ${depot} (run scripts/fetch-logitech-assets.sh)` });
  return null;
}

function metaImage(depot, key) {
  return OP.meta[depot]?.images?.find((i) => i.key === key) || null;
}

// ---------- home ----------
function greeting() {
  const h = new Date().getHours();
  return t(h < 12 ? "home.morning" : h < 18 ? "home.afternoon" : "home.evening");
}

const linkText = (link) => ({ bolt: "Logi Bolt", bluetooth: "Bluetooth", "zone-receiver": t("link.receiver"), off: t("link.off") })[link] || "";

function opHome() {
  const list = state.devices;
  const cards =
    list == null
      ? `<div class="spinner"></div>`
      : list.length === 0
        ? `<div class="op-empty">${t("home.connect")}</div>`
        : list
            .map(
              (d) => `<button class="op-card ${d.online ? "" : "off"}" data-open="${esc(d.id)}">
                <div class="op-pic op-${d.kind}">${picture(d.kind, d.name)}</div>
                <div class="op-pill">${!d.online ? t("home.off") : d.battery ? batteryHtml(d.battery) : `<span class="op-spin"></span>${t("home.setup")}`}</div>
                <div class="op-via">${esc(d.name)} · ${linkText(d.link)}</div>
              </button>`,
            )
            .join("");
  return `<div class="op">
    <header class="op-header">
      <h1>${greeting()}</h1>
      <nav>
        <button class="op-link" data-act="rescan">${t("home.refresh")}</button>
        <span class="op-divider"></span>
        <button class="op-icon" data-act="flow-home" title="Flow">⇄</button>
        <button class="op-icon" data-act="settings" title="${esc(t("home.settings"))}">⚙</button>
      </nav>
    </header>
    <main class="op-carousel">${cards}</main>
  </div>`;
}

// ---------- device page ----------
// Tab ids; their labels are "tab.<id>" in i18n.js.
const MOUSE_TABS = ["buttons", "point", "easyswitch", "flow"];
const KEYBOARD_TABS = ["keys", "layout", "backlight", "easyswitch", "flow"];
const HEADSET_TABS = ["audio", "settings", "about"];

function imageKeyFor(tab, kind) {
  if (kind === "keyboard") return { keys: "device_keys_image", backlight: "device_Backlight_effect_image", easyswitch: "device_easyswitch_image" }[tab];
  return { buttons: "device_buttons_image", point: "device_point_scroll_image", easyswitch: "device_easyswitch_image" }[tab];
}

function pictureFor(s, tab) {
  if (s.kind !== "mouse") return slugOf(s.name);
  if (tab === "easyswitch") return `${slugOf(s.name)}-back`;
  return `${slugOf(s.name)}-side`;
}

// Hotspot percentages refer to the metadata's origin box, with the picture centered and
// contained in it, so the figure takes that box's proportions.
function figureAspect(s, tab) {
  if (s.kind === "mouse" && tab === "easyswitch") return "";
  const img = metaImage(depotOf(s.name), imageKeyFor(tab, s.kind));
  return img?.origin ? `aspect-ratio:${img.origin.width} / ${img.origin.height}` : "";
}

// Hotspots of the current tab: [{slotId, cid ("c226"), slotName, marker, styles}]
function slots(s, tab) {
  const img = metaImage(depotOf(s.name), imageKeyFor(tab, s.kind));
  return (img?.assignments || []).filter((a) => a.marker).map((a) => ({ ...a, cid: a.slotId.split("_").pop(), origin: img.origin }));
}

function hotspots(s, tab) {
  return slots(s, tab)
    .map((a) => {
      const isKey = s.kind === "keyboard" && a.styles?.width;
      const style = isKey
        ? `left:${a.marker.x}%;top:${a.marker.y}%;width:${(a.styles.width / a.origin.width) * 100}%;height:${(a.styles.height / a.origin.height) * 100}%`
        : `left:${a.marker.x}%;top:${a.marker.y}%`;
      const active = state.op?.slot === a.slotId ? "active" : "";
      return `<button class="${isKey ? "op-hotkey" : "op-hotspot"} ${active}" style="${style}" data-slot="${esc(a.slotId)}" data-cid="${esc(a.cid)}" data-slotname="${esc(a.slotName || "")}" title="${esc(slotLabel(a.slotName))}"></button>`;
    })
    .join("");
}

function opFrame(s, tabs, tab, stage, panel) {
  return `<div class="op">
    <header class="op-dev-header">
      <button class="op-back" data-act="home">←</button>
      <div><div class="op-dev-name">${esc(s.name)}</div>${batteryHtml(s.battery)}</div>
      <nav class="op-tabs">${tabs.map((k) => `<button class="op-tab ${k === tab ? "active" : ""}" data-tab="${k}">${t(`tab.${k}`)}</button>`).join("")}</nav>
    </header>
    <div class="op-body with-panel">${stage}<aside class="op-panel">${panel}</aside></div>
  </div>`;
}

function opDevice(s) {
  if (s.kind === "headset") return opHeadset(s);
  const tabs = s.kind === "keyboard" ? KEYBOARD_TABS : MOUSE_TABS;
  const tab = state.op?.tab || tabs[0];
  const stage =
    tab === "flow"
      ? `<div class="op-stage op-flow-stage">${flowArrangement()}</div>`
      : `<div class="op-stage"><div class="op-figure op-fig-${s.kind}" style="${figureAspect(s, tab)}">
          <img src="lgasset://localhost/devices/${pictureFor(s, tab)}.webp" alt="" onerror="this.style.display='none'">
          ${tab === "layout" || (s.kind === "mouse" && tab === "easyswitch") ? "" : hotspots(s, tab)}
        </div></div>`;
  return opFrame(s, tabs, tab, stage, opPanel(s, tab));
}

// ---------- side panel ----------
const close = `<button class="close" data-act="panel-close">✕</button>`;

function actionLabel(binding) {
  if (binding == null || binding === "default") return t("action.default");
  if (typeof binding === "object") return t("action.gestures");
  return ACTIONS.includes(binding) ? t(`action.${binding}`) : binding;
}

function actionList(current, key) {
  return ACTIONS.map(
    (id) =>
      `<button class="op-option" data-key="${key}" data-bind="${esc(id)}"><span>${esc(t(`action.${id}`))}</span><span class="radio ${current === id ? "on" : ""}"></span></button>`,
  ).join("");
}

function serviceBanner(name, what) {
  const st = OP.services[name];
  if (!st || st.active) return "";
  return `<div class="op-banner"><div><b>${t("service.off", { name: what })}</b><br>${t("service.offNote")}</div>
    <button class="op-primary small" data-service="${name}">${t("service.enable")}</button></div>`;
}

function opPanel(s, tab) {
  if (tab === "flow") return flowPanel();
  if (tab === "easyswitch") return easySwitchPanel(s);
  if (tab === "layout") return layoutPanel(s);
  if (tab === "backlight") return backlightPanel(s);
  if (tab === "point") return pointPanel(s);
  return controlsPanel(s, tab);
}

function easySwitchPanel(s) {
  const h = s.hosts;
  if (!h) return `<h2>Easy-Switch</h2><p class="op-note">${t("es.noChannels")}</p>`;
  return `<h2>Easy-Switch</h2>
    <p class="op-note">${t(s.kind === "keyboard" ? "es.note.keyboard" : "es.note.mouse")}</p>
    ${Array.from({ length: h.count }, (_, i) => {
      const n = h.names?.[i]?.[1] || t("es.channel", { n: i + 1 });
      const here = i === h.current;
      return `<button class="op-channel ${here ? "current" : ""}" ${here ? "" : `data-host="${i}"`}>
        <span class="op-channel-n">${i + 1}</span>
        <span class="op-channel-name">${esc(n)}<small>${t(here ? "common.thisComputer" : "es.tapToSwitch")}</small></span>
      </button>`;
    }).join("")}
    <p class="op-note">${t(s.kind === "keyboard" ? "es.button.keyboard" : "es.button.mouse")}</p>`;
}

function layoutPanel(s) {
  const L = OP.layout;
  const typing = !L
    ? `<div class="spinner"></div>`
    : L.options
        .map(
          (o) => `<button class="op-option tall" data-layout="${esc(o.id)}"><span><b>${esc(hasT(`layout.${o.id}.name`) ? t(`layout.${o.id}.name`) : o.name)}</b><small>${esc(hasT(`layout.${o.id}.hint`) ? t(`layout.${o.id}.hint`) : o.hint)}</small></span><span class="radio ${o.id === L.current ? "on" : ""}"></span></button>`,
        )
        .join("");
  const p = s.platform;
  const OS_PT = { linux: "Linux", windows: "Windows", macos: "macOS", ios: "iOS", android: "Android", chrome: "ChromeOS" };
  const keysOs = p
    ? `<div class="op-sub">${t("layout.specialKeys")}</div>
      <p class="op-note">${t("layout.specialKeysNote")}</p>
      ${p.options
        .map(([id, names]) => {
          const label = names.map((n) => OS_PT[n] || n).join(", ");
          return `<button class="op-option" data-platform="${esc(names[0])}"><span>${esc(label)}</span><span class="radio ${id === p.current ? "on" : ""}"></span></button>`;
        })
        .join("")}`
    : "";
  return `<h2>${t("layout.title")}</h2>
    <p class="op-note">${t("layout.note")}</p>
    ${typing}
    ${L?.smartAccents ? `<p class="op-note">${t("layout.smartAccents")}</p>` : ""}
    ${keysOs}`;
}

function backlightPanel(s) {
  const b = s.backlight;
  if (!b) return `<h2>${t("backlight.title")}</h2><p class="op-note">${t("backlight.none")}</p>`;
  return `<h2>${t("backlight.title")}</h2>
    <div class="op-row"><span>${t("backlight.light")}</span>${toggle("backlight.mode", b.mode !== "off")}</div>
    ${b.auto ? `<div class="op-row"><span>${t("backlight.auto")}</span>${toggle("backlight.auto", b.mode === "auto")}</div>` : ""}
    ${b.manual && b.levels > 1 ? `<div class="op-sub">${t("backlight.level")}</div><input class="line" type="range" min="0" max="${b.levels - 1}" value="${b.level}" data-backlight style="--fill:${(b.level / (b.levels - 1)) * 100}%">` : ""}
    <p class="op-note">${t("backlight.note")}</p>`;
}

function pointPanel(s) {
  const i = Math.max(0, (s.dpiChoices || []).indexOf(s.dpi));
  const n = Math.max(1, (s.dpiChoices || [1]).length - 1);
  const ss = s.smartshift;
  return `<h2>${t("point.title")}</h2>
    ${s.dpiChoices ? `<div class="op-sub">${t("point.speedCaps")} <b>${s.dpi ?? "—"} DPI</b></div>
    <input class="line" type="range" min="0" max="${n}" value="${i}" data-dpi-live style="--fill:${(i / n) * 100}%">` : ""}
    ${ss ? `<div class="op-sub">${t("point.wheelCaps")}</div>
    <div class="op-row"><span>${t("point.smartshift")}</span>${toggle("smartshift.ratchet", !ss.ratchet)}</div>
    <div class="op-sub">${t("point.sensitivityCaps")} <b>${ss.threshold >= 50 ? t("point.max") : ss.threshold}</b></div>
    <input class="line" type="range" min="1" max="50" value="${Math.min(50, ss.threshold)}" data-threshold style="--fill:${(Math.min(50, ss.threshold) / 50) * 100}%">` : ""}
    ${s.wheel ? `<div class="op-row"><span>${t("point.smooth")}</span>${toggle("wheel.hires", s.wheel.hires)}</div>
    <div class="op-row"><span>${t("point.natural")}</span>${toggle("wheel.invert", s.wheel.invert)}</div>` : ""}
    ${s.thumbwheelInvert != null ? `<div class="op-sub">${t("point.thumbCaps")}</div>
    <div class="op-row"><span>${t("point.invertDir")}</span>${toggle("thumbwheelInvert", s.thumbwheelInvert)}</div>` : ""}`;
}

// Buttons (mouse) and keys (keyboard): a list when nothing is picked, actions otherwise.
function controlsPanel(s, tab) {
  const r = OP.remap;
  const what = t(s.kind === "keyboard" ? "controls.keys" : "controls.buttons");
  const banner = serviceBanner("hidpilot-remap", t("service.remap"));
  if (!r) return `<h2>${what}</h2><div class="spinner"></div>`;
  const sl = slots(s, tab);
  const slot = state.op?.slot;
  if (!slot) {
    const items = sl
      .map((a) => {
        const key = a.cid.startsWith("c") ? cidHex(a.cid.slice(1)) : null;
        const b = key ? r.bindings[key] : null;
        const thumb = /thumb_wheel/.test(a.slotId);
        const value = thumb ? t(`thumb.${typeof r.thumbwheel === "string" ? r.thumbwheel : "scroll"}`) : actionLabel(b);
        return `<button class="op-option" data-slot="${esc(a.slotId)}" data-cid="${esc(a.cid)}" data-slotname="${esc(a.slotName || "")}"><span>${esc(slotLabel(a.slotName))}</span><span class="value">${esc(value)} ›</span></button>`;
      })
      .join("");
    return `${banner}<h2>${what}</h2><p class="op-note">${t(s.kind === "keyboard" ? "controls.pick.keyboard" : "controls.pick.mouse")}</p>${items || `<p class="op-note">${t("controls.noData", { script: "scripts/fetch-logitech-assets.sh" })}</p>`}`;
  }
  const back = `<button class="op-secondary" data-act="panel-close">← ${what}</button>`;
  const name = slotLabel(state.op.slotName);
  if (/thumb_wheel/.test(slot)) return `${banner}${back}<h2>${esc(name)}</h2>${thumbPresets()}`;
  const key = cidHex(String(state.op.cid).replace(/^c/, ""));
  if (!r.buttons.some((b) => b.cid === key)) {
    return `${back}<h2>${esc(name)}</h2><p class="op-note">${t("controls.fixed")}</p>`;
  }
  const binding = r.bindings[key];
  if (binding && typeof binding === "object") {
    return `${banner}${back}<h2>${esc(name)}</h2><div class="op-sub">${t("controls.gesturesCaps")}</div>
      ${GESTURE_DIRS.map(
        (dir) => `<div class="op-row"><span>${t(`gesture.${dir}`)}</span><select data-gesture="${dir}" data-key="${key}">
          ${ACTIONS.map((id) => `<option value="${id}" ${binding[dir] === id ? "selected" : ""}>${esc(t(`action.${id}`))}</option>`).join("")}
        </select></div>`,
      ).join("")}
      <button class="op-secondary" data-bind="default" data-key="${key}">${t("controls.restore")}</button>`;
  }
  const current = typeof binding === "string" ? binding : "default";
  return `${banner}${back}<h2>${esc(name)}</h2><div class="op-list">${actionList(current, key)}</div>`;
}

function thumbPresets() {
  const cur = OP.remap?.thumbwheel ?? "scroll";
  const curName = typeof cur === "string" ? cur : "custom";
  return `<div class="op-sub">${t("thumb.action")}</div>${THUMB_PRESETS
    .map((id) => `<button class="op-option" data-thumb="${id}"><span>${t(`thumb.${id}`)}</span><span class="radio ${curName === id ? "on" : ""}"></span></button>`)
    .join("")}`;
}

// ---------- headset, in the same frame as the other devices ----------
function opHeadset(s) {
  const tab = state.op?.tab || "audio";
  const stage = `<div class="op-stage"><div class="op-headset-hero">${picture("headset", s.name)}</div></div>`;
  let panel = "";
  if (s.btOnly) {
    panel = `<h2>${t("bt.title")}</h2>
      <div class="op-row"><span>${t("common.battery")}</span>${s.battery ? batteryHtml(s.battery) : `<span class="value">—</span>`}</div>
      <p class="op-note">${t("bt.note")}</p>`;
    return opFrame(s, ["bluetooth"], "bluetooth", stage, panel);
  }
  if (tab === "audio") {
    const eq = s.equalizer || { bands: [0, 0, 0, 0, 0], preset: "Default" };
    const pct = s.sidetone == null ? 0 : Math.round((s.sidetone / s.sidetoneMax) * 100);
    panel = `<h2>${t("eq.title")}</h2>
      <div class="eq-box op-eq"><div class="eq">${eqCurve(eq.bands)}${eq.bands
        .map(
          (v, i) => `<div class="band" data-band="${i}" role="slider" aria-label="${esc(t("eq.band", { n: i + 1 }))}" aria-valuenow="${v}">
            <div class="rail"></div><div class="fill" style="height:${eqFrac(v) * 100}%"></div><div class="knob" style="bottom:${eqFrac(v) * 100}%"></div></div>`,
        )
        .join("")}</div>
        <div class="eq-labels"><span>${t("eq.bass")}</span><span>${t("eq.mid")}</span><span>${t("eq.treble")}</span></div></div>
      <div class="op-presets">${[...s.presets.map((p) => p.name), "Custom"]
        .map((n) => `<button class="op-chip ${n === eq.preset ? "on" : ""}" ${n === "Custom" ? "disabled" : `data-preset="${esc(n)}"`}>${esc(presetName(n))}</button>`)
        .join("")}</div>
      <div class="op-sub">${t("headset.sidetoneCaps")} <b>${pct}%</b></div>
      <input class="line" type="range" min="0" max="${s.sidetoneMax}" value="${s.sidetone ?? 0}" data-sidetone style="--fill:${pct}%">
      <div class="op-sub">${t("headset.micLevelCaps")} <b>${s.micLevel ?? "—"}%</b></div>
      <input class="line" type="range" min="0" max="100" step="5" value="${s.micLevel ?? 0}" data-mic style="--fill:${s.micLevel ?? 0}%">
      <div class="op-row"><span>${t("headset.micMuted")}</span>${toggle("micMuted", s.micMuted)}</div>`;
  } else if (tab === "settings") {
    panel = `<h2>${t("headset.settings")}</h2>
      <div class="op-sub">${t("headset.nameCaps")}</div>
      <div class="op-inline"><input class="text-input" maxlength="30" value="${esc(s.name)}" data-rename><button class="op-primary small" data-act="rename">${t("common.saveCaps")}</button></div>
      <p class="op-note">${t("headset.nameHint")}</p>
      <div class="op-row"><span>${t("headset.rotateToMute")}</span>${toggle("rotateToMute", s.rotateToMute)}</div>
      <div class="op-row"><span>${t("headset.voicePrompts")}</span>${toggle("voicePrompts", s.voicePrompts)}</div>
      <div class="op-row"><span>${t("headset.autoOff")}</span><select data-autooff>${[5, 10, 15, 30, 60, 120, 240]
        .map((m) => `<option value="${m}" ${m === s.autoSleep ? "selected" : ""}>${minutesText(m)}</option>`)
        .join("")}</select></div>
      <div class="op-row"><span>${t("headset.btConnected")}</span><span class="value">${s.bluetoothConnections == null ? "—" : t("devices.count", { n: s.bluetoothConnections })}</span></div>`;
  } else {
    panel = `<h2>${t("about.short")}</h2>
      <div class="op-row"><span>${t("about.model")}</span><span class="value">Zone Vibe Wireless</span></div>
      <div class="op-row"><span>${t("about.serial")}</span><span class="value">${esc(s.serial || "—")}</span></div>
      <div class="op-row"><span>${t("common.battery")}</span>${batteryHtml(s.battery)}</div>
      <button class="op-primary" data-act="diag">${t("headset.runDiag")}</button>
      ${(state.diag || []).map((r) => `<div class="op-row"><span>${esc(t(r.name))}</span><span class="value">${r.ok ? "✓ OK" : "✕ " + esc(r.detail ?? t("diag.noAnswer"))}</span></div>`).join("")}`;
  }
  return opFrame(s, HEADSET_TABS, tab, stage, panel);
}
// ---------- app settings ----------
function opSettings() {
  const st = OP.settings;
  const body = !st
    ? `<div class="spinner"></div>`
    : `<div class="op-row"><span><b>${t("settings.autostart")}</b><small class="op-small">${t("settings.autostartNote")}</small></span>${settingToggle("autostart", st.autostart)}</div>
       <div class="op-row ${st.autostart ? "" : "disabled"}"><span><b>${t("settings.openOnLogin")}</b><small class="op-small">${t("settings.openOnLoginNote")}</small></span>${settingToggle("openOnLogin", st.openOnLogin)}</div>
       <p class="op-note">${t("settings.trayNote", { quit: t("tray.quit") })}</p>
       <div class="op-row"><span><b>${t("settings.language")}</b></span><select data-language>
         <option value="auto">${esc(t("settings.languageAuto", { name: languageName(resolveLanguage("auto")) }))}</option>
         ${LANGUAGES.map(([code, name]) => `<option value="${code}" ${st.language === code ? "selected" : ""}>${esc(name)}</option>`).join("")}
       </select></div>`;
  return `<div class="op">
    <header class="op-dev-header"><button class="op-back" data-act="home">←</button><div><div class="op-dev-name">${t("settings.title")}</div></div></header>
    <div class="op-settings">${body}</div>
  </div>`;
}
const languageName = (code) => LANGUAGES.find(([c]) => c === code)?.[1] || code;
const settingToggle = (key, on) => `<button class="toggle ${on ? "on" : ""}" data-setting="${key}"></button>`;

async function loadSettings() {
  OP.settings = await invoke("settings_get").catch(() => null);
  render();
}

// ---------- Flow ----------
// Everything goes through the running hidpilot-flow service (flow_api in main.rs), which
// answers every call with its whole state. While a Flow screen is open it is asked again
// every second, so a computer asking to pair with this one shows up on its own.
const SIDES = ["left", "right", "top", "bottom"];

// The service's own error texts for a failed pairing, by a piece of their wording.
const FLOW_ERRORS = [
  ["wrong code", "flow.err.wrongCode"],
  ["code expired", "flow.err.expired"],
  ["could not reach", "flow.err.unreachable"],
  ["no longer on the network", "flow.err.gone"],
];
const flowError = (e) => {
  const hit = FLOW_ERRORS.find(([text]) => String(e).includes(text));
  return hit ? t(hit[1]) : String(e);
};

async function flowCall(call, body = {}, quiet = false) {
  try {
    OP.flow = { ...(await invoke("flow_api", { call, body })), running: true };
  } catch (e) {
    if (String(e) === "flow-not-running") OP.flow = { running: false };
    else if (!quiet) toast(flowError(e));
    if (call === "code") OP.codeSent = false;
  }
  // A new pairing step starts with an empty code box.
  if (OP.flow?.outgoing?.status !== "code") {
    OP.codeDraft = "";
    OP.codeSent = false;
  }
  return OP.flow;
}

async function loadFlow() {
  await flowCall("state");
  flowRender();
}

const flowOpen = () => state.page === "flow" || (state.page === "device" && state.op?.tab === "flow" && state.snap?.kind !== "headset");

// Redraws without losing the digits being typed in the code box.
function flowRender() {
  const typing = document.activeElement?.dataset?.flowCode !== undefined;
  render();
  OP.flowShown = JSON.stringify(OP.flow);
  const box = app.querySelector("[data-flow-code]");
  if (box && (typing || !document.activeElement || document.activeElement === document.body)) {
    box.focus();
    box.setSelectionRange(box.value.length, box.value.length);
  }
}

setInterval(async () => {
  if (!flowOpen() || document.visibilityState !== "visible") return;
  await flowCall("state", {}, true);
  // An open side menu would close under a redraw; the next second catches up.
  if (!flowOpen() || document.activeElement?.tagName === "SELECT") return;
  if (JSON.stringify(OP.flow) !== OP.flowShown) flowRender();
}, 1000);

function flowArrangement() {
  const f = OP.flow;
  if (!f) return `<div class="spinner"></div>`;
  const peers = f.peers || [];
  const placed = (side) => peers.find((p) => p.side === side);
  const screen = (label, cls, me) => `<div class="op-screen ${cls} ${me ? "me" : ""}">${esc(label)}</div>`;
  const cell = (side) => {
    const p = placed(side);
    return p ? screen(p.name || p.key.slice(0, 8), `${side} ${p.online ? "" : "offline"}`) : `<div class="op-screen-slot ${side}"></div>`;
  };
  return `<div class="op-arrange">${cell("top")}<div class="op-arrange-row">${cell("left")}${screen(f.name || t("common.thisComputer"), "center", true)}${cell("right")}</div>${cell("bottom")}</div>`;
}

function flowIncoming(f) {
  const i = f.incoming;
  if (!i) return "";
  return `<div class="op-incoming" role="alert">
    <b>${esc(t("flow.incoming.title", { name: i.name }))}</b>
    <div class="op-code">${esc(i.code.slice(0, 3))} ${esc(i.code.slice(3))}</div>
    <p>${esc(t("flow.incoming.note", { name: i.name }))}</p>
    <small>${t("flow.incoming.expires", { n: i.seconds })}</small>
  </div>`;
}

// This computer asking another one: waiting, code box, success or failure.
function flowOutgoing(f) {
  const o = f.outgoing;
  if (!o) return "";
  const cancel = `<button class="op-secondary" data-flow="cancel">${t("common.cancel")}</button>`;
  let body;
  if (o.status === "asking") {
    body = `<div class="op-wait"><span class="op-spin"></span>${esc(t("flow.asking", { name: o.name }))}</div>
      <p class="op-note">${esc(t("flow.askingNote", { name: o.name }))}</p>${cancel}`;
  } else if (o.status === "code") {
    body = `<label class="op-wait" for="flow-code">${esc(t("flow.typeCode", { name: o.name }))}</label>
      <input id="flow-code" class="text-input op-code-input" inputmode="numeric" autocomplete="off" maxlength="6" placeholder="000000"
        value="${esc(OP.codeDraft || "")}" data-flow-code ${OP.codeSent ? "disabled" : ""}>
      ${OP.codeSent ? `<div class="op-wait"><span class="op-spin"></span>${t("flow.checking")}</div>` : ""}${cancel}`;
  } else if (o.status === "done") {
    body = `<div class="op-wait ok">✓ ${esc(t("flow.paired", { name: o.name }))}</div>
      <button class="op-primary" data-flow="cancel">${t("common.ok")}</button>`;
  } else {
    body = `<div class="op-wait fail">✕ ${esc(t("flow.failed", { name: o.name }))}</div>
      ${o.error ? `<p class="op-note">${esc(flowError(o.error))}</p>` : ""}
      <button class="op-primary" data-flow="retry" data-key="${esc(o.key)}">${t("flow.retry")}</button>${cancel}`;
  }
  return `<div class="op-pairing">${body}</div>`;
}

// Where the other computer's screen sits, chosen before asking it.
function flowSidePicker(n) {
  const side = OP.pick.side;
  const b = (sd) => `<button class="op-side ${sd} ${side === sd ? "on" : ""}" data-flow="side" data-side="${sd}">${t(`side.${sd}`)}</button>`;
  return `<div class="op-pick">
    <div class="op-sub">${esc(t("flow.whereScreen", { name: n.name }))}</div>
    <div class="op-sides">${b("top")}${b("left")}<div class="op-side-me">${t("flow.thisScreen")}</div>${b("right")}${b("bottom")}</div>
    <button class="op-primary" data-flow="ask" ${side ? "" : "disabled"}>${t("flow.connect")}</button>
    <button class="op-secondary" data-flow="unpick">${t("common.cancel")}</button>
  </div>`;
}

function flowPanel() {
  const f = OP.flow;
  if (!f) return `<div class="spinner"></div>`;
  if (!f.running) {
    return `<h2>Flow</h2>
      <div class="op-banner"><div><b>${t("flow.notRunning")}</b><br>${t("flow.notRunningNote")}</div>
        <button class="op-primary small" data-service="hidpilot-flow">${t("service.enable")}</button></div>
      <p class="op-note">${t("flow.note")}</p>`;
  }
  const peers = f.peers.length
    ? f.peers
        .map(
          (p) => `<div class="op-peer"><div><b><i class="op-dot ${p.online ? "on" : ""}"></i>${esc(p.name || t("flow.computer"))}</b>
            <div class="op-note">${t(p.online ? "flow.online" : "flow.offline")} · <button class="op-forget" data-flow="forget" data-key="${esc(p.key)}">${t(OP.forget === p.key ? "flow.forgetConfirm" : "flow.forget")}</button></div></div>
          <select data-flow-side="${esc(p.key)}">
            <option value="">${t("flow.unplaced")}</option>
            ${SIDES.map((sd) => `<option value="${sd}" ${p.side === sd ? "selected" : ""}>${t(`side.${sd}`)}</option>`).join("")}
          </select></div>`,
        )
        .join("")
    : `<p class="op-note">${t("flow.noPeers")}</p>`;
  const busy = !!f.outgoing;
  const nearby = f.nearby.length
    ? f.nearby
        .map((n) =>
          OP.pick?.key === n.key && !busy
            ? `<div class="op-peer"><b>${esc(n.name)}</b></div>${flowSidePicker(n)}`
            : `<div class="op-peer"><b>${esc(n.name)}</b>${busy ? "" : `<button class="op-primary small" data-flow="pick" data-key="${esc(n.key)}">${t("flow.connect")}</button>`}</div>`,
        )
        .join("")
    : `<p class="op-note">${t("flow.noNearby")}</p>`;
  return `${flowIncoming(f)}<h2>Flow</h2>
    <p class="op-note">${t("flow.note")}</p>
    <div class="op-row"><span>${t("flow.clipboard")}</span>${flowToggle("clipboard", f.clipboard)}</div>
    <div class="op-row"><span>${t("flow.keyboardFollows")}</span>${flowToggle("keyboardFollows", f.keyboardFollows)}</div>
    <div class="op-row"><span>${t("flow.audioFollows")}</span>${flowToggle("audioFollows", f.audioFollows)}</div>
    <p class="op-note">${t("flow.audioNote")}</p>
    <div class="op-sub">${t("flow.pairedCaps")}</div>${peers}
    <p class="op-note">${t("flow.trust")}</p>
    <div class="op-sub">${t("flow.nearbyCaps")}</div>${flowOutgoing(f)}${nearby}`;
}

const flowToggle = (key, on) => `<button class="toggle ${on ? "on" : ""}" data-flow-toggle="${key}"></button>`;

// Buttons of the Flow panel (data-flow="...").
async function flowClick(el) {
  const d = el.dataset;
  const f = OP.flow || {};
  switch (d.flow) {
    case "pick":
      OP.pick = { key: d.key, side: null };
      return flowRender();
    case "side":
      OP.pick = { ...OP.pick, side: d.side };
      return flowRender();
    case "unpick":
      OP.pick = null;
      return flowRender();
    case "ask":
      if (!OP.pick?.side) return;
      OP.lastAsk = { ...OP.pick };
      OP.pick = null;
      await flowCall("pair", OP.lastAsk);
      return flowRender();
    case "retry":
      // Same computer and side as before, or choose the side again.
      if (OP.lastAsk?.key === d.key) await flowCall("pair", OP.lastAsk);
      else {
        await flowCall("cancel");
        OP.pick = { key: d.key, side: null };
      }
      return flowRender();
    case "cancel":
      await flowCall("cancel");
      return flowRender();
    case "forget":
      // Two clicks: the first one turns the link into a confirmation.
      if (OP.forget !== d.key) {
        OP.forget = d.key;
        return flowRender();
      }
      OP.forget = null;
      if (f.running) await flowCall("forget", { key: d.key });
      return flowRender();
  }
}

// Digits of the code shown on the other computer; sent as soon as all six are in.
function flowCodeInput(el) {
  const digits = el.value.replace(/\D/g, "").slice(0, 6);
  if (el.value !== digits) el.value = digits;
  OP.codeDraft = digits;
  if (digits.length === 6 && !OP.codeSent) {
    OP.codeSent = true;
    flowCall("code", { code: digits }).then(flowRender);
    flowRender();
  }
}

async function loadRemap() {
  OP.remap = null;
  try {
    OP.remap = await invoke("remap_get", { id: state.id });
  } catch (e) {
    toast(e);
  }
  render();
}

// ---------- events (registered by app.js through opClick/opChange) ----------
async function loadServices() {
  for (const name of ["hidpilot-remap", "hidpilot-flow"]) {
    OP.services[name] = await invoke("service_status", { name }).catch(() => null);
  }
  render();
}

async function loadLayout() {
  OP.layout = await invoke("layout_get").catch((e) => (toast(e), null));
  render();
}

// Loads what a tab needs the first time it is shown.
function enterTab(tab) {
  if (tab === "flow") loadFlow();
  if (tab === "layout") loadLayout();
  if (tab === "buttons" || tab === "keys") loadRemap();
  if (tab === "flow" || tab === "buttons" || tab === "keys") loadServices();
}

async function opClick(el) {
  const d = el.dataset;
  if (d.tab) {
    state.op = { tab: d.tab };
    enterTab(d.tab);
    return render();
  }
  if (d.slot) {
    state.op = { ...state.op, slot: d.slot, cid: d.cid, slotName: d.slotname };
    if (!OP.remap) loadRemap();
    return render();
  }
  if (d.service) {
    OP.services[d.service] = await invoke("service_enable", { name: d.service }).catch((e) => (toast(e), OP.services[d.service]));
    if (d.service === "hidpilot-flow") setTimeout(loadFlow, 800);
    return render();
  }
  if (d.layout) {
    OP.layout = await invoke("layout_set", { id: d.layout }).catch((e) => (toast(e), OP.layout));
    toast(t("layout.applied"));
    return render();
  }
  if (d.platform) return set("platform", d.platform);
  if (d.setting) {
    OP.settings = await invoke("settings_set", { key: d.setting, value: !el.classList.contains("on") }).catch((e) => (toast(e), OP.settings));
    return render();
  }
  if (d.bind !== undefined && d.key) {
    const value = d.bind === "default" ? null : d.bind;
    await invoke("remap_set", { id: state.id, cid: d.key, binding: value }).catch(toast);
    return loadRemap();
  }
  if (d.thumb) {
    await invoke("thumbwheel_set", { id: state.id, preset: d.thumb }).catch(toast);
    return loadRemap();
  }
  if (d.flowToggle) {
    await flowCall("set", { key: d.flowToggle, value: !el.classList.contains("on") });
    return flowRender();
  }
  if (d.flow) return flowClick(el);
  return false;
}

async function opChange(el) {
  const d = el.dataset;
  if (d.gesture) {
    const cur = OP.remap.bindings[d.key] || {};
    const next = { ...cur, [d.gesture]: el.value };
    await invoke("remap_set", { id: state.id, cid: d.key, binding: next }).catch(toast);
    return loadRemap();
  }
  if (d.flowSide) {
    await flowCall("place", { key: d.flowSide, side: el.value });
    return flowRender();
  }
  if (d.dpiLive !== undefined) return set("dpi", state.snap.dpiChoices[Number(el.value)]);
  if (d.sidetone !== undefined) return set("sidetone", Number(el.value));
  if (d.mic !== undefined) return set("micLevel", Number(el.value));
  if (d.autooff !== undefined) return set("autoSleep", Number(el.value));
  if (d.threshold !== undefined) return set("smartshift.threshold", Number(el.value));
  if (d.backlight !== undefined) return set("backlight.level", Number(el.value));
  if (d.language !== undefined) {
    OP.settings = await invoke("settings_set", { key: "language", value: el.value }).catch((e) => (toast(e), OP.settings));
    setLanguage(OP.settings?.language ?? el.value);
    return render();
  }
  return false;
}
