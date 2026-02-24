# Code Signing & Notarization

## Status

Currently the macOS build is **unsigned** (ad-hoc signed). The Homebrew Cask postflight removes quarantine via `xattr -cr` so users don't have to do it manually, but Gatekeeper will still warn about an unidentified developer if the app is opened outside of Homebrew.

## What's needed

1. **Apple Developer Organization enrollment** — register a legal entity (e.g. LLC) and enroll at developer.apple.com as an Organization ($99/yr). This gives a team name like "Airlock HQ LLC" instead of a personal name in Gatekeeper dialogs.

2. **Developer ID Application certificate** — generated from the org account, used to sign the .app bundle and embedded binaries.

3. **Notarization** — submit the signed DMG to Apple for notarization so Gatekeeper trusts it without quarantine removal.

## Changes required

Once the org enrollment is ready, update the release workflow (`.github/workflows/release-please.yml`):

1. **Add back certificate import step** — import the .p12 into a temporary keychain
2. **Sign with Developer ID** — replace ad-hoc `codesign --sign -` with `codesign --sign "$APPLE_SIGNING_IDENTITY" --options runtime --timestamp`
3. **Add notarization step** — `xcrun notarytool submit --wait` + `xcrun stapler staple`
4. **Remove quarantine workaround** — remove `xattr -cr` from Cask postflight and workflow Cask template

### GitHub secrets to add

| Secret                       | Purpose                                                     |
| ---------------------------- | ----------------------------------------------------------- |
| `APPLE_CERTIFICATE`          | Base64 Developer ID Application .p12                        |
| `APPLE_CERTIFICATE_PASSWORD` | .p12 password                                               |
| `APPLE_SIGNING_IDENTITY`     | e.g. `"Developer ID Application: Airlock HQ LLC (TEAM_ID)"` |
| `APPLE_ID`                   | Apple ID email for notarization                             |
| `APPLE_PASSWORD`             | App-specific password for notarization                      |
| `APPLE_TEAM_ID`              | Developer Team ID                                           |
