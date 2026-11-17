import { useState, type ReactNode } from 'react'
import { motion, useReducedMotion } from 'motion/react'
import { Check, Copy } from 'lucide-react'

export const INSTALL = 'curl -fsSL https://raw.githubusercontent.com/Rorogogogo/keymule/main/install.sh | sh'
export const GITHUB = 'https://github.com/Rorogogogo/keymule'

/** Fades and lifts its children in once, when they scroll into view. */
export function Reveal({ children, delay = 0, className }: { children: ReactNode; delay?: number; className?: string }) {
  const reduce = useReducedMotion()
  return (
    <motion.div
      className={className}
      initial={reduce ? false : { opacity: 0, y: 24 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: '-80px' }}
      transition={{ duration: 0.7, delay, ease: [0.22, 1, 0.36, 1] }}
    >
      {children}
    </motion.div>
  )
}

/** keymule's keyhole mark: a black square with a white keyhole. */
export function Logo({ className = 'size-7', invert = false }: { className?: string; invert?: boolean }) {
  const bg = invert ? '#f3f0e8' : '#141412'
  const fg = invert ? '#141412' : '#f3f0e8'
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true">
      <rect width="24" height="24" rx="6" fill={bg} />
      <circle cx="12" cy="9.6" r="3.3" fill={fg} />
      <path d="M10.3 11.8 9.4 17.6h5.2l-.9-5.8z" fill={fg} />
    </svg>
  )
}

const TAG_TONES = {
  seal: 'text-seal border-seal/40 bg-seal/5',
  ok: 'text-ok border-ok/40 bg-ok/5',
  accent: 'text-accent border-accent/40 bg-accent/5',
  ink: 'text-ink-2 border-line bg-card',
  night: 'text-paper/70 border-white/15 bg-white/5',
} as const

/** A status tag in the manifest style: SEALED, LOCKED, REDACTED… */
export function Tag({ children, tone = 'ink' }: { children: ReactNode; tone?: keyof typeof TAG_TONES }) {
  return (
    <span className={`label inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 whitespace-nowrap ${TAG_TONES[tone]}`}>
      {children}
    </span>
  )
}

export function SectionHead({ kicker, title, children }: { kicker: string; title: ReactNode; children?: ReactNode }) {
  return (
    <Reveal className="max-w-3xl">
      <p className="label text-ink-3">{kicker}</p>
      <h2 className="mt-4 font-display text-[clamp(40px,6vw,76px)] leading-[0.95] tracking-[-0.02em]">{title}</h2>
      {children && <p className="mt-5 max-w-2xl text-lg leading-relaxed text-ink-2">{children}</p>}
    </Reveal>
  )
}

/** The install one-liner with a copy button. */
export function CopyCommand({ command = INSTALL, dark = false }: { command?: string; dark?: boolean }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command)
      setCopied(true)
      setTimeout(() => setCopied(false), 1600)
    } catch {
      /* clipboard blocked: the text is still selectable */
    }
  }
  return (
    <div
      className={`group flex w-full items-center gap-3 rounded-2xl border py-2 pl-4 pr-2 font-mono text-[13px] ${
        dark ? 'border-white/12 bg-white/[0.04] text-paper' : 'border-line bg-card text-ink shadow-[0_1px_0_rgb(20_20_18/0.04)]'
      }`}
    >
      <span className={dark ? 'text-paper/40' : 'text-ink-3'} aria-hidden="true">
        $
      </span>
      <code className="min-w-0 flex-1 overflow-x-auto whitespace-nowrap [scrollbar-width:none] select-all">{command}</code>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? 'Copied' : 'Copy the install command'}
        className={`flex h-9 shrink-0 items-center gap-1.5 rounded-xl px-3 font-sans text-[13px] font-medium transition-colors ${
          dark ? 'bg-paper text-ink hover:bg-white' : 'bg-ink text-paper hover:bg-black'
        }`}
      >
        {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
        {copied ? 'Copied' : 'Copy'}
      </button>
    </div>
  )
}

/** GitHub's mark (lucide no longer ships brand icons). */
export function GithubIcon({ className = 'size-4' }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} fill="currentColor" aria-hidden="true">
      <path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.52-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.7 1.26 3.36.96.1-.75.4-1.26.73-1.55-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.29 1.19-3.1-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.81 1.19 1.84 1.19 3.1 0 4.42-2.7 5.39-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z" />
    </svg>
  )
}
