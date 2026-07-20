# Android release signing

The Android release pipeline intentionally distinguishes **test-signed release candidates** from **release/update-signed builds**.

## Signing modes

### Untagged `Kotlin` builds and manual release-candidate runs

These use an ephemeral CI-generated test certificate. They are suitable for installation and validation only.

A test-signed APK must not be published as a Beta or stable GitHub release because a later build signed by a different certificate cannot update the same installed application ID.

### `android-v*` tags

Release tags are fail-closed and require the persistent Android release/update signing identity. The workflow refuses to produce a tagged release artifact when any required signing secret is missing.

Required GitHub Actions secrets:

- `MUKEI_ANDROID_RELEASE_KEYSTORE_B64` — base64-encoded release keystore bytes.
- `MUKEI_ANDROID_RELEASE_STORE_PASSWORD` — keystore password.
- `MUKEI_ANDROID_RELEASE_KEY_ALIAS` — signing key alias.
- `MUKEI_ANDROID_RELEASE_KEY_PASSWORD` — signing key password.

The keystore is decoded only into runner-temporary storage, used by Android `apksigner`, and deleted before the job ends. The keystore itself is never uploaded as an artifact.

## Release certificate custody

The externally retained source keystore is the update identity for `ai.mukei.android`.

Before creating the first public Beta tag:

1. Store the source keystore in durable, access-controlled backup storage outside GitHub Actions.
2. Record and independently back up the keystore password, key alias, and key password.
3. Provision the four GitHub Actions secrets listed above.
4. Create an `android-v*` release-candidate tag and require the `android-release-candidate` status to succeed.
5. Preserve the certificate fingerprint from `SIGNING-INFO.txt` as part of release records.
6. Install and validate the exact ARM64 artifact on a physical supported Android device before publishing it as a GitHub Beta release.

## Release gate

A Beta or stable release is blocked unless all of the following are true:

- the exact tagged commit passes Android/Rust/release-hardening CI;
- the `android-release-candidate` workflow reports **release-signed**, not test-signed;
- `apksigner` and `zipalign` verification pass;
- the published SHA-256 matches the tested APK;
- physical-device end-to-end validation passes on that exact artifact.

Do not regenerate the release keystore for later releases. Losing or replacing the signing identity breaks normal update continuity for existing installations.