import { useEffect, useRef, useState } from 'react'
import { motion, useReducedMotion, useScroll, useTransform, type MotionValue } from 'motion/react'
import { ArrowRight, KeyRound, Lock, Moon, Sun } from 'lucide-react'
import { CopyCommand, GITHUB, GithubIcon, Logo } from './ui'

const EASE = [0.22, 1, 0.36, 1] as const

function ThemeToggle() {
  const [dark, setDark] = useState(() => document.documentElement.dataset.theme === 'dark')
  const flip = () => {
    const next = !dark
    setDark(next)
    if (next) document.documentElement.dataset.theme = 'dark'
    else delete document.documentElement.dataset.theme
    try {
      localStorage.setItem('km-theme', next ? 'dark' : 'light')
    } catch {
      /* private mode: just this visit */
    }
  }
  return (
    <button
      type="button"
      onClick={flip}
      aria-label={dark ? 'Switch to light' : 'Switch to dark'}
      title={dark ? 'Light' : 'Dark'}
      className="flex size-9 items-center justify-center rounded-full text-fg-2 transition-colors hover:bg-tint/5 hover:text-fg"
    >
      {dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
    </button>
  )
}

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
    <header className="fixed inset-x-0 top-0 z-40 flex justify-center px-4 pt-3">
      <nav
        aria-label="Main"
        className={`flex h-12 w-full max-w-5xl items-center gap-6 rounded-full pr-1.5 pl-4 transition-[background-color,box-shadow,border-color] duration-300 ${
          scrolled ? 'border border-line bg-bg/75 shadow-[0_8px_30px_rgb(0_0_0/0.12)] backdrop-blur-xl' : 'border border-transparent'
        }`}
      >
        <a href="#top" className="flex items-center gap-2 text-[15px] font-semibold tracking-tight">
          <Logo className="size-[22px]" />
          keymule
        </a>
        <div className="hidden items-center gap-6 text-[13.5px] text-fg-2 md:flex">
          {links.map(([label, href]) => (
            <a key={href} href={href} className="transition-colors hover:text-fg">
              {label}
            </a>
          ))}
        </div>
        <div className="ml-auto flex items-center gap-1">
          <ThemeToggle />
          <a href={GITHUB} className="hidden h-9 items-center gap-2 rounded-full px-3 text-[13.5px] text-fg-2 transition-colors hover:text-fg sm:flex">
            <GithubIcon /> GitHub
          </a>
          <a href="#install" className="flex h-9 items-center rounded-full bg-fg px-4 text-[13.5px] font-medium text-bg transition-opacity hover:opacity-90">
            Get keymule
          </a>
        </div>
      </nav>
    </header>
  )
}

/** The agent's side: a terminal where the request goes out by name. */
function TerminalCard() {
  return (
    <div className="dark-scope window w-[340px] overflow-hidden rounded-xl text-left">
      <div className="flex items-center gap-1.5 border-b border-line px-3.5 py-2.5">
        <span className="size-2 rounded-full bg-tint/15" />
        <span className="size-2 rounded-full bg-tint/15" />
        <span className="size-2 rounded-full bg-tint/15" />
        <span className="ml-2 font-mono text-[11px] text-fg-3">claude · ~/code/acme</span>
      </div>
      <div className="space-y-2.5 p-4 font-mono text-[11.5px] leading-relaxed">
        <p className="text-fg">
          <span className="text-fg-3">›</span> why did my last CI run fail?
        </p>
        <p className="text-fg-2">
          <span className="text-accent">⏺ keymule</span>.http_request(
          <br />
          <span className="pl-4">credential=</span>
          <span className="text-ok">"github-acme"</span>,
          <br />
          <span className="pl-4">path="/repos/acme/app/actions/runs")</span>
        </p>
        <p className="text-fg-3">
          ⎿ 200 · Authorization: Bearer <span className="rounded bg-tint/10 px-1 text-fg-2">[redacted]</span>
        </p>
        <p className="text-fg">
          Run #482 failed in integration tests<span className="blink">▍</span>
        </p>
      </div>
    </div>
  )
}

/** keymule's side: the window where you type the token, not the chat. */
function DialogCard() {
  return (
    <div className="window beam w-[300px] rounded-xl p-4 text-left">
      <div className="flex items-center gap-2 text-[11px] text-fg-3">
        <Logo className="size-3.5" /> keymule
      </div>
      <div className="mt-3 flex gap-3">
        <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-accent/15 text-accent ring-1 ring-accent/25 ring-inset">
          <KeyRound className="size-4" />
        </span>
        <div className="min-w-0">
          <p className="text-[13px] font-semibold tracking-tight">Save a token for github-acme</p>
          <p className="mt-1 flex items-center gap-1 text-[11.5px] text-fg-2">
            <Lock className="size-3" /> Locked to api.github.com
          </p>
        </div>
      </div>
      <div className="mt-3.5 flex h-8 items-center rounded-lg border border-line-2 bg-tint/[0.03] px-3 font-mono text-[13px] tracking-[0.2em] text-fg-2">
        ●●●●●●●●●●●●●●<span className="blink ml-0.5 tracking-normal text-accent">|</span>
      </div>
      <div className="mt-3.5 flex justify-end gap-2 text-[12px]">
        <span className="rounded-md px-2.5 py-1 text-fg-2">Cancel</span>
        <span className="rounded-md bg-accent px-3 py-1 font-medium text-white">Save</span>
      </div>
    </div>
  )
}

function Floating({ y, children, className }: { y: MotionValue<number>; children: React.ReactNode; className: string }) {
  return (
    <motion.div style={{ y }} className={`absolute hidden lg:block ${className}`}>
      {children}
    </motion.div>
  )
}

export function Hero() {
  const reduce = useReducedMotion()
  const scene = useRef<HTMLDivElement>(null)
  const { scrollYProgress } = useScroll({ target: scene, offset: ['start end', 'end start'] })
  const near = useTransform(scrollYProgress, [0, 1], [reduce ? 0 : 90, reduce ? 0 : -90])
  const far = useTransform(scrollYProgress, [0, 1], [reduce ? 0 : 40, reduce ? 0 : -40])
  const rise = (d: number) =>
    reduce
      ? {}
      : { initial: { opacity: 0, y: 16, filter: 'blur(6px)' }, animate: { opacity: 1, y: 0, filter: 'blur(0px)' }, transition: { duration: 0.9, delay: d, ease: EASE } }

  return (
    <section id="top" className="relative overflow-hidden pt-36 sm:pt-44">
      {/* Light from above, and the horizon line the product sits on. */}
      <div
        className="pointer-events-none absolute inset-x-0 top-0 h-[900px]"
        style={{ background: 'radial-gradient(60% 50% at 50% 0%, var(--glow), transparent 70%)' }}
        aria-hidden="true"
      />
      <div className="relative mx-auto max-w-5xl px-5 text-center sm:px-8">
        <motion.a
          {...rise(0)}
          href={`${GITHUB}/releases/latest`}
          className="group inline-flex items-center gap-2 rounded-full border border-line bg-tint/[0.03] py-1 pr-3 pl-3 text-[13px] text-fg-2 transition-colors hover:border-line-2 hover:text-fg"
        >
          <span className="size-1.5 rounded-full bg-accent shadow-[0_0_10px_var(--color-accent)]" />
          v0.18 · Reveal now works in dev containers
          <ArrowRight className="size-3.5 transition-transform group-hover:translate-x-0.5" />
        </motion.a>
        <motion.h1 {...rise(0.08)} className="mt-8 text-[clamp(40px,6vw,76px)] leading-[1.04] font-semibold">
          <span className="headline">Your agents use the keys.</span>
          <br />
          <span className="headline dim">They never see them.</span>
        </motion.h1>
        <motion.p {...rise(0.16)} className="mx-auto mt-6 max-w-[34rem] text-[17px] leading-relaxed text-fg-2 sm:text-lg">
          A local vault for Claude Code, Codex and Cursor. Agents use your tokens, passwords and SSH logins by name. keymule adds the
          secret and strips it from every reply.
        </motion.p>
        <motion.div {...rise(0.24)} className="mt-9 flex flex-wrap items-center justify-center gap-3">
          <a href="#install" className="flex h-11 items-center rounded-full bg-fg px-6 text-[15px] font-medium text-bg transition-opacity hover:opacity-90">
            Get keymule
          </a>
          <a
            href={GITHUB}
            className="flex h-11 items-center gap-2 rounded-full border border-line-2 bg-tint/[0.03] px-5 text-[15px] font-medium text-fg transition-colors hover:bg-tint/[0.07]"
          >
            <GithubIcon /> View on GitHub
          </a>
        </motion.div>
        <motion.div {...rise(0.32)} className="mx-auto mt-6 max-w-lg">
          <CopyCommand />
        </motion.div>
      </div>

      <div ref={scene} className="relative mx-auto mt-24 max-w-6xl px-5 pb-28 sm:px-8">
        {/* horizon */}
        <div className="pointer-events-none absolute -top-24 left-1/2 h-[600px] w-[1400px] -translate-x-1/2" aria-hidden="true">
          <div
            className="absolute inset-0 rounded-[100%] border-t border-tint/15"
            style={{ boxShadow: '0 -20px 80px -20px color-mix(in srgb, var(--color-accent) 45%, transparent) inset', maskImage: 'linear-gradient(90deg, transparent, black 25%, black 75%, transparent)' }}
          />
        </div>
        <motion.div
          initial={reduce ? false : { opacity: 0, y: 40 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 1.1, delay: 0.4, ease: EASE }}
          className="relative"
        >
          <motion.div style={{ y: far }} className="window relative overflow-hidden rounded-2xl">
            <div className="flex items-center gap-1.5 border-b border-line bg-bg-3/60 px-4 py-3">
              <span className="size-2.5 rounded-full bg-tint/12" />
              <span className="size-2.5 rounded-full bg-tint/12" />
              <span className="size-2.5 rounded-full bg-tint/12" />
              <span className="mx-auto font-mono text-[11px] text-fg-3">keymule</span>
            </div>
            <img
              src="/app-light.png"
              alt="The keymule page: credentials with their hosts, one opened to show its note, and live activity"
              className="only-light block w-full"
              width={2640}
              height={1640}
            />
            <img src="/app-dark.png" alt="" aria-hidden="true" className="only-dark block w-full" width={2640} height={1640} />
            <div className="pointer-events-none absolute inset-x-0 bottom-0 h-1/3 bg-gradient-to-t from-bg via-bg/60 to-transparent" />
          </motion.div>
          <Floating y={near} className="-left-10 bottom-16">
            <TerminalCard />
          </Floating>
          <Floating y={near} className="-right-8 top-24">
            <DialogCard />
          </Floating>
        </motion.div>
      </div>
    </section>
  )
}
