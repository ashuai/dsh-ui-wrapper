#!/bin/bash
# 打包 DSH.app(macOS):编译 + 生成图标(.icns,DeepSeek 大胖鲸) + 组装 .app
set -e
cd "$(dirname "$0")"

echo "==> 编译 release"
cargo build --release

echo "==> 生成应用图标 DSH.icns(源: assets/dsh-whale.jpg)"
ICONSET="target/DSH.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for spec in "16 icon_16x16" "32 icon_16x16@2x" "32 icon_32x32" "64 icon_32x32@2x" \
            "128 icon_128x128" "256 icon_128x128@2x" "256 icon_256x256" \
            "512 icon_256x256@2x" "512 icon_512x512" "1024 icon_512x512@2x"; do
  set -- $spec
  sips -s format png -z "$1" "$1" assets/dsh-whale.jpg --out "$ICONSET/$2.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o assets/DSH.icns
rm -rf "$ICONSET"

echo "==> 生成自举页 logo(base64 内嵌,256px)"
TMPPNG=$(mktemp -t dsh-logo).png
sips -s format png -Z 256 assets/dsh-whale.jpg --out "$TMPPNG" >/dev/null
base64 -i "$TMPPNG" | tr -d '\n' > assets/boot-logo.b64
rm -f "$TMPPNG"

echo "==> 组装 target/DSH.app"
APP="target/DSH.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/DSH "$APP/Contents/MacOS/DSH"
cp assets/DSH.icns "$APP/Contents/Resources/DSH.icns"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>DSH</string>
  <key>CFBundleDisplayName</key>
  <string>DSH</string>
  <key>CFBundleIdentifier</key>
  <string>local.DSH</string>
  <key>CFBundleVersion</key>
  <string>0.1.0</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleExecutable</key>
  <string>DSH</string>
  <key>CFBundleIconFile</key>
  <string>DSH</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSAppTransportSecurity</key>
  <dict>
    <key>NSAllowsLocalNetworking</key>
    <true/>
  </dict>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.developer-tools</string>
</dict>
</plist>
PLIST

echo "==> 完成:$APP"
echo "运行: open $APP"
