import { useEffect, useState } from 'react'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { ArrowDown, Lock } from 'lucide-react'
import { CopyCommand, GITHUB, GithubIcon, Logo, Tag } from './ui'

export function Nav() {
  const [scrolled, setScrolled] = useState(false)
  useEffect(() => {
    const on = () => setScrolled(window.scrollY > 8)
    on()
    window.addEventListener('scroll', on, { passive: true })
    return () => window.removeEventListener('scroll', on)
  }, [])
  const links = [
    ['How it works', '#how'],
    ['Features', '#features'],
    ['Security', '#security'],
    ['FAQ', '#faq'],
  ]
  return (
    <header
      className={`sticky top-0 z-40 transition-[background-color,border-color,backdrop-filter] duration-300 ${
        scrolled ? 'border-b border-line bg-paper/80 backdrop-blur-xl' : 'border-b border-transparent'
      }`}
    >
      <nav className="mx-auto flex h-16 max-w-7xl items-center gap-8 px-5 sm:px-8" aria-label="Main">
        <a href="#top" className="flex items-center gap-2.5 font-medium tracking-tight">
          <Logo />
          <span className="text-[17px]">keymule</span>
        </a>
        <div className="hidden items-center gap-7 text-sm text-ink-2 md:flex">
          {links.map(([label, href]) => (
            <a key={href} href={href} className="transition-colors hover:text-ink">
              {label}
            </a>
          ))}
        </div>
        <div className="ml-auto flex items-center gap-2">
          <a
            href={GITHUB}
            className="hidden h-9 items-center gap-2 rounded-full px-3.5 text-sm text-ink-2 transition-colors hover:bg-ink/5 hover:text-ink sm:flex"
          >
            <GithubIcon /> GitHub
          </a>
          <a
            href="#install"
            className="flex h-9 items-center rounded-full bg-ink px-4 text-sm font-medium text-paper transition-transform hover:-translate-y-px active:translate-y-0"
          >
            Install
          </a>
        </div>
      </nav>
    </header>
  )
}

const PACK = [
  ['github-launchstory', 'api.github.com'],
  ['stripe-shop', 'api.stripe.com'],
  ['vultr-ssh', '203.0.113.7:22'],
]

// One request, told in four beats. Loops.
const BEATS = [
  { who: 'You', tone: 'text-ink', text: 'why did my last CI run fail?' },
  { who: 'Agent', tone: 'text-accent', text: 'http_request(credential="github-launchstory", path="/repos/acme/app/actions/runs")' },
  { who: 'keymule', tone: 'text-seal', text: 'adds the token ●●●●●●●● · strips it from the reply' },
  { who: 'Agent', tone: 'text-accent', text: 'Run #482 failed in "integration tests": a timeout in checkout.spec.ts.' },
]

function ManifestCard() {
  const reduce = useReducedMotion()
  const [step, setStep] = useState(reduce ? BEATS.length : 0)
  useEffect(() => {
    if (reduce) return
    const t = setTimeout(() => setStep(s => (s >= BEATS.length + 2 ? 0 : s + 1)), step === 0 ? 700 : 1500)
    return () => clearTimeout(t)
  }, [step, reduce])
  const shown = Math.min(step, BEATS.length)

  return (
    <div className="relative rounded-[28px] border border-line bg-card p-2 shadow-[0_30px_80px_-30px_rgb(20_20_18/0.35)]">
      <div className="rounded-[22px] border border-line bg-paper/40">
        <div className="flex items-center justify-between border-b border-line px-5 py-3.5">
          <span className="label text-ink-3">Pack manifest</span>
          <span className="label flex items-center gap-1.5 text-ink-3">
            <Lock className="size-3" /> vault.enc
          </span>
        </div>
        <ul>
          {PACK.map(([name, host]) => (
            <li key={name} className="grid grid-cols-[1fr_auto] items-center gap-3 border-b border-line px-5 py-3 sm:grid-cols-[1.1fr_1fr_auto]">
              <span className="font-mono text-[13px]">{name}</span>
              <span className="hidden font-mono text-[12px] text-ink-3 sm:block">→ {host}</span>
              <Tag tone="seal">Sealed</Tag>
            </li>
          ))}
        </ul>
        <div className="min-h-[214px] space-y-3 px-5 py-4" aria-live="polite">
          <AnimatePresence initial={false}>
            {BEATS.slice(0, shown).map((b, i) => (
              <motion.div
                key={`${i}-${b.who}`}
                initial={reduce ? false : { opacity: 0, y: 8, filter: 'blur(4px)' }}
                animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
                exit={{ opacity: 0, transition: { duration: 0.2 } }}
                transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
                className="grid grid-cols-[64px_1fr] gap-3 text-[13px] leading-relaxed"
              >
                <span className={`label pt-[3px] ${b.tone}`}>{b.who}</span>
                <span className={`break-words ${i === 1 ? 'font-mono text-[12px] text-ink-2' : 'text-ink'}`}>{b.text}</span>
              </motion.div>
            ))}
          </AnimatePresence>
        </div>
      </div>
      <div className="pointer-events-none absolute -bottom-4 left-8 rotate-[-4deg] rounded-full bg-card">
        <Tag tone="accent">The agent never saw the token</Tag>
      </div>
    </div>
  )
}

export function Hero() {
  const reduce = useReducedMotion()
  const rise = (d: number) =>
    reduce ? {} : { initial: { opacity: 0, y: 28 }, animate: { opacity: 1, y: 0 }, transition: { duration: 0.8, delay: d, ease: [0.22, 1, 0.36, 1] as const } }
  return (
    <section id="top" className="relative mx-auto grid max-w-7xl grid-cols-1 items-center gap-14 px-5 pt-12 pb-24 sm:px-8 lg:grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)] lg:pt-20 lg:pb-32">
      <div className="min-w-0">
        <motion.div {...rise(0)} className="flex flex-wrap gap-2">
          <Tag>Open source</Tag>
          <Tag>macOS · Linux · containers</Tag>
        </motion.div>
        <motion.h1 {...rise(0.08)} className="mt-7 font-display text-[clamp(52px,8.4vw,112px)] leading-[0.9] tracking-[-0.025em]">
          Your agents carry the keys.
          <br />
          <em className="text-ink-2">They never open the pack.</em>
        </motion.h1>
        <motion.p {...rise(0.18)} className="mt-7 max-w-xl text-lg leading-relaxed text-ink-2">
          keymule lets Claude Code, Codex and Cursor use your API tokens, passwords and SSH logins by name. It adds the secret
          itself and strips it from every reply, so it never reaches the model, the chat or a transcript.
        </motion.p>
        <motion.div {...rise(0.28)} className="mt-9 max-w-xl">
          <CopyCommand />
          <div className="mt-4 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm text-ink-2">
            <a href="#film" className="group inline-flex items-center gap-1.5 hover:text-ink">
              Watch the 30-second film <ArrowDown className="size-4 transition-transform group-hover:translate-y-0.5" />
            </a>
            <span>
              or <code className="font-mono text-[13px]">brew install rorogogogo/tap/keymule</code>
            </span>
          </div>
        </motion.div>
      </div>
      <motion.div {...rise(0.35)}>
        <ManifestCard />
      </motion.div>
    </section>
  )
}
