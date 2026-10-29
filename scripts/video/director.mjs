// What the viewer sees besides the app: a drawn mouse pointer (the screencast has none), click
// ripples, chapter captions and title cards. Moves are eased and paced so they can be followed.

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** Runs in the page: overlay elements above everything, ignoring the mouse. */
function overlay() {
  if (document.getElementById('video-cursor')) return
  const style = document.createElement('style')
  style.textContent = `
    #video-cursor { position: fixed; z-index: 2147483647; pointer-events: none; width: 22px; height: 22px; left: 0; top: 0;
      transform: translate(640px, 360px); filter: drop-shadow(0 1px 2px rgb(0 0 0 / .35)); }
    .video-ripple { position: fixed; z-index: 2147483646; pointer-events: none; width: 34px; height: 34px; margin: -17px 0 0 -17px;
      border-radius: 50%; border: 2px solid #2F7A78; animation: video-ripple .55s ease-out forwards; }
    @keyframes video-ripple { from { transform: scale(.3); opacity: .9 } to { transform: scale(1.4); opacity: 0 } }
    #video-caption { position: fixed; z-index: 2147483645; pointer-events: none; left: 28px; bottom: 26px; padding: 10px 18px;
      border-radius: 10px; background: #2F7A78; color: white; font: 600 17px/1.2 'Inter Variable', Inter, sans-serif;
      box-shadow: 0 6px 24px rgb(0 0 0 / .18); opacity: 0; transform: translateY(8px); transition: opacity .4s, transform .4s; }
    #video-caption.show { opacity: 1; transform: none; }
    #video-card { position: fixed; inset: 0; z-index: 2147483644; pointer-events: none; display: flex; flex-direction: column;
      align-items: center; justify-content: center; gap: 14px; background: #F7F5F2; opacity: 0; transition: opacity .6s; }
    #video-card.show { opacity: 1; }
    #video-card h1 { font: 400 64px/1.1 'Libertinus Serif', serif; color: #1f2a2a; margin: 0; }
    #video-card p { font: 400 21px/1.4 'Inter Variable', Inter, sans-serif; color: #5b6463; margin: 0; }
  `
  document.head.append(style)
  const cursor = document.createElement('div')
  cursor.id = 'video-cursor'
  cursor.innerHTML =
    '<svg viewBox="0 0 22 22" width="22" height="22"><path d="M3 2 L3 18 L7.2 14.2 L10 20.5 L12.8 19.3 L10.1 13.2 L15.8 13.2 Z" fill="#111" stroke="#fff" stroke-width="1.4" stroke-linejoin="round"/></svg>'
  const caption = document.createElement('div')
  caption.id = 'video-caption'
  const card = document.createElement('div')
  card.id = 'video-card'
  document.body.append(cursor, caption, card)
  window.addEventListener('mousemove', (e) => (cursor.style.transform = `translate(${e.clientX - 3}px, ${e.clientY - 2}px)`), true)
  window.addEventListener(
    'mousedown',
    (e) => {
      const r = document.createElement('div')
      r.className = 'video-ripple'
      r.style.left = `${e.clientX}px`
      r.style.top = `${e.clientY}px`
      document.body.append(r)
      setTimeout(() => r.remove(), 700)
    },
    true,
  )
}

export class Director {
  constructor(page, { dry }) {
    this.page = page
    this.dry = dry
    this.x = 640
    this.y = 360
    /** Epoch seconds when the current shot's voice ends (set by the recorder). */
    this.until = 0
  }

  async install() {
    await this.page.evaluate(overlay)
  }

  async wait(ms) {
    await sleep(ms)
  }

  /** Waits until `seconds` before the voice of this shot ends (to time the last action). */
  async untilVoiceEnds(seconds = 0) {
    const left = this.until - seconds - Date.now() / 1000
    if (left > 0) await this.wait(left * 1000)
  }

  async moveTo(x, y, ms = 650) {
    const steps = Math.max(8, Math.round((this.dry ? 120 : ms) / 16))
    const [x0, y0] = [this.x, this.y]
    for (let i = 1; i <= steps; i++) {
      const t = i / steps
      const e = t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2
      await this.page.mouse.move(x0 + (x - x0) * e, y0 + (y - y0) * e)
      await sleep(this.dry ? 0 : 16)
    }
    this.x = x
    this.y = y
  }

  /** A locator, or a CSS / text selector. */
  locate(target) {
    return typeof target === 'string' ? this.page.locator(target).first() : target.first()
  }

  async point(target, { ms = 650, dx = 0.5, dy = 0.5 } = {}) {
    const el = this.locate(target)
    await el.waitFor({ state: 'visible', timeout: this.dry ? 6000 : 20000 })
    // Near the edges a sticky footer or a toast may cover it: bring it to the middle, smoothly.
    let box = await el.boundingBox()
    const h = await this.page.evaluate(() => window.innerHeight)
    if (box.y < 70 || box.y + box.height > h - 90) {
      await el.evaluate((e) => e.scrollIntoView({ block: 'center', behavior: 'smooth' }))
      await sleep(700)
      box = await el.boundingBox()
    }
    await this.moveTo(box.x + box.width * dx, box.y + box.height * dy, ms)
    return el
  }

  async hover(target, ms = 1200, opts) {
    await this.point(target, opts)
    await this.wait(ms)
  }

  async click(target, opts = {}) {
    await this.point(target, opts)
    await this.wait(opts.before ?? 250)
    await this.page.mouse.down()
    await sleep(70)
    await this.page.mouse.up()
    await this.wait(opts.after ?? 600)
  }

  async type(text, delay = 70) {
    await this.page.keyboard.type(text, { delay: this.dry ? 0 : delay })
  }

  async press(keys) {
    await this.page.keyboard.press(keys)
  }

  /** Smooth wheel scrolling over the element (or wherever the pointer is). */
  async scroll(dy, ms = 1400) {
    const steps = Math.max(6, Math.round((this.dry ? 100 : ms) / 30))
    for (let i = 0; i < steps; i++) {
      await this.page.mouse.wheel(0, dy / steps)
      await sleep(this.dry ? 0 : 30)
    }
    await this.wait(300)
  }

  /** Navigates like a click on the sidebar entry would. */
  async nav(label) {
    await this.click(this.page.locator('aside').first().getByRole('link', { name: label, exact: true }))
  }

  async caption(text, ms = 3600) {
    await this.page.evaluate((t) => {
      const c = document.getElementById('video-caption')
      c.textContent = t
      c.classList.add('show')
    }, text)
    await sleep(this.dry ? 0 : ms)
    await this.page.evaluate(() => document.getElementById('video-caption').classList.remove('show'))
  }

  async card(title, line) {
    await this.page.evaluate(
      ([t, l]) => {
        const c = document.getElementById('video-card')
        c.innerHTML = ''
        const h = document.createElement('h1')
        h.textContent = t
        const p = document.createElement('p')
        p.textContent = l
        c.append(h, p)
        c.classList.add('show')
        document.getElementById('video-cursor').style.opacity = '0'
      },
      [title, line],
    )
  }

  async hideCard() {
    await this.page.evaluate(() => {
      document.getElementById('video-card').classList.remove('show')
      document.getElementById('video-cursor').style.opacity = '1'
    })
  }
}
