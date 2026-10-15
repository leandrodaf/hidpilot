// Logi Options+ style screens for mice and keyboards: home carousel, device page with
// tabs, hotspots placed from Logitech's own metadata, and a side panel per hotspot.
// Shares `state`, `invoke`, `esc`, `set`, `toast`, `render` with app.js.

const OP = {
  meta: {}, // depot name -> parsed metadata json (or null when missing)
  remap: null,
  flow: null,
  pairCode: null,
};

const ACTION_PT = {
  none: "Não fazer nada",
  default: "Função original",
  "click:left": "Clique com o botão esquerdo",
  "click:right": "Clique com o botão direito",
  "click:middle": "Clique com o botão do meio",
  "click:back": "Voltar",
  "click:forward": "Avançar",
  overview: "Visão geral das atividades",
  "show-desktop": "Mostrar área de trabalho",
  "workspace-left": "Área de trabalho anterior",
  "workspace-right": "Próxima área de trabalho",
  "app-switcher": "Alternar aplicativos",
  "close-window": "Fechar janela",
  lock: "Bloquear tela",
  screenshot: "Captura de tela",
  emoji: "Emoji",
  copy: "Copiar",
  paste: "Colar",
  cut: "Recortar",
  undo: "Desfazer",
  redo: "Refazer",
  "zoom-in": "Ampliar",
  "zoom-out": "Reduzir",
  "zoom-reset": "Zoom padrão",
  "tab-next": "Próxima aba",
  "tab-previous": "Aba anterior",
  "media:play-pause": "Reproduzir/pausar",
  "media:next": "Próxima faixa",
  "media:previous": "Faixa anterior",
  "media:volume-up": "Aumentar volume",
  "media:volume-down": "Diminuir volume",
  "media:mute": "Silenciar",
  "media:mic-mute": "Silenciar microfone",
  smartshift: "Alternar SmartShift",
};
const THUMB_PT = {
  scroll: "Rolagem horizontal",
  volume: "Volume",
  zoom: "Zoom",
  workspaces: "Áreas de trabalho",
  tabs: "Navegação por abas",
};
const SLOT_PT = {
  SLOT_NAME_MIDDLE_BUTTON: "Botão do meio",
  SLOT_NAME_MODESHIFT_BUTTON: "Botão de alternância de modo",
  SLOT_NAME_THUMBWHEEL: "Roda para o polegar",
  SLOT_NAME_FORWARD_BUTTON: "Botão Avançar",
  SLOT_NAME_BACK_BUTTON: "Botão Voltar",
  SLOT_NAME_GESTURE_BUTTON: "Botão de gesto",
  SLOT_NAME_GESTURE_LEFT_BUTTON: "Esquerda",
  SLOT_NAME_GESTURE_RIGHT_BUTTON: "Direita",
  SLOT_NAME_GESTURE_UP_BUTTON: "Para cima",
  SLOT_NAME_GESTURE_DOWN_BUTTON: "Para baixo",
  SLOT_NAME_GESTURE_CLICK_BUTTON: "Clique",
  SLOT_NAME_BACKLIGHT_DOWN: "Diminuir luz de fundo",
  SLOT_NAME_BACKLIGHT_UP: "Aumentar luz de fundo",
  SLOT_NAME_DICTATION: "Ditado",
  SLOT_NAME_EMOJI: "Emoji",
  SLOT_NAME_SCREENSHOT: "Captura de tela",
  SLOT_NAME_SCREEN_CAPTURE: "Captura de tela",
  SLOT_NAME_MIC_MUTE: "Mudo do microfone",
  SLOT_NAME_MUTE_MICROPHONE: "Mudo do microfone",
  SLOT_NAME_SEARCH: "Pesquisa",
  SLOT_NAME_PLAY_PAUSE: "Reproduzir/pausar",
  SLOT_NAME_MUTE: "Mudo",
  SLOT_NAME_VOLUME_DOWN: "Diminuir volume",
  SLOT_NAME_VOLUME_UP: "Aumentar volume",
  SLOT_NAME_HOME: "Home",
  SLOT_NAME_END: "End",
  SLOT_NAME_PAGE_UP: "Page Up",
  SLOT_NAME_PAGE_DOWN: "Page Down",
};
// Unknown slot names still read decently: SLOT_NAME_FOO_BAR -> "Foo bar".
const slotLabel = (n) => SLOT_PT[n] || String(n || "Botão").replace(/^SLOT_NAME_/, "").replace(/_/g, " ").toLowerCase().replace(/^./, (c) => c.toUpperCase());
const GESTURE_DIRS = [
  ["click", "Clique"],
  ["up", "Para cima"],
  ["down", "Para baixo"],
  ["left", "Esquerda"],
  ["right", "Direita"],
];

const depotOf = (name) => String(name).toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
const slugOf = (name) => String(name).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
const cidHex = (dec) => "0x" + Number(dec).toString(16).padStart(4, "0");

async function loadMeta(depot) {
  if (depot in OP.meta) return OP.meta[depot];
  OP.meta[depot] = null;
  for (const f of ["core_metadata.json", "metadata.json"]) {
    try {
      const r = await fetch(`lgasset://localhost/optionsplus/${depot}/${f}`);
      if (r.ok) {
        OP.meta[depot] = await r.json();
        break;
      }
    } catch {}
  }
  return OP.meta[depot];
}

function metaImage(depot, key) {
  return OP.meta[depot]?.images?.find((i) => i.key === key) || null;
}

// ---------- home ----------
function greeting() {
  const h = new Date().getHours();
  return h < 12 ? "Bom dia" : h < 18 ? "Boa tarde" : "Boa noite";
}

function opHome() {
  const list = state.devices;
  const cards =
    list == null
      ? `<div class="spinner"></div>`
      : list.length === 0
        ? `<div class="op-empty">CONECTE SEUS DISPOSITIVOS</div>`
        : list
            .map(
              (d) => `<button class="op-card" data-open="${esc(d.id)}">
                <div class="op-pic op-${d.kind}">${picture(d.kind, d.name)}</div>
                <div class="op-pill">${d.battery ? batteryHtml(d.battery) : `<span class="op-spin"></span>CONFIGURAÇÃO`}</div>
              </button>`,
            )
            .join("");
  return `<div class="op">
    <header class="op-header">
      <h1>${greeting()}</h1>
      <nav>
        <button class="op-link" data-act="rescan">⟳ ATUALIZAR</button>
        <span class="op-divider"></span>
        <button class="op-icon" data-act="flow-home" title="Flow">⇄</button>
      </nav>
    </header>
    <main class="op-carousel">${cards}</main>
  </div>`;
}

// ---------- device page ----------
const MOUSE_TABS = [
  ["buttons", "BOTÕES"],
  ["point", "PONTEIRO E ROLAGEM"],
  ["easyswitch", "EASY-SWITCH"],
  ["flow", "FLOW"],
];
const KEYBOARD_TABS = [
  ["keys", "TECLAS"],
  ["backlight", "ILUMINAÇÃO"],
  ["easyswitch", "EASY-SWITCH"],
  ["flow", "FLOW"],
];

function imageKeyFor(tab, kind) {
  if (kind === "keyboard") return { keys: "device_keys_image", backlight: "device_Backlight_effect_image", easyswitch: "device_easyswitch_image" }[tab];
  return { buttons: "device_buttons_image", point: "device_point_scroll_image", easyswitch: "device_easyswitch_image" }[tab];
}

function pictureFor(s, tab) {
  if (s.kind === "keyboard") return slugOf(s.name);
  if (tab === "easyswitch") return `${slugOf(s.name)}-back`;
  return `${slugOf(s.name)}-side`;
}

// Hotspot percentages refer to the metadata's origin box, with the picture centered and
// contained in it, so the figure takes that box's proportions.
function figureAspect(s, tab) {
  const img = metaImage(depotOf(s.name), imageKeyFor(tab, s.kind));
  return img?.origin ? `aspect-ratio:${img.origin.width} / ${img.origin.height}` : "";
}

function hotspots(s, tab) {
  const depot = depotOf(s.name);
  const img = metaImage(depot, imageKeyFor(tab, s.kind));
  if (!img?.assignments) return "";
  return img.assignments
    .filter((a) => a.marker)
    .map((a) => {
      const id = a.slotId.split("_").pop();
      const isKey = s.kind === "keyboard" && a.styles?.width;
      const style = isKey
        ? `left:${a.marker.x}%;top:${a.marker.y}%;width:${(a.styles.width / img.origin.width) * 100}%;height:${(a.styles.height / img.origin.height) * 100}%`
        : `left:${a.marker.x}%;top:${a.marker.y}%`;
      const active = state.op?.slot === a.slotId ? "active" : "";
      return `<button class="${isKey ? "op-hotkey" : "op-hotspot"} ${active}" style="${style}" data-slot="${esc(a.slotId)}" data-cid="${esc(id)}" data-slotname="${esc(a.slotName || "")}" title="${esc(slotLabel(a.slotName))}"></button>`;
    })
    .join("");
}

function opDevice(s) {
  const tabs = s.kind === "keyboard" ? KEYBOARD_TABS : MOUSE_TABS;
  const tab = state.op?.tab || tabs[0][0];
  const showPicture = tab !== "flow";
  return `<div class="op">
    <header class="op-dev-header">
      <button class="op-back" data-act="home">←</button>
      <div><div class="op-dev-name">${esc(s.name)}</div>${batteryHtml(s.battery)}</div>
      <nav class="op-tabs">${tabs.map(([k, l]) => `<button class="op-tab ${k === tab ? "active" : ""}" data-tab="${k}">${l}</button>`).join("")}</nav>
    </header>
    <div class="op-body ${state.op?.slot || tab === "flow" || tab === "backlight" ? "with-panel" : ""}">
      ${
        showPicture
          ? `<div class="op-stage"><div class="op-figure op-fig-${s.kind}" style="${figureAspect(s, tab)}">
              <img src="lgasset://localhost/devices/${pictureFor(s, tab)}.webp" alt="" onerror="this.style.display='none'">
              ${hotspots(s, tab)}
            </div></div>`
          : `<div class="op-stage op-flow-stage">${flowArrangement()}</div>`
      }
      ${opPanel(s, tab)}
    </div>
  </div>`;
}

// ---------- side panel ----------
function actionList(current, attr) {
  return Object.entries(ACTION_PT)
    .map(
      ([id, label]) =>
        `<button class="op-option" ${attr}="${esc(id)}"><span>${esc(label)}</span><span class="radio ${current === id ? "on" : ""}"></span></button>`,
    )
    .join("");
}

function opPanel(s, tab) {
  const slot = state.op?.slot;
  const close = `<button class="close" data-act="panel-close">✕</button>`;
  const r = OP.remap;
  if (tab === "flow") return `<aside class="op-panel">${flowPanel()}</aside>`;
  if (tab === "backlight" && s.backlight) {
    const b = s.backlight;
    return `<aside class="op-panel"><h2>Configurações de iluminação</h2>
      <div class="op-row"><span>Iluminação</span>${toggle("backlight.mode", b.mode !== "off")}</div>
      ${b.auto ? `<div class="op-row"><span>Ajuste automático</span>${toggle("backlight.auto", b.mode === "auto")}</div>` : ""}
      ${b.manual && b.levels > 1 ? `<div class="op-sub">NÍVEL</div><input class="line" type="range" min="0" max="${b.levels - 1}" value="${b.level}" data-backlight style="--fill:${(b.level / (b.levels - 1)) * 100}%">` : ""}
      <p class="op-note">As teclas de luz do teclado continuam funcionando normalmente.</p></aside>`;
  }
  if (!slot) return "";
  const cid = state.op.cid;
  const name = slotLabel(state.op.slotName);

  if (tab === "point") {
    if (cid === "settings" || /mouse_settings$/.test(slot)) {
      const i = Math.max(0, (s.dpiChoices || []).indexOf(s.dpi));
      const n = (s.dpiChoices || [1]).length - 1;
      return `<aside class="op-panel">${close}<h2>Velocidade do ponteiro</h2>
        <div class="op-sub">VELOCIDADE <b>${s.dpi ?? "—"} DPI</b></div>
        <input class="line" type="range" min="0" max="${n}" value="${i}" data-dpi-live style="--fill:${(i / Math.max(1, n)) * 100}%"></aside>`;
    }
    if (/scroll_wheel_settings$/.test(slot)) {
      const ss = s.smartshift;
      return `<aside class="op-panel">${close}<h2>Roda de rolagem</h2>
        ${ss ? `<div class="op-row"><span>SmartShift</span>${toggle("smartshift.ratchet", !ss.ratchet)}</div>
        <div class="op-sub">SENSIBILIDADE DO SMARTSHIFT <b>${ss.threshold >= 50 ? "máx." : ss.threshold}</b></div>
        <input class="line" type="range" min="1" max="50" value="${Math.min(50, ss.threshold)}" data-threshold style="--fill:${(Math.min(50, ss.threshold) / 50) * 100}%">` : ""}
        ${s.wheel ? `<div class="op-row"><span>Rolagem suave</span>${toggle("wheel.hires", s.wheel.hires)}</div>
        <div class="op-row"><span>Direção de rolagem natural</span>${toggle("wheel.invert", s.wheel.invert)}</div>` : ""}</aside>`;
    }
    if (/thumb_wheel_settings$/.test(slot)) {
      return `<aside class="op-panel">${close}<h2>Roda para o polegar</h2>
        ${s.thumbwheelInvert != null ? `<div class="op-row"><span>Inverter direção</span>${toggle("thumbwheelInvert", s.thumbwheelInvert)}</div>` : ""}
        ${thumbPresets()}</aside>`;
    }
  }
  if (tab === "easyswitch") {
    const h = s.hosts;
    if (!h) return "";
    return `<aside class="op-panel">${close}<h2>Easy-Switch</h2>
      ${Array.from({ length: h.count }, (_, i) => {
        const n = h.names?.[i]?.[1] || `Canal ${i + 1}`;
        return `<button class="op-option" data-host="${i}"><span>${i + 1}. ${esc(n)}</span><span class="radio ${i === h.current ? "on" : ""}"></span></button>`;
      }).join("")}
      <p class="op-note">Trocar de canal leva o dispositivo para o outro computador.</p></aside>`;
  }
  // Buttons and keys.
  if (!r) return `<aside class="op-panel">${close}<div class="spinner"></div></aside>`;
  if (/thumb_wheel_adapter$/.test(slot)) {
    return `<aside class="op-panel">${close}<h2>${esc(name)}</h2>${thumbPresets()}</aside>`;
  }
  const key = cidHex(String(cid).replace(/^c/, ""));
  const known = r.buttons.some((b) => b.cid === key);
  if (!known) {
    return `<aside class="op-panel">${close}<h2>${esc(name)}</h2><p class="op-note">Este controle não pode ser personalizado neste dispositivo.</p></aside>`;
  }
  const binding = r.bindings[key];
  if (binding && typeof binding === "object") {
    return `<aside class="op-panel">${close}<h2>${esc(name)}</h2><div class="op-sub">GESTOS</div>
      ${GESTURE_DIRS.map(
        ([dir, label]) => `<div class="op-row"><span>${label}</span><select data-gesture="${dir}" data-key="${key}">
          ${Object.entries(ACTION_PT).map(([id, l]) => `<option value="${id}" ${binding[dir] === id ? "selected" : ""}>${esc(l)}</option>`).join("")}
        </select></div>`,
      ).join("")}
      <button class="op-secondary" data-bind="default" data-key="${key}">Restaurar padrão</button></aside>`;
  }
  const current = typeof binding === "string" ? binding : "default";
  return `<aside class="op-panel">${close}<h2>${esc(name)}</h2><div class="op-list">${actionList(current, `data-key="${key}" data-bind`)}</div></aside>`;
}

function thumbPresets() {
  const cur = OP.remap?.thumbwheel ?? "scroll";
  const curName = typeof cur === "string" ? cur : "custom";
  return `<div class="op-sub">AÇÃO</div>${Object.entries(THUMB_PT)
    .map(([id, l]) => `<button class="op-option" data-thumb="${id}"><span>${l}</span><span class="radio ${curName === id ? "on" : ""}"></span></button>`)
    .join("")}`;
}

// ---------- Flow ----------
const SIDE_PT = { left: "à esquerda", right: "à direita", top: "acima", bottom: "abaixo" };

function flowArrangement() {
  const f = OP.flow;
  if (!f) return `<div class="spinner"></div>`;
  const placed = (side) => f.peers.find((p) => p.side === side);
  const screen = (label, cls, me) => `<div class="op-screen ${cls} ${me ? "me" : ""}">${esc(label)}</div>`;
  const cell = (side) => {
    const p = placed(side);
    return p ? screen(p.name || p.key.slice(0, 8), side) : `<div class="op-screen-slot ${side}"></div>`;
  };
  return `<div class="op-arrange">${cell("top")}<div class="op-arrange-row">${cell("left")}${screen(f.name || "Este computador", "center", true)}${cell("right")}</div>${cell("bottom")}</div>`;
}

function flowPanel() {
  const f = OP.flow;
  if (!f) return `<div class="spinner"></div>`;
  const peers = f.peers.length
    ? f.peers
        .map(
          (p) => `<div class="op-peer"><div><b>${esc(p.name || "Computador")}</b><div class="op-note">${esc(p.key.slice(0, 16))}</div></div>
          <select data-flow-side="${esc(p.key)}">
            <option value="">não posicionado</option>
            ${["left", "right", "top", "bottom"].map((sd) => `<option value="${sd}" ${p.side === sd ? "selected" : ""}>${SIDE_PT[sd]}</option>`).join("")}
          </select></div>`,
        )
        .join("")
    : `<p class="op-note">Nenhum computador pareado ainda.</p>`;
  return `<h2>Flow</h2>
    <p class="op-note">Leve o cursor até a borda da tela para passar mouse e teclado para o outro computador. A conexão é cifrada ponta a ponta e só funciona entre computadores pareados.</p>
    <div class="op-row"><span>Serviço Flow</span><span class="value">${f.running ? "ativo" : "parado"}</span></div>
    <div class="op-row"><span>Copiar e colar entre computadores</span>${flowToggle("clipboard", f.clipboard)}</div>
    <div class="op-row"><span>Teclado acompanha o mouse</span>${flowToggle("keyboardFollows", f.keyboardFollows)}</div>
    <div class="op-sub">COMPUTADORES</div>${peers}
    ${OP.pairCode ? `<div class="op-code">${OP.pairCode.slice(0, 3)} ${OP.pairCode.slice(3)}</div><p class="op-note">No outro computador rode <code>hidpilot-flow pair --to "${esc(f.name)}"</code> e digite este código. Ele vale por 2 minutos.</p>` : `<button class="op-primary" data-act="flow-pair">PAREAR COMPUTADOR</button>`}`;
}

const flowToggle = (key, on) => `<button class="toggle ${on ? "on" : ""}" data-flow-toggle="${key}"></button>`;

async function loadFlow() {
  try {
    OP.flow = await invoke("flow_get");
  } catch (e) {
    OP.flow = { name: "", peers: [], clipboard: true, keyboardFollows: true, running: false };
    toast(e);
  }
  render();
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
async function opClick(el) {
  const d = el.dataset;
  if (d.tab) {
    state.op = { tab: d.tab };
    if (d.tab === "flow") loadFlow();
    return render();
  }
  if (d.slot) {
    state.op = { ...state.op, slot: d.slot, cid: d.cid, slotName: d.slotname };
    if (!OP.remap) loadRemap();
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
    OP.flow = await invoke("flow_set", { key: d.flowToggle, value: !el.classList.contains("on"), peer: null }).catch((e) => (toast(e), OP.flow));
    return render();
  }
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
    OP.flow = await invoke("flow_set", { key: "side", value: el.value || null, peer: d.flowSide }).catch((e) => (toast(e), OP.flow));
    return render();
  }
  if (d.dpiLive !== undefined) return set("dpi", state.snap.dpiChoices[Number(el.value)]);
  if (d.threshold !== undefined) return set("smartshift.threshold", Number(el.value));
  if (d.backlight !== undefined) return set("backlight.level", Number(el.value));
  return false;
}

async function flowPair() {
  try {
    OP.pairCode = await invoke("flow_pair");
    render();
    setTimeout(() => {
      OP.pairCode = null;
      loadFlow();
    }, 120_000);
  } catch (e) {
    toast(e);
  }
}
