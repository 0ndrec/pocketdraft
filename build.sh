#!/usr/bin/env bash
# A shortcut for the one Gradle command that builds everything (the Rust core with cargo-ndk,
# its Kotlin bindings with UniFFI, the APK). Works in any POSIX shell: Linux, macOS, Git Bash.
#   ./build.sh            debug APK
#   ./build.sh release    release APK, signed with the debug key so it installs
#   ABI=x86_64 ./build.sh release    the same for the x86_64 emulator
set -euo pipefail
cd "$(dirname "$0")/android"
variant="${1:-debug}"
task="assemble$(printf '%s' "${variant:0:1}" | tr '[:lower:]' '[:upper:]')${variant:1}"
gradle=./gradlew
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) gradle=./gradlew.bat ;; esac
"$gradle" ":app:$task" "-Pabi=${ABI:-arm64-v8a}" -PdebugSigned
ls -la app/build/outputs/apk/"$variant"/
