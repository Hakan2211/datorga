# Releasing a new Theralo version

A push to `main` changes nothing for therapists. They get a change only through a **release**:
the installer + `latest.json` on the GitHub releases of `Hakan2211/theralo-landing`. The landing
page's download button (`releases/latest/download/theralo-setup-x64.exe`) and the app's updater
(`releases/latest/download/latest.json`) both point there. Installed apps (0.2.0 and later) see
the new version at their next start and update in place; their data stays (PLAN-AMENDMENTS §30).

## Steps (from `D:\Datorga`, PowerShell)

1. **Slice done**: tests green, committed, pushed to `main` (the usual after-slice routine).
2. **Pick the version** (semver: `0.x.y`; new features → minor, fixes only → patch):
   `node scripts/release.mjs bump 0.3.0`
3. **Write the release notes** in `docs/releases/0.3.0.md`: German, for the therapist, plain text
   with `- ` lines, only what she notices (no slice numbers, no tech terms). Start with
   `Neu in Theralo 0.3.0`. They appear in the app's update dialog and on the GitHub release.
4. **Commit + push**: `git add Cargo.toml Cargo.lock docs/releases/0.3.0.md` →
   `git commit -m "Theralo 0.3.0: version and release notes"` → `git push origin main`.
5. **Build (signed)**: `cd ui; npm run app:build` (runs `scripts/release.mjs build`; the user's
   release app must be closed).
6. **Prepare**: `node scripts/release.mjs prepare` → `target/release/upload/` with
   `theralo-setup-x64.exe` + `latest.json`.
7. **Publish**: `node scripts/release.mjs publish` (uses `gh`, logged in as Hakan2211) →
   creates release `v0.3.0` on theralo-landing. Check afterwards:
   `curl -sL https://github.com/Hakan2211/theralo-landing/releases/latest/download/latest.json`
   must show the new version.

Alternative to 5–7: `git tag v0.3.0; git push origin v0.3.0` runs `.github/workflows/release.yml`
(needs the repo secrets `TAURI_SIGNING_PRIVATE_KEY` and `RELEASE_TOKEN`; untested until its first run).

## Rules

- **The updater key** `C:\Users\User\.tauri\theralo-updater.key` (no password) signs every
  release. Never commit, print or upload it. Without it no update can ever be released; with a
  new key, installed apps reject updates (their public key is in `crates/app/tauri.conf.json`).
- The version lives only in `Cargo.toml` `[workspace.package]`; never add one to `tauri.conf.json`.
- Never reuse or delete a published version: always bump. A release without `latest.json` breaks
  the updater for everyone (`releases/latest` would point at it).
- A database change = a new migration `Vn__name.sql` appended to `MIGRATIONS` (never edit an old
  one). The app copies the database to `vor-update/` before migrating; an older app refuses a
  newer database, so a release can't be undone by reinstalling the old version.
- The installer is not code-signed (user decision): SmartScreen warns on the first manual
  download only.
