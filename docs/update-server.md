# Torch update server guide

This guide describes a small HTTPS update server for Torch, starting with the macOS app. It is based on the repository at commit `9fcef29`, inspected on 20 October 2026.

**The server can be simple: a version JSON file plus signed app downloads. Torch needs a one-time updater integration before it can use it.** This document is a setup plan; it does not enable updates, deploy a server, or create a release pipeline.

## What you need

- A domain or subdomain you control, such as `updates.your-domain.com`.
- A Linux server with Docker Engine and the Compose plugin, or an existing static HTTPS host.
- A Mac with the Torch build dependencies and Xcode.
- Your own Apple Developer account, Developer ID Application signing certificate/private key, and Apple Team ID.
- A way to upload releases, initially SSH/SCP and later a release workflow.

Every `updates.example.com`, `deploy`, and `YOUR_TEAM_ID` below is a placeholder.

Recommended first setup: **Caddy serving static files on a small Linux server**. There is no database, login service, or application API to maintain. If you already have static HTTPS hosting, use the same file layout there and skip the Caddy section. Caddy supports static file serving and automatic certificate management. [Static files](https://caddyserver.com/docs/caddyfile/directives/file_server), [Automatic HTTPS](https://caddyserver.com/docs/automatic-https).

The intended flow is:

```text
Build Torch → sign and notarize → upload versioned DMGs
           → publish channel_versions.json last

Installed Torch → fetch JSON → download matching DMG
                → verify your signing identity → offer update/restart
```

## 1. Make Torch ready to use your server

This is development work that must happen once. Uploading files or passing `--features autoupdate` alone will not work with the current code.

| Current code | Required Torch change |
| --- | --- |
| [`app/src/bin/oss.rs`](../app/src/bin/oss.rs) sets `autoupdate_config: None`. | Configure your HTTPS release base URL and enable update menu items for distribution builds. |
| [`crates/torch_features/src/lib.rs`](../crates/torch_features/src/lib.rs) forces `Autoupdate` off under `local_only`. | Permit only Torch's independent updater in the intended release builds. Keep the other local-only restrictions. |
| [`app/src/features.rs`](../app/src/features.rs) controls the runtime flag from build features. | Make release feature selection and runtime policy agree; leave ordinary development builds out of polling. |
| [`app/src/autoupdate/channel_versions.rs`](../app/src/autoupdate/channel_versions.rs) calls the account-oriented server API before its JSON fallback. | For Torch, fetch the static JSON directly with a plain HTTP client. Do not route through login, token refresh, experiment headers, or Warp endpoints. |
| [`app/src/autoupdate/mod.rs`](../app/src/autoupdate/mod.rs) rejects `Channel::Oss` in version selection and asset paths. | Explicitly support Torch's existing `Oss` channel. For the layout below, map it to the JSON `stable` entry and `/stable/{version}` asset directory. |
| [`app/src/autoupdate/mac.rs`](../app/src/autoupdate/mac.rs) uses `warp-oss` names for `Oss`. | Use `Torch.app`, executable `torch`, and the DMG names below. Ensure bundled executable lookup resolves to `Contents/MacOS/torch`. |
| [`crates/torch_core/src/macos.rs`](../crates/torch_core/src/macos.rs) contains Warp's Apple Team ID. | Configure your Team ID and verify downloaded bundles against your identity. Never remove signature verification to make an update install. |
| [`script/macos/bundle`](../script/macos/bundle) and [`script/Entitlements.plist`](../script/Entitlements.plist) retain upstream release assumptions. | Create a Torch-specific distribution path: your identity, credentials, app names, necessary entitlements, and no upstream services. |

Keep `dev.tombyte.Torch`, the existing data locations, and `local_only`. Do not switch the whole application to Warp's `Stable` channel just to make updates work. The stable *feed* mapping can be explicit without changing the app's channel or identity.

The intended channel configuration is approximately:

```rust
autoupdate_config: Some(AutoupdateConfig {
    releases_base_url: "https://updates.example.com/torch".into(),
    show_autoupdate_menu_items: true,
}),
```

This is an integration sketch, not a complete patch. Gate it on your distribution configuration, add the import, and implement the other rows above.

The updater integration should also:

- Check HTTP status before parsing or saving responses; bound download size and timeout.
- Require HTTPS, including redirects, and accept downloads only from the configured release service.
- Verify a valid Apple-issued Developer ID signature from your Team ID, the expected bundle identity, and a bundle version matching the advertised version.
- Reject older versions unless a deliberate rollback policy allows them.
- Offer **Check for Updates** and an understandable update/restart action. Respect running processes and the app's existing quit confirmation; session restoration does not preserve arbitrary running shell jobs.
- Leave the installed app usable when a download, verification, or replacement fails.

For the first release, disable automatic update availability on Windows and Linux until their installers, URLs, signatures, and upgrade paths have been adapted and tested separately.

## 2. Agree on the release layout and versions

Use this layout after implementing the mappings above:

```text
public/
└── torch/
    ├── channel_versions.json
    └── stable/
        └── v0.2026.10.20.18.00.stable_01/
            ├── Torch-arm64.dmg
            ├── Torch.dmg
            ├── SHA256SUMS
            └── release-notes.md
```

- `Torch-arm64.dmg`: Apple Silicon build.
- `Torch.dmg`: universal build containing both Apple Silicon and Intel code. The inherited macOS selector requests this on Intel; do not rename an ARM-only build to this filename.
- Each DMG must contain **`Torch.app` at its root**.
- Version directories are immutable. Publish changed bytes under a new version.
- `SHA256SUMS` helps with upload verification; the existing updater does not consume it. A checksum beside a download is not a replacement for signature validation.
- `release-notes.md` is for people initially. Serving it does not automatically connect it to Torch's changelog UI.

The current version parser uses tags such as:

```text
v0.2026.10.20.18.00.stable_01
```

Use a consistently increasing UTC timestamp and increment the final counter if publishing again in the same minute. Plain `v0.2.0` requires changing the parser first. See [`crates/channel_versions/src/lib.rs`](../crates/channel_versions/src/lib.rs).

Example `channel_versions.json`:

```json
{
  "dev":     { "version": "v0.2026.10.20.18.00.stable_01" },
  "preview": { "version": "v0.2026.10.20.18.00.stable_01" },
  "stable":  { "version": "v0.2026.10.20.18.00.stable_01" }
}
```

The inherited JSON type requires all three entries. The proposed Torch mapping uses only `stable`; the other entries satisfy the schema, not three separate Torch release tracks.

Start with optional rollout/deadline fields omitted. For an Apple-Silicon-only pilot, explicitly restrict update eligibility to that architecture instead of advertising an update whose Intel download is missing.

## 3. Set up the HTTPS server

These commands are for the **Linux server**. Install Docker Engine and its Compose plugin first using the [official installation instructions](https://docs.docker.com/engine/install/).

1. Point your subdomain's DNS `A` record at the server. Add an `AAAA` record only if IPv6 actually works.
2. Allow inbound TCP ports **80 and 443**, plus your restricted SSH access.
3. Create a deployment directory owned by the SSH account you will use:

```sh
sudo install -d -o "$USER" -g "$(id -gn)" /opt/torch-updates
cd /opt/torch-updates
mkdir -p public/torch/stable incoming
```

Save this as `/opt/torch-updates/Caddyfile`, replacing the domain:

```caddyfile
updates.example.com {
    root * /srv

    @manifest path /torch/channel_versions.json
    header @manifest Cache-Control "no-store"

    @releases path /torch/stable/*
    header @releases Cache-Control "public, max-age=31536000, immutable"

    file_server
}
```

Save this as `/opt/torch-updates/compose.yaml`:

```yaml
services:
  caddy:
    image: caddy:2
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - ./public:/srv:ro
      - caddy_data:/data
      - caddy_config:/config

volumes:
  caddy_data:
  caddy_config:
```

The public directory is read-only to the web server. Keep signing keys and upload credentials outside it. Preserve the named volumes so certificate state survives container recreation. Once tested, pin the Caddy image to a specific tested version or digest for reproducible deployment. [Caddy deployment guidance](https://caddyserver.com/docs/running).

Start and validate it:

```sh
cd /opt/torch-updates
docker compose run --rm --no-deps caddy \
  caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
docker compose up -d
docker compose logs --tail=50 caddy
```

Before publishing the first release, a `404` at `/torch/channel_versions.json` is expected. Do not publish a real version announcement until its downloads exist.

The JSON should not be cached; versioned artifacts can be cached indefinitely because they never change. If you later add a CDN, preserve that distinction. [Caddy response headers](https://caddyserver.com/docs/caddyfile/directives/header).

## 4. Prepare a proper macOS release

The existing `./script/run --release` path builds an optimized app but still signs using an Apple Development identity or ad-hoc signing and debug entitlements. It is **not** a finished public distribution pipeline.

Build the one-time Torch release script around these steps:

1. Check out the exact commit being released and run the focused tests and build checks.
2. Set `GIT_RELEASE_TAG` during compilation, for example:

   ```sh
   export GIT_RELEASE_TAG=v0.2026.10.20.18.00.stable_01
   ```

   A Git tag alone is insufficient: `ChannelState::app_version()` reads the compiled environment value.

3. Build the `torch` binary and resources with `gui,local_only,release_bundle,extern_plist,autoupdate` after the integration in section 1. Make the built-in version, bundled metadata, and `Info.plist` consistent. The current plist script translates the inherited tag format; don't accidentally leave a static `0.1.0` version.
4. Produce the required architecture bundles. Audit all bundled native libraries/helpers as well as the main executable when building universal.
5. Include the existing copyright/licence notices, dependency notices, and matching source-release information. The developer run script currently skips generated third-party licences with `NO_LICENSES=1`; the distribution path should not inherit that shortcut.
6. Sign nested executable code from the inside out, then the app, using **your Developer ID Application identity**, hardened runtime, a secure timestamp, and reviewed release entitlements. Remove the inherited Warp application-group entitlement and the debug `get-task-allow` entitlement; don't copy either entitlement file blindly. See [Apple's signing guidance](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac/).
7. Notarize the signed app archive and staple the accepted ticket to the app before placing it in the DMG. Keep the notarization credentials in Keychain or protected CI secrets.

Example commands **after the release script has created and distribution-signed `dist/Torch.app`**:

```sh
# Run once, interactively; enter your Apple credentials when prompted.
xcrun notarytool store-credentials "torch-notary"

ditto -c -k --keepParent dist/Torch.app dist/Torch-notarization.zip
xcrun notarytool submit dist/Torch-notarization.zip \
  --keychain-profile "torch-notary" --wait

# Continue only when the submission reports Accepted.
xcrun stapler staple dist/Torch.app
xcrun stapler validate dist/Torch.app
codesign --verify --deep --strict --verbose=2 dist/Torch.app
spctl --assess --type execute --verbose=2 dist/Torch.app
```

Package that app at the DMG root, sign the DMG, submit it with `notarytool`, and staple the accepted DMG ticket. Generate `SHA256SUMS` **after** all signing and stapling, since those operations change bytes. Repeat for each shipped architecture. [Apple's notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).

Store the signing private key and notarization credentials securely with a recovery copy. The update server needs only the public artifacts; it does not need your Apple credentials.

## 5. Publish an update

The important rule is **upload artifacts first, publish the JSON last**. This prevents installed apps discovering an update that is only half uploaded.

On your **Mac**, after both final DMGs are in `dist/`:

```sh
export TORCH_VERSION=v0.2026.10.20.18.00.stable_01
(
  cd dist
  shasum -a 256 Torch-arm64.dmg Torch.dmg > SHA256SUMS
)

ssh deploy@updates.example.com \
  "mkdir -p /opt/torch-updates/incoming/$TORCH_VERSION"
scp dist/Torch-arm64.dmg dist/Torch.dmg dist/SHA256SUMS \
  "deploy@updates.example.com:/opt/torch-updates/incoming/$TORCH_VERSION/"
```

Upload release notes to that incoming directory too. On the **server**, verify and promote the directory:

```sh
set -eu
TORCH_VERSION=v0.2026.10.20.18.00.stable_01
cd "/opt/torch-updates/incoming/$TORCH_VERSION"
sha256sum -c SHA256SUMS
test ! -e "/opt/torch-updates/public/torch/stable/$TORCH_VERSION"
cd /opt/torch-updates
mv "incoming/$TORCH_VERSION" "public/torch/stable/$TORCH_VERSION"
```

Before announcing it, fetch each artifact over HTTPS and confirm the downloaded checksum matches. On the server, compare against the checksums you already verified during upload:

```sh
set -eu
TORCH_VERIFY_DIR=$(mktemp -d)
cp "/opt/torch-updates/public/torch/stable/$TORCH_VERSION/SHA256SUMS" \
  "$TORCH_VERIFY_DIR/SHA256SUMS"
(
  cd "$TORCH_VERIFY_DIR"
  for artifact in Torch-arm64.dmg Torch.dmg; do
    curl --fail --show-error --location --proto '=https' --proto-redir '=https' \
      --output "$artifact" \
      "https://updates.example.com/torch/stable/$TORCH_VERSION/$artifact"
  done
  sha256sum -c SHA256SUMS
)
```

Remove that temporary verification directory when you have finished checking it.

Prepare the new JSON shown in section 2 as `/opt/torch-updates/incoming/channel_versions.json`. Validate it with Python 3 and replace the public manifest in one rename:

```sh
set -eu
cd /opt/torch-updates
python3 -m json.tool incoming/channel_versions.json >/dev/null
# Save the old manifest outside the public tree if one exists.
if [ -f public/torch/channel_versions.json ]; then
  cp public/torch/channel_versions.json incoming/channel_versions.previous.json
fi
mv incoming/channel_versions.json public/torch/channel_versions.json

curl --fail --show-error --silent \
  https://updates.example.com/torch/channel_versions.json
curl --fail --head https://updates.example.com/torch/channel_versions.json
```

Use the same filesystem for `incoming/` and `public/` so the renames are atomic. Confirm the response contains the intended version and `Cache-Control: no-store`.

Changing the JSON is the release announcement. A Git push or tag does not update installed apps by itself.

## 6. Bootstrap and test the first upgrade

Your currently installed development build cannot acquire the updater automatically. Install the first updater-enabled, signed Torch release manually once, preferably into `/Applications/Torch.app`.

Then test a real **A → B** upgrade:

1. Install release A; confirm its compiled version and ordinary terminal behavior.
2. Publish a newer B to a separate staging feed wired into a staging build.
3. Check for updates; verify the app requests only your release service and downloads the correct architecture.
4. Confirm the update action and relaunch. Check the installed version, app identity, signature, projects, themes, groups, and restored session directories.
5. Test a failed download, invalid JSON, and a bundle signed by the wrong identity. None should replace the working app.
6. Test cancellation with a running shell command. Also test a non-writable installation location and a path containing spaces.
7. With B installed, point the feed back at A without rollback permission; it should stay on B.

Do this on every supported architecture and a clean Mac user account before broad release. Server responses alone do not validate the updater.

The existing `WARP_CHANNEL_VERSIONS_PATH` override can help development tests by loading a local manifest. It does not by itself change artifact URLs, signing checks, or update eligibility.

## 7. Make future releases easy

Once the manual A → B test passes, put the same steps into a **Torch-owned** release workflow:

1. Trigger from a release tag or manual release action.
2. Build the exact tagged commit on macOS, with that version passed into compilation.
3. Test, package, sign, and notarize.
4. Upload immutable artifacts and verify their public URLs/checksums.
5. Publish the manifest only after all required artifacts pass.
6. Retain previous releases and matching source archives.

Only the protected release job should receive signing and deployment credentials. Prevent concurrent jobs from publishing an older manifest over a newer one. Do not restore upstream Warp release or sync workflows.

For a bad release, prefer publishing a fixed build with a **higher** version. Restoring an older manifest stops further upgrades but normally does not downgrade users already on the bad version. The inherited `is_rollback` field permits downgrades; use it only after explicitly testing configuration/database compatibility.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| No update menu or no network request | Runtime flag, local-only updater exception, distribution config, and polling startup. |
| “No version tag set” | `GIT_RELEASE_TAG` was missing during compilation. |
| OSS channel unsupported error | `fetch_version` and asset-directory mapping still reject `Channel::Oss`. |
| Download returns 404 | Filename, architecture, exact version path, or manifest published before artifacts. |
| Signature verification fails | Your Team ID, nested code signatures, entitlements, expected executable path, and whether bytes changed after signing. |
| Download works but installation fails | Installation permissions, DMG root layout, executable name, and replacement/relaunch errors. |
| Old release still advertised | Manifest cache headers, CDN caching, or a stale publishing job. |
| macOS blocks first launch | Developer ID signing, notarization acceptance, and stapled tickets. |

## First-time checklist

- [ ] Choose the update domain and hosting.
- [ ] Obtain your Developer ID certificate and Team ID.
- [ ] Implement the independent updater changes in section 1.
- [ ] Create a Torch release script with the correct versioning, signing, and DMGs.
- [ ] Deploy the HTTPS static server.
- [ ] Publish and manually install the first updater-enabled build.
- [ ] Verify a real A → B upgrade on staging.
- [ ] Automate artifact upload and manifest publication.

After that, the routine is: **release a tested build, let the pipeline publish it, then update Torch from inside the app.**
