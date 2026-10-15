#!/usr/bin/env bash
# Copies the fonts and device pictures of Logi Tune and Logi Options+ into
# ~/.local/share/hidpilot/logitech, where hidpilot-gui looks for them. They come from
# Logitech's own installers, downloaded from Logitech, and stay on this machine:
# hidpilot never ships them.
#
# Usage: fetch-logitech-assets.sh [--skip-optionsplus]   (Options+ is a ~720 MB download)
# Needs: curl, 7z (p7zip-full), node/npx (for the Electron archive), python3.
set -euo pipefail

url="https://software.vc.logitech.com/downloads/tune/LogiTuneSetup.msi"
oplus_url="https://download01.logi.com/web/ftp/pub/techsupport/optionsplus/logioptionsplus_installer_offline.exe"
# Device depots inside the Options+ installer whose pictures the app uses.
oplus_depots=(mx_master_3s mx_mechanical_mini)
skip_oplus=0
[ "${1:-}" = "--skip-optionsplus" ] && skip_oplus=1
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
rm -rf "$work/msi" "$work/files" "$work/tune.msi"

if [ "$skip_oplus" = 0 ]; then
  echo "Downloading Logi Options+ from Logitech (large)..."
  curl -fSL --progress-bar -o "$work/oplus.exe" "$oplus_url"
  mkdir -p "$work/oplus"
  7z x -y -o"$work/oplus" "$work/oplus.exe" "${oplus_depots[@]/%/.depot}" -r >/dev/null
  rm -f "$work/oplus.exe"
  for depot in "${oplus_depots[@]}"; do
    file="$(find "$work/oplus" -name "$depot.depot" | head -1)"
    [ -n "$file" ] || { echo "warning: $depot.depot not found in the installer" >&2; continue; }
    # .depot: magic(4) | u32 header length | JSON {"files": [...]} | (u32 size | bytes) per file
    python3 -I - "$file" "$dest/optionsplus/$depot" <<'PY'
import json, os, struct, sys
src, dst = sys.argv[1], os.path.abspath(sys.argv[2])
d = open(src, "rb").read()
n = struct.unpack("<I", d[4:8])[0]
off = 8 + n
for f in json.loads(d[8:off])["files"]:
    # Every entry, directories included, is preceded by its size.
    size = struct.unpack("<I", d[off:off + 4])[0]
    off += 4
    name = f["name"].replace("\\", "/")
    out = os.path.normpath(os.path.join(dst, name))
    if not out.startswith(dst + os.sep):
        raise SystemExit(f"unsafe path in depot: {name}")
    if f.get("type") == "dir" or name.endswith("/"):
        os.makedirs(out, exist_ok=True)
        continue
    os.makedirs(os.path.dirname(out), exist_ok=True)
    open(out, "wb").write(d[off:off + size])
    off += size
PY
    echo "pictures: $depot"
  done
fi

map="$(dirname "$0")/logitech-devices.map"
if [ -f "$map" ]; then
  while read -r slug file; do
    [ -z "$slug" ] || [ "${slug:0:1}" = "#" ] && continue
    [ -f "$dest/$file" ] && cp "$dest/$file" "$dest/devices/$slug.webp"
  done < "$map"
fi
echo "Done: $dest"
