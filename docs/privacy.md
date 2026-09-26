# Privacy

Supa Diska Klinah works locally. It has no account, no analytics and no telemetry. It makes a network connection only after you turn on the matching setting, and only when you start the action.

## What stays on this device

- Scan results, file lists, paths and sizes. Scans never leave the PC.
- Cleanup plans, journals and quarantined files.
- System-management inventories and change journals.
- Protection evidence, such as installed programs, signers, startup entries and Defender history.
- Settings and scheduled-scan summaries.

The app sends no crash reports, usage statistics or identifiers. The network policy in `protection-core/src/policy.rs` has no telemetry purpose. A stored policy with an unknown field such as `telemetry` is rejected.

## Network connections

All outgoing requests go through one HTTPS sink in `src-tauri/crates/windows-platform/src/protection/net.rs`. It only accepts the fixed `Endpoint` values below. Each purpose needs a capability token that exists only while its setting is on. Every setting is **off by default**. Nothing is contacted while a setting is off.

| Endpoint | Host and path | What is sent | Setting (default off) |
| --- | --- | --- | --- |
| `RulePack` | `raw.githubusercontent.com` `/creativeprofit22/supa-diska-klinah/rule-packs/pack.json` | A plain GET. Nothing about your files. | Protection → **Allow downloading signed rule packs**, and only when you press Download |
| `RuleSignature` | `raw.githubusercontent.com` `/creativeprofit22/supa-diska-klinah/rule-packs/pack.sig` | A plain GET. | Same as `RulePack` |
| `PasswordRange` | `api.pwnedpasswords.com` `/range/<5 hex chars>` | Only the first 5 uppercase hex characters of the password's SHA-1 hash (k-anonymity). The request asks for padded responses (`Add-Padding: true`), so the response size does not reveal the prefix. The password never leaves the PC. | Protection → **Allow the password breach check** |
| `UpdateManifest` | `raw.githubusercontent.com` `/creativeprofit22/supa-diska-klinah/updates/update.json` | A plain GET. | Settings → App updates → **Allow checking for updates**, and only when you press Check |
| `UpdateSignature` | `raw.githubusercontent.com` `/creativeprofit22/supa-diska-klinah/updates/update.json.sig` | A plain GET. | Same as `UpdateManifest` |
| `UpdateInstaller` | `github.com` `/creativeprofit22/supa-diska-klinah/releases/download/v<version>/Supa-Diska-Klinah_<version>_x64-setup.exe`, with exactly one redirect allowed to `release-assets.githubusercontent.com` or `objects.githubusercontent.com` | A plain GET. | Same as `UpdateManifest`, and only when you press **Download and verify** |

Other endpoints do not follow redirects. Like any HTTPS request, each connection shows your IP address to the host. The app adds no cookies or identifiers.

Actions you start yourself can open other programs. For example, Windows Update is managed through Windows' own services. Those programs follow their own privacy rules.

## Where data is stored

The app data folder is `%APPDATA%\com.supadiskaklinah.app`. The name comes from the bundle identifier in `src-tauri/tauri.conf.json`. It contains:

- `app-settings.json`: language, scan speed and the update-check setting.
- `updates\`: staging for a downloaded update installer and its state. See [updates](updates.md#if-an-update-does-not-finish).
- Cleanup plans, journals and the app-managed quarantine. See [cleanup recovery](cleanup-recovery.md).
- System-change journals. See [system management](system-management.md).
- `scheduled-scans\summaries.json`: results of scheduled scans.
- `protection\`: protection settings, network opt-ins and installed rule packs. See [protection](protection.md).

`%LOCALAPPDATA%\com.supadiskaklinah.app` holds the WebView2 browser data that Tauri creates for the app window.

## Removing your data

- **Keep data, remove the app:** uninstall from Windows Settings → Apps. Your data folders stay.
- **Remove everything:** tick the uninstaller's option to delete the app data. The uninstaller then deletes the folders above for the current user.
- **Manual removal:** after uninstalling, delete `%APPDATA%\com.supadiskaklinah.app` and `%LOCALAPPDATA%\com.supadiskaklinah.app`.

Quarantined files live inside the app data folder. Restore anything you want to keep **before** you delete the folder. Uninstalling always removes the app's scheduled tasks. See [installation](installation.md#uninstall).
