import { Check, Minus, Plus } from 'lucide-react'
import { CopyCommand, GITHUB, GithubIcon, Logo, Reveal, SectionHead, Tag, spotlight } from './ui'

const GUARDS = [
  'The value never enters the model’s context, the chat or a transcript',
  'Tokens only go to the hosts they’re locked to, and those rules are sealed in the vault',
  'New programs, new hosts, copies to GitHub and deletes ask you in a window the agent can’t click',
  'Every use is logged, without the secret',
  'Everything stays on your machine. No account, no cloud, no telemetry',
]
const LIMITS = [
  'A program you let use a token can still misuse it. keymule decides who gets it, not what they do with it',
  'In a container the vault’s passphrase is in the environment, so an agent set on decrypting the vault can',
  'Allow all trades questions for convenience in one project. Deleting always asks',
]

export function Security() {
  return (
    <section id="security" className="mx-auto max-w-6xl px-5 py-24 sm:px-8 lg:py-32">
      <SectionHead kicker="Security, said plainly" title="What keymule guards, and what it doesn’t." />
      <div className="mt-14 grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
        <Reveal>
          <div onMouseMove={spotlight} className="spot glass h-full rounded-3xl p-8">
            <Tag tone="ok">Guarded</Tag>
            <ul className="mt-6 space-y-4">
              {GUARDS.map(g => (
                <li key={g} className="flex gap-3 leading-relaxed text-fg">
                  <Check className="mt-1 size-4 shrink-0 text-ok" /> {g}
                </li>
              ))}
            </ul>
          </div>
        </Reveal>
        <Reveal delay={0.08}>
          <div className="h-full rounded-3xl border border-dashed border-line-2 p-8">
            <Tag>Limits</Tag>
            <ul className="mt-6 space-y-4">
              {LIMITS.map(l => (
                <li key={l} className="flex gap-3 leading-relaxed text-fg-2">
                  <Minus className="mt-1 size-4 shrink-0 text-fg-3" /> {l}
                </li>
              ))}
            </ul>
          </div>
        </Reveal>
      </div>
      <Reveal delay={0.1} className="mt-4">
        <div className="glass overflow-hidden rounded-3xl">
          <div className="flex items-center gap-2 border-b border-line px-5 py-3">
            <span className="size-2.5 rounded-full bg-white/15" />
            <span className="size-2.5 rounded-full bg-white/15" />
            <span className="size-2.5 rounded-full bg-white/15" />
            <span className="ml-3 font-mono text-[12px] text-fg-3">keymule doctor · made-up values</span>
          </div>
          <pre className="overflow-x-auto p-6 font-mono text-[12.5px] leading-[1.85] text-fg-2 [scrollbar-width:none]">
            <span className="text-fg-3">$ keymule doctor</span>
            {'\n'}scanned 48 agent transcripts (390 MB), 5 agent configs, 2 .env files{'\n\n'}
            <span className="text-bad">● Sent to an AI model: rotate these (2)</span>
            {'\n'}  sk_l…7Qa   Stripe live key     Codex session in ~/code/shop{'\n'}  ac_4…k9P   ACME_API_KEY        Claude Code session in ~/code/api ×3{'\n\n'}
            <span className="text-warn">● Plain text in agent configs: move these into keymule (1)</span>
            {'\n'}  ghp_…T4x   GitHub token        ~/.cursor/mcp.json · github.env.GITHUB_TOKEN
          </pre>
        </div>
      </Reveal>
    </section>
  )
}

export function Install() {
  return (
    <section id="install" className="px-5 py-24 sm:px-8 lg:py-32">
      <Reveal className="relative mx-auto max-w-5xl overflow-hidden rounded-[32px] border border-line px-6 py-16 text-center sm:px-12 sm:py-20">
        <div
          className="pointer-events-none absolute inset-0 -z-10"
          style={{ background: 'radial-gradient(ellipse 80% 70% at 50% 0%, rgb(139 123 255 / 0.28), transparent 70%), #0c0c11' }}
          aria-hidden="true"
        />
        <p className="kicker">Get started</p>
        <h2 className="headline mt-4 text-[clamp(36px,5.2vw,64px)]">Set up in about a minute.</h2>
        <p className="mx-auto mt-5 max-w-xl text-lg leading-relaxed text-fg-2">
          One command installs keymule and connects Claude Code, Codex, Cursor, Claude Desktop and Antigravity. Every file it edits is
          backed up first.
        </p>
        <div className="mx-auto mt-10 max-w-xl">
          <CopyCommand />
        </div>
        <p className="mt-4 text-sm text-fg-3">
          or <code className="font-mono text-[13px] text-fg-2">brew install rorogogogo/tap/keymule && keymule init</code>
        </p>
        <div className="mx-auto mt-12 max-w-xl border-t border-line pt-10">
          <p className="text-sm text-fg-3">Then restart your agent and ask</p>
          <p className="mt-3 text-3xl font-semibold tracking-[-0.03em]">“connect my github”</p>
          <p className="mt-3 text-fg-2">A keymule window opens, you type the token there, and you’re done.</p>
        </div>
      </Reveal>
    </section>
  )
}

const FAQ = [
  ['Does my token go to a server?', 'No. keymule runs on your machine. The vault is an encrypted file, its key is in your OS keychain, and requests go straight from your machine to the service.'],
  ['Which agents does it work with?', 'Claude Code, Codex, Cursor, Claude Desktop and Antigravity, through MCP. Paste-to-save works in Claude Code, Codex and Cursor. CLI tools like gh, git, psql and ssh work through keymule exec.'],
  ['What if I paste a token anyway?', 'keymule’s prompt hook catches it before the message is sent, asks whether to save it, and sends the message on with the token’s name instead.'],
  ['Can I see a stored value?', 'Yes. Reveal shows it in a keymule window, or in your paired browser in a container, for 30 seconds. Never on stdout, where an agent could read it.'],
  ['Does it work in a dev container?', 'Yes. Set KEYMULE_PASSPHRASE where your agents run (there’s usually no keychain), and answer keymule’s questions from the page in your own browser.'],
  ['What does it cost?', 'Nothing. keymule is open source under MIT or Apache-2.0.'],
]

export function Faq() {
  return (
    <section id="faq" className="mx-auto grid max-w-6xl grid-cols-1 gap-12 px-5 py-24 sm:px-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)] lg:py-32">
      <SectionHead kicker="FAQ" title="Questions, answered." />
      <div className="border-t border-line">
        {FAQ.map(([q, a]) => (
          <details key={q} className="group border-b border-line">
            <summary className="flex cursor-pointer items-center justify-between gap-6 py-5 text-[17px] font-medium tracking-tight transition-colors hover:text-white">
              {q}
              <Plus className="size-5 shrink-0 text-fg-3 transition-transform duration-300 group-open:rotate-45" />
            </summary>
            <p className="-mt-1 pb-5 leading-relaxed text-fg-2">{a}</p>
          </details>
        ))}
      </div>
    </section>
  )
}

export function Footer() {
  return (
    <footer className="relative overflow-hidden border-t border-line">
      <div className="mx-auto max-w-6xl px-5 pt-16 pb-10 sm:px-8">
        <div className="flex flex-wrap items-center justify-between gap-6">
          <div className="flex items-center gap-3">
            <Logo className="size-7" />
            <div>
              <p className="font-semibold tracking-tight">keymule</p>
              <p className="text-sm text-fg-3">Your agents use the keys. They never see them.</p>
            </div>
          </div>
          <a
            href={GITHUB}
            className="glass inline-flex h-10 items-center gap-2 rounded-full px-4 text-sm font-medium transition-colors hover:bg-white/10"
          >
            <GithubIcon /> Star on GitHub
          </a>
        </div>
        <div className="mt-12 flex flex-wrap items-center justify-between gap-4 border-t border-line pt-6 text-sm text-fg-3">
          <span>MIT or Apache-2.0</span>
          <div className="flex flex-wrap gap-6">
            <a className="transition-colors hover:text-fg" href={`${GITHUB}#readme`}>Docs</a>
            <a className="transition-colors hover:text-fg" href={`${GITHUB}/releases`}>Releases</a>
            <a className="transition-colors hover:text-fg" href="https://nomoreide.com">A NoMoreIDE project</a>
          </div>
        </div>
      </div>
      <p
        className="pointer-events-none -mb-[0.22em] text-center text-[clamp(80px,19vw,280px)] leading-none font-semibold tracking-[-0.06em] select-none"
        style={{ background: 'linear-gradient(180deg, rgb(255 255 255 / 0.10), transparent 85%)', WebkitBackgroundClip: 'text', backgroundClip: 'text', color: 'transparent' }}
        aria-hidden="true"
      >
        keymule
      </p>
    </footer>
  )
}
