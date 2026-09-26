# Embedded baseline rule pack — provenance

`pack.json` (sequence 1) is compiled into the binary and signed in `pack.sig` with the key whose public half is `src-tauri/keys/rule-pack.pub`. It exists so scanning always works offline and after any rule-store corruption.

| Rule ID | Source | Notes |
| --- | --- | --- |
| `eicar.sha256` | [EICAR anti-malware test file](https://www.eicar.org/download-anti-malware-testfile/) | SHA-256 of the canonical 68-byte file, `275a021b…51fd0f`. Recomputed locally from the published string on 2026-09-23. |
| `eicar.bytes` | EICAR specification | Matches the 68-byte string at offset 0 in a file of at most 128 bytes, which the specification allows for trailing whitespace. The pattern is stored as hex so this repository's files do not themselves trigger antivirus products. |

No real-malware hashes are included. We could not independently verify third-party hash lists offline, and an unverified entry would be a false claim of detection. Real detection content must arrive as a separately signed pack with its own provenance table.

## Signing key status

The current key is a **test key**; its private half is committed at `src-tauri/crates/windows-platform/src/protection/fixtures/test-rule-pack.pem`. Release builds therefore disable pack import and download (see `docs/protection.md`, "Rule provenance and key handling"). To ship real packs, the maintainer generates a release key outside the repository with `node scripts/rule-pack.mjs keygen`, replaces `src-tauri/keys/rule-pack.pub`, and re-signs this baseline.

## Re-signing

```sh
node scripts/rule-pack.mjs sign --private <key.pem> --pack src-tauri/crates/windows-platform/src/protection/baseline/pack.json --out src-tauri/crates/windows-platform/src/protection/baseline/pack.sig
node scripts/rule-pack.mjs verify --public src-tauri/keys/rule-pack.pub --pack src-tauri/crates/windows-platform/src/protection/baseline/pack.json --sig src-tauri/crates/windows-platform/src/protection/baseline/pack.sig
```
