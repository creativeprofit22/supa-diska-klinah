# User guide

Supa Diska Klinah finds and cleans up disk space, manages some Windows settings, and checks the PC for unwanted software. It runs as a standard user. When a change needs administrator rights, Windows asks you first.

To install the app, see [installation](installation.md).

## Getting around

The sidebar lists every page in one column, from **Dashboard** to **Settings**. The current page is highlighted. Everything works by keyboard, too: the first Tab stop is **Skip to content**, and Tab then moves through the sidebar in order. See [accessibility](accessibility.md).

Nothing is selected or changed for you. Scans only read. Changes always show a review step and a confirmation first.

## Storage

These pages find space you can reclaim: **Drives**, **Disk analyzer**, **Large files**, **Duplicate files**, **Empty folders**, **Rule cleaner**, **Browser caches**, **Installed programs** and **Cleanup**.

- [Storage](storage.md): scans, results, and what "incomplete" means.
- [Cleanup recovery](cleanup-recovery.md): quarantine, restoring items and automatic cleanup.
- [Project artifacts](project-artifacts.md): rebuildable build output under project folders you choose (on the Cleanup page).
- [Build artifact budgets](build-artifact-budgets.md): opt-in limits for build output of registered profiles.

## System

**Quick optimization**, **Startup apps**, **Windows services**, **Privacy**, **Firewall**, **Hosts file**, **Power and hibernation**, **Driver packages**, **Restore points**, **Windows Update** and **Scheduled scans** change Windows settings. Every change follows the same steps: select, review, confirm, apply. Most changes record how to undo them.

- [System management](system-management.md)

## Protection

**Protection** checks files and running programs using signed rules and local heuristics. It works offline and complements your antivirus. It does not replace it. Downloading rule packs and the password breach check are off until you turn them on.

- [Protection](protection.md)

## Settings

**Settings** holds:

- **Language**. See [below](#language).
- **App updates**: off by default. See [updates](updates.md).
- **Automatic cleanup** and build artifact budgets.
- **Scan speed**.

Settings are stored on this PC only. See [privacy](privacy.md).

## Language

Under **Settings → Language**, choose one of these:

- **System (Windows)**: follows the Windows display language. Any Spanish Windows language (`es-*`) uses Latin American Spanish. Any other language uses English.
- **English**
- **Español (Latinoamérica)**

The choice applies to the app and to its Windows confirmation dialogs. See [localization](localization.md).

## Getting help

- [Storage](storage.md)
- [System management](system-management.md)
- [Protection](protection.md)
- [Project artifacts](project-artifacts.md)
- [Build artifact budgets](build-artifact-budgets.md)
- [Cleanup recovery](cleanup-recovery.md)
- [Updates](updates.md)
- [Privacy](privacy.md)
- [Accessibility](accessibility.md)

To report a bug, open an issue at <https://github.com/creativeprofit22/supa-diska-klinah/issues>. Do not include file paths or other personal data you do not want to be public.
