import { useEffect, useRef, useState } from 'react'
import { motion, useInView, useReducedMotion } from 'motion/react'
import {
  ClipboardCheck,
  Container,
  GitBranch,
  Globe,
  KeyRound,
  Menu,
  ScanSearch,
  Terminal,
} from 'lucide-react'
import { Reveal, SectionHead, Tag } from './ui'

const AGENTS = ['Claude Code', 'Codex', 'Cursor', 'Claude Desktop', 'Antigravity', 'gh', 'git', 'ssh', 'GitHub Actions', 'MCP servers']

export function Agents() {
  const row = [...AGENTS, ...AGENTS]
  return (
    <section aria-label="Works with" className="border-y border-line bg-paper-2/60 py-5">
      <div className="flex overflow-hidden [mask-image:linear-gradient(90deg,transparent,black_10%,black_90%,transparent)]">
        <div className="marquee flex shrink-0 items-center gap-12 pr-12">
          {row.map((a, i) => (
            <span key={i} className="flex items-center gap-12 whitespace-nowrap font-display text-3xl text-ink-2 italic">
              {a}
              <span className="text-base text-ink-3 not-italic" aria-hidden="true">✦</span>
            </span>
          ))}
        </div>
      </div>
    </section>
  )
}

export function Film() {
  return (
    <section id="film" className="bg-night px-5 py-24 text-paper sm:px-8 lg:py-32">
      <div className="mx-auto max-w-6xl">
        <Reveal className="flex flex-wrap items-end justify-between gap-6">
          <div>
            <p className="label text-paper/50">The film · 30 seconds</p>
            <h2 className="mt-4 font-display text-[clamp(40px,6vw,76px)] leading-[0.95] tracking-[-0.02em]">
              One key's journey, <em className="text-paper/60">start to finish.</em>
            </h2>
          </div>
          <Tag tone="night">No real secrets were harmed</Tag>
        </Reveal>
        <Reveal delay={0.1} className="mt-12">
          <div className="overflow-hidden rounded-[28px] border border-white/10 bg-black shadow-[0_40px_120px_-40px_rgb(99_91_255/0.45)]">
            <video
              className="block aspect-video w-full"
              src="/keymule-film.mp4"
              poster="/film-poster.png"
              controls
              playsInline
              preload="metadata"
            />
          </div>
        </Reveal>
      </div>
    </section>
  )
}

const PAINS = [
  ['01 · Pasted token', '“I notice you shared an API key. You should rotate it right away.”', 'You pasted it so the agent could check a pipeline. Now it sits in a provider log and a transcript on your disk.'],
  ['02 · Refused', '“I can’t read your .env file.”', 'So you run the command yourself and paste the output back. Or the agent reads it anyway, and the key lands in the transcript.'],
  ['03 · Again', 'The same token, every session.', 'New session, new agent, new dev container: each one wants it again, through the clipboard and the chat.'],
  ['04 · Plain text', 'Keys in ~/.claude.json.', 'Your MCP servers keep API keys in config files every agent can read, and in every dotfile backup.'],
]

export function Pains() {
  return (
    <section className="mx-auto max-w-7xl px-5 py-24 sm:px-8 lg:py-32">
      <SectionHead kicker="Sound familiar?" title={<>Secrets keep ending up <em className="text-ink-2">in the chat.</em></>} />
      <div className="mt-14 grid grid-cols-1 gap-4 md:grid-cols-2">
        {PAINS.map(([tag, quote, body], i) => (
          <Reveal key={tag} delay={i * 0.06}>
            <article className="group h-full rounded-3xl border border-line bg-card p-7 transition-[transform,box-shadow] duration-300 hover:-translate-y-1 hover:shadow-[0_24px_60px_-30px_rgb(20_20_18/0.35)]">
              <p className="label text-seal">{tag}</p>
              <h3 className="mt-5 font-display text-[30px] leading-[1.05] tracking-[-0.01em]">{quote}</h3>
              <p className="mt-4 leading-relaxed text-ink-2">{body}</p>
            </article>
          </Reveal>
        ))}
      </div>
    </section>
  )
}

const STEPS = [
  ['01', 'You type it once, into keymule.', 'Ask your agent to “connect my github”. A keymule window opens; you type the token there, never in the chat. Pasted one by habit? keymule catches it before it’s sent.', 'Sealed'],
  ['02', 'The agent asks by name.', 'It sees names, hosts and your notes: github-launchstory → api.github.com. Never the value.', 'Locked to host'],
  ['03', 'keymule does the secret part.', 'It attaches the token to the request (or the CLI’s environment), then blanks it out of anything that comes back.', 'Redacted'],
]

export function How() {
  return (
    <section id="how" className="border-t border-line bg-paper-2/40">
      <div className="mx-auto max-w-7xl px-5 py-24 sm:px-8 lg:py-32">
        <SectionHead kicker="How it works" title={<>Use without <em className="text-ink-2">reveal.</em></>}>
          Your tokens live in keymule's encrypted vault, with its key in your OS keychain. The agent refers to them by name, and keymule
          does the rest.
        </SectionHead>
        <ol className="mt-14 border-t border-line">
          {STEPS.map(([n, title, body, tag], i) => (
            <Reveal key={n} delay={i * 0.06}>
              <li className="grid gap-4 border-b border-line py-8 md:grid-cols-[80px_1fr_1.3fr_auto] md:items-baseline md:gap-8">
                <span className="font-mono text-sm text-ink-3">{n}</span>
                <h3 className="font-display text-[32px] leading-[1.05] tracking-[-0.01em]">{title}</h3>
                <p className="leading-relaxed text-ink-2">{body}</p>
                <div>
                  <Tag tone={i === 0 ? 'seal' : i === 1 ? 'accent' : 'ok'}>{tag}</Tag>
                </div>
              </li>
            </Reveal>
          ))}
        </ol>
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
      <div className="h-1.5 overflow-hidden rounded-full bg-ink/10">
        <motion.div className="h-full rounded-full bg-accent" style={{ width: `${(mb / 617) * 100}%` }} />
      </div>
      <p className="mt-2 font-mono text-[12px] text-ink-3">
        {mb} MB of 617 MB · {Math.round((mb / 617) * 14)} possible keys so far
      </p>
    </div>
  )
}

const FEATURES = [
  { icon: Globe, title: 'Locked to hosts', body: 'A GitHub token only ever goes to GitHub. Redirects elsewhere drop it. The hosts are sealed with the token, so editing a file doesn’t move them.', span: 'lg:col-span-2' },
  { icon: ClipboardCheck, title: 'Paste-to-save', body: 'Paste a token into Claude Code, Codex or Cursor by habit and keymule holds the message, saves the token and sends its name instead.' },
  { icon: Terminal, title: 'CLI tools too', body: 'keymule exec github --env GH_TOKEN -- gh pr create. The token goes to that program only, and out of its output.' },
  { icon: KeyRound, title: 'Passwords, SSH, AWS', body: 'Database passwords, private keys as temporary files, AWS key pairs, and SSH logins where keymule answers the password prompt itself.', span: 'lg:col-span-2' },
  { icon: GitBranch, title: 'GitHub Actions secrets', body: 'One dialog puts several stored secrets into a repo, encrypted with its key and uploaded with your gh login.' },
  { icon: Menu, title: 'Menu bar and page', body: 'Credentials, MCP servers, live activity and the leak scan in one calm page. Reveal shows a value to you, never to the agent.' },
  { icon: ScanSearch, title: 'Leak check', body: 'Scans agent transcripts, configs, .env files and shell history for keys that already leaked, and says what to rotate.', meter: true, span: 'lg:col-span-2' },
  { icon: Container, title: 'Dev containers', body: 'No screen? Questions wait in the page, answered from a browser you paired. Every answer is signed by it, and Reveal works there too.', span: 'lg:col-span-2' },
]

export function Features() {
  return (
    <section id="features" className="mx-auto max-w-7xl px-5 py-24 sm:px-8 lg:py-32">
      <SectionHead kicker="Features" title={<>Everything the agent needs. <em className="text-ink-2">Nothing it can leak.</em></>} />
      <div className="mt-14 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {FEATURES.map((f, i) => (
          <Reveal key={f.title} delay={(i % 4) * 0.05} className={f.span}>
            <article className="h-full rounded-3xl border border-line bg-card p-7 transition-colors duration-300 hover:border-ink/25">
              <span className="flex size-10 items-center justify-center rounded-xl bg-ink text-paper">
                <f.icon className="size-[18px]" strokeWidth={1.8} />
              </span>
              <h3 className="mt-6 text-lg font-semibold tracking-tight">{f.title}</h3>
              <p className="mt-2 leading-relaxed text-ink-2">{f.body}</p>
              {f.meter && <ScanMeter />}
            </article>
          </Reveal>
        ))}
      </div>
    </section>
  )
}
