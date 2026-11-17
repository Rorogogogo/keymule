import { Check, Minus, Plus } from 'lucide-react'
import { CopyCommand, GITHUB, GithubIcon, Logo, Reveal, SectionHead, Tag } from './ui'

const GUARDS = [
  'The value never enters the model’s context, the chat or a transcript',
  'Tokens only go to the hosts they’re locked to; the rules are sealed in the vault',
  'New programs, new hosts, copies to GitHub and deletes ask you in a window the agent can’t click',
  'Every use is logged, without the secret',
  'Everything stays on your machine. No account, no cloud, no telemetry',
]
const LIMITS = [
  'A program you let use a token can still misuse it. keymule decides who gets it, not what they do with it',
  'In a container, the vault’s passphrase is in the environment, so an agent set on decrypting the vault can',
  'Allow all trades questions for convenience in one project. Deleting always asks',
]

export function Security() {
  return (
    <section id="security" className="bg-night text-paper">
      <div className="mx-auto max-w-7xl px-5 py-24 sm:px-8 lg:py-32">
        <Reveal className="max-w-3xl">
          <p className="label text-paper/50">Security, said plainly</p>
          <h2 className="mt-4 font-display text-[clamp(40px,6vw,76px)] leading-[0.95] tracking-[-0.02em]">
            What keymule guards, <em className="text-paper/60">and what it doesn’t.</em>
          </h2>
        </Reveal>
        <div className="mt-14 grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
          <Reveal>
            <div className="h-full rounded-3xl border border-white/10 bg-night-2 p-8">
              <Tag tone="night">Guarded</Tag>
              <ul className="mt-6 space-y-4">
                {GUARDS.map(g => (
                  <li key={g} className="flex gap-3 leading-relaxed text-paper/85">
                    <Check className="mt-1 size-4 shrink-0 text-[#8ce99a]" /> {g}
                  </li>
                ))}
              </ul>
            </div>
          </Reveal>
          <Reveal delay={0.08}>
            <div className="h-full rounded-3xl border border-white/10 p-8">
              <Tag tone="night">Limits</Tag>
              <ul className="mt-6 space-y-4">
                {LIMITS.map(l => (
                  <li key={l} className="flex gap-3 leading-relaxed text-paper/65">
                    <Minus className="mt-1 size-4 shrink-0 text-paper/40" /> {l}
                  </li>
                ))}
              </ul>
            </div>
          </Reveal>
        </div>
        <Reveal delay={0.1} className="mt-10">
          <div className="overflow-hidden rounded-3xl border border-white/10 bg-black/40">
            <div className="flex items-center gap-2 border-b border-white/10 px-5 py-3">
              <span className="size-2.5 rounded-full bg-white/15" />
              <span className="size-2.5 rounded-full bg-white/15" />
              <span className="size-2.5 rounded-full bg-white/15" />
              <span className="label ml-3 text-paper/40">keymule doctor · made-up values</span>
            </div>
            <pre className="overflow-x-auto p-6 font-mono text-[12.5px] leading-[1.8] text-paper/80 [scrollbar-width:none]">
              <span className="text-paper/45">$ keymule doctor</span>
              {'\n'}scanned 48 agent transcripts (390 MB), 5 agent configs, 2 .env files{'\n\n'}
              <span className="text-[#ff8787]">● Sent to an AI model: rotate these (2)</span>
              {'\n'}  sk_l…7Qa   Stripe live key     Codex session in ~/code/shop{'\n'}  ac_4…k9P   ACME_API_KEY        Claude Code session in ~/code/api ×3{'\n\n'}
              <span className="text-[#ffc078]">● Plain text in agent configs: move these into keymule (1)</span>
              {'\n'}  ghp_…T4x   GitHub token        ~/.cursor/mcp.json · github.env.GITHUB_TOKEN
            </pre>
          </div>
        </Reveal>
      </div>
    </section>
  )
}

export function Install() {
  return (
    <section id="install" className="mx-auto max-w-7xl px-5 py-24 sm:px-8 lg:py-32">
      <SectionHead kicker="Get started" title={<>Two steps. <em className="text-ink-2">About a minute.</em></>} />
      <div className="mt-14 grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Reveal>
          <div className="flex h-full flex-col rounded-3xl border border-line bg-card p-8">
            <p className="label text-ink-3">Step 1 · Install and set up</p>
            <p className="mt-4 leading-relaxed text-ink-2">
              Installs keymule, then connects Claude Code, Codex, Cursor, Claude Desktop and Antigravity. Every file it edits is backed up
              first. On a Mac, click <strong className="font-semibold text-ink">Always Allow</strong> when macOS asks about the keychain.
            </p>
            <div className="mt-6">
              <CopyCommand />
            </div>
            <p className="mt-4 text-sm text-ink-3">
              Homebrew: <code className="font-mono text-[13px] text-ink-2">brew install rorogogogo/tap/keymule && keymule init</code>
            </p>
          </div>
        </Reveal>
        <Reveal delay={0.08}>
          <div className="flex h-full flex-col rounded-3xl border border-line bg-ink p-8 text-paper">
            <p className="label text-paper/50">Step 2 · Restart your agent and ask</p>
            <p className="mt-6 font-display text-[44px] leading-[1.02] tracking-[-0.01em] italic">“connect my github”</p>
            <p className="mt-6 leading-relaxed text-paper/70">
              A keymule window opens and you type the token there. The agent only hears “saved, verified as you”, and from then on it can
              use it: <em>“why did my last CI run fail?”</em>
            </p>
          </div>
        </Reveal>
      </div>
    </section>
  )
}

const FAQ = [
  ['Does my token go to a server?', 'No. keymule runs on your machine. The vault is an encrypted file, its key is in your OS keychain, and requests go straight from your machine to the service.'],
  ['Which agents does it work with?', 'Claude Code, Codex, Cursor, Claude Desktop and Antigravity, through MCP. Paste-to-save works in Claude Code, Codex and Cursor. CLI tools like gh, git, psql and ssh work through keymule exec.'],
  ['What if I paste a token anyway?', 'keymule’s prompt hook catches it before the message is sent, asks whether to save it, and sends the message on with the token’s name instead.'],
  ['Can I see a stored value?', 'Yes: Reveal shows it in a keymule window, or in your paired browser in a container, for 30 seconds. Never on stdout, where an agent could read it.'],
  ['Does it work in a dev container?', 'Yes. Set KEYMULE_PASSPHRASE where your agents run (there’s usually no keychain), and answer keymule’s questions from the page in your Mac’s browser.'],
  ['What does it cost?', 'Nothing. keymule is open source under MIT or Apache-2.0.'],
]

export function Faq() {
  return (
    <section id="faq" className="border-t border-line bg-paper-2/40">
      <div className="mx-auto grid max-w-7xl grid-cols-1 gap-12 px-5 py-24 sm:px-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)] lg:py-32">
        <SectionHead kicker="FAQ" title={<>Before you <em className="text-ink-2">hand over the keys.</em></>} />
        <div className="border-t border-line">
          {FAQ.map(([q, a]) => (
            <details key={q} className="group border-b border-line">
              <summary className="flex cursor-pointer items-center justify-between gap-6 py-6 text-lg font-medium tracking-tight">
                {q}
                <Plus className="size-5 shrink-0 text-ink-3 transition-transform duration-300 group-open:rotate-45" />
              </summary>
              <p className="-mt-1 pb-6 leading-relaxed text-ink-2">{a}</p>
            </details>
          ))}
        </div>
      </div>
    </section>
  )
}

export function Footer() {
  return (
    <footer className="bg-night text-paper">
      <div className="mx-auto max-w-7xl px-5 pt-20 pb-10 sm:px-8">
        <div className="flex flex-wrap items-end justify-between gap-8">
          <h2 className="font-display text-[clamp(64px,14vw,200px)] leading-[0.8] tracking-[-0.04em]">keymule</h2>
          <a
            href={GITHUB}
            className="mb-3 inline-flex h-11 items-center gap-2 rounded-full bg-paper px-5 text-sm font-medium text-ink transition-transform hover:-translate-y-px"
          >
            <GithubIcon /> Star on GitHub
          </a>
        </div>
        <div className="mt-14 flex flex-wrap items-center justify-between gap-6 border-t border-white/10 pt-8 text-sm text-paper/55">
          <div className="flex items-center gap-3">
            <Logo className="size-6" invert />
            <span>Your agents carry the keys. They never open the pack.</span>
          </div>
          <div className="flex flex-wrap gap-6">
            <a className="hover:text-paper" href={`${GITHUB}#readme`}>Docs</a>
            <a className="hover:text-paper" href={`${GITHUB}/releases`}>Releases</a>
            <a className="hover:text-paper" href="https://nomoreide.com">A NoMoreIDE project</a>
            <span>MIT or Apache-2.0</span>
          </div>
        </div>
      </div>
    </footer>
  )
}
