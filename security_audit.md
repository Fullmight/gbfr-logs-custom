# Brief Security Audit

Date: 2026-07-18

## Executive summary

No known malware package, covert telemetry, remote command-and-control code, or silent log upload was found in the checked source and locked dependency metadata. This is a source/dependency audit, not a guarantee that every third-party package is benign.

The application has one intentional, user-triggered data transfer: clicking the calculator action serializes a player's log data into a URL and opens `https://relink-damage.vercel.app/`. The updater contains a GitHub endpoint but is disabled. Game events otherwise move between the injected hook and desktop app over a local-only Windows named pipe.

The main concern is dependency age. The live npm audit reported 29 advisories in the full development graph (2 critical, 17 high, 9 moderate, 1 low), of which 5 are in the production graph (4 high, 1 moderate). RustSec reported 11 vulnerabilities in the 547-package Cargo lockfile. These are vulnerability advisories, not malware findings.

## Findings

### SEC-01 — Known vulnerable dependencies

- Severity: High for maintenance priority; actual exposure varies by code path.
- Location: `package.json:17-57`, `package-lock.json`, `src-tauri/Cargo.toml:15-38`, and `Cargo.lock`.
- Evidence: `npm audit` found 29 total advisories and `npm audit --omit=dev` found 5 production advisories. Production findings are the React Router chain, Lodash through Mantine/Recharts, and Babel runtime. `cargo audit` found: `bytes` RUSTSEC-2026-0007; `crossbeam-channel` RUSTSEC-2025-0024; `crossbeam-epoch` RUSTSEC-2026-0204; `idna` RUSTSEC-2024-0421; `quick-xml` RUSTSEC-2026-0194 and -0195; `rkyv` RUSTSEC-2026-0001; `tar` RUSTSEC-2026-0067 and -0068; `time` RUSTSEC-2026-0009; and `tracing-subscriber` RUSTSEC-2025-0055.
- Impact: Several advisories concern denial of service, memory unsafety, redirects/XSS, file handling, or development-server compromise. The critical npm items are old Vitest issues and are development-only. The source scan did not find untrusted redirect use matching the React Router advisories, but retaining affected versions is unnecessary risk.
- Fix: Upgrade the frontend toolchain and runtime packages in controlled groups, then update Cargo dependencies—especially the Tauri 1.x stack—and rerun tests plus both audits. Do not use an unreviewed forced major-version audit fix.
- Mitigation: Do not expose Vite/Vitest development servers to untrusted networks, and use `npm ci` from the committed lockfile in CI.
- False-positive notes: Some Cargo findings are target-specific or only reachable through narrow APIs. RustSec audits every locked package, including packages for non-Windows targets.

### SEC-02 — Player log data is sent to an external calculator after a user action

- Severity: Low (intentional behavior with privacy implications).
- Location: `src/utils.ts:209-214` and the click path at `src/pages/logs/View.tsx:222-224,585`.
- Evidence: `openDamageCalculator` serializes the complete `PlayerData` object and places it in the `logsdata` query parameter for `https://relink-damage.vercel.app/`.
- Impact: When the user clicks the calculator button, the serialized player record is disclosed to that site and may also appear in browser history, logs, and URL handling infrastructure. This is not background telemetry.
- Fix: Label the action as opening a third-party site and describe what is shared; optionally require confirmation before first use. Prefer a fragment or another design that avoids server-visible query data if the calculator supports it.
- Mitigation: Streamer mode should be checked to ensure names are removed before export if player names are considered sensitive.
- False-positive notes: The destination host is hard-coded, so this is not an open-redirect path.

### SEC-03 — Desktop webview hardening is permissive

- Severity: Low.
- Location: `src-tauri/tauri.conf.json:15-17,34-35,69`.
- Evidence: shell URL opening and all path APIs are enabled, and CSP is `null`.
- Impact: This does not itself phone home, but it increases the impact of a future frontend injection bug. No dangerous HTML injection or dynamic-code sink was found in the application source.
- Fix: Add a Tauri-compatible CSP and reduce the path/shell allowlists to the operations actually used.
- Mitigation: Keep all outbound destinations constant or strictly allowlisted.

## Network and supply-chain observations

- The updater endpoint at `src-tauri/tauri.conf.json:89-95` points to the maintainer's raw GitHub `update.json`, but `active` is `false`; it should not be contacted by the updater.
- The only npm packages marked with install scripts are `esbuild@0.19.12` and optional `fsevents@2.3.3`. These are expected native-platform setup packages; the project itself defines no install lifecycle script.
- npm packages resolve to the npm registry with integrity metadata. Cargo registry packages have checksums; the three Tauri plugins are locked to commit `8cd4a398` from the official `tauri-apps/plugins-workspace` repository.
- No `fetch`, Axios, WebSocket, HTTP client, analytics, telemetry, webhook, Sentry, or remote-script-loading code was found in application source.
- The hook's pipe listener uses `accept_remote(false)` at `src-hook/src/lib.rs:43-48`, restricting it to the local machine.

## Checks run

- Source/config scan for URLs, network APIs, telemetry SDKs, dynamic code/HTML sinks, secrets, process execution, and IPC.
- Lifecycle-script and lockfile-origin inspection.
- `npm audit --json` and `npm audit --omit=dev --json` against the live npm advisory service.
- `cargo audit 0.22.2 --json` against RustSec database commit `b5fc89b8be99e96f79194d8a6f11e9b4143b99f0` (updated 2026-07-17).

