import { useEffect, useRef, useState, type ReactNode } from 'react'
import { AnimatePresence, motion, useInView, useMotionValueEvent, useReducedMotion, useScroll, useTransform, type MotionValue } from 'motion/react'
import { Check, KeyRound, Lock, X } from 'lucide-react'
import { Logo, Reveal, SectionHead, spotlight } from './ui'

const EASE = [0.22, 1, 0.36, 1] as const

const AGENTS = ['Claude Code', 'Codex', 'Cursor', 'Claude Desktop', 'Antigravity', 'gh', 'git', 'ssh', 'psql', 'GitHub Actions', 'MCP servers']

export function Agents() {
  const row = [...AGENTS, ...AGENTS]
  return (
    <section aria-label="Works with" className="border-y border-line py-8">
      <div className="mx-auto flex max-w-6xl items-center gap-10 px-5 sm:px-8">
        <p className="hidden shrink-0 text-[13px] text-fg-3 md:block">Works with</p>
        <div className="flex min-w-0 flex-1 overflow-hidden [mask-image:linear-gradient(90deg,transparent,black_12%,black_88%,transparent)]">
          <div className="marquee flex shrink-0 items-center gap-12 pr-12">
            {row.map((a, i) => (
              <span key={i} className="text-[17px] font-medium tracking-tight whitespace-nowrap text-fg-2">
                {a}
              </span>
            ))}
          </div>
        </div>
      </div>
    </section>
  )
}

function Word({ children, progress, range }: { children: string; progress: MotionValue<number>; range: [number, number] }) {
  const opacity = useTransform(progress, range, [0.18, 1])
  return <motion.span style={{ opacity }}>{children} </motion.span>
}

const STATEMENT =
  'Every token you paste into a chat lands in a provider’s logs, a transcript on your disk and the next agent’s context. keymule makes pasting unnecessary: your agents use the keys by name, and the values never leave the vault.'

/** A statement that lights up word by word as you scroll through it. */
export function Statement() {
  const ref = useRef<HTMLParagraphElement>(null)
  const reduce = useReducedMotion()
  const { scrollYProgress } = useScroll({ target: ref, offset: ['start 85%', 'end 45%'] })
  const words = STATEMENT.split(' ')
  return (
    <section className="mx-auto max-w-4xl px-5 py-32 sm:px-8 lg:py-44">
      <p ref={ref} className="text-[clamp(26px,3.4vw,42px)] leading-[1.25] font-medium tracking-[-0.03em]">
        {reduce
          ? STATEMENT
          : words.map((w, i) => (
              <Word key={i} progress={scrollYProgress} range={[i / words.length, (i + 1) / words.length]}>
                {w}
              </Word>
            ))}
      </p>
    </section>
  )
}

/* ---------- How it works: a pinned panel that changes with each step ---------- */

function DialogVisual() {
  return (
    <div className="window beam mx-auto w-full max-w-[360px] rounded-2xl p-5">
      <div className="flex items-center gap-2 text-[12px] text-fg-3">
        <Logo className="size-4" /> keymule
      </div>
      <div className="mt-4 flex gap-3">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-accent/15 text-accent ring-1 ring-accent/25 ring-inset">
          <KeyRound className="size-5" />
        </span>
        <div>
          <p className="font-semibold tracking-tight">Save a token for github-acme</p>
          <p className="mt-1 text-[13px] text-fg-2">It will only be sent to api.github.com.</p>
        </div>
      </div>
      <div className="mt-5 flex h-10 items-center rounded-lg border border-line-2 bg-black/30 px-3 font-mono tracking-[0.2em] text-fg-2">
        ●●●●●●●●●●●●●●●●<span className="blink ml-0.5 tracking-normal text-accent">|</span>
      </div>
      <div className="mt-5 flex justify-end gap-2 text-[13px]">
        <span className="rounded-lg px-3 py-1.5 text-fg-2">Cancel</span>
        <span className="rounded-lg bg-accent px-4 py-1.5 font-medium text-white">Save</span>
      </div>
    </div>
  )
}

function ManifestVisual() {
  const rows = [
    ['github-acme', 'api.github.com', 'Acme org account'],
    ['stripe-shop', 'api.stripe.com', 'Test key for the demo shop'],
    ['vultr-ssh', '203.0.113.7:22', 'Sydney VPS'],
    ['postgres', 'MCP server', 'Local dev database'],
  ]
  return (
    <div className="window mx-auto w-full max-w-[440px] overflow-hidden rounded-2xl">
      <div className="flex items-center justify-between border-b border-line px-5 py-3">
        <span className="font-mono text-[12px] text-fg-3">list_credentials()</span>
        <span className="font-mono text-[11px] text-fg-3">what the agent sees</span>
      </div>
      {rows.map(([n, h, d]) => (
        <div key={n} className="flex items-center gap-3 border-b border-line px-5 py-3 last:border-0">
          <div className="min-w-0 flex-1">
            <p className="font-mono text-[13px] text-fg">{n}</p>
            <p className="truncate text-[12px] text-fg-3">{d}</p>
          </div>
          <span className="font-mono text-[12px] text-fg-2">{h}</span>
          <span className="rounded-md bg-white/[0.06] px-2 py-0.5 font-mono text-[11px] text-fg-3">no value</span>
        </div>
      ))}
    </div>
  )
}

function RedactVisual() {
  return (
    <div className="window mx-auto w-full max-w-[460px] overflow-hidden rounded-2xl font-mono text-[12.5px]">
      <div className="flex items-center gap-1.5 border-b border-line px-4 py-3">
        <span className="size-2 rounded-full bg-white/15" />
        <span className="size-2 rounded-full bg-white/15" />
        <span className="size-2 rounded-full bg-white/15" />
        <span className="ml-2 text-[11px] text-fg-3">request · keymule</span>
      </div>
      <div className="space-y-1.5 p-5 leading-relaxed">
        <p className="text-fg-2">GET https://api.github.com/repos/acme/app/actions/runs</p>
        <p className="text-fg-3">
          Authorization: Bearer <span className="rounded bg-ok/15 px-1 text-ok">ghp_••••••••••••</span>
          <span className="ml-2 text-[11px] text-ok">← added by keymule</span>
        </p>
        <p className="pt-3 text-fg-2">200 OK</p>
        <p className="text-fg-3">{'{ "workflow_runs": [{ "id": 482, "conclusion": "failure", … }] }'}</p>
        <p className="pt-3 text-fg-3">
          token in the reply? <span className="rounded bg-white/10 px-1 text-fg-2">[redacted]</span>
          <span className="ml-2 text-[11px] text-accent">→ what the agent gets</span>
        </p>
      </div>
    </div>
  )
}

const STEPS: { title: string; body: string; visual: ReactNode }[] = [
  { title: 'You type it once, into keymule.', body: 'Ask your agent to “connect my github”. keymule opens its own window and you type the token there, never in the chat. Paste one by habit and keymule catches it before it’s sent.', visual: <DialogVisual /> },
  { title: 'The agent asks by name.', body: 'It sees names, hosts and your notes, so it picks the right credential. It never sees a value.', visual: <ManifestVisual /> },
  { title: 'keymule does the secret part.', body: 'It attaches the token to the request, or to the CLI’s environment, then blanks it out of anything that comes back.', visual: <RedactVisual /> },
]

export function How() {
  const ref = useRef<HTMLDivElement>(null)
  const [active, setActive] = useState(0)
  const { scrollYProgress } = useScroll({ target: ref, offset: ['start 60%', 'end 60%'] })
  useMotionValueEvent(scrollYProgress, 'change', v => setActive(Math.min(STEPS.length - 1, Math.max(0, Math.floor(v * STEPS.length)))))
  return (
    <section id="how" className="mx-auto max-w-6xl px-5 py-24 sm:px-8 lg:py-32">
      <SectionHead kicker="How it works" title="Use without reveal.">
        Tokens live in keymule’s encrypted vault, with its key in your OS keychain.
      </SectionHead>
      <div ref={ref} className="mt-16 grid grid-cols-1 gap-12 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
        <ol className="space-y-6 lg:space-y-[28vh] lg:py-[14vh]">
          {STEPS.map((s, i) => (
            <li key={s.title} className={`transition-opacity duration-500 ${active === i ? 'opacity-100' : 'lg:opacity-35'}`}>
              <p className="font-mono text-[12px] text-fg-3">0{i + 1}</p>
              <h3 className="mt-3 text-[26px] font-semibold tracking-[-0.03em]">{s.title}</h3>
              <p className="mt-3 max-w-md text-[16px] leading-relaxed text-fg-2">{s.body}</p>
              <div className="mt-8 lg:hidden">{s.visual}</div>
            </li>
          ))}
        </ol>
        <div className="hidden lg:block">
          <div className="sticky top-[22vh] flex h-[56vh] items-center justify-center rounded-3xl border border-line bg-[radial-gradient(60%_60%_at_50%_40%,rgb(139_123_255/0.12),transparent)] p-8">
            <AnimatePresence mode="wait">
              <motion.div
                key={active}
                className="w-full"
                initial={{ opacity: 0, y: 16, filter: 'blur(6px)', scale: 0.98 }}
                animate={{ opacity: 1, y: 0, filter: 'blur(0px)', scale: 1 }}
                exit={{ opacity: 0, y: -12, filter: 'blur(6px)', scale: 0.98 }}
                transition={{ duration: 0.45, ease: EASE }}
              >
                {STEPS[active].visual}
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
      </div>
    </section>
  )
}

/* ---------- Features: each card with its own small illustration ---------- */

function useLoop(n: number, ms: number) {
  const ref = useRef<HTMLDivElement>(null)
  const inView = useInView(ref)
  const reduce = useReducedMotion()
  const [i, setI] = useState(0)
  useEffect(() => {
    if (!inView || reduce) return
    const t = setInterval(() => setI(x => (x + 1) % n), ms)
    return () => clearInterval(t)
  }, [inView, reduce, n, ms])
  return [ref, i] as const
}

function HostsArt() {
  const [ref, i] = useLoop(2, 2200)
  const ok = i === 0
  return (
    <div ref={ref} className="flex h-full items-center justify-center gap-4 font-mono text-[12px]">
      <span className="rounded-lg border border-line-2 bg-bg-3 px-3 py-2 text-fg">github-acme</span>
      <div className="relative h-px w-16 bg-line-2">
        <motion.span
          key={i}
          className={`absolute top-1/2 size-1.5 -translate-y-1/2 rounded-full ${ok ? 'bg-ok shadow-[0_0_10px_var(--color-ok)]' : 'bg-bad shadow-[0_0_10px_var(--color-bad)]'}`}
          initial={{ left: 0 }}
          animate={{ left: ok ? '100%' : '55%' }}
          transition={{ duration: 0.9, ease: 'easeInOut' }}
        />
      </div>
      <AnimatePresence mode="wait">
        <motion.span
          key={i}
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -6 }}
          className={`flex items-center gap-1.5 rounded-lg border px-3 py-2 ${ok ? 'border-ok/30 bg-ok/10 text-ok' : 'border-bad/30 bg-bad/10 text-bad'}`}
        >
          {ok ? <Check className="size-3.5" /> : <X className="size-3.5" />}
          {ok ? 'api.github.com' : 'evil.example'}
        </motion.span>
      </AnimatePresence>
    </div>
  )
}

function PasteArt() {
  const [ref, i] = useLoop(2, 2400)
  return (
    <div ref={ref} className="flex h-full flex-col justify-center gap-3 px-2">
      <div className="rounded-xl border border-line-2 bg-bg-3 px-4 py-3 text-[13px] text-fg-2">
        here’s the token for acme:{' '}
        <AnimatePresence mode="wait">
          <motion.span
            key={i}
            initial={{ opacity: 0, filter: 'blur(4px)' }}
            animate={{ opacity: 1, filter: 'blur(0px)' }}
            exit={{ opacity: 0, filter: 'blur(4px)' }}
            className={`rounded px-1 font-mono text-[12px] ${i === 0 ? 'bg-bad/15 text-bad' : 'bg-ok/15 text-ok'}`}
          >
            {i === 0 ? 'ghp_7Kq2vX9m…' : '‹github-acme›'}
          </motion.span>
        </AnimatePresence>
      </div>
      <p className="text-center font-mono text-[11px] text-fg-3">{i === 0 ? 'caught before it’s sent' : 'saved · the agent gets its name'}</p>
    </div>
  )
}

function ExecArt() {
  return (
    <div className="flex h-full flex-col justify-center rounded-xl border border-line bg-black/40 p-4 font-mono text-[11.5px] leading-relaxed">
      <p className="text-fg">
        <span className="text-fg-3">$</span> keymule exec github -- gh auth status
      </p>
      <p className="mt-1 text-fg-3">✓ Logged in to github.com as acme-bot</p>
      <p className="text-fg-3">
        - Token: <span className="text-fg-2">[redacted]</span>
      </p>
    </div>
  )
}

function ScanArt() {
  const ref = useRef<HTMLDivElement>(null)
  const inView = useInView(ref, { once: true })
  const reduce = useReducedMotion()
  const [mb, setMb] = useState(reduce ? 617 : 0)
  useEffect(() => {
    if (!inView || reduce) return
    let raf = 0
    const start = performance.now()
    const tick = (t: number) => {
      const p = Math.min(1, (t - start) / 2600)
      setMb(Math.round(617 * (1 - Math.pow(1 - p, 3))))
      if (p < 1) raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [inView, reduce])
  const keys = Math.round((mb / 617) * 14)
  return (
    <div ref={ref} className="flex h-full flex-col justify-center gap-3">
      <div className="flex items-baseline justify-between">
        <span className="text-[13px] text-fg-2">Reading agent transcripts</span>
        <span className="font-mono text-[12px] text-fg-3">{mb} / 617 MB</span>
      </div>
      <div className="h-1 overflow-hidden rounded-full bg-white/[0.08]">
        <div className="h-full rounded-full bg-gradient-to-r from-accent-2 to-accent" style={{ width: `${(mb / 617) * 100}%` }} />
      </div>
      <div className="flex gap-6 pt-1 font-mono text-[12px]">
        <span className="text-bad">● {Math.round(keys * 0.3)} leaked</span>
        <span className="text-warn">● {Math.round(keys * 0.5)} plain text</span>
        <span className="text-fg-3">● {keys - Math.round(keys * 0.3) - Math.round(keys * 0.5)} readable</span>
      </div>
    </div>
  )
}

function PairArt() {
  return (
    <div className="flex h-full items-center justify-center gap-3">
      <div className="rounded-xl border border-line-2 bg-bg-3 px-4 py-3 text-center">
        <p className="text-[11px] text-fg-3">Pair this browser</p>
        <p className="mt-1 font-mono text-lg tracking-[0.15em] text-fg">482-907</p>
      </div>
      <div className="flex items-center gap-1.5 rounded-full border border-ok/30 bg-ok/10 px-3 py-1.5 text-[12px] text-ok">
        <Lock className="size-3" /> signed answers
      </div>
    </div>
  )
}

function SecretsArt() {
  const items = ['STRIPE_SECRET_KEY', 'OPENAI_API_KEY', 'NPM_TOKEN']
  return (
    <div className="flex h-full flex-col justify-center gap-2 font-mono text-[12px]">
      {items.map((s, i) => (
        <motion.div
          key={s}
          initial={{ opacity: 0, x: -8 }}
          whileInView={{ opacity: 1, x: 0 }}
          viewport={{ once: true }}
          transition={{ delay: 0.15 * i, duration: 0.5, ease: EASE }}
          className="flex items-center justify-between rounded-lg border border-line bg-bg-3 px-3 py-2"
        >
          <span className="text-fg-2">{s}</span>
          <Check className="size-3.5 text-ok" />
        </motion.div>
      ))}
    </div>
  )
}

const FEATURES: { title: string; body: string; art: ReactNode; span?: string }[] = [
  { title: 'Locked to hosts', body: 'A GitHub token only ever goes to GitHub. The hosts are sealed with the token, so editing a file doesn’t move them.', art: <HostsArt />, span: 'lg:col-span-2' },
  { title: 'Paste-to-save', body: 'Paste a token by habit: keymule holds the message and swaps the token for its name.', art: <PasteArt /> },
  { title: 'CLI tools too', body: 'gh, git, psql, ssh and aws get the secret in their environment, and out of their output.', art: <ExecArt /> },
  { title: 'Leak check', body: 'Finds keys that already leaked into transcripts, configs, .env files and shell history, and says what to rotate.', art: <ScanArt />, span: 'lg:col-span-2' },
  { title: 'GitHub Actions secrets', body: 'One dialog copies several secrets into a repo, encrypted with its key.', art: <SecretsArt /> },
  { title: 'Dev containers', body: 'No screen? Questions wait in the page, answered from a browser you paired. Reveal works there too.', art: <PairArt />, span: 'lg:col-span-2' },
]

export function Features() {
  return (
    <section id="features" className="mx-auto max-w-6xl px-5 py-24 sm:px-8 lg:py-32">
      <SectionHead kicker="Features" title={<>Everything the agent needs.<br /><span className="dim">Nothing it can leak.</span></>} />
      <div className="mt-14 grid grid-cols-1 gap-3 md:grid-cols-2 lg:grid-cols-3">
        {FEATURES.map((f, i) => (
          <Reveal key={f.title} delay={(i % 3) * 0.05} className={f.span}>
            <article onMouseMove={spotlight} className="spot panel flex h-full flex-col overflow-hidden rounded-2xl">
              <div className="h-44 border-b border-line bg-[radial-gradient(80%_80%_at_50%_0%,rgb(255_255_255/0.03),transparent)] p-6">{f.art}</div>
              <div className="p-6">
                <h3 className="text-[16px] font-semibold tracking-tight">{f.title}</h3>
                <p className="mt-1.5 text-[14.5px] leading-relaxed text-fg-2">{f.body}</p>
              </div>
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
        <div className="window rounded-2xl p-1.5">
          <video
            className="block aspect-video w-full rounded-xl bg-black"
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
