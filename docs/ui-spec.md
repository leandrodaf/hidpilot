# UI spec: Logi Options+ (mouse and keyboard) and Logi Tune (headset)

Reverse engineered from the official installers so hidpilot can reproduce the look with its own code.
No Logitech asset lives in this repo: `scripts/fetch-logitech-assets.sh` downloads the installers
and copies fonts and device pictures to `~/.local/share/hidpilot/logitech` on the user's machine.

Reference screenshots (rendered from the real UI bundles with a fake backend) are kept outside the
repo in `~/dev/hidpilot-refs/ui-shots/{tune,optionsplus}/`.

## Where the assets come from

### Logi Options+

| What | Source |
|---|---|
| Installer | `https://download01.logi.com/web/ftp/pub/techsupport/optionsplus/logioptionsplus_installer_offline.exe` (about 724 MB). The online installer `logioptionsplus_installer.exe` (50 MB) downloads the same depots later, so use the offline one. |
| Outer archive | The `.exe` opens with `7z`. It holds 223 `*.depot` files at its root. |
| `.depot` format | `magic(4) | u32 LE header_len | JSON {"files":[{"name","mode"}]} | for each file: u32 LE size + bytes`. ~/dev/hidpilot-refs/tools/undepot.py unpacks one. |
| UI (Electron) | `logioptionsplus.depot` → `resources/app.asar` → extract with `npx @electron/asar extract`. `app.min.css` has the tokens and fonts; `app.min.js` is obfuscated (javascript-obfuscator). |
| Strings | `logioptionsplus.depot` → `data/strings/<lang>.yaml` (`pt-BR.yaml`, `en-US.yaml`, ...), 4808 keys, plain YAML. |
| Fonts | `app.asar` → `assets/*.woff2`, names from `@font-face` in `app.min.css`: BrownPro-Light `c941ba321f86c9065492`, BrownPro-Regular `8c6a18fd7d9468524945`, BrownPro-Bold `669ffb72bcc1521bf72c`. |
| MX Master 3S | `mx_master_3s.depot`: `front_core.webp` (691×1024), `side_core.webp` (636×1024), `back_core.webp` (692×1024), `core_metadata.json` (hotspots), `manifest.json`, `OnboardData.json`, `default_configurations.json`. Color variants: `*_ext_<n>.webp` (`ext1` pale grey, `ext2` ...), picked by model id `2b034_ext<n>`. |
| MX Mechanical Mini | `mx_mechanical_mini.depot` (Windows layout; `mx_mechanical_mini_mac.depot` for Mac legends): `front.webp` (1889×800, used for every tab), `metadata.json` (hotspots), `easy_switch_indicator_{1,2,3}[_dark].webp`, `fn_key_f5[_dark].webp`, `fn_key_backlight[_dark].webp`, backlight effect previews `static.webp`, `breathing.webm`, `contrast.webm`, `random.webm`, `reaction.webm`, `waves.webm`. |

`manifest.json` maps resource keys to files for each model id, which is how the app picks the picture per tab:

| key | MX Master 3S | MX Mechanical Mini |
|---|---|---|
| `device_image` (home card) | `front_core.webp` | `front.webp` |
| `device_buttons_image` / `device_keys_image` | `side_core.webp` | `front.webp` |
| `device_point_scroll_image` | `side_core.webp` | – |
| `device_easyswitch_image` | `back_core.webp` | `front.webp` |
| `device_Backlight_effect_image` | – | `front.webp` |
| `image_metadata` | `core_metadata.json` | `metadata.json` |

### Logi Tune

Installer `https://software.vc.logitech.com/downloads/tune/LogiTuneSetup.msi` → cabs → Electron asar → `assets/`.
Zone Vibe Wireless pictures (256 px tall, by `primaryColorCode`): `1dec034cbe8aa1aea561.webp` graphite (0),
`cdd6c5422b29c81af28f.webp` off-white (256), `768c2b86541486d18978.webp` rose (512). Fonts: BrownPan-{Light,Regular,Bold}.otf.

## Options+ design tokens

Fonts: `BrownPro-Regular` for body text, `BrownPro-Bold` for titles and buttons, `BrownPro-Light` as an accent. Titles and
buttons are uppercase with letter spacing (e.g. "ADICIONAR DISPOSITIVO", tab labels).
The theme is a class on `<body>` (`body.light` / `body.dark`). In the dark theme the accent switches from purple to **teal**.

```css
body.light {
  --primary: #814efa;            --primary-hover: #673ec8;      --primary-active-hover: #6039b2;
  --primary-light: #d3c3fe;      --primary-light-hover: #b18dff; --primary-dark: #170e2e;
  --primary-200: #cdb8fd; --primary-300: #b395fc; --primary-400: #9a71fb; --primary-500: #814efa;
  --primary-transparent: rgba(129,78,250,.1);
  --primary-transparent-darker: rgba(129,78,250,.3);
  --primary-transparent-shade-1: rgba(129,78,250,.4);
  --primary-hotspot-hover: #f2edfe;
  --primary-header-icon-hover: #f9f6ff; --primary-header-icon-hover-border: #e6dcfe;
  --primary-background-1: #fff;  --primary-background-2: #000;  --primary-background-3: #f9f8ff;
  --primary-background-4: #f5f5f5; --primary-background-5: #d9d9d9; --primary-background-6: #fbfbfb;
  --secondary-100: #fbfbfb; --secondary-200: #f0f0f0; --secondary-400: #d9d9d9;
  --text: #222425;               /* most frequent text color in the stylesheet */
  --error: #ff2947; --error-hover: #dc1c37;
  --battery-full: #79e053; --battery-warning: #ffa414; --battery-empty: #f43d3d;
  --battery-border-color: #f0f0f0; --battery-de-active-color: #fbfbfb;
  --overlay-light: rgba(0,0,0,.1); --overlay-medium: rgba(0,0,0,.2); --overlay-dark: rgba(0,0,0,.4);
  --body-background: #fff;       /* main window shows #fbfbfb behind the content */
  --splash-background: #814efa;
}
body.dark {
  --primary: #00ead0;            --primary-hover: #03dbc3;      --primary-active-hover: #03baa5;
  --primary-light: #99f7ec;      --primary-light-hover: #66f2e3; --primary-dark: #112c36;
  --primary-transparent: rgba(0,234,208,.1);
  --primary-transparent-darker: rgba(0,234,208,.3);
  --primary-transparent-shade-1: rgba(0,234,208,.4);
  --primary-hotspot-hover: #f2fefd;
  --primary-background-1: #000;  --primary-background-2: #fff;  --primary-background-3: rgba(255,255,255,.2);
  --primary-background-4: #060606; --primary-background-5: #5c5c5c; --primary-background-6: #000;
  --secondary-100: #191919; --secondary-200: #333; --secondary-400: #666;
  --error: #ff2947;
  --battery-full: #79e053; --battery-warning: #ffa414; --battery-empty: #f43d3d;
  --battery-border-color: #333; --battery-de-active-color: #191919;
  --overlay-dark: rgba(0,0,0,.8);
  --body-background: #000;
  --splash-background: #814efa;
}
```

## Options+ screens

Main window: 1100×700 default, minimum 960×544 (from `main.js`).

### Splash

Full window `--splash-background`, white spinner, then a checklist (✓ green done, ○ pending):
"Inicializando aplicativo", "Carregando configuração", "Lendo permissões", "Obtendo recursos".
Screenshot: the Tune splash looks the same.

### Home (`01-home.png`)

- Header row, 40 px side padding, ~76 px from the top: greeting on the left ("Boa noite" / "Bom dia" / "Boa tarde",
  BrownPro-Bold ~28 px, `--text`); on the right "+ ADICIONAR DISPOSITIVO" (BrownPro-Bold 13 px uppercase),
  two thin 1 px vertical dividers, then account and settings icons (24 px outline icons).
- Device carousel centered vertically: one card per device, no visible card background, just the product picture
  (`device_image`) at its natural aspect (mouse ~180 px tall, keyboard ~265×110 px at 1100 px width), side by side
  with large gaps. Under each picture a white pill (rounded 16 px, light shadow) with the device status: a battery
  icon + percentage when ready, or a spinner + "CONFIGURAÇÃO" while resources load.
- Arrows `#page-back` / `#page-next` appear when devices overflow.
- Empty state: illustration plus "CONECTE SEUS DISPOSITIVOS" (BrownPro-Regular uppercase, grey).
- Clicking a picture opens the device page.

### Device page (common frame)

- Left: back arrow and device name with battery; the product picture large in the center, with **hotspots** on it.
- Tabs along the top of the picture area (uppercase BrownPro-Bold, active tab in `--primary` with an underline).
  - Mouse: "BOTÕES", "PONTEIRO E ROLAGEM", "EASY-SWITCH", "FLOW", plus "SMART ACTIONS".
  - Keyboard: "TECLAS", "ILUMINAÇÃO" (backlight), "EASY-SWITCH", plus "SMART ACTIONS".
- Clicking a hotspot opens a right-side panel with the options for that slot (action picker for buttons,
  sliders/toggles for pointer and scroll settings).

### Hotspots

Positions come from the device metadata JSON (`core_metadata.json` / `metadata.json`), one block per image key:
`marker.{x,y}` are **percent of the image box** (they can go below 0 or above 100 for gesture arrows outside the
mouse); `label.{x,y}` is the direction the label is drawn in (-1 left/up, 0 centered, 1 right/down).
`origin` is the size the percentages were authored against.

Mouse hotspot (`.HotSpot`): 16 px circle (14/16/18/20 px by breakpoint), `border: 2px solid var(--primary-background-1)`,
transparent fill, `filter: drop-shadow(0 4px 8px var(--overlay-medium))`. Hover: border `#b18dff`. Active:
fill `--primary-transparent-darker`, border `--primary`, shadow in `--primary-transparent-shade-1`.
Keyboard hotspot (`.HotSpotSquare`): a rounded rectangle (radius 4 px; corner keys get one larger radius) over the
whole key, same border and states as above; `styles.{width,height}` in the metadata give the key size in origin pixels.

#### MX Master 3S (model `2b034`, origin 636×1024 for side/front views)

`device_buttons_image` (side_core.webp), tab Buttons:

| slot | id | x % | y % | label |
|---|---|---|---|---|
| Botão médio (wheel click) | `_c82` | 71 | 15 | right |
| Botão de alternância de modos (ratchet toggle, top) | `_c196` | 81 | 34 | right |
| Roda para o polegar | `_thumb_wheel_adapter` | 55 | 51.5 | right |
| Botão Avançar | `_c86` | 35 | 43 | left |
| Botão Voltar | `_c83` | 45 | 60 | below |
| Botão de gesto | `_c195` | 8 | 58 | left |

`device_gesture_buttons_image` (gesture button sub-view): left −40/30, right 130/30, up 45/−14, down 45/72, click 15/40.

`device_point_scroll_image` (side_core.webp), tab Point & scroll:

| slot | x % | y % | label |
|---|---|---|---|
| Velocidade do ponteiro (`_mouse_settings`) | 83 | 54 | right |
| Roda de rolagem (`_mouse_scroll_wheel_settings`) | 73 | 16 | right |
| Roda para o polegar (`_mouse_thumb_wheel_settings`) | 55 | 51 | left |

`device_easyswitch_image` (back_core.webp; origin 1872×728 in the metadata): channel 1 at 21.5/65.5,
channel 2 at 25.5/63.7, channel 3 at 29.5/65.4, host change button at 24.5/69.4 (labels up-left).

#### MX Mechanical Mini (model `2b367`, front.webp, origin 1872×728)

`device_keys_image`, tab Keys (each key 90×126 origin px, labels up-left):

| key | id | x % | y % |
|---|---|---|---|
| Diminuir luz de fundo | `_c226` | 25.9 | 11 |
| Aumentar luz de fundo | `_c227` | 32.12 | 11 |
| Ditado | `_c259` | 38.25 | 11 |
| Emoji | `_c264` | 44.4 | 11 |
| Captura de tela | `_c266` | 50.5 | 11 |
| Mudo do microfone | `_c284` | 56.78 | 11 |
| Pesquisa | `_c212` | 62.9 | 11 |
| Reproduzir/pausar | `_c321` | 69.13 | 11 |
| Mudo | `_c231` | 75.2 | 11 |
| Diminuir volume | `_c232` | 81.4 | 11 |
| Aumentar volume | `_c233` | 87.35 | 11 |
| Home | `_c280` | 93.6 | 25.3 |
| End | `_c281` | 93.6 | 39.7 |
| Page Up | `_c282` | 93.6 | 54 |
| Page Down | `_c283` | 93.6 | 68.7 |

`device_easyswitch_image`: Easy-Switch 1 `_c209` 7.6/11, 2 `_c210` 13.8/11, 3 `_c211` 19.8/11; the
`easy_switch_indicator_<n>.webp` overlay lights the active channel's LED.

`device_Backlight_effect_image`, tab Backlight: backlight-down key `_c226` at 81.6/11 and Fn key `_c52` at 69.2/83.6
(highlight only, no click, 85×120 / 86×120); label-only hotspots "Configurações de iluminação" at 31.4/13 and
"Efeitos de iluminação" at 62.7/11. Effects: static, breathing, contrast, random, reaction, waves (preview videos
in the depot).

## Logi Tune (headset) tokens, for reference

Already implemented in `gui/ui/styles.css`. Window 420×775 (min 380×680, max 460×1000). Fonts BrownPan.
Light: primary `#814efa`, page `#f4f4f4`, cards `#fff` radius 4.8 px, rows 52 px with 12 px padding and
1 px `rgba(0,0,0,.1)` separators, title text `#2f3132`, values in primary, battery `#55d464` / `#ffa414`.
Dark: page `#282828`, cards `#373a3b`, primary text `#fbfbfb`, buttons `#8574d4`. Switch 28×16 with a 12 px thumb.
Screenshots of every headset screen (light and dark) are in `ui-shots/tune/`.

## Gaps

- The Options+ device pages (tabs, side panels, sliders) were not rendered: the obfuscated app needs the agent's
  per-device resource and settings messages, which were not emulated. Layout above comes from the CSS, the strings
  and the metadata; exact panel measurements still need a capture from a real Options+ (Windows/macOS) or more
  backend emulation (`~/dev/hidpilot-refs/ui-render/oplus`, `__boot` in `agent.js`).
