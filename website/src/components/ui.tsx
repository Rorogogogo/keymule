import { useState, type MouseEvent, type ReactNode } from 'react'
import { motion, useReducedMotion } from 'motion/react'
import { Check, Copy } from 'lucide-react'

export const INSTALL = 'curl -fsSL https://raw.githubusercontent.com/Rorogogogo/keymule/main/install.sh | sh'
export const GITHUB = 'https://github.com/Rorogogogo/keymule'

const EASE = [0.22, 1, 0.36, 1] as const

/** Fades, lifts and un-blurs its children once, as they scroll into view. */
export function Reveal({ children, delay = 0, className }: { children: ReactNode; delay?: number; className?: string }) {
  const reduce = useReducedMotion()
  return (
    <motion.div
      className={className}
      initial={reduce ? false : { opacity: 0, y: 20, filter: 'blur(6px)' }}
      whileInView={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
      viewport={{ once: true, margin: '-80px' }}
      transition={{ duration: 0.8, delay, ease: EASE }}
    >
      {children}
    </motion.div>
  )
}

/** keymule's keyhole mark. */
export function Logo({ className = 'size-7' }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true">
      <rect width="24" height="24" rx="6" fill="#f4f4f6" />
      <circle cx="12" cy="9.6" r="3.3" fill="#07070a" />
      <path d="M10.3 11.8 9.4 17.6h5.2l-.9-5.8z" fill="#07070a" />
    </svg>
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

/** Tracks the cursor so `.spot` cards light up under it. */
export function spotlight(e: MouseEvent<HTMLElement>) {
  const r = e.currentTarget.getBoundingClientRect()
  e.currentTarget.style.setProperty('--x', `${e.clientX - r.left}px`)
  e.currentTarget.style.setProperty('--y', `${e.clientY - r.top}px`)
}

const TONES = {
  accent: 'text-accent bg-accent/10 ring-accent/25',
  ok: 'text-ok bg-ok/10 ring-ok/25',
  warn: 'text-warn bg-warn/10 ring-warn/25',
  bad: 'text-bad bg-bad/10 ring-bad/25',
  muted: 'text-fg-2 bg-white/5 ring-white/10',
} as const

export function Tag({ children, tone = 'muted' }: { children: ReactNode; tone?: keyof typeof TONES }) {
  return (
    <span className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 font-mono text-[11.5px] whitespace-nowrap ring-1 ring-inset ${TONES[tone]}`}>
      {children}
    </span>
  )
}

export function SectionHead({ kicker, title, children, center = false }: { kicker: string; title: ReactNode; children?: ReactNode; center?: boolean }) {
  return (
    <Reveal className={center ? 'mx-auto max-w-3xl text-center' : 'max-w-3xl'}>
      <p className="eyebrow">{kicker}</p>
      <h2 className="headline mt-5 text-[clamp(32px,4.2vw,52px)]">{title}</h2>
      {children && <p className={`mt-5 text-[17px] leading-relaxed text-fg-2 ${center ? 'mx-auto max-w-xl' : 'max-w-xl'}`}>{children}</p>}
    </Reveal>
  )
}

/** The install one-liner with a copy button. */
export function CopyCommand({ command = INSTALL }: { command?: string }) {
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
    <div className="panel flex w-full items-center gap-3 rounded-xl py-1.5 pr-1.5 pl-4 font-mono text-[13px] backdrop-blur-xl">
      <span className="text-fg-3" aria-hidden="true">
        $
      </span>
      <code className="min-w-0 flex-1 overflow-x-auto whitespace-nowrap text-left text-fg [scrollbar-width:none] select-all">{command}</code>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? 'Copied' : 'Copy the install command'}
        className="flex h-8 shrink-0 items-center gap-1.5 rounded-lg bg-white/[0.08] px-3 font-sans text-[13px] font-medium text-fg ring-1 ring-white/10 ring-inset transition-colors hover:bg-white/[0.14]"
      >
        {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
        {copied ? 'Copied' : 'Copy'}
      </button>
    </div>
  )
}
