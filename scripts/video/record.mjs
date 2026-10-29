// Records the explainer video (slice 35) on the Demo-Praxis.
//
//   node record.mjs            voice-over + recording → out/thera-in-5-minuten.mp4 (+ .vtt)
//   node record.mjs --dry      no voice, no recording: runs the shots, one screenshot each (out/dry/)
//   node record.mjs --voice sage --keep
//
// The voice-over text comes from docs/video/skript.md; each shot lasts as long as its text. It
// needs the debug build (target/debug/thera-app.exe, for THERA_DATA_DIR) and the Vite dev server
// on :3000 (started here if it isn't running), ffmpeg/ffprobe on PATH, and an OpenAI key for the
// voice: OPENAI_API_KEY, else thera's own OpenAI key from the keychain. Only the script text is
// sent to OpenAI. AI steps inside the demo answer from a local mock (mock-llm.mjs), so the run
// needs no AI provider and shows the same results every time.
import { spawn, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright-core'
import { startMockLlm } from './mock-llm.mjs'
import { SHOTS } from './shots.mjs'
import { Director } from './director.mjs'

const HERE = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(HERE, '../..')
const OUT = path.join(HERE, 'out')
const SKRIPT = path.join(ROOT, 'docs/video/skript.md')
const APP = path.join(ROOT, 'target/debug/thera-app.exe')
const NAME = 'thera-in-5-minuten'
const CDP_PORT = 9333
/** CSS pixels; with the scale factor the frames are 1920 × 1080. */
const VIEW = { width: 1280, height: 720, scale: 1.5 }
const TTS = { model: 'gpt-4o-mini-tts', instructions: 'Sprich ruhig, freundlich und deutlich auf Deutsch, in gemächlichem Tempo, wie in einem Erklärvideo.' }
/** Silence before a shot's voice starts, and after it ends before the next shot. */
const LEAD = 0.4
const TAIL = 0.8

const args = process.argv.slice(2)
const flag = (name) => args.includes(`--${name}`)
const option = (name, fallback) => {
  const i = args.indexOf(`--${name}`)
  return i >= 0 ? args[i + 1] : fallback
}
const DRY = flag('dry')
const UNTIL = option('until', null)
/** Leaves the app running (CDP on 9333) to look around; implied by --until. */
const KEEP = flag('keep') || UNTIL !== null
const VOICE = option('voice', 'coral')

// ---------- script ----------

/** `## chapter`, `### id · title`, `> voice-over` lines. */
export function parseSkript(source) {
  const shots = []
  let chapter = null
  for (const raw of source.split(/\r?\n/)) {
    const line = raw.trimEnd()
    let m
    if ((m = line.match(/^## (.+)$/))) chapter = m[1].trim()
    else if ((m = line.match(/^### (\S+) · (.+)$/))) shots.push({ id: m[1], title: m[2].trim(), chapter, text: '' })
    else if (line.startsWith('> ') && shots.length) {
      const shot = shots[shots.length - 1]
      shot.text = (shot.text ? `${shot.text} ` : '') + line.slice(2).trim()
    }
  }
  for (const s of shots) if (!s.text) throw new Error(`Einstellung ${s.id} hat keinen Sprechertext`)
  return shots
}

// ---------- voice ----------

function run(cmd, cmdArgs, opts = {}) {
  const r = spawnSync(cmd, cmdArgs, { encoding: 'utf8', maxBuffer: 64 << 20, ...opts })
  if (r.status !== 0) throw new Error(`${cmd} ${cmdArgs.slice(0, 3).join(' ')} …: ${r.stderr || r.error}`)
  return r.stdout
}

function duration(file) {
  return Number(run('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', file]).trim())
}

/** thera's OpenAI key (Settings → Stimme) from the Windows credential store. */
function keychainKey() {
  const ps = `
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public static class TheraCred {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  public struct CREDENTIAL { public int Flags; public int Type; public string TargetName; public string Comment; public int WrittenLow; public int WrittenHigh; public int BlobSize; public IntPtr Blob; public int Persist; public int AttributeCount; public IntPtr Attributes; public string TargetAlias; public string UserName; }
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool CredRead(string target, int type, int flags, out IntPtr cred);
  [DllImport("advapi32.dll")] public static extern void CredFree(IntPtr cred);
  public static byte[] Read(string t) { IntPtr p; if (!CredRead(t, 1, 0, out p)) return null; var c = (CREDENTIAL)Marshal.PtrToStructure(p, typeof(CREDENTIAL)); var b = new byte[c.BlobSize]; Marshal.Copy(c.Blob, b, 0, b.Length); CredFree(p); return b; }
}
"@
$b = [TheraCred]::Read('llm-openai.de.datorga.thera'); if ($b) { [Convert]::ToBase64String($b) }`
  const out = run('powershell', ['-NoProfile', '-NonInteractive', '-Command', ps]).trim()
  if (!out) return null
  const bytes = Buffer.from(out, 'base64')
  // keyring stores UTF-16 on Windows; accept UTF-8 too.
  const utf16 = bytes.length > 1 && bytes[1] === 0
  return (utf16 ? bytes.toString('utf16le') : bytes.toString('utf8')).trim()
}

async function voice(shots) {
  const dir = path.join(OUT, 'voice')
  fs.mkdirSync(dir, { recursive: true })
  let key = process.env.OPENAI_API_KEY || null
  for (const shot of shots) {
    const hash = createHash('sha1').update(JSON.stringify([TTS, VOICE, shot.text])).digest('hex').slice(0, 10)
    shot.audio = path.join(dir, `${shot.id}-${hash}.mp3`)
    if (!fs.existsSync(shot.audio)) {
      key ??= keychainKey()
      if (!key) throw new Error('Kein OpenAI-Schlüssel: OPENAI_API_KEY setzen oder in thera unter Einstellungen → Stimme hinterlegen')
      process.stdout.write(`  Stimme: ${shot.id} … `)
      const res = await fetch('https://api.openai.com/v1/audio/speech', {
        method: 'POST',
        headers: { Authorization: `Bearer ${key}`, 'Content-Type': 'application/json' },
        body: JSON.stringify({ model: TTS.model, voice: VOICE, input: shot.text, instructions: TTS.instructions, response_format: 'mp3' }),
      })
      if (!res.ok) throw new Error(`OpenAI ${res.status}: ${await res.text()}`)
      fs.writeFileSync(shot.audio, Buffer.from(await res.arrayBuffer()))
      console.log('ok')
    }
    shot.seconds = duration(shot.audio)
  }
}

// ---------- app ----------

async function reachable(url) {
  try {
    await fetch(url)
    return true
  } catch {
    return false
  }
}

async function ensureVite() {
  if (await reachable('http://localhost:3000')) return null
  console.log('Vite-Server starten …')
  const vite = spawn('npm', ['run', 'dev'], { cwd: path.join(ROOT, 'ui'), shell: true, stdio: 'ignore' })
  for (let i = 0; i < 60 && !(await reachable('http://localhost:3000')); i++) await sleep(1000)
  return vite
}

/** A fresh home with the real speech models hard-linked in (the demo shares them). It lives in
 * the temp folder: hard links need the same drive as %APPDATA%. */
function freshHome() {
  const home = path.join(os.tmpdir(), 'thera-video', `home-${Date.now()}`)
  fs.mkdirSync(path.join(home, 'models'), { recursive: true })
  const real = path.join(process.env.APPDATA ?? '', 'de.datorga.thera', 'models')
  if (fs.existsSync(real)) {
    for (const f of fs.readdirSync(real)) {
      try {
        fs.linkSync(path.join(real, f), path.join(home, 'models', f))
      } catch {
        // Another drive: the demo just shows "Kein Sprachmodell".
      }
    }
  }
  return home
}

function launch(home) {
  if (!fs.existsSync(APP)) throw new Error(`${APP} fehlt: erst "cargo build -p thera-app"`)
  return spawn(APP, ['--demo'], {
    env: {
      ...process.env,
      THERA_DATA_DIR: home,
      THERA_KEYRING_USER: 'video-master-key',
      WEBVIEW2_USER_DATA_FOLDER: path.join(home, 'webview'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
    },
    stdio: 'ignore',
  })
}

async function connect() {
  for (let i = 0; i < 90; i++) {
    try {
      const browser = await chromium.connectOverCDP(`http://127.0.0.1:${CDP_PORT}`)
      const page = browser.contexts()[0]?.pages()[0]
      if (page) return { browser, page }
      await browser.close()
    } catch {
      // not up yet
    }
    await sleep(1000)
  }
  throw new Error('thera antwortet nicht über CDP')
}

/** The demo's keychain entries (video-master-key → demo-video-master-key, + its AI keys). */
function cleanupKeychain() {
  const list = run('cmdkey', ['/list'])
  for (const m of list.matchAll(/(LegacyGeneric:target=\S*video-master-key\S*)/g)) spawnSync('cmdkey', [`/delete:${m[1]}`])
}

// ---------- recording ----------

function startScreencast(cdp, dir) {
  fs.mkdirSync(dir, { recursive: true })
  const frames = []
  cdp.on('Page.screencastFrame', ({ data, metadata, sessionId }) => {
    const file = path.join(dir, `${String(frames.length).padStart(6, '0')}.jpg`)
    fs.writeFileSync(file, Buffer.from(data, 'base64'))
    frames.push({ file, t: metadata.timestamp })
    cdp.send('Page.screencastFrameAck', { sessionId }).catch(() => {})
  })
  return {
    frames,
    start: () => cdp.send('Page.startScreencast', { format: 'jpeg', quality: 92, maxWidth: 1920, maxHeight: 1080, everyNthFrame: 1 }),
    stop: () => cdp.send('Page.stopScreencast'),
  }
}

/** Frames arrive only when something changes: each one stays until the next. */
function buildVideo(frames, end, dir) {
  const list = path.join(dir, 'frames.txt')
  const lines = []
  frames.forEach((f, i) => {
    const next = i + 1 < frames.length ? frames[i + 1].t : end
    lines.push(`file '${f.file.replaceAll('\\', '/')}'`, `duration ${Math.max(0.001, next - f.t).toFixed(4)}`)
  })
  lines.push(`file '${frames[frames.length - 1].file.replaceAll('\\', '/')}'`)
  fs.writeFileSync(list, lines.join('\n'))
  const video = path.join(dir, 'video.mp4')
  run('ffmpeg', ['-y', '-v', 'error', '-f', 'concat', '-safe', '0', '-i', list, '-vf', 'fps=30,scale=1920:1080:flags=lanczos,format=yuv420p', '-c:v', 'libx264', '-preset', 'slow', '-crf', '18', video])
  return video
}

/** The voice is padded with silence: the picture (end card) decides the length. */
function mux(video, shots, out) {
  const inputs = shots.flatMap((s) => ['-i', s.audio])
  const filters = shots.map((s, i) => `[${i + 1}:a]adelay=${Math.round(s.voiceAt * 1000)}:all=1[a${i}]`)
  const mix = `${shots.map((_, i) => `[a${i}]`).join('')}amix=inputs=${shots.length}:normalize=0:dropout_transition=0,apad[voice]`
  run('ffmpeg', ['-y', '-v', 'error', '-i', video, ...inputs, '-filter_complex', [...filters, mix].join(';'), '-map', '0:v', '-map', '[voice]', '-c:v', 'copy', '-c:a', 'aac', '-b:a', '160k', '-movflags', '+faststart', '-shortest', out])
}

function vtt(shots, out) {
  const ts = (s) => {
    const ms = Math.round(s * 1000)
    const p = (n, w = 2) => String(n).padStart(w, '0')
    return `${p(Math.floor(ms / 3600000))}:${p(Math.floor(ms / 60000) % 60)}:${p(Math.floor(ms / 1000) % 60)}.${p(ms % 1000, 3)}`
  }
  const cues = []
  for (const s of shots) {
    // One cue per sentence, timed by its share of the characters.
    const sentences = s.text.match(/[^.!?]+[.!?]+["“”„]?/g) ?? [s.text]
    const total = sentences.reduce((n, x) => n + x.length, 0)
    let at = s.voiceAt
    for (const sentence of sentences) {
      const len = (sentence.length / total) * s.seconds
      cues.push(`${ts(at)} --> ${ts(at + len)}\n${sentence.trim()}`)
      at += len
    }
  }
  fs.writeFileSync(out, `WEBVTT\n\n${cues.join('\n\n')}\n`)
}

// ---------- main ----------

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function main() {
  const shots = parseSkript(fs.readFileSync(SKRIPT, 'utf8'))
  for (const s of shots) if (!SHOTS[s.id]) throw new Error(`shots.mjs kennt die Einstellung ${s.id} nicht`)
  if (DRY) for (const s of shots) s.seconds = s.text.split(/\s+/).length / 2.4
  else await voice(shots)
  const total = shots.reduce((n, s) => n + LEAD + s.seconds + TAIL, 0)
  console.log(`${shots.length} Einstellungen, etwa ${Math.floor(total / 60)}:${String(Math.round(total % 60)).padStart(2, '0')} min`)

  const vite = await ensureVite()
  const mock = await startMockLlm()
  const home = freshHome()
  const app = launch(home)
  let browser
  try {
    let page
    ;({ browser, page } = await connect())
    await page.waitForLoadState('domcontentloaded')
    await page.evaluate(() => localStorage.setItem('thera.tour', 'skipped'))
    await page.reload()
    await page.waitForSelector('nav, [data-tour="nav-dashboard"]', { timeout: 30000 })
    const cdp = await page.context().newCDPSession(page)
    await cdp.send('Emulation.setDeviceMetricsOverride', { width: VIEW.width, height: VIEW.height, deviceScaleFactor: VIEW.scale, mobile: false })
    await page.evaluate(
      (config) => window.__TAURI_INTERNALS__.invoke('llm_config_set', { config }),
      { active: 'custom', providers: { custom: { model: 'thera-demo', base_url: mock.url } } },
    )
    const director = new Director(page, { dry: DRY })
    await director.install()
    await sleep(1500)

    const runDir = path.join(OUT, 'run', `rec-${Date.now()}`)
    const cast = DRY ? null : startScreencast(cdp, path.join(runDir, 'frames'))
    await cast?.start()
    const t0 = Date.now() / 1000
    let chapter = null
    for (const shot of shots) {
      const start = Date.now() / 1000
      shot.voiceAt = start - t0 + LEAD
      if (shot.chapter !== chapter) {
        chapter = shot.chapter
        void director.caption(chapter)
      }
      console.log(`  ${shot.id} (${shot.seconds.toFixed(1)} s)`)
      director.until = start + LEAD + shot.seconds
      try {
        await SHOTS[shot.id](director, { mock })
      } catch (e) {
        // A dry run reports every broken shot in one go.
        if (!DRY) throw e
        console.log(`    ! ${e.message.split('\n').slice(0, 3).join(' / ')}`)
      }
      const left = start + LEAD + shot.seconds + TAIL - Date.now() / 1000
      if (DRY) {
        if (left < -1.5) console.log(`    ! ${(-left).toFixed(1)} s länger als der Text`)
        fs.mkdirSync(path.join(OUT, 'dry'), { recursive: true })
        await page.screenshot({ path: path.join(OUT, 'dry', `${shot.id}.png`) })
      } else if (left > 0) await sleep(left * 1000)
      if (shot.id === UNTIL) break
    }
    const end = Date.now() / 1000
    await cast?.stop()

    if (!DRY && !UNTIL) {
      console.log(`Video bauen (${cast.frames.length} Bilder) …`)
      const video = buildVideo(cast.frames, end, runDir)
      const out = path.join(OUT, `${NAME}.mp4`)
      mux(video, shots, out)
      vtt(shots, path.join(OUT, `${NAME}.de.vtt`))
      console.log(`Fertig: ${out} (${duration(out).toFixed(1)} s)`)
    }
  } finally {
    if (!KEEP) {
      await browser?.close().catch(() => {})
      app.kill()
      await sleep(1500)
      cleanupKeychain()
      mock.close()
      vite?.kill()
    }
  }
}

await main()
if (!KEEP) process.exit(0)
