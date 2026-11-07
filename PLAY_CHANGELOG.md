# Google Play changelog

Running list of changes for the next Play upload of the Rust app. Keep adding to **Pending
release** as changes land. After uploading, move the section to **Submitted releases** with the
date and track, then start a new pending section.

Play Console limits "What's new" to **500 characters** per language, so keep the copy block short;
details go in the notes below it.

## Pending release

Version: **1.3.0**, versionCode **14** (Play's latest before this was 13, the Expo app's 1.2.1)  
Build: `.\android\build.ps1 -Release -VersionCode 14` →
`android\app\build\outputs\bundle\release\app-release.aab`  
Status: not uploaded yet. Upload to **Internal testing** first: this is the first build of the new
app installed over the Expo one.

### Copy for Google Play — What's new

Copy only the text inside this block:

```text
• Rebuilt from the ground up: the app now uses about a third less memory.
• Notifications are now delivered directly through Firebase.
• Staff: please sign in once more after updating.
• Staff: gallery photos are rotated correctly, resized, and have location data removed before upload.
• Staff: sending a notification to everyone now asks for confirmation.
• Staff: multi-line fields grow to fit their text.
```

### Details

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

## Submitted releases

The Expo app's history (up to 1.2.1, versionCode 13) is in the Expo app's repository
(`PENDING_GOOGLE_PLAY_CHANGELOG.md`).
