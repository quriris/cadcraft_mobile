# CADCraft Mobile (Android)

`apps/cadcraft-android` runs the same CADCraft as the desktop on arm64 Android phones and
tablets (Android 8.0+, Vulkan or OpenGL ES 3).

## Установка

1. Откройте на устройстве страницу релиза **android-latest** этого репозитория и скачайте
   `cadcraft-mobile-arm64.apk`.
2. Разрешите установку из этого источника (браузер или файловый менеджер попросит сам).
3. При первом открытии чертежа нажмите «Разрешить доступ» и включите «Доступ ко всем файлам»,
   чтобы открывать DWG/DXF из «Загрузок», «Документов» и мессенджеров.

Новые чертежи, сохранения и экспорт попадают в `Документы/CADCraft`. Свои шрифты (например,
ГОСТ type A в формате TTF) положите в `Документы/CADCraft/fonts` и перезапустите приложение.

## Сенсорные кнопки

Справа на экране: **⏎** (Enter), **Esc**, **↶** (отменить), **ПКМ** (следующее касание будет
правым кликом), **⌨** (экранная клавиатура для командной строки), **📂** (открыть чертёж),
**▶** (свернуть кнопки). Кнопка «Назад» Android работает как Esc. Масштаб — двумя пальцами.

## Building

The workflow `.github/workflows/android.yml` builds on every push to the `android` branch:
cargo-apk (pinned revision), the runner's Android NDK, `aarch64-linux-android`. Locally:

```sh
rustup target add aarch64-linux-android
cargo install --git https://github.com/parasyte/cargo-apk.git --rev 282639508eeed7d73f2e1eaeea042da2716436d5 cargo-apk
export ANDROID_HOME=… ANDROID_NDK_ROOT=…
# icons: see the "App icon" step of the workflow (target/android-res)
CARGO_APK_RELEASE_KEYSTORE=apps/cadcraft-android/signing/cadcraft-mobile.p12 \
CARGO_APK_RELEASE_KEYSTORE_PASSWORD=cadcraft-mobile \
cargo apk build -p cadcraft-android --lib --release
```

**Signing.** The key in `apps/cadcraft-android/signing/` is public on purpose: it only keeps the
signature stable so a new APK installs over the old one. To sign with a private key instead, set
the repository secrets `ANDROID_KEYSTORE_BASE64` (the PKCS#12 file, base64) and
`ANDROID_KEYSTORE_PASSWORD`; the workflow prefers them. Switching keys means uninstalling once.

## Known limits

- The on-screen keyboard sends plain key events (NativeActivity has no text input connection):
  Latin letters, digits and symbols work; some keyboards don't send Cyrillic this way. A hardware
  keyboard works fully.
- OPEN uses an in-app browser; IMPORT, template and linetype-file pickers also open it, and a
  picked file is opened as a drawing.
- Save As picks no folder: files go to `Documents/CADCraft` (a drawing opened from elsewhere is
  saved back where it came from).
