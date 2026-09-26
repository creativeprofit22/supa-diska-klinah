# System management: user guide

These pages change Windows settings. Every change follows the same four steps:

1. **Select.** Nothing is selected for you, except on Quick optimization. There, low-risk changes that can be undone start ticked, and you can untick any of them.
2. **Review.** The app reads the current state of each selected item and lists, for each one:
   - what it affects
   - its risk
   - whether a restart or sign-out is needed
   - whether it needs administrator permission
   - whether it **can be undone**, can be undone from a backup, or **cannot be undone**

   The review expires after 60 seconds.
3. **Confirm in Windows.** A Windows dialog, not the app page, lists the changes again and asks you to confirm. If any change needs administrator rights, Windows shows **one** UAC prompt for the whole batch.
4. **Results.** Each change reports its own result:

   | Result | Meaning |
   | --- | --- |
   | Applied | The change was made. |
   | Already in place | Nothing needed changing. |
   | Skipped: the setting changed since review | Something else changed it after you reviewed; nothing was written. |
   | Skipped: not supported | This device or edition does not support the change. |
   | Denied | Administrator permission was refused. |
   | Failed / Not attempted | The change did not complete; later changes in the batch still ran or are listed as not attempted. |

   If some changes fail, the others are still reported individually. Nothing fails silently.

## Undoing changes

Each page shows its **Change history**. Tick the entries you want to undo, then choose **Review undo**. Undo restores the exact value recorded before the change, and goes through the same review and Windows confirmation.

- **Cannot be undone:** removing a driver package. The Drivers page suggests creating a restore point first and ticks that option for you. If the restore point in the same plan fails, irreversible changes after it are not attempted.
- **Undo from a backup:** hosts file edits. The file is backed up before every edit.
- **Interrupted:** the app stopped mid-change, for example because of a crash or power loss. The setting's state is unknown, so check it on its page before retrying.

## What each page does

| Page | What you can change | Admin needed | Undo |
| --- | --- | --- | --- |
| Quick optimization | Suggested service, privacy, performance, and power-plan changes, each listed separately | For some | Per change |
| Startup apps | Turn startup entries on or off (entries are never deleted) | Only for all-users entries | Yes |
| Windows services | Set the start type of listed optional services (Automatic/Manual/Disabled); services are not stopped immediately | Yes | Yes |
| Privacy | Apply recommended privacy and performance values, restore Windows defaults, toggle Microsoft telemetry tasks | For machine-wide items | Yes |
| Firewall | Turn profiles or individual rules on or off; review audit findings | Yes | Yes |
| Hosts file | Disable or restore individual mapping lines | Yes | From backup |
| Power and hibernation | Turn hibernation on or off (off also disables Fast Startup and frees the hibernation file); choose the active power plan | Hibernation only | Yes |
| Driver packages | Remove superseded third-party driver packages that no device uses | Yes | **No** |
| Restore points | Create a restore point; view existing points and protection | Yes | Not applicable |
| Windows Update | Set documented update policies; check for updates now | Yes | Yes |
| Scheduled scans | Add or remove this app's own scheduled scans (read-only: they never delete anything) | No | Remove |

## Troubleshooting

- **"You cancelled the Windows confirmation":** nothing was changed. Review again when you are ready.
- **"The review expired":** more than 60 seconds passed. Review again, because the app re-reads the current state.
- **Denied:** UAC was declined, or your account is not an administrator. Ask an administrator, or skip those changes.
- **"Managed by your organization":** a domain or MDM policy controls this setting, and local changes would be overridden.
- **"Not honored by this Windows edition":** Windows Home ignores Group Policy values, so the app will not write them there.
- **Skipped: the setting changed since review:** another tool or Windows changed it. Refresh the page and review again.
- **Restore point not created:** Windows skips a new point if one was made recently (see the creation frequency on the Restore points page), or if System Protection is off.
- **The firewall page shows no data:** the Windows Defender Firewall service may be stopped or replaced by another firewall product.
