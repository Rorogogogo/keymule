import { useEffect, useRef, useState } from 'react'
import { motion, useReducedMotion, useScroll, useTransform } from 'motion/react'
import { ArrowRight, Play } from 'lucide-react'
import { CopyCommand, GITHUB, GithubIcon, Logo } from './ui'

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
      className={`fixed inset-x-0 top-0 z-40 transition-[background-color,border-color] duration-300 ${
        scrolled ? 'border-b border-line bg-bg/70 backdrop-blur-xl' : 'border-b border-transparent'
      }`}
    >
      <nav className="mx-auto flex h-16 max-w-6xl items-center gap-8 px-5 sm:px-8" aria-label="Main">
        <a href="#top" className="flex items-center gap-2.5 font-semibold tracking-tight">
          <Logo className="size-6" />
          <span>keymule</span>
        </a>
        <div className="hidden items-center gap-7 text-sm text-fg-2 md:flex">
          {links.map(([label, href]) => (
            <a key={href} href={href} className="transition-colors hover:text-fg">
              {label}
            </a>
          ))}
        </div>
        <div className="ml-auto flex items-center gap-2">
          <a href={GITHUB} className="hidden h-9 items-center gap-2 rounded-full px-3.5 text-sm text-fg-2 transition-colors hover:text-fg sm:flex">
            <GithubIcon /> GitHub
          </a>
          <a
            href="#install"
            className="flex h-9 items-center rounded-full bg-white px-4 text-sm font-medium text-black transition-transform hover:scale-[1.03] active:scale-[0.98]"
          >
            Install
          </a>
        </div>
      </nav>
    </header>
  )
}

export function Hero() {
  const reduce = useReducedMotion()
  const shot = useRef<HTMLDivElement>(null)
  // The app screenshot starts tilted back and stands up as you scroll.
  const { scrollYProgress } = useScroll({ target: shot, offset: ['start end', 'start 30%'] })
  const rotateX = useTransform(scrollYProgress, [0, 1], [reduce ? 0 : 22, 0])
  const scale = useTransform(scrollYProgress, [0, 1], [reduce ? 1 : 0.92, 1])
  const rise = (d: number) =>
    reduce
      ? {}
      : {
          initial: { opacity: 0, y: 24, filter: 'blur(8px)' },
          animate: { opacity: 1, y: 0, filter: 'blur(0px)' },
          transition: { duration: 0.9, delay: d, ease: [0.22, 1, 0.36, 1] as const },
        }

  return (
    <section id="top" className="relative overflow-hidden pt-36 sm:pt-44">
      <div className="grid-bg pointer-events-none absolute inset-0" aria-hidden="true" />
      <div
        className="pointer-events-none absolute top-[-280px] left-1/2 h-[720px] w-[1100px] -translate-x-1/2 rounded-full opacity-70 blur-3xl"
        style={{ background: 'radial-gradient(closest-side, rgb(139 123 255 / 0.45), rgb(91 140 255 / 0.15) 55%, transparent)' }}
        aria-hidden="true"
      />
      <div className="relative mx-auto max-w-6xl px-5 text-center sm:px-8">
        <motion.a
          {...rise(0)}
          href={`${GITHUB}/releases/latest`}
          className="glass group inline-flex items-center gap-2 rounded-full py-1 pr-3 pl-1 text-[13px] text-fg-2 backdrop-blur-xl transition-colors hover:text-fg"
        >
          <span className="rounded-full bg-accent/20 px-2 py-0.5 text-[12px] font-medium text-accent">New</span>
          Reveal now works in dev containers
          <ArrowRight className="size-3.5 transition-transform group-hover:translate-x-0.5" />
        </motion.a>
        <motion.h1 {...rise(0.08)} className="headline mt-8 text-[clamp(42px,6.4vw,88px)]">
          Your agents use the keys.
          <br />
          They never see them.
        </motion.h1>
        <motion.p {...rise(0.16)} className="mx-auto mt-6 max-w-2xl text-lg leading-relaxed text-fg-2 sm:text-xl">
          keymule lets Claude Code, Codex and Cursor use your API tokens, passwords and SSH logins by name. It adds the secret itself
          and strips it from every reply, so it never reaches the model, the chat or a transcript.
        </motion.p>
        <motion.div {...rise(0.24)} className="mx-auto mt-10 max-w-xl">
          <CopyCommand />
          <div className="mt-5 flex flex-wrap items-center justify-center gap-x-6 gap-y-2 text-sm text-fg-2">
            <a href="#film" className="group inline-flex items-center gap-2 transition-colors hover:text-fg">
              <span className="flex size-6 items-center justify-center rounded-full bg-white/10 ring-1 ring-white/15 ring-inset">
                <Play className="size-3 fill-current" />
              </span>
              Watch the 30-second film
            </a>
            <span className="text-fg-3">macOS · Linux · dev containers · free and open source</span>
          </div>
        </motion.div>
      </div>

      <div className="relative mx-auto mt-20 max-w-6xl px-5 pb-24 sm:px-8 [perspective:1600px]">
        <motion.div ref={shot} style={{ rotateX, scale, transformOrigin: '50% 0%' }} className="relative">
          <div
            className="pointer-events-none absolute -inset-x-10 -bottom-10 top-1/3 rounded-full opacity-60 blur-3xl"
            style={{ background: 'radial-gradient(closest-side, rgb(139 123 255 / 0.35), transparent)' }}
            aria-hidden="true"
          />
          <div className="glass relative overflow-hidden rounded-[22px] p-1.5 sm:p-2">
            <img
              src="/app-dark.png"
              alt="The keymule page: credentials with their hosts, one opened to show its note, and live activity"
              className="block w-full rounded-[16px] border border-line"
              width={2640}
              height={1640}
            />
          </div>
          <div className="pointer-events-none absolute inset-x-0 bottom-0 h-40 bg-gradient-to-t from-bg to-transparent" aria-hidden="true" />
        </motion.div>
      </div>
    </section>
  )
}
