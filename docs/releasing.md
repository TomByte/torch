# Releasing Torch

Publishing a GitHub release builds Torch for macOS, signs and notarizes it, and attaches the DMG to the release. While Torch is open, it checks the latest release every 10 minutes and offers the update in the app. There is no update server to run.

## How it works

1. You publish a release on GitHub tagged with the date, for example `v2026.10.23`.
2. [`.github/workflows/release.yml`](../.github/workflows/release.yml) builds a universal `Torch.app` on a GitHub macOS runner, signs it with your Developer ID certificate, notarizes it with Apple and packs it into `Torch.dmg`.
3. The workflow attaches three files to the release: `Torch.dmg`, `SHA256SUMS` and `channel_versions.json`. The JSON file goes up last, so the update is only announced once the download exists.
4. Torch reads `https://github.com/TomByte/torch/releases/latest/download/channel_versions.json`. When the version there is newer than its own, it downloads `Torch.dmg` from that release, checks that it's signed by your Apple team (`8M65369K4Z`, set in `crates/torch_core/src/macos.rs`), and offers to restart into it.

## Versions

Torch uses calendar versions: the tag is the release date, `vYYYY.MM.DD`. A second release on the same day adds a number, `v2026.10.23.2`, then `.3` and so on. The version says when a build was released, not what changed in it.

The tag is the version Torch shows in Settings (Finder shows it without the `v`), and the one it compares to find updates. The workflow refuses any other tag format before it starts building.

Prereleases and drafts are never offered as updates, because GitHub's "latest" release skips them. Use a prerelease to try a build before releasing it to everyone.

Updates are macOS-only. Linux and Windows builds don't check for updates, and the workflow doesn't build them.

## One-time setup

You need the [GitHub CLI](https://cli.github.com) (`brew install gh`, then `gh auth login`) and a Mac with your Developer ID certificate in its keychain. Run the commands from this repository's folder.

The workflow reads five secrets, stored in a `release` environment that only `main` and `v*` tags can deploy to. Secrets are never passed to workflows that run on pull requests from forks, and this workflow only runs when a release is published or when you start it by hand.

### 1. Signing certificate: `MACOS_CERTIFICATE_P12`, `MACOS_CERTIFICATE_PASSWORD`

Check that the certificate is installed:

```bash
security find-identity -v -p codesigning
```

You should see `Developer ID Application: C10UD LTD (8M65369K4Z)`. If it's missing, create one at [developer.apple.com/account/resources/certificates](https://developer.apple.com/account/resources/certificates/list) (type **Developer ID Application**), download it and double-click it to install.

Export it with its private key:

1. Open **Keychain Access**, choose the **login** keychain and the **My Certificates** tab.
2. Right-click **Developer ID Application: C10UD LTD**, choose **Export**, and save it as `torch-signing.p12`.
3. Set a password when asked. You'll store this password as a secret too.

Store both as secrets:

```bash
base64 -i torch-signing.p12 | gh secret set MACOS_CERTIFICATE_P12 --env release --repo TomByte/torch
```

```bash
gh secret set MACOS_CERTIFICATE_PASSWORD --env release --repo TomByte/torch
```

The second command asks for the password you set during export. Then delete the exported file:

```bash
rm torch-signing.p12
```

### 2. Notarization key: `APPLE_API_KEY_P8`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID`

1. Go to [App Store Connect → Users and Access → Integrations → App Store Connect API](https://appstoreconnect.apple.com/access/integrations/api) and open **Team Keys**. If it's your first key, click **Request Access** and wait for approval.
2. Click **Generate API Key** (or **+**), name it `Torch notarization` and give it the **Developer** role.
3. Download the key. It's a file named `AuthKey_<KEY ID>.p8`, and Apple only lets you download it once.
4. Note the **Key ID** shown next to the key and the **Issuer ID** shown above the list.

Check that the key works:

```bash
xcrun notarytool history --key AuthKey_ABC123XYZ.p8 --key-id ABC123XYZ --issuer 00000000-0000-0000-0000-000000000000
```

Replace the file name, Key ID and Issuer ID with yours. An empty list or a list of past submissions means it works.

Store all three as secrets:

```bash
gh secret set APPLE_API_KEY_P8 --env release --repo TomByte/torch < AuthKey_ABC123XYZ.p8
```

```bash
gh secret set APPLE_API_KEY_ID --env release --repo TomByte/torch --body ABC123XYZ
```

```bash
gh secret set APPLE_API_ISSUER_ID --env release --repo TomByte/torch --body 00000000-0000-0000-0000-000000000000
```

Keep the `.p8` file somewhere safe, such as your password manager, and delete it from your downloads.

### 3. Check the secrets

```bash
gh secret list --env release --repo TomByte/torch
```

You should see all five: `MACOS_CERTIFICATE_P12`, `MACOS_CERTIFICATE_PASSWORD`, `APPLE_API_KEY_P8`, `APPLE_API_KEY_ID` and `APPLE_API_ISSUER_ID`.

## Publishing a release

From the GitHub website: **Releases → Draft a new release**, create a tag with today's date, such as `v2026.10.23`, write the notes and click **Publish release**.

Or from the terminal:

```bash
gh release create "v$(date +%Y.%m.%d)" --generate-notes
```

If you've already released today, add the next number yourself, for example `gh release create v2026.10.23.2 --generate-notes`.

Follow the build under **Actions → Release**, or with:

```bash
gh run watch --repo TomByte/torch
```

The first build takes longest, because nothing is cached yet. When it finishes, the release shows `Torch.dmg`, `SHA256SUMS` and `channel_versions.json`.

To rebuild a release, for example after a failed run, start the workflow by hand with the release's tag. It replaces that release's files:

```bash
gh workflow run release.yml --repo TomByte/torch -f tag=v2026.10.23
```

## The first updater-enabled release

Builds from before this workflow can't update themselves. Install the first release made by this workflow by hand: open `Torch.dmg` and drag Torch into **Applications**. From then on, Torch updates itself.

To test the update path once, install release A, publish release B, then choose **Check for updates** from the command palette or Settings. It should download B and offer to restart.

## Optional settings

Set these under **Settings → Secrets and variables → Actions → Variables** if you need them:

| Variable | Use |
| --- | --- |
| `MACOS_RELEASE_RUNNER` | A different runner label, such as `macos-26-xlarge`, if the standard runner is too slow or runs out of memory. Larger runners are billed. |
| `TORCH_DMG_SKIP_FINDER` | Set to `1` if the build hangs while laying out the DMG window. The DMG then opens without the custom icon layout. |

## Troubleshooting

| Problem | Check |
| --- | --- |
| `Codesigning identity ... not found` | `MACOS_CERTIFICATE_P12` must contain the private key. Re-export from **My Certificates**, not **Certificates**. |
| `Notarization profile "torch" not found` | The three `APPLE_API_*` secrets. Run the `notarytool history` check above. |
| Notarization rejected | The job log prints Apple's report. Usually a nested binary that isn't signed. |
| Torch never offers the update | The release must be published, not a draft or prerelease, and must have `channel_versions.json` attached. Updates also only apply to copies installed from a release DMG. |
| Update downloads but won't install | Torch must be in a folder you can write to, such as `/Applications`, and the new build must be signed by the same team. |

## Building a release locally

`./script/release` builds the same signed, notarized DMG on your Mac without publishing it. It uses the certificate in your login keychain and a `notarytool` profile named `torch`, which you can create with:

```bash
xcrun notarytool store-credentials torch --key AuthKey_ABC123XYZ.p8 --key-id ABC123XYZ --issuer 00000000-0000-0000-0000-000000000000
```
