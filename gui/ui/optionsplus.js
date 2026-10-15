// Logi Options+ style screens for mice and keyboards: home carousel, device page with
// tabs, hotspots placed from Logitech's own metadata, and a side panel per hotspot.
// Shares `state`, `invoke`, `esc`, `set`, `toast`, `render` with app.js.

const OP = {
  meta: {}, // depot name -> parsed metadata json
  remap: null,
  flow: null,
  pairCode: null,
  layout: null,
  services: {}, // service name -> {active, installed}
};

// Page errors go to ~/.local/state/hidpilot/gui.log.
window.addEventListener("error", (e) => invoke("log_js", { msg: `${e.message} @ ${e.filename}:${e.lineno}` }).catch(() => {}));
window.addEventListener("unhandledrejection", (e) => invoke("log_js", { msg: `unhandled: ${e.reason}` }).catch(() => {}));

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
  SLOT_NAME_MUTE_UNMUTE_AUDIO: "Mudo do microfone",
  SLOT_NAME_EASYSWITCH_1: "Canal 1",
  SLOT_NAME_EASYSWITCH_2: "Canal 2",
  SLOT_NAME_EASYSWITCH_3: "Canal 3",
  SLOT_NAME_EASYSWITCH_HOST_CHANGE_BUTTON: "Botão Easy-Switch",
  SLOT_NAME_POINTER_SETTINGS: "Velocidade do ponteiro",
  SLOT_NAME_SCROLL_WHEEL_SETTINGS: "Roda de rolagem",
  SLOT_NAME_THUMBWHEEL_SETTINGS: "Roda para o polegar",
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
        <button class="op-icon" data-act="settings" title="Configurações">⚙</button>
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
  ["layout", "LAYOUT"],
  ["backlight", "ILUMINAÇÃO"],
  ["easyswitch", "EASY-SWITCH"],
  ["flow", "FLOW"],
];
const HEADSET_TABS = [
  ["audio", "ÁUDIO"],
  ["settings", "CONFIGURAÇÕES"],
  ["about", "SOBRE"],
];

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
      <nav class="op-tabs">${tabs.map(([k, l]) => `<button class="op-tab ${k === tab ? "active" : ""}" data-tab="${k}">${l}</button>`).join("")}</nav>
    </header>
    <div class="op-body with-panel">${stage}<aside class="op-panel">${panel}</aside></div>
  </div>`;
}

function opDevice(s) {
  if (s.kind === "headset") return opHeadset(s);
  const tabs = s.kind === "keyboard" ? KEYBOARD_TABS : MOUSE_TABS;
  const tab = state.op?.tab || tabs[0][0];
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
  if (binding == null || binding === "default") return "Função original";
  if (typeof binding === "object") return "Gestos";
  return ACTION_PT[binding] || binding;
}

function actionList(current, key) {
  return Object.entries(ACTION_PT)
    .map(
      ([id, label]) =>
        `<button class="op-option" data-key="${key}" data-bind="${esc(id)}"><span>${esc(label)}</span><span class="radio ${current === id ? "on" : ""}"></span></button>`,
    )
    .join("");
}

function serviceBanner(name, what) {
  const st = OP.services[name];
  if (!st || st.active) return "";
  return `<div class="op-banner"><div><b>${what} desligado</b><br>As escolhas abaixo ficam salvas, mas só funcionam com o serviço ativo.</div>
    <button class="op-primary small" data-service="${name}">ATIVAR</button></div>`;
}

function opPanel(s, tab) {
  if (tab === "flow") return serviceBanner("hidpilot-flow", "Flow") + flowPanel();
  if (tab === "easyswitch") return easySwitchPanel(s);
  if (tab === "layout") return layoutPanel(s);
  if (tab === "backlight") return backlightPanel(s);
  if (tab === "point") return pointPanel(s);
  return controlsPanel(s, tab);
}

function easySwitchPanel(s) {
  const h = s.hosts;
  if (!h) return `<h2>Easy-Switch</h2><p class="op-note">Este dispositivo não informa os canais.</p>`;
  return `<h2>Easy-Switch</h2>
    <p class="op-note">Cada canal guarda um computador. Escolha um canal para levar o ${s.kind === "keyboard" ? "teclado" : "mouse"} até ele agora.</p>
    ${Array.from({ length: h.count }, (_, i) => {
      const n = h.names?.[i]?.[1] || `Canal ${i + 1}`;
      const here = i === h.current;
      return `<button class="op-channel ${here ? "current" : ""}" ${here ? "" : `data-host="${i}"`}>
        <span class="op-channel-n">${i + 1}</span>
        <span class="op-channel-name">${esc(n)}<small>${here ? "Este computador" : "Toque para trocar"}</small></span>
      </button>`;
    }).join("")}
    <p class="op-note">O botão Easy-Switch embaixo do ${s.kind === "keyboard" ? "teclado (teclas 1, 2 e 3)" : "mouse"} faz o mesmo.</p>`;
}

function layoutPanel(s) {
  const L = OP.layout;
  const typing = !L
    ? `<div class="spinner"></div>`
    : L.options
        .map(
          (o) => `<button class="op-option tall" data-layout="${esc(o.id)}"><span><b>${esc(o.name)}</b><small>${esc(o.hint)}</small></span><span class="radio ${o.id === L.current ? "on" : ""}"></span></button>`,
        )
        .join("");
  const p = s.platform;
  const OS_PT = { linux: "Linux", windows: "Windows", macos: "macOS", ios: "iOS", android: "Android", chrome: "ChromeOS" };
  const keysOs = p
    ? `<div class="op-sub">TECLAS ESPECIAIS DO TECLADO</div>
      <p class="op-note">Define como o teclado manda ⌘/Alt e as teclas de função (o mesmo que Fn+O / Fn+P).</p>
      ${p.options
        .map(([id, names]) => {
          const label = names.map((n) => OS_PT[n] || n).join(", ");
          return `<button class="op-option" data-platform="${esc(names[0])}"><span>${esc(label)}</span><span class="radio ${id === p.current ? "on" : ""}"></span></button>`;
        })
        .join("")}`
    : "";
  return `<h2>Layout de digitação</h2>
    <p class="op-note">Como o computador entende o que você digita. Vale para todo o sistema e muda na hora.</p>
    ${typing}
    ${L?.smartAccents ? `<p class="op-note">✓ Acentos inteligentes do hidpilot ativos: ´+c = ç, e aspas e apóstrofos saem normais fora das vogais.</p>` : ""}
    ${keysOs}`;
}

function backlightPanel(s) {
  const b = s.backlight;
  if (!b) return `<h2>Iluminação</h2><p class="op-note">Este teclado não tem iluminação.</p>`;
  return `<h2>Iluminação</h2>
    <div class="op-row"><span>Luz de fundo</span>${toggle("backlight.mode", b.mode !== "off")}</div>
    ${b.auto ? `<div class="op-row"><span>Ajustar com a luz ambiente</span>${toggle("backlight.auto", b.mode === "auto")}</div>` : ""}
    ${b.manual && b.levels > 1 ? `<div class="op-sub">NÍVEL</div><input class="line" type="range" min="0" max="${b.levels - 1}" value="${b.level}" data-backlight style="--fill:${(b.level / (b.levels - 1)) * 100}%">` : ""}
    <p class="op-note">As teclas de luz do teclado continuam ajustando o brilho.</p>`;
}

function pointPanel(s) {
  const i = Math.max(0, (s.dpiChoices || []).indexOf(s.dpi));
  const n = Math.max(1, (s.dpiChoices || [1]).length - 1);
  const ss = s.smartshift;
  return `<h2>Ponteiro e rolagem</h2>
    ${s.dpiChoices ? `<div class="op-sub">VELOCIDADE DO PONTEIRO <b>${s.dpi ?? "—"} DPI</b></div>
    <input class="line" type="range" min="0" max="${n}" value="${i}" data-dpi-live style="--fill:${(i / n) * 100}%">` : ""}
    ${ss ? `<div class="op-sub">RODA DE ROLAGEM</div>
    <div class="op-row"><span>SmartShift (giro livre automático)</span>${toggle("smartshift.ratchet", !ss.ratchet)}</div>
    <div class="op-sub">SENSIBILIDADE DO SMARTSHIFT <b>${ss.threshold >= 50 ? "máx." : ss.threshold}</b></div>
    <input class="line" type="range" min="1" max="50" value="${Math.min(50, ss.threshold)}" data-threshold style="--fill:${(Math.min(50, ss.threshold) / 50) * 100}%">` : ""}
    ${s.wheel ? `<div class="op-row"><span>Rolagem suave</span>${toggle("wheel.hires", s.wheel.hires)}</div>
    <div class="op-row"><span>Direção de rolagem natural</span>${toggle("wheel.invert", s.wheel.invert)}</div>` : ""}
    ${s.thumbwheelInvert != null ? `<div class="op-sub">RODA PARA O POLEGAR</div>
    <div class="op-row"><span>Inverter direção</span>${toggle("thumbwheelInvert", s.thumbwheelInvert)}</div>` : ""}`;
}

// Buttons (mouse) and keys (keyboard): a list when nothing is picked, actions otherwise.
function controlsPanel(s, tab) {
  const r = OP.remap;
  const what = s.kind === "keyboard" ? "Teclas" : "Botões";
  const banner = serviceBanner("hidpilot-remap", "Serviço de botões e teclas");
  if (!r) return `<h2>${what}</h2><div class="spinner"></div>`;
  const sl = slots(s, tab);
  const slot = state.op?.slot;
  if (!slot) {
    const items = sl
      .map((a) => {
        const key = a.cid.startsWith("c") ? cidHex(a.cid.slice(1)) : null;
        const b = key ? r.bindings[key] : null;
        const thumb = /thumb_wheel/.test(a.slotId);
        const value = thumb ? THUMB_PT[typeof r.thumbwheel === "string" ? r.thumbwheel : "scroll"] : actionLabel(b);
        return `<button class="op-option" data-slot="${esc(a.slotId)}" data-cid="${esc(a.cid)}" data-slotname="${esc(a.slotName || "")}"><span>${esc(slotLabel(a.slotName))}</span><span class="value">${esc(value)} ›</span></button>`;
      })
      .join("");
    return `${banner}<h2>${what}</h2><p class="op-note">Escolha um ${s.kind === "keyboard" ? "tecla" : "botão"} na imagem ou na lista.</p>${items || `<p class="op-note">Sem dados de hotspots: rode scripts/fetch-logitech-assets.sh.</p>`}`;
  }
  const back = `<button class="op-secondary" data-act="panel-close">← ${what}</button>`;
  const name = slotLabel(state.op.slotName);
  if (/thumb_wheel/.test(slot)) return `${banner}${back}<h2>${esc(name)}</h2>${thumbPresets()}`;
  const key = cidHex(String(state.op.cid).replace(/^c/, ""));
  if (!r.buttons.some((b) => b.cid === key)) {
    return `${back}<h2>${esc(name)}</h2><p class="op-note">Este controle não pode ser personalizado neste dispositivo.</p>`;
  }
  const binding = r.bindings[key];
  if (binding && typeof binding === "object") {
    return `${banner}${back}<h2>${esc(name)}</h2><div class="op-sub">GESTOS (SEGURE E MOVA O MOUSE)</div>
      ${GESTURE_DIRS.map(
        ([dir, label]) => `<div class="op-row"><span>${label}</span><select data-gesture="${dir}" data-key="${key}">
          ${Object.entries(ACTION_PT).map(([id, l]) => `<option value="${id}" ${binding[dir] === id ? "selected" : ""}>${esc(l)}</option>`).join("")}
        </select></div>`,
      ).join("")}
      <button class="op-secondary" data-bind="default" data-key="${key}">Restaurar função original</button>`;
  }
  const current = typeof binding === "string" ? binding : "default";
  return `${banner}${back}<h2>${esc(name)}</h2><div class="op-list">${actionList(current, key)}</div>`;
}

function thumbPresets() {
  const cur = OP.remap?.thumbwheel ?? "scroll";
  const curName = typeof cur === "string" ? cur : "custom";
  return `<div class="op-sub">AÇÃO</div>${Object.entries(THUMB_PT)
    .map(([id, l]) => `<button class="op-option" data-thumb="${id}"><span>${l}</span><span class="radio ${curName === id ? "on" : ""}"></span></button>`)
    .join("")}`;
}

// ---------- headset, in the same frame as the other devices ----------
function opHeadset(s) {
  const tab = state.op?.tab || "audio";
  const stage = `<div class="op-stage"><div class="op-headset-hero">${picture("headset", s.name)}</div></div>`;
  let panel = "";
  if (tab === "audio") {
    const eq = s.equalizer || { bands: [0, 0, 0, 0, 0], preset: "Default" };
    const pct = s.sidetone == null ? 0 : Math.round((s.sidetone / s.sidetoneMax) * 100);
    panel = `<h2>Equalizador</h2>
      <div class="eq-box op-eq"><div class="eq">${eqCurve(eq.bands)}${eq.bands
        .map(
          (v, i) => `<div class="band" data-band="${i}" role="slider" aria-label="Banda ${i + 1}" aria-valuenow="${v}">
            <div class="rail"></div><div class="fill" style="height:${eqFrac(v) * 100}%"></div><div class="knob" style="bottom:${eqFrac(v) * 100}%"></div></div>`,
        )
        .join("")}</div>
        <div class="eq-labels"><span>GRAVES</span><span>MÉDIOS</span><span>AGUDOS</span></div></div>
      <div class="op-presets">${[...s.presets.map((p) => p.name), "Custom"]
        .map((n) => `<button class="op-chip ${n === eq.preset ? "on" : ""}" ${n === "Custom" ? "disabled" : `data-preset="${esc(n)}"`}>${n === "Custom" ? "Personalizado" : esc(n)}</button>`)
        .join("")}</div>
      <div class="op-sub">SIDETONE (SUA VOZ NO FONE) <b>${pct}%</b></div>
      <input class="line" type="range" min="0" max="${s.sidetoneMax}" value="${s.sidetone ?? 0}" data-sidetone style="--fill:${pct}%">
      <div class="op-sub">NÍVEL DO MICROFONE <b>${s.micLevel ?? "—"}%</b></div>
      <input class="line" type="range" min="0" max="100" step="5" value="${s.micLevel ?? 0}" data-mic style="--fill:${s.micLevel ?? 0}%">
      <div class="op-row"><span>Microfone silenciado</span>${toggle("micMuted", s.micMuted)}</div>`;
  } else if (tab === "settings") {
    panel = `<h2>Configurações</h2>
      <div class="op-sub">NOME DO DISPOSITIVO</div>
      <div class="op-inline"><input class="text-input" maxlength="30" value="${esc(s.name)}" data-rename><button class="op-primary small" data-act="rename">SALVAR</button></div>
      <p class="op-note">É o nome que celulares e computadores veem no Bluetooth (até 30 caracteres, sem acentos).</p>
      <div class="op-row"><span>Girar para silenciar</span>${toggle("rotateToMute", s.rotateToMute)}</div>
      <div class="op-row"><span>Notificações por voz</span>${toggle("voicePrompts", s.voicePrompts)}</div>
      <div class="op-row"><span>Desligamento automático</span><select data-autooff>${[5, 10, 15, 30, 60, 120, 240]
        .map((m) => `<option value="${m}" ${m === s.autoSleep ? "selected" : ""}>${minutesText(m)}</option>`)
        .join("")}</select></div>
      <div class="op-row"><span>Bluetooth conectado</span><span class="value">${s.bluetoothConnections == null ? "—" : s.bluetoothConnections === 1 ? "1 aparelho" : `${s.bluetoothConnections} aparelhos`}</span></div>`;
  } else {
    panel = `<h2>Sobre</h2>
      <div class="op-row"><span>Modelo</span><span class="value">Zone Vibe Wireless</span></div>
      <div class="op-row"><span>Número de série</span><span class="value">${esc(s.serial || "—")}</span></div>
      <div class="op-row"><span>Bateria</span>${batteryHtml(s.battery)}</div>
      <button class="op-primary" data-act="diag">EXECUTAR DIAGNÓSTICO</button>
      ${(state.diag || []).map((r) => `<div class="op-row"><span>${esc(r.name)}</span><span class="value">${r.ok ? "✓ OK" : "✕ " + esc(r.detail)}</span></div>`).join("")}`;
  }
  return opFrame(s, HEADSET_TABS, tab, stage, panel);
}
// ---------- app settings ----------
function opSettings() {
  const st = OP.settings;
  const body = !st
    ? `<div class="spinner"></div>`
    : `<div class="op-row"><span><b>Iniciar com o computador</b><small class="op-small">O hidpilot sobe ao entrar na sessão e fica na bandeja.</small></span>${settingToggle("autostart", st.autostart)}</div>
       <div class="op-row ${st.autostart ? "" : "disabled"}"><span><b>Abrir a janela ao iniciar</b><small class="op-small">Desligado: ao entrar na sessão o hidpilot fica só na bandeja.</small></span>${settingToggle("openOnLogin", st.openOnLogin)}</div>
       <p class="op-note">Fechar ou minimizar a janela sempre deixa o hidpilot na bandeja; para sair de vez use "Sair" no menu do ícone.</p>`;
  return `<div class="op">
    <header class="op-dev-header"><button class="op-back" data-act="home">←</button><div><div class="op-dev-name">Configurações</div></div></header>
    <div class="op-settings">${body}</div>
  </div>`;
}
const settingToggle = (key, on) => `<button class="toggle ${on ? "on" : ""}" data-setting="${key}"></button>`;

async function loadSettings() {
  OP.settings = await invoke("settings_get").catch(() => null);
  render();
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
    if (d.service === "hidpilot-flow") loadFlow();
    return render();
  }
  if (d.layout) {
    OP.layout = await invoke("layout_set", { id: d.layout }).catch((e) => (toast(e), OP.layout));
    toast("Layout aplicado");
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
  if (d.sidetone !== undefined) return set("sidetone", Number(el.value));
  if (d.mic !== undefined) return set("micLevel", Number(el.value));
  if (d.autooff !== undefined) return set("autoSleep", Number(el.value));
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
