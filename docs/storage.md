# Storage cleanup and analysis

Supa Diska Klinah provides eight storage workflows. Scans do not change files. Cleanup requires explicit selection, an immutable native plan, and confirmation. Supported behavior and remaining verification are recorded in [storage verification](verification/storage-parity.md) and the [parity matrix](parity.md).

## Choose the right tool

| Page | Purpose | Available action |
| --- | --- | --- |
| Fixed drives | Fixed-drive capacity, readiness and system-drive identity | Read-only inventory |
| Disk analyzer | Subtree totals, child folders and extension totals, largest first | Read-only analysis |
| Large files | Minimum/maximum size, extension, category, sort and depth filters | App recovery or permanent deletion of selected files |
| Rule cleaner | Reviewed native catalog scopes, age/exclusion rules and unsupported targets | App recovery or permanent deletion of eligible selected files |
| Duplicate files | Independent physical copies verified by partial and full hashing | Keep a protected copy; recover or permanently delete selected other copies |
| Empty folders | Bottom-up empty directories, with root retention and blocked-parent rules | Permanent, atomic empty-only removal |
| Browser caches | Native profile/shared cache scopes and optional service-worker caches | App recovery or permanent deletion of eligible selected cache files |
| Installed programs | Registry inventory and separate vendor uninstall jobs | Native-confirmed vendor operation; never filesystem leftover deletion |

Fixed-drive headings show the native drive letter beside the volume label, for example `Data (D:\)` or `Unlabelled drive (E:\)`. Equal or empty labels remain distinguishable without list order or capacity guesses. The shared `DriveSummary.displayMount` field survives both `list_drive_inventory` and paged `StorageRecord::Drive` responses. It is display-only; opaque `driveId` remains the identity. Fixed-drive eligibility and account/quota capacity calculations are unchanged.

## Scan, inspect, then decide

1. For analyzer, large files, duplicates or empty folders, use **Choose folder**. Paths are display-only after the native picker; the backend uses opaque, expiring authorizations.
2. For rule cleaner or browser caches, select and authorize a **native scope**. Unsupported/unavailable entries cannot authorize a scan. You cannot type a catalog/profile path to bypass this boundary.
3. Configure filters, then start the scan. Root authorizations are single-use; choose or authorize again for another scan. Changing scope or filters discards previous results and selection.
4. Inspect bounded pages. A page holds at most 100 scan rows; it replaces the previous page. **First page** returns to the start. At most 1,000 candidate IDs can be selected. No files are selected automatically.
5. Select eligible items explicitly, review the native plan and confirm the actual disposition. A changed identity, rule, protection, profile or duplicate keeper can make the operation refuse an item even after review.
6. Read the outcome and recovery history. Processing completion is not the same as successful cleanup; failed/unknown items remain visible as needing attention. Counts are independent of bytes, so a failed empty folder or zero-byte file still counts as a failed item. **Item outcomes** shows up to 20 rows at a time, with separate Previous/Next items controls that do not change the execution-history page. Each row shows its immutable candidate ID, native display location, state and a safe explanation. Unknown or unproven recovery identities are uncertain—not successful deletion or guaranteed recovery.

Display locations come from the already-retained immutable native plan, not retained scans or frontend path authority. Control/direction overrides are replaced and text is capped at 1,024 characters plus a truncation marker; the full candidate ID distinguishes truncated locations. If plan metadata is unavailable, the journal outcome and ID remain visible with an explicit unavailable-location label. Collapsed history details do not mount item rows. Native limits remain 1,000 items per execution and 20 executions per UI history page; vendor history is separate.

Only one cooperative scan runs at a time. Cancel discards results and selections, stops accepting stale responses, and releases the snapshot. It is not a cleanup action. Expired, unavailable or busy results require a fresh scan or waiting for the current worker to finish. Never treat incomplete totals or a truncated result list as a complete disk-wide ranking.

Traversal depth counts nested directories, starting with the chosen folder at zero. Files in the last allowed directory are included; deeper directories are not traversed. Depth-limited results are marked incomplete. The maximum configurable traversal depth remains 64.

### Scope-specific details

- **Analyzer:** displayed folder depth controls rows, not measured subtree totals within traversal limits. Unknown allocation stays unknown. Hard links are deduplicated per subtree/extension; sibling totals can overlap and need not sum to the root. Directory and extension pages sort by measured logical bytes descending.
- **Large files:** the UI starts at 10 MiB with a traversal depth of 20, matching the pinned reference. Set a minimum of 0 to include smaller files. A blank maximum is unlimited within native budgets. Extensions are comma-separated names such as `pdf, zip`; a leading dot is accepted and normalized. Categories and sorting are backend filters, not a filter over only the visible page.
- **Rule cleaner:** categories filter catalog information, not mutation authority. Every scan still uses the native rules matching its exact authorized root. The catalog shows provenance, age, exclusions and unsupported operations. Work across different roots is sequential, with distinct snapshots/plans, not one atomic cleanup.
- **Duplicates:** minimum/maximum size and extension filters apply before grouping. The default minimum is 1 MiB and default traversal depth is 20. Review one group at a time. The first member of its first page is reserved as keeper, including while viewing later member pages. Switching groups clears selection. Hard-link aliases are not independent copies; the backend independently rejects removing every copy and checks keepers again before mutation.
- **Empty folders:** the default traversal depth is 20 and the chosen root survives. Hidden/system, protected, inaccessible, link-like or non-empty contents block their parents. New children block removal at the native filesystem operation, rather than being recursively removed.
- **Browser caches:** close supported browsers before scanning or cleaning. Active or unknown browser activity is refused, and activity/profile evidence is checked again before mutation and recovery. Cookies, logins, bookmarks, history, sessions and private profile databases remain excluded. Service-worker caches are off initially; opting in can lose offline cache content or require sites to download it again. Read the displayed native disclosure.

## Recovery is not reclaimed space

**Move to app recovery** uses this app's existing quarantine journal and undo history. It is not Windows Recycle Bin. Storage cleanup does not fall back to an unsafe pathname-based recycle operation.

- Recovery supports regular files on the recovery store's volume only. Native plan creation checks volume identity before saving a recovery plan; execution checks again. An unsupported volume returns a sanitized reason, disables recovery for that selection, and never opens a reviewed recovery plan. Permanent deletion, where allowed, remains an explicit separate review and Windows confirmation—not an automatic alternative. Cross-volume moves and directory recovery are refused; there is no Recycle Bin or copy/delete fallback.
- The native engine retains the original file identity while moving it, pins no-follow ancestors, and refuses destination collisions.
- Files held for recovery still occupy disk space. They are not automatically purged. This is recovery storage, not an independent backup: file identity and metadata are checked again before restore.
- **Load cleanup history**, then **Undo cleanup** for recoverable items. Use **Older cleanup history** to reach retained executions beyond the newest 20; **Refresh newest cleanup history** returns to the newest page. Both cleanup views retain only one page, and Undo targets the execution's immutable ID. Restoring never overwrites an occupied original path. Changed roots, reparse points, changed recovery files or active browser scope can prevent undo without deleting the recovery copy.
- If undo is refused, preserve the history and recovery data. Do not delete application journals or rename recovery files to force a result. Inspect the original location and recorded reason. A refused recovery or uncertain item suppresses further Undo for that execution; there is no automatic retry or copy fallback. Ordinary failed cleanup items do not prevent Undo of other confirmed retained items.

**Delete permanently** cannot be undone. It opens a separate native Windows confirmation with the native-resolved plan, count, bytes and path preview. No is the default. The engine revalidates after confirmation. Empty-folder removal uses this mode because moving a directory after an emptiness check could capture a newly created child.

Protection quarantine is a separate store from storage recovery: it holds files a Protection scan flagged, in a scrambled form, and has its own restore and delete actions. See [protection](protection.md).

The original temporary-cache Cleanup page has its own existing dispositions. Do not assume every legacy disposition is supported by every storage module.

## Installed programs are different

Refresh inventory, select one reported program, and **Review vendor uninstall**. Preparation does not launch anything. **Continue to Windows confirmation** shows the backend-resolved vendor operation; Windows or the vendor may then request elevation or show its own interface.

The app cannot undo vendor uninstall. Estimated installed size is not reclaimed space. A launcher exit is not proof of removal; non-MSI exit meanings may remain unknown. Refresh inventory to observe registry changes, not to prove every file was removed. Timeout/cancellation may stop waiting without stopping the installer. Submitted jobs are not killed merely because you navigate away, and persisted commands are never replayed after restart.

Unknown-ownership leftovers are informational and never selectable. The app does not turn an install-location/name/publisher guess into deletion authority. An unresolved outcome may prevent another operation for that same registered program; inspect its state instead of retrying blindly.

### Retained history and capacity

Vendor history shows up to **64 outcomes per page**, newest creation first. **Older retained jobs** inspects older outcomes without preparing, confirming or replaying a job. **Refresh retained history** returns to the newest page. Completed, cancelled and unknown outcomes remain available across restart; preparation expiry releases consent, not history.

History pages are live views, not snapshots: updated outcomes appear when revisited, while newer entries appear on newest-page refresh. Cursors do not expire with scans or preparations. An empty terminal page means end of history, not a load failure; a failed load preserves the visible page and does not erase a successful operation outcome.

The vendor ledger deliberately stops accepting new preparations at **1,000 retained outcomes** or earlier when its **8 MiB file budget**, including reserved transition/restart headroom, is full. Existing history remains inspectable; outcomes are never evicted to make room. Further growth requires a future journal-store migration. Do not delete journals, especially unknown outcomes, to bypass capacity or replay restrictions.

## Verification and development

Use strict development port 1520 and strict browser-preview port 1521 only; [development setup](development.md) explains the commands. Strict preview 1521 has been verified on the current host after an earlier reservation failure. No Windows service or unrelated process was changed to bypass that failure.

Native fixtures use disposable directories and a purpose-built harmless process. Browser fixture replays intercept mutation commands. Manual Narrator, live native confirmations and full installed-app scan/execute/undo coverage remain incomplete; do not interpret a successful unit-test or read-only smoke run as completion of those gates. See the [evidence ledger](verification/storage-parity.md).
