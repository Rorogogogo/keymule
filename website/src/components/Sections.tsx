import { useEffect, useRef, useState } from 'react'
import { AnimatePresence, motion, useInView, useReducedMotion } from 'motion/react'
import { ClipboardCheck, Container, GitBranch, Globe, KeyRound, LayoutDashboard, Lock, ScanSearch, Terminal } from 'lucide-react'
import { Reveal, SectionHead, Tag, spotlight } from './ui'

const AGENTS = ['Claude Code', 'Codex', 'Cursor', 'Claude Desktop', 'Antigravity', 'gh', 'git', 'ssh', 'psql', 'GitHub Actions', 'MCP servers']

export function Agents() {
  const row = [...AGENTS, ...AGENTS]
  return (
    <section aria-label="Works with" className="relative py-10">
      <p className="text-center text-sm text-fg-3">Works with the agents and tools you already use</p>
      <div className="mt-6 flex overflow-hidden [mask-image:linear-gradient(90deg,transparent,black_15%,black_85%,transparent)]">
        <div className="marquee flex shrink-0 items-center gap-14 pr-14">
          {row.map((a, i) => (
            <span key={i} className="text-xl font-semibold tracking-tight whitespace-nowrap text-fg-3">
              {a}
            </span>
          ))}
        </div>
      </div>
    </section>
  )
}

const PAINS = [
  ['Pasted token', '“You shared an API key. Rotate it right away.”', 'You pasted it so the agent could check a pipeline. Now it’s in a provider log and a transcript on your disk.'],
  ['Refused', '“I can’t read your .env file.”', 'So you run the command yourself and paste the output back. Or the agent reads it anyway.'],
  ['Every session', 'The same token, again.', 'New session, new agent, new dev container: each one wants it through the clipboard and the chat.'],
  ['Plain text', 'Keys in ~/.claude.json.', 'MCP servers keep API keys in config files every agent can read, and every dotfile backup copies.'],
]

export function Pains() {
  return (
    <section className="mx-auto max-w-6xl px-5 py-24 sm:px-8 lg:py-32">
      <SectionHead kicker="Sound familiar?" title={<>Secrets keep ending up in the chat.</>} />
      <div className="mt-14 grid grid-cols-1 gap-4 md:grid-cols-2">
        {PAINS.map(([tag, quote, body], i) => (
          <Reveal key={tag} delay={i * 0.06}>
            <article onMouseMove={spotlight} className="spot glass h-full rounded-3xl p-7">
              <Tag tone="bad">{tag}</Tag>
              <h3 className="mt-5 text-2xl font-semibold tracking-[-0.025em]">{quote}</h3>
              <p className="mt-3 leading-relaxed text-fg-2">{body}</p>
            </article>
          </Reveal>
        ))}
      </div>
    </section>
  )
}

// One request, told in four beats. Loops.
const BEATS = [
  { who: 'You', tone: 'text-fg', text: 'why did my last CI run fail?' },
  { who: 'Agent', tone: 'text-accent', text: 'http_request(credential="github-acme", path="/repos/acme/app/actions/runs")', mono: true },
  { who: 'keymule', tone: 'text-ok', text: 'adds the token ●●●●●●●● · strips it from the reply' },
  { who: 'Agent', tone: 'text-accent', text: 'Run #482 failed in “integration tests”: a timeout in checkout.spec.ts.' },
]

function RequestCard() {
  const reduce = useReducedMotion()
  const ref = useRef<HTMLDivElement>(null)
  const inView = useInView(ref, { margin: '-100px' })
  const [step, setStep] = useState(reduce ? BEATS.length : 0)
  useEffect(() => {
    if (reduce || !inView) return
    const t = setTimeout(() => setStep(s => (s >= BEATS.length + 2 ? 0 : s + 1)), step === 0 ? 600 : 1500)
    return () => clearTimeout(t)
  }, [step, reduce, inView])
  const shown = Math.min(step, BEATS.length)
  return (
    <div ref={ref} className="glass rounded-3xl p-2 backdrop-blur-xl">
      <div className="rounded-[18px] border border-line bg-bg-2/80">
        <div className="flex items-center gap-2 border-b border-line px-5 py-3.5">
          <span className="size-2.5 rounded-full bg-white/15" />
          <span className="size-2.5 rounded-full bg-white/15" />
          <span className="size-2.5 rounded-full bg-white/15" />
          <span className="ml-3 font-mono text-[12px] text-fg-3">claude · ~/code/acme</span>
          <span className="ml-auto flex items-center gap-1.5 font-mono text-[11.5px] text-fg-3">
            <Lock className="size-3" /> github-acme → api.github.com
          </span>
        </div>
        <div className="min-h-[248px] space-y-4 px-5 py-5" aria-live="polite">
          <AnimatePresence initial={false}>
            {BEATS.slice(0, shown).map((b, i) => (
              <motion.div
                key={i}
                initial={reduce ? false : { opacity: 0, y: 8, filter: 'blur(4px)' }}
                animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
                exit={{ opacity: 0, transition: { duration: 0.2 } }}
                transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
                className="grid grid-cols-[72px_1fr] gap-3 text-[14px] leading-relaxed"
              >
                <span className={`pt-px font-mono text-[12px] ${b.tone}`}>{b.who}</span>
                <span className={b.mono ? 'font-mono text-[12.5px] break-words text-fg-2' : 'text-fg'}>{b.text}</span>
              </motion.div>
            ))}
          </AnimatePresence>
        </div>
      </div>
    </div>
  )
}

const STEPS = [
  ['You type it once, into keymule.', 'Ask your agent to “connect my github”. A keymule window opens and you type the token there, never in the chat.'],
  ['The agent asks by name.', 'It sees names, hosts and your notes, like github-acme → api.github.com. Never the value.'],
  ['keymule does the secret part.', 'It attaches the token to the request or the CLI’s environment, then blanks it out of anything that comes back.'],
]

export function How() {
  return (
    <section id="how" className="mx-auto max-w-6xl px-5 py-24 sm:px-8 lg:py-32">
      <div className="grid grid-cols-1 items-center gap-14 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
        <div>
          <SectionHead kicker="How it works" title="Use without reveal.">
            Tokens live in keymule's encrypted vault, with its key in your OS keychain. The agent refers to them by name.
          </SectionHead>
          <ol className="mt-10 space-y-6">
            {STEPS.map(([title, body], i) => (
              <Reveal key={title} delay={i * 0.06}>
                <li className="flex gap-4">
                  <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-white/5 font-mono text-[12px] text-fg-2 ring-1 ring-white/10 ring-inset">
                    {i + 1}
                  </span>
                  <div>
                    <h3 className="font-semibold tracking-tight">{title}</h3>
                    <p className="mt-1 leading-relaxed text-fg-2">{body}</p>
                  </div>
                </li>
              </Reveal>
            ))}
          </ol>
        </div>
        <Reveal delay={0.1} className="min-w-0">
          <RequestCard />
        </Reveal>
      </div>
    </section>
  )
}

/** A leak scan, counting up as it comes into view. */
function ScanMeter() {
  const ref = useRef<HTMLDivElement>(null)
  const inView = useInView(ref, { once: true })
  const reduce = useReducedMotion()
  const [mb, setMb] = useState(reduce ? 617 : 0)
  useEffect(() => {
    if (!inView || reduce) return
    let raf = 0
    const start = performance.now()
    const tick = (t: number) => {
      const p = Math.min(1, (t - start) / 2400)
      setMb(Math.round(617 * (1 - Math.pow(1 - p, 3))))
      if (p < 1) raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [inView, reduce])
  return (
    <div ref={ref} className="mt-6">
      <div className="h-1 overflow-hidden rounded-full bg-white/10">
        <div className="h-full rounded-full bg-gradient-to-r from-accent-2 to-accent" style={{ width: `${(mb / 617) * 100}%` }} />
      </div>
      <p className="mt-2 font-mono text-[12px] text-fg-3">
        {mb} MB of 617 MB · {Math.round((mb / 617) * 14)} possible keys so far
      </p>
    </div>
  )
}

const FEATURES = [
  { icon: Globe, title: 'Locked to hosts', body: 'A GitHub token only ever goes to GitHub, and redirects elsewhere drop it. The hosts are sealed with the token, so editing a file doesn’t move them.', span: 'lg:col-span-2' },
  { icon: ClipboardCheck, title: 'Paste-to-save', body: 'Paste a token by habit and keymule holds the message, saves the token and sends its name instead.' },
  { icon: Terminal, title: 'CLI tools too', body: 'keymule exec github --env GH_TOKEN -- gh pr create. The token goes to that program only.' },
  { icon: KeyRound, title: 'Passwords, SSH, AWS', body: 'Database passwords, private keys as temporary files, AWS key pairs, and SSH logins where keymule answers the password prompt itself.', span: 'lg:col-span-2' },
  { icon: GitBranch, title: 'GitHub Actions secrets', body: 'One dialog puts several secrets into a repo, encrypted with its key.' },
  { icon: LayoutDashboard, title: 'Menu bar and page', body: 'Credentials, MCP servers, live activity and the leak scan, in one page.' },
  { icon: ScanSearch, title: 'Leak check', body: 'Scans agent transcripts, configs, .env files and shell history for keys that already leaked, and says what to rotate.', meter: true, span: 'lg:col-span-2' },
  { icon: Container, title: 'Dev containers', body: 'No screen? Questions wait in the page, answered from a browser you paired. Every answer is signed by it, and Reveal works there too.', span: 'lg:col-span-2' },
]

export function Features() {
  return (
    <section id="features" className="mx-auto max-w-6xl px-5 py-24 sm:px-8 lg:py-32">
      <SectionHead kicker="Features" title={<>Everything the agent needs.<br />Nothing it can leak.</>} />
      <div className="mt-14 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {FEATURES.map((f, i) => (
          <Reveal key={f.title} delay={(i % 4) * 0.05} className={f.span}>
            <article onMouseMove={spotlight} className="spot glass h-full rounded-3xl p-7">
              <span className="flex size-10 items-center justify-center rounded-xl bg-gradient-to-b from-accent/30 to-accent/5 text-fg ring-1 ring-accent/30 ring-inset">
                <f.icon className="size-[18px]" strokeWidth={1.8} />
              </span>
              <h3 className="mt-6 text-lg font-semibold tracking-tight">{f.title}</h3>
              <p className="mt-2 leading-relaxed text-fg-2">{f.body}</p>
              {f.meter && <ScanMeter />}
            </article>
          </Reveal>
        ))}
      </div>
    </section>
  )
}

export function Film() {
  return (
    <section id="film" className="mx-auto max-w-6xl px-5 py-24 sm:px-8 lg:py-32">
      <SectionHead center kicker="The film" title="One key’s journey, in 30 seconds." />
      <Reveal delay={0.1} className="mt-12">
        <div className="glass rounded-[26px] p-2 shadow-[0_40px_140px_-40px_rgb(139_123_255/0.5)]">
          <video
            className="block aspect-video w-full rounded-[20px] bg-black"
            src="/keymule-film.mp4"
            poster="/film-poster.png"
            controls
            playsInline
            preload="metadata"
          />
        </div>
      </Reveal>
    </section>
  )
}
