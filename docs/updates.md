# App updates

The app can update itself, but only when you ask it to. Checking is off by default. Nothing is downloaded or installed without your confirmation. The code is in `src-tauri/crates/windows-platform/src/self_update/`, `src-tauri/crates/protection-core/src/update.rs` and `src/features/settings/UpdateSettings.tsx`.

## How updates work

1. You turn on update checks in Settings.
2. You press **Check for updates**. The app downloads a small manifest (`update.json`) and its Ed25519 signature (`update.json.sig`) from the `updates` branch of this repository through `raw.githubusercontent.com`.
3. If the signed manifest describes a newer version, the app shows it with its size.
4. You press **Download and verify**. The app downloads the installer from the GitHub release and checks it against the manifest.
5. You press **Install update…**. Windows asks you to confirm. Then the installer runs.

The app never checks, downloads or installs in the background. See [privacy](privacy.md#network-connections) for the exact endpoints.

If `src-tauri/keys/update.pub` still holds the `unconfigured` marker, update checks cannot work. Settings then shows "Updates are not available in this build".

## Turning update checks on

Open **Settings → App updates** and turn on **Allow checking for updates**. Until you do, the app contacts no update host. Turning it off again blocks every update request, because the network sink needs the `UpdateCheck` capability, and that capability only exists while the setting is on.

The setting is stored in `app-settings.json` in the app data folder.

## Installing an update

1. After **Download and verify** succeeds, the page says the version is downloaded and verified.
2. Press **Install update…**. A native Windows confirmation dialog appears. **No** is the default button.
3. If you choose **Yes**, the app opens the verified installer and closes itself.
4. The NSIS installer is per-machine, so Windows raises a UAC prompt. While releases are unsigned, the publisher shows as Unknown.
5. The installer replaces the app. Your settings, data and scheduled scans are kept. The uninstall step skips scheduled-task removal during updates.

## If an update does not finish

At startup, a background thread checks what happened last time:

- **Partial download:** deleted. Partial downloads never survive a restart.
- **Verified installer that was not launched:** hashed again. If it still matches, it stays available. If not, it is deleted.
- **Installer launched, but the app is still on the old version:** the installer is hashed again. Settings then shows that the update "didn't finish". You can try the installation again with the same file, or discard it.
- **Installer launched and the new version is running:** the staged files are deleted.

Staged files live in the `updates\` folder in the app data folder.

## What is verified

The manifest must pass every check in `protection-core/src/update.rs`, or it is rejected:

- The Ed25519 signature must match the embedded update key, `src-tauri/keys/update.pub`. This key is separate from the rule-pack key.
- The version must be newer than the running version. No rollback is possible, and re-offering the same version is refused.
- The running version must not be below the manifest's minimum version.
- The validity window can be at most 90 days. A manifest that has expired or is not yet valid is rejected. The check allows 10 minutes of clock skew.
- The installer name must be exactly `Supa-Diska-Klinah_<version>_x64-setup.exe`.
- The installer size must be no more than 512 MiB. The manifest itself is capped at 16 KiB.

The installer download:

- comes from `github.com/creativeprofit22/supa-diska-klinah/releases/download/v<version>/…`;
- may follow exactly one redirect, over HTTPS, to `release-assets.githubusercontent.com` or `objects.githubusercontent.com`. No other redirect is followed;
- must match the manifest's size and SHA-256 exactly.

Authenticode policy (`authenticode_allows` in `self_update/mod.rs`):

- The manifest declares `signing: none` or `signing: authenticode`. With `authenticode`, it also gives the expected certificate thumbprint.
- For an `authenticode` manifest, the installer must be validly signed with that thumbprint.
- A signed install only accepts an update signed with its own certificate. It never accepts an unsigned or differently signed update, whatever the manifest says.
- An invalid or unreadable signature is always rejected.

When you press **Install update…**, the installer is checked again before the confirmation dialog appears. From that check until launch, the file is held open with a share mode that allows reading only. Other processes cannot write, rename or delete it in between. If the check fails, the staged file is deleted.

## Key rotation

The update key is the trust root for in-app updates. To replace it:

1. Generate the new key pair outside the release pipeline.
2. Ship a release that embeds the **new** public key in `src-tauri/keys/update.pub`, and sign its manifest with the **old** key. Installed apps only trust the old key, so this release is how they learn the new one.
3. Put the new private key into the `UPDATE_SIGNING_KEY` secret. Sign all later releases with it.

`scripts/generate-update-key.mjs` creates the first key. It writes `update.pub` only while the file holds `unconfigured`, so it refuses to overwrite a configured key. Rotation is a deliberate manual edit. The private key is printed once, or written with `--private-out` to a path outside the repository. It is never stored in the repository.

If the key is lost, installed apps can no longer be updated in-app. Users must install a new release manually.

## Moving from unsigned to signed releases

Releases are currently unsigned. When code signing starts (see [release](release.md#turning-on-code-signing)):

- The first signed release's manifest declares `signing: authenticode` and the certificate thumbprint.
- Existing unsigned installs accept it, because an unsigned install may move to a signed one that matches the signed manifest.
- From then on, the signed install refuses unsigned updates and updates signed with a different certificate.
