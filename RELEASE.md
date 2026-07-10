# Release Checklist

Echo ships as a Developer ID signed and notarized macOS build. Every push to
`main` runs the release workflow, which builds, signs, notarizes, and publishes a
GitHub Release that the in-app updater consumes.

## Before Merging

1. Confirm versions match (the committed version stays `0.1.0`; CI stamps the real
   `0.1.<run_number>` at build time):

   ```sh
   node -e "console.log(require('./package.json').version)"
   node -e "console.log(require('./src-tauri/tauri.conf.json').version)"
   ```

2. Run the local validation suite:

   ```sh
   bun install
   bun run test
   bun run build
   cd src-tauri
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```

3. Build a local DMG on Apple Silicon (local builds are unsigned and never need the
   signing keys):

   ```sh
   bun run tauri build --bundles dmg
   ```

4. Smoke-test the app:

   - Install from `src-tauri/target/release/bundle/dmg/Echo_*.dmg`.
   - Launch Echo from Applications.
   - Grant Accessibility permission when prompted.
   - Confirm the tray menu shows Today time, Pause/Resume, Open Echo, and Quit.
   - Confirm closing the window hides it without stopping tracking.
   - Confirm activity appears in day/week/month views.
   - Confirm project assignment, ignored rules, and period notes persist after restart.

## Publishing

Merge the release-ready branch to `main`. The release workflow
(`.github/workflows/release.yml`) runs on every push to `main` and:

1. Creates the tag `v0.1.<run_number>` and a **draft** GitHub Release named
   `Echo <version>`.
2. Builds the Apple Silicon (`aarch64-apple-darwin`) app, signs it with the
   Developer ID certificate, notarizes it via the App Store Connect API key, and
   staples the ticket. The DMG and updater artifacts (`Echo.app.tar.gz`, `.sig`)
   are uploaded to the release.
3. Composes `latest.json` (what installed apps poll for updates) and uploads it,
   then promotes the draft to published + latest.

If the build fails, the release stays a draft, so `releases/latest` never serves a
broken build.

## After the Workflow Finishes

1. Open GitHub Releases and confirm the latest release is named `Echo <version>`
   and is published (not a draft).
2. Confirm the DMG, `Echo.app.tar.gz`, `Echo.app.tar.gz.sig`, and `latest.json`
   assets are attached.
3. Download the DMG, install locally, and confirm it launches without a Gatekeeper
   warning.
4. On a previously installed copy, confirm the in-app updater offers and installs
   the new version.

## Signing Secrets

The release workflow relies on GitHub Actions secrets — Apple Developer ID
signing (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_SIGNING_IDENTITY`), App Store Connect notarization
(`APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_BASE64`), and the Tauri
updater signing key (`TAURI_SIGNING_PRIVATE_KEY`). These live only in repository
secrets; nothing sensitive is committed.
