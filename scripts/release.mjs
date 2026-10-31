// Release helper for Theralo (Windows). No dependencies.
//
//   node scripts/release.mjs bump 0.2.0   set the version (Cargo.toml workspace)
//   node scripts/release.mjs build        signed `tauri build` (also what `npm run app:build` runs)
//   node scripts/release.mjs prepare      target/release/upload/: theralo-setup-x64.exe + latest.json
//   node scripts/release.mjs publish      GitHub release v<version> on the landing repo (needs gh)
//
// The updater key: TAURI_SIGNING_PRIVATE_KEY (key text or path; CI secret), else
// ~/.tauri/theralo-updater.key. Release notes for the therapist: docs/releases/<version>.md.
import { execFileSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
/** Where the installer and `latest.json` are published; the landing page and the app's updater
 * both point at `releases/latest/download/` there. */
const RELEASE_REPO = 'Hakan2211/theralo-landing'
/** Stable name the landing page's download button links to. */
const ASSET = 'theralo-setup-x64.exe'
const UPLOAD = join(ROOT, 'target', 'release', 'upload')

const cargoToml = join(ROOT, 'Cargo.toml')

function version() {
  const m = readFileSync(cargoToml, 'utf8').match(/\[workspace\.package\][^[]*?\nversion = "([^"]+)"/)
  if (!m) throw new Error('version not found in Cargo.toml [workspace.package]')
  return m[1]
}

function bump(next) {
  if (!/^\d+\.\d+\.\d+$/.test(next ?? '')) throw new Error('usage: bump <major.minor.patch>')
  const text = readFileSync(cargoToml, 'utf8')
  const out = text.replace(/(\[workspace\.package\][^[]*?\nversion = ")[^"]+(")/, `$1${next}$2`)
  writeFileSync(cargoToml, out)
  const notes = join(ROOT, 'docs', 'releases', `${next}.md`)
  console.log(`version ${next}${existsSync(notes) ? '' : `; write the release notes in docs/releases/${next}.md`}`)
}

function build() {
  const env = { ...process.env }
  if (!env.TAURI_SIGNING_PRIVATE_KEY) {
    const key = join(homedir(), '.tauri', 'theralo-updater.key')
    if (!existsSync(key)) throw new Error(`no updater key: set TAURI_SIGNING_PRIVATE_KEY or put it at ${key}`)
    env.TAURI_SIGNING_PRIVATE_KEY = key
  }
  env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ??= ''
  const tauri = join(ROOT, 'ui', 'node_modules', '.bin', process.platform === 'win32' ? 'tauri.cmd' : 'tauri')
  execFileSync(tauri, ['build', ...process.argv.slice(3)], { cwd: join(ROOT, 'crates', 'app'), env, stdio: 'inherit', shell: true })
}

function notes(v) {
  const file = join(ROOT, 'docs', 'releases', `${v}.md`)
  return existsSync(file) ? readFileSync(file, 'utf8').trim() : ''
}

function prepare() {
  const v = version()
  const tag = process.env.GITHUB_REF_NAME
  if (tag && tag.startsWith('v') && tag !== `v${v}`) throw new Error(`tag ${tag} does not match Cargo.toml version ${v}`)
  const setup = join(ROOT, 'target', 'release', 'bundle', 'nsis', `Theralo_${v}_x64-setup.exe`)
  if (!existsSync(setup) || !existsSync(`${setup}.sig`)) throw new Error(`missing ${setup}(.sig): run "build" first`)
  mkdirSync(UPLOAD, { recursive: true })
  copyFileSync(setup, join(UPLOAD, ASSET))
  const latest = {
    version: v,
    notes: notes(v),
    pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    platforms: {
      'windows-x86_64': {
        signature: readFileSync(`${setup}.sig`, 'utf8').trim(),
        url: `https://github.com/${RELEASE_REPO}/releases/download/v${v}/${ASSET}`,
      },
    },
  }
  writeFileSync(join(UPLOAD, 'latest.json'), `${JSON.stringify(latest, null, 2)}\n`)
  console.log(`${UPLOAD}: ${ASSET}, latest.json (v${v})${latest.notes ? '' : ' (no release notes)'}`)
}

function publish() {
  const v = version()
  const files = [join(UPLOAD, ASSET), join(UPLOAD, 'latest.json')]
  for (const f of files) if (!existsSync(f)) throw new Error(`missing ${f}: run "prepare" first`)
  const latest = JSON.parse(readFileSync(files[1], 'utf8'))
  if (latest.version !== v) throw new Error(`latest.json is for ${latest.version}, Cargo.toml says ${v}: run "prepare" again`)
  const notesFile = join(UPLOAD, 'notes.md')
  writeFileSync(notesFile, latest.notes || `Theralo ${v}`)
  execFileSync('gh', ['release', 'create', `v${v}`, ...files, '--repo', RELEASE_REPO, '--title', `Theralo ${v}`, '--notes-file', notesFile], {
    stdio: 'inherit',
  })
}

const commands = { bump: () => bump(process.argv[3]), build, prepare, publish }
const command = commands[process.argv[2]]
if (!command) {
  console.error('usage: node scripts/release.mjs bump <version> | build | prepare | publish')
  process.exit(1)
}
command()
