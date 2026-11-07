# Google Play changelog

Running list of changes for the next Play upload of the Rust app. Keep adding to **Pending
release** as changes land. After uploading, move the section to **Submitted releases** with the
date and track, then start a new pending section.

Play Console limits "What's new" to **500 characters** per language, so keep the copy block short;
details go in the notes below it. The GitHub release workflow uploads the copy block under Pending
release as the en-US "What's new", and refuses to upload if it is empty or too long.

## Pending release

Version: next after **1.4.0** (bump `version` in `Cargo.toml`), versionCode **16**  
Build: GitHub Actions → Play release → Run workflow with versionCode 16 (uploads to Internal
testing), or locally `.\android\build.ps1 -Release -VersionCode 16`  
Status: nothing pending yet.

### Copy for Google Play — What's new

Copy only the text inside this block:

```text
```

### Details

## Submitted releases

### 1.4.0 (versionCode 15) — Internal testing

Uploaded to Internal testing on 2026-11-07 by the Play release workflow. Desktop builds are on the
GitHub release `v1.4.0`.

#### What's new (as submitted)

```text
• TalkBack support: the screen reader can now read every screen, open buttons, tabs and cards, and scroll pages.
```

#### Details

User-facing:
- TalkBack works in the app. The Rust 1.3.0 app showed TalkBack nothing but an empty screen (the
  Slint toolkit's Android backend has no accessibility support), a regression from the Expo app.
  The app now builds an accessibility tree from the UI (`crates/fwg-app/src/android_a11y.rs`,
  AccessKit): labels, roles, buttons, tabs, text fields, and scroll views, so swiping to the next
  item scrolls the page. Checked on the Android 16 emulator with TalkBack 16: touch exploration
  reads items, double-tap opens them, swipe navigation scrolls the Home page. Not yet checked on a
  phone. There is no green focus outline (NativeActivity draws the window itself), and text fields
  don't support reading by character or word yet.
- Server, stream and staff game-server cards can be activated by screen readers (they had a
  button role and label but no action). Text fields expose their content to screen readers
  (never a password's) and can be focused and filled by them. Verified on Windows (UI
  Automation) and Linux (AT-SPI) by the desktop end-to-end tests.

### 1.3.0 (versionCode 14) — live

First release of the Rust app, replacing the Expo app's 1.2.1 (versionCode 13). Live on Google
Play as of 2026-11-07.

#### What's new (as submitted)

```text
• Rebuilt from the ground up: the app now uses about a third less memory.
• Notifications are now delivered directly through Firebase.
• Staff: please sign in once more after updating.
• Staff: gallery photos are rotated correctly, resized, and have location data removed before upload.
• Staff: sending a notification to everyone now asks for confirmation.
• Staff: multi-line fields grow to fit their text.
```

#### Details

User-facing:
- The whole app is rewritten in Rust with the Slint UI toolkit, replacing Expo / React Native.
  Measured on a Pixel 10 after visiting every screen: about 147–164 MB vs 234–240 MB for the Expo
  app.
- Same screens, links, sharing, back-button behaviour and notification taps as 1.2.1.
- Push notifications go straight through Firebase Cloud Messaging instead of Expo's push service.
  Phones still on the Expo version keep receiving pushes through Expo until they update; after
  updating, the app registers itself with Firebase at launch if notifications were turned on.

Staff:
- Every staff screen from the Expo app is included: publish news, send push, gallery upload,
  manage news/gallery/staff/game servers, notification history, password and security.
- The staff session is stored encrypted with an Android Keystore key. The Expo app's stored
  session can't be read, so staff sign in once after updating.
- Gallery uploads: JPEG/WebP photos are re-encoded with the camera rotation applied and EXIF data
  (including GPS location) removed; images over 2560 px are scaled down to stay under the
  website's 10 MB limit. GIFs are uploaded unchanged.
- Sending a push now asks "Send to everyone?" first (the Expo app sent immediately).
- Deletes, deactivations and resets ask for confirmation; Back closes the prompt.

Compatibility:
- Requires the website's FCM push support (`api/v1/_fcm.php`, `push_devices.provider`), deployed
  2026-11-06.
- Android 8.0+ (minSdk 26), as before. Includes 64-bit ARM, 32-bit ARM and x86_64.

The Expo app's history (up to 1.2.1, versionCode 13) is in the Expo app's repository
(`PENDING_GOOGLE_PLAY_CHANGELOG.md`).
