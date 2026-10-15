#!/usr/bin/env bash
# Copies the fonts and device pictures of Logi Tune into ~/.local/share/hidpilot/logitech,
# where hidpilot-gui looks for them. They come from Logitech's own installer, downloaded
# from Logitech, and stay on this machine: hidpilot never ships them.
#
# Needs: curl, 7z (p7zip-full), node/npx (for the Electron archive).
set -euo pipefail

url="https://software.vc.logitech.com/downloads/tune/LogiTuneSetup.msi"
dest="${XDG_DATA_HOME:-$HOME/.local/share}/hidpilot/logitech"

for tool in curl 7z npx python3; do
  command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 1; }
done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "Downloading Logi Tune from Logitech..."
curl -fSL --progress-bar -o "$work/tune.msi" "$url"

echo "Unpacking..."
mkdir -p "$work/msi" "$work/files"
7z x -y -o"$work/msi" "$work/tune.msi" >/dev/null
for cab in "$work"/msi/*.cab; do 7z x -y -o"$work/files" "$cab" >/dev/null; done

asar=""
for f in "$work"/files/*; do
  if head -c 16 "$f" | od -An -tx1 | grep -q '^ 04 00 00 00'; then
    if grep -qa '"main.bundle.js"' "$f" 2>/dev/null; then asar="$f"; break; fi
  fi
done
[ -n "$asar" ] || { echo "could not find the Logi Tune UI archive in the installer" >&2; exit 1; }
npx -y @electron/asar@3.2.17 extract "$asar" "$work/ui" >/dev/null

mkdir -p "$dest/fonts" "$dest/devices" "$dest/tune"
# Fonts: map the hashed file names back to their family names through the stylesheet.
python3 -I - "$work/ui" "$dest/fonts" <<'PY'
import re, shutil, sys, pathlib
ui, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
css = (ui / "app.min.css").read_text(errors="replace")
for fam, src in re.findall(r"@font-face\{font-family:(BrownPan-[A-Za-z]+);[^}]*?src:url\((assets/[0-9a-f]+\.otf)\)", css):
    shutil.copy(ui / src, out / f"{fam}.otf")
    print("font", fam)
PY
# Everything else stays under tune/ with its original name, for the device pictures map.
cp -r "$work/ui/assets/." "$dest/tune/"
if [ -f "$(dirname "$0")/logitech-devices.map" ]; then
  while read -r slug file; do
    [ -z "$slug" ] || [ "${slug:0:1}" = "#" ] && continue
    [ -f "$dest/tune/$file" ] && cp "$dest/tune/$file" "$dest/devices/$slug.webp"
  done < "$(dirname "$0")/logitech-devices.map"
fi
echo "Done: $dest"
