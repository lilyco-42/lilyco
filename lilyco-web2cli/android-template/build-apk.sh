#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
SDK="${ANDROID_HOME:-$HOME/Android/Sdk}"
BT="$SDK/build-tools/36.1.0"
PLATFORM="$SDK/platforms/android-36/android.jar"
OUT="$DIR/build"
rm -rf "$OUT"
mkdir -p "$OUT/classes" "$OUT/dex"

# 1. Compile Java -> class
javac -source 11 -target 11 -cp "$PLATFORM" -d "$OUT/classes" \
  "$DIR/src/top/lain42/workbench/MainActivity.java"

# 2. Dex
"$BT/d8" --lib "$PLATFORM" --output "$OUT/dex" \
  "$OUT/classes/top/lain42/workbench/"*.class

# 3. Link manifest into base APK
"$BT/aapt2" link -o "$OUT/unsigned.apk" \
  -I "$PLATFORM" \
  --manifest "$DIR/AndroidManifest.xml"

# 4. Pack classes.dex into APK
(cd "$OUT/dex" && zip -q -u "$OUT/unsigned.apk" classes.dex)

# 5. Align
"$BT/zipalign" -f 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"

# 6. Debug keystore (create once)
KEYSTORE="$HOME/.android/debug.keystore"
mkdir -p "$(dirname "$KEYSTORE")"
if [ ! -f "$KEYSTORE" ]; then
  keytool -genkeypair -v -keystore "$KEYSTORE" -storepass android \
    -alias androiddebugkey -keypass android -keyalg RSA -keysize 2048 \
    -validity 10000 -dname "CN=Android Debug,O=Android,C=US"
fi

# 7. Sign APK
"$BT/apksigner" sign --ks "$KEYSTORE" --ks-pass pass:android \
  --key-pass pass:android --ks-key-alias androiddebugkey \
  --out "$OUT/lain42-workbench-debug.apk" "$OUT/aligned.apk"

"$BT/apksigner" verify "$OUT/lain42-workbench-debug.apk"
ls -lh "$OUT/lain42-workbench-debug.apk"
