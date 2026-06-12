# Release Checklist

Echo currently ships as a private, unsigned macOS build.

## Before Tagging

1. Confirm versions match:

   ```sh
   node -e "console.log(require('./package.json').version)"
   node -e "console.log(require('./src-tauri/tauri.conf.json').version)"
   ```

2. Run the local validation suite:

   ```sh
   bun install
   bun run test:coverage
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

## Tagging

```sh
git tag v0.1.0
git push origin v0.1.0
```

The release workflow creates a draft GitHub Release with unsigned Intel and Apple
Silicon DMGs. Review the draft, edit notes as needed, then publish it manually.

## Unsigned Build Notes

The release workflow uses ad-hoc macOS signing (`APPLE_SIGNING_IDENTITY=-`). This
is enough for a private build artifact, but users will still see macOS warnings
because the app is not Developer ID signed or notarized.

For public distribution, replace ad-hoc signing with Developer ID signing and
notarization secrets before publishing releases.
