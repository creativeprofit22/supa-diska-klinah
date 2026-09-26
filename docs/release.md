# Release process

Releases are built, verified and published only by the `windows-release-build` and `windows-release-publish` jobs in `.github/workflows/ci.yml`. There is no local release path.

Current signing mode: unsigned

The line above is a marker. `scripts/check-signing-mode.ps1` and `pnpm check:docs` both read it. It must match the `WINDOWS_SIGNING_MODE` repository variable.

## Signing modes

| Mode | Installer and binaries | Update manifest `signing` | Status |
| --- | --- | --- | --- |
| `unsigned` | No Authenticode signature. `verify-windows-release.ps1 -SigningMode unsigned` requires the status `NotSigned`. A partial or corrupt signature fails. | `none` | **Current** |
| `authenticode` | Signed with the one expected certificate. The verifier requires the expected thumbprint, the expected subject, and a timestamp. | `authenticode` plus the thumbprint | Pending a certificate |

In both modes, the update manifest is signed with the Ed25519 update key. See [updates](updates.md).

## Secrets and variables

Repository variables:

| Name | Used in | Value |
| --- | --- | --- |
| `WINDOWS_SIGNING_MODE` | both modes | Exactly `unsigned` or `authenticode`. It must match the marker line above. If it is missing or different, the release fails before any build step. |
| `WINDOWS_CODESIGN_EXPECTED_THUMBPRINT` | `authenticode` only | Thumbprint of the product signing certificate |
| `WINDOWS_CODESIGN_EXPECTED_SUBJECT` | `authenticode` only | Common name of the certificate subject |

Secrets in the `windows-release` environment:

| Name | Used in |
| --- | --- |
| `UPDATE_SIGNING_KEY` | Always. The Ed25519 private key that signs `update.json`. |
| `WINDOWS_CODESIGN_PFX_BASE64` | `authenticode` only |
| `WINDOWS_CODESIGN_PFX_PASSWORD` | `authenticode` only |

## Key custody

- The update private key exists only in the `UPDATE_SIGNING_KEY` secret and in the owner's offline backup. It is never committed. `src-tauri/keys/update.pub` holds only the public key.
- Only jobs that use the `windows-release` environment receive the key. Only the staging step of `windows-release-build` reads it.
- While releases are unsigned, this key is the only integrity root for in-app updates. Protect the `windows-release` environment with required reviewers and restrict it to protected tags.
- The update key is separate from the rule-pack signing key. Rotate them separately. See [updates](updates.md#key-rotation).
- The PFX certificate, when there is one, lives only in its two secrets. The build imports it for signing and removes it at the end of the job, even when the job fails.

## Cutting a release

1. Bump the version in `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`. `pnpm check:docs` fails if they differ.
2. Complete the [release checklist](release-checklist.md) and [release verification](verification/release.md).
3. Push a tag `v<version>`. You can also run the workflow manually with `release: true` to build and verify without publishing. Publishing always requires a `v*` tag.
4. `windows-release-build` runs after `quality`, `native-smoke` and `dependency-audit` pass.
5. `windows-release-publish` runs only for tags. It creates the GitHub release, and then publishes the update manifest as its last step.

## What the pipeline verifies

Build job (`windows-release-build`):

1. `check-signing-mode.ps1`: the mode is exactly `unsigned` or `authenticode`, and it matches the marker.
2. Builds the per-machine NSIS bundle for `x86_64-pc-windows-msvc`. The bundle is signed in `authenticode` mode.
3. `verify-windows-release.ps1` installs the bundle silently. It checks the installer, app and helper against the declared mode, checks that the helper sits beside the app, and checks that standard users cannot modify the `Program Files` ACLs. It then runs the native smoke test at standard integrity and uninstalls.
4. `stage-release-assets.ps1` produces exactly these files: the installer, `update.json` (from `sign-update-manifest.mjs`, valid for at most 90 days), `update.json.sig`, `dependency-inventory.json` and `SHA256SUMS`.
5. `actions/attest-build-provenance` attests every asset.

Publish job (`windows-release-publish`):

1. `verify-release-assets.mjs` (logic in `release-assets.mjs`) checks the asset set, every checksum, and the manifest signature against the committed `update.pub`. It also checks the manifest version, installer name, size and hash, the signing mode, and the validity window.
2. `gh attestation verify` checks each asset against this repository and the `ci.yml` workflow.
3. `publish-release.mjs` creates the release with release notes that match the signing mode.
4. `publish-update-manifest.mjs` writes `update.json` and `update.json.sig` to the `updates` branch. This runs last, so clients never see a manifest before its installer is published.

## Verifying a published release

Follow [installation → verify your download](installation.md#verify-your-download):

```powershell
Get-FileHash .\Supa-Diska-Klinah_<version>_x64-setup.exe -Algorithm SHA256   # compare with SHA256SUMS
gh attestation verify .\Supa-Diska-Klinah_<version>_x64-setup.exe --repo creativeprofit22/supa-diska-klinah
```

For `update.json`, run `node scripts/verify-release-assets.mjs --dir <folder with all assets> --tag v<version> --signing-mode unsigned`. It repeats the publish-side checks locally.

## Rollback

- Never re-publish an older version as an update. Clients refuse any version that is not newer than the one they run.
- To undo a bad release, ship a fixed, higher version.
- You can mark a bad release as a pre-release or delete it on GitHub. The next release replaces the manifest on the `updates` branch. Until then, clients that already downloaded the bad installer still verify it against the manifest they received.
- If users should stop being offered the bad version before a fix is ready, publish the fix as soon as possible. Do not edit the manifest by hand, because only a signed manifest is accepted.

## Turning on code signing

1. Add the `WINDOWS_CODESIGN_PFX_BASE64` and `WINDOWS_CODESIGN_PFX_PASSWORD` secrets to the `windows-release` environment. Add the `WINDOWS_CODESIGN_EXPECTED_THUMBPRINT` and `WINDOWS_CODESIGN_EXPECTED_SUBJECT` variables.
2. Set `WINDOWS_SIGNING_MODE=authenticode`.
3. Change the marker line to `Current signing mode: authenticode`. Remove the unsigned notices from `README.md`, `docs/installation.md` and `docs/security.md`. In the app, rewrite the `updates.unsignedNote` text in `src/features/settings/strings.ts` for both languages ("Releases are currently not code-signed" and "Por ahora, las versiones no tienen firma de código"). `pnpm check:docs` enforces that the docs and the app text match the mode.
4. Cut a release. Existing unsigned installs accept it, because its signed manifest announces `authenticode` and the thumbprint. From then on, those installs refuse unsigned updates.
