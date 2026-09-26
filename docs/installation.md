# Installing Supa Diska Klinah

Supa Diska Klinah ships as one per-machine NSIS installer for 64-bit Windows. Releases are published on GitHub Releases for the `creativeprofit22/supa-diska-klinah` repository.

## Requirements

- 64-bit (x64) Windows. Releases are built for `x86_64-pc-windows-msvc` only. Manual acceptance has run on Windows 10 22H2 x64. Windows 11 has not been tested (see [release checklist](release-checklist.md)).
- An administrator account, or an administrator's approval, to install. The app itself runs as a standard user.
- Microsoft Edge WebView2 Runtime. Current Windows versions include it. If it is missing, the installer's default Tauri bootstrapper downloads it.

## Download

1. Open <https://github.com/creativeprofit22/supa-diska-klinah/releases>.
2. Under the release you want, download:
   - `Supa-Diska-Klinah_<version>_x64-setup.exe`, the installer.
   - `SHA256SUMS`, the checksum list.

The release also contains `update.json`, `update.json.sig` and `dependency-inventory.json`. You do not need these to install. The in-app updater uses the first two.

## This release is not code-signed

Releases currently have no Authenticode signature. That has two visible effects:

- When you open the installer, Microsoft Defender SmartScreen may show **"Windows protected your PC"**. The only way past it is **More info → Run anyway**.
- The UAC prompt shows **Unknown publisher** instead of a verified publisher name.

These warnings mean Windows cannot tell who built the file. Before you click **Run anyway**, check that the file is the one this project published. See the next section.

## Verify your download

Do this **before** you choose **More info → Run anyway**.

1. In PowerShell, in the folder you downloaded to, run:

   ```powershell
   Get-FileHash .\Supa-Diska-Klinah_<version>_x64-setup.exe -Algorithm SHA256
   ```

2. Open `SHA256SUMS` in a text editor. Find the line for the installer and compare the hash. Case does not matter. If the hashes differ, delete the file and do not run it.
3. Optional: if you have the GitHub CLI, check that the file came from this repository's CI:

   ```powershell
   gh attestation verify .\Supa-Diska-Klinah_<version>_x64-setup.exe --repo creativeprofit22/supa-diska-klinah
   ```

   This checks the build provenance attestation that the release pipeline creates for every asset.

A matching checksum only proves that the file matches the published list. The attestation also proves that this repository's workflow built the file.

## Install

1. Run the installer. If SmartScreen appears and you have verified the file, choose **More info → Run anyway**.
2. Approve the UAC prompt. The installer is per-machine, so it always asks for administrator approval. While releases are unsigned, the publisher shows as Unknown.
3. The installer places the app and its privileged helper in `Program Files`. The installer is available in English and Spanish.

The app starts as a standard user. Only specific operations ask for elevation, through the bundled helper. See [security](security.md).

## Uninstall

1. Open **Windows Settings → Apps → Installed apps** and choose **Supa Diska Klinah → Uninstall**.
2. Approve the UAC prompt.

The uninstaller removes:

- the app files, including the privileged helper;
- the app's uninstall registry entry;
- the app's scheduled scans (the `\SupaDiskaKlinah` Task Scheduler folder, for all users). Before it removes any files, the uninstaller runs the bundled helper with `--remove-scheduled-tasks` (see `src-tauri/windows/installer-hooks.nsh`). This step is skipped when an update runs the uninstall step, so your schedules are kept during updates. If the step fails, the uninstall still continues.

Your data (settings, cleanup journals, quarantine and scan summaries) stays unless you tick the uninstaller's option to delete the app data. See [privacy](privacy.md#removing-your-data).
