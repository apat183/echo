# Release Checklist

Echo currently ships as a private, unsigned macOS build.

## Before Merging

1. Confirm versions match:

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

3. Build a local DMG on Apple Silicon:

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

Merge the release-ready branch to `main`. The release workflow runs on every
push to `main`, creates or reuses a commit tag named `build-<shortsha>`, and
publishes a GitHub Release named `Echo build-<shortsha>` as the latest release.

The workflow builds unsigned Apple Silicon and Intel DMGs, then uploads both DMG
assets to the published release. No manual `v*` tag or draft review step is
required for the current private release path.

After the workflow finishes:

1. Open GitHub Releases and confirm the latest release is named
   `Echo build-<shortsha>`.
2. Confirm both Apple Silicon and Intel DMG assets are attached.
3. Download one DMG, install locally, and confirm the unsigned macOS approval
   flow is the only expected launch warning.

## Unsigned Build Notes

The release workflow uses ad-hoc macOS signing (`APPLE_SIGNING_IDENTITY=-`). This
is enough for a private build artifact, but users will still see macOS warnings
because the app is not Developer ID signed or notarized.

For public distribution, replace ad-hoc signing with Developer ID signing and
notarization secrets before publishing releases.
