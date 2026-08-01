# Installing wltimer on an Android phone

wltimer is not on any store — you build an APK and sideload it. Two ways to get
it onto the phone: over USB with `adb`, or by copying the file to the phone and
tapping it. Both install the same APK.

The app appears on the launcher as **Weightlifting Timer**; its package id is
`com.pcorreia.wltimer`.

## 1. Build the APK

Needs the toolchain described in the [README](README.md#build-for-android)
(JDK 17, Android SDK + NDK, Rust target `aarch64-linux-android`):

```sh
export JAVA_HOME=~/Android/jdk-17.0.20+8
export ANDROID_HOME=~/Android/sdk
export NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
npm install
npm run android:apk
```

The result is a signed release APK, written twice into
`src-tauri/gen/android/app/build/outputs/apk/universal/release/`:

- `app-universal-release.apk` — Gradle's own output
- `wltimer-<version>.apk` — a copy named after the version in `package.json`

Use the versioned copy when you're going to send the file somewhere; some
browsers and cloud apps serve a cached file for a name they've already seen.

Signing uses the keystore at `~/Android/wltimer-release.jks` with the
credentials in `src-tauri/gen/android/keystore.properties` (gitignored). Keep
that keystore — Android only lets an installed app be upgraded by an APK signed
with the same key. See [Reinstalling and
upgrades](#reinstalling-and-upgrades).

## 2a. Install over USB (`adb`)

On the phone, once:

1. **Settings → About phone**, tap **Build number** seven times to unlock
   developer options.
2. **Settings → System → Developer options → USB debugging**, turn it on.
3. Plug the phone into the computer. A dialog asks whether to *Allow USB
   debugging* — accept it (tick "always allow from this computer").

Then:

```sh
adb devices                 # phone should be listed as "device", not "unauthorized"
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/release/wltimer-0.0.1.apk
```

`-r` reinstalls over an existing copy, keeping its data.

### Over Wi-Fi instead of a cable

Android 11+: **Developer options → Wireless debugging → Pair device with
pairing code**, then on the computer:

```sh
adb pair <phone-ip>:<pairing-port>     # enter the 6-digit code shown on screen
adb connect <phone-ip>:<debug-port>    # the port on the Wireless debugging screen
adb install -r .../wltimer-0.0.1.apk
```

The pairing port and the connect port are different numbers; both are shown on
the phone.

## 2b. Install without a computer

Copy `wltimer-<version>.apk` to the phone by whatever route you like — USB file
transfer (MTP), a cloud drive, self-messaging on Signal/Telegram, `adb push`,
or a local HTTP server on the same Wi-Fi:

```sh
cd src-tauri/gen/android/app/build/outputs/apk/universal/release
python3 -m http.server 8000     # then open http://<computer-ip>:8000 on the phone
```

Then open the file on the phone (Files app → Downloads → tap the APK). Android
will ask to allow installing unknown apps **for the app doing the opening**
(the browser or file manager) — allow it, then confirm the install. On Android
8+ this permission is per-app, so it may ask again if you install from a
different app next time.

Vendor extras: Xiaomi/MIUI hides the toggle under **Settings → Privacy
protection → Special permissions → Install unknown apps**, and may additionally
require turning off **MIUI Optimization**. Samsung and Huawei phones show a
similar one-off prompt.

## Reinstalling and upgrades

- Rebuilding with the same keystore: `adb install -r <apk>` (or tapping the
  file) upgrades in place and **keeps your workouts and calendar**.
- Signed with a different key — a fresh keystore, or a debug build over a
  release one — the install fails with `INSTALL_FAILED_UPDATE_INCOMPATIBLE`.
  The only fix is `adb uninstall com.pcorreia.wltimer` first, **which deletes
  the app's data**. Export anything you care about first (a workout's Copy
  button puts its markdown on the clipboard).
- The version in `package.json` is only used for the APK filename; it does not
  gate upgrades.

## Where your data lives

Everything is in the app's private data directory on the phone
(`workouts/*.md.zst`, `days/*.json.zst` — see
[Storage](README.md#storage)). It is not visible to other apps and is not
backed up anywhere by this app. Uninstalling erases it; "Clear storage" in
Android's app settings erases it too.

## Troubleshooting

| Symptom | Cause / fix |
| --- | --- |
| `adb devices` shows nothing | Cable is charge-only, or USB debugging is off. Try another cable/port; re-check developer options. |
| Device shows as `unauthorized` | The *Allow USB debugging* prompt wasn't accepted. Unplug, replug, watch the phone screen. Revoke via **Developer options → Revoke USB debugging authorisations** to force the prompt again. |
| `INSTALL_FAILED_UPDATE_INCOMPATIBLE` | Different signing key — uninstall first (see above). |
| `INSTALL_FAILED_USER_RESTRICTED` | MIUI/vendor block: enable **Install via USB** in developer options, and disable MIUI Optimization. |
| `INSTALL_FAILED_NO_MATCHING_ABIS` | The APK has no build for that phone's CPU. `npm run android:apk` targets `aarch64` (every modern phone); an x86 emulator needs `--target x86_64`. |
| Tapping the APK does nothing | The file manager lacks "install unknown apps" permission, or the download was truncated — re-download and compare sizes. |
| App installs but the screen sleeps mid-workout | The `FLAG_KEEP_SCREEN_ON` patch in `MainActivity.kt` was lost by regenerating `src-tauri/gen/android`. See the README. |
