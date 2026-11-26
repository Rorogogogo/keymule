// keymule launch film, v4: "one key's journey".
// Flat, front-facing UI; GSAP timelines (SplitText, ScrambleText, Flip, TextPlugin)
// seeked per frame; every motion is followed by a hold long enough to read.
import React from "react";
import { AbsoluteFill, Img, Sequence, interpolate, staticFile } from "remotion";
import { Audio } from "@remotion/media";
import { Flip, MONO, SANS, gsap, reveal, typeIn, useGsap } from "./gsapRemotion";

const C = {
  bg: "#f4f4f7",
  ink: "#1d1d1f",
  ink2: "#6e6e73",
  ink3: "#a1a1a6",
  accent: "#635bff",
  accentSoft: "#a5a0ff",
  termBg: "#141416",
  termBar: "#202023",
  tText: "#e8e8ea",
  tDim: "#8b8b92",
  tGreen: "#4ade80",
  tAmber: "#f5a524",
  tAmberText: "#ffd38f",
  red: "rgba(255,107,107,0.95)",
};

// Made-up values in real formats.
const TOKEN = "ghp_7Kq2vX9mZ4pL8wR3nT6yB1cF5hJ0dS";
const OPENAI = "sk-proj-Q2demoX7dEmo4FakeK3yNotReal9";
const DOTS = "•".repeat(30);
const SCRAMBLE = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz0123456789#$%&*+";

// ---- copy, in English and Chinese -------------------------------------------------------
// The product's own UI (dialog, page, hook output, tool calls) stays in English, as it ships.
type Lang = "en" | "zh";
const LangCtx = React.createContext<Lang>("en");
const STR = {
  en: {
    ls: "-0.03em",
    p1: "deploy's broken, here's my token ",
    warn1: "That's a live GitHub token, in plain text.",
    warn2: (c: string) => (
      <>
        <b style={{ color: c }}>Revoke it now</b> and create a new one.
      </>
    ),
    newSession: "acme-web — agent · new session",
    p2: "is CI green now?",
    r1: "I don't have your GitHub token anymore.",
    r2: (c: string) => (
      <>
        Could you <b style={{ color: c }}>paste it again</b>?
      </>
    ),
    t1: "Give your agent",
    t2a: "a",
    kv: "key vault.",
    fp: "why is CI failing on acme/web?",
    wait: "⎿  A keymule dialog is open. Waiting for you…",
    saved: "Saved ",
    savedRest: "the agent never saw the token",
    cap1: "Your agent uses it. It never sees it.",
    clock: "Mon 16 Nov 21:52",
    cap2: "One vault. Every use on the record.",
    later: "A new session. The agent remembers nothing.",
    p3: "CI is red again, take a look",
    flaky: "⎿  200 OK · a flaky test in auth.spec.ts. Re-ran it: green.",
    cap3: "Still there. Nothing to paste.",
    gp: "summarize CI with this key: ",
    cap4: "Pasted it by habit? keymule catches it.",
    head: "Your agent's key vault.",
    sub: "Used by your agent, never seen by it.",
    services: ["GitHub", "GitLab", "Jenkins", "Bamboo", "OpenAI", "Stripe", "SSH logins", "Passwords", "Any HTTP API", "MCP servers"],
  },
  zh: {
    ls: "0em",
    p1: "部署挂了，token 给你：",
    warn1: "这是一个有效的 GitHub token，而且是明文。",
    warn2: (c: string) => (
      <>
        <b style={{ color: c }}>请立即撤销</b>，然后重新生成一个。
      </>
    ),
    newSession: "acme-web — agent · 新会话",
    p2: "CI 现在过了吗？",
    r1: "我这里已经没有你的 GitHub token 了。",
    r2: (c: string) => (
      <>
        能<b style={{ color: c }}>再贴一次</b>吗？
      </>
    ),
    t1: "给 agent 一个",
    t2a: "",
    kv: "专属的 key vault",
    fp: "acme/web 的 CI 为什么挂了？",
    wait: "⎿  已打开 keymule 弹窗，等你输入…",
    saved: "已保存 ",
    savedRest: "agent 全程没看到 token",
    cap1: "agent 能用，但永远看不到。",
    clock: "11月16日 周一 21:52",
    cap2: "一个 vault，每次使用都有记录。",
    later: "新会话，agent 什么都不记得。",
    p3: "CI 又红了，看一下",
    flaky: "⎿  200 OK · auth.spec.ts 里有个不稳定的测试，重跑后通过。",
    cap3: "key 还在，不用再贴。",
    gp: "用这个 key 总结一下 CI：",
    cap4: "顺手贴了 key？keymule 帮你拦下。",
    head: "agent 专属的 key vault",
    sub: "Token、密码、SSH 登录，交给 keymule 保管。",
    services: ["GitHub", "GitLab", "Jenkins", "Bamboo", "OpenAI", "Stripe", "SSH 登录", "密码", "任意 HTTP API", "MCP 服务器"],
  },
};
const useT = () => {
  const lang = React.useContext(LangCtx);
  return { ...STR[lang], lang };
};

// ---- timeline (absolute frames, 30 fps). Keep in step with scripts/make_audio.py.
const T = {
  problem: [0, 150], // 0.0 – 5.0
  title: [150, 240], // 5.0 – 8.0
  flow: [240, 516], // 8.0 – 17.2
  ui: [516, 630], // 17.2 – 21.0
  reuse: [630, 744], // 21.0 – 24.8
  guard: [744, 834], // 24.8 – 27.8
  end: [834, 900], // 27.8 – 30.0
} as const;

// ---- shared pieces --------------------------------------------------------------------

const Backdrop: React.FC<{ dark?: boolean }> = ({ dark }) => (
  <AbsoluteFill style={{ background: dark ? "#0a0a0c" : C.bg, overflow: "hidden" }}>
    <div
      style={{
        position: "absolute",
        width: 1300,
        height: 1300,
        left: -300,
        top: -560,
        borderRadius: "50%",
        background: dark
          ? "radial-gradient(circle, rgba(99,91,255,0.35), rgba(99,91,255,0) 65%)"
          : "radial-gradient(circle, rgba(165,160,255,0.4), rgba(165,160,255,0) 65%)",
      }}
    />
    <div
      style={{
        position: "absolute",
        width: 1200,
        height: 1200,
        right: -320,
        bottom: -600,
        borderRadius: "50%",
        background: dark
          ? "radial-gradient(circle, rgba(255,140,90,0.16), rgba(255,140,90,0) 65%)"
          : "radial-gradient(circle, rgba(255,186,150,0.38), rgba(255,186,150,0) 65%)",
      }}
    />
  </AbsoluteFill>
);

const Logo: React.FC<{ size: number }> = ({ size }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" style={{ display: "block" }}>
    <rect width="24" height="24" rx="6" fill={C.ink} />
    <circle cx="12" cy="9.6" r="3.3" fill="#fff" />
    <path d="M10.3 11.8 9.4 17.6h5.2l-.9-5.8z" fill="#fff" />
  </svg>
);

const KeyIcon: React.FC<{ size: number; color: string }> = ({ size, color }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth={2.2} strokeLinecap="round" strokeLinejoin="round">
    <path d="M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z" />
    <circle cx="16.5" cy="7.5" r=".5" fill={color} />
  </svg>
);

/** The named key: the object that travels through the film. */
const Pill: React.FC<{ name: string; size?: number; style?: React.CSSProperties; className?: string }> = ({
  name,
  size = 27,
  style,
  className,
}) => (
  <span
    className={className}
    style={{
      display: "inline-flex",
      alignItems: "center",
      gap: 9,
      padding: "4px 16px",
      borderRadius: 999,
      background: C.accent,
      color: "#fff",
      fontFamily: SANS,
      fontWeight: 600,
      fontSize: size,
      lineHeight: 1.25,
      verticalAlign: "middle",
      ...style,
    }}
  >
    <KeyIcon size={size * 0.85} color="#fff" />
    {name}
  </span>
);

const row: React.CSSProperties = { display: "flex", lineHeight: 1.5, whiteSpace: "pre-wrap" };
const Gut: React.FC<{ g: string; color: string }> = ({ g, color }) => (
  <span style={{ color, width: "1.6em", flex: "none", display: "inline-block" }}>{g}</span>
);
const Prompt: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <div style={{ ...row, background: "rgba(255,255,255,0.06)", borderRadius: 12, padding: "10px 16px", margin: "0 -18px 4px" }}>
    <Gut g=">" color={C.tDim} />
    <span style={{ color: C.tText, flex: 1, wordBreak: "break-all" }}>{children}</span>
  </div>
);

const Window: React.FC<{
  title: string;
  left: number;
  top: number;
  width: number;
  height: number;
  size: number;
  className?: string;
  titleClass?: string;
  children: React.ReactNode;
}> = ({ title, left, top, width, height, size, className, titleClass, children }) => (
  <div
    className={className}
    style={{
      position: "absolute",
      left,
      top,
      width,
      height,
      borderRadius: 22,
      overflow: "hidden",
      background: C.termBg,
      boxShadow: "0 40px 100px rgba(20,20,40,0.25), 0 0 0 1px rgba(0,0,0,0.25)",
    }}
  >
    <div style={{ height: 48, background: C.termBar, display: "flex", alignItems: "center", gap: 10, padding: "0 20px" }}>
      {["#ff5f57", "#febc2e", "#28c840"].map((c) => (
        <div key={c} style={{ width: 14, height: 14, borderRadius: 7, background: c }} />
      ))}
      <div className={titleClass} style={{ flex: 1, textAlign: "center", fontFamily: SANS, fontSize: 21, color: C.tDim, marginRight: 62 }}>
        {title}
      </div>
    </div>
    <div style={{ position: "relative", padding: "30px 46px", fontFamily: MONO, fontSize: size, height: height - 48 }}>{children}</div>
  </div>
);

const Caption: React.FC<{ text: string; top: number }> = ({ text, top }) => {
  const t = useT();
  return (
  <div
    className="caption"
    style={{
      position: "absolute",
      left: 0,
      right: 0,
      top,
      textAlign: "center",
      fontFamily: SANS,
      fontSize: 62,
      fontWeight: 700,
      letterSpacing: t.ls,
      color: C.ink,
    }}
  >
    {text}
  </div>
  );
};

const hidden: React.CSSProperties = { opacity: 0, visibility: "hidden" };

const Cursor: React.FC<{ className: string }> = ({ className }) => (
  <svg className={className} width={46} height={46} viewBox="0 0 24 24" style={{ position: "absolute", left: 0, top: 0, ...hidden }}>
    <path d="M4 2.5 19 13.2l-6.4.9 3.6 7-2.6 1.3-3.6-7.1L4.6 19.6z" fill="#111" stroke="#fff" strokeWidth={1.4} strokeLinejoin="round" />
  </svg>
);

// ---- 1. the problem (0 – 5 s) ------------------------------------------------------------

const Problem: React.FC = () => {
  const t = useT();
  const root = useGsap((tl, q) => {
    typeIn(tl, q(".p1-typed")[0], t.p1, 0.05, 48);
    tl.fromTo(q(".p1-tok"), { autoAlpha: 0 }, { autoAlpha: 1, duration: 0.01 }, 0.72);
    tl.fromTo(q(".p1-tok"), { backgroundColor: "rgba(255,107,107,0.75)" }, { backgroundColor: "rgba(255,107,107,0.22)", duration: 0.6 }, 0.73);
    reveal(tl, q(".w1 .txt"), 1.0);
    reveal(tl, q(".w2 .txt"), 1.12);
    // a new session: the conversation slides away, and the agent has forgotten
    tl.to(q(".s1"), { yPercent: -30, autoAlpha: 0, duration: 0.45, ease: "power3.in" }, 2.65);
    tl.to(q(".ttl"), { duration: 0.4, text: { value: t.newSession } }, 2.75);
    tl.set(q(".s2"), { autoAlpha: 1 }, 2.95);
    tl.from(q(".divider"), { scaleX: 0, duration: 0.5, ease: "expo.out" }, 2.95);
    typeIn(tl, q(".p2-typed")[0], t.p2, 3.1, 40);
    reveal(tl, q(".r1 .txt"), 3.65);
    reveal(tl, q(".r2 .txt"), 3.78);
  });
  return (
    <AbsoluteFill>
      <Backdrop />
      <div ref={root} style={{ position: "absolute", inset: 0 }}>
        <Window title="acme-web — agent" titleClass="ttl" left={150} top={220} width={1620} height={560} size={42}>
          <div className="s1" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            <Prompt>
              <span className="p1-typed">{t.p1}</span>
              <span className="p1-tok" style={{ color: "#fff", borderRadius: 5, boxShadow: "0 0 0 2px rgba(255,107,107,0.6)", ...hidden }}>
                {TOKEN}
              </span>
            </Prompt>
            <div className="w1" style={row}>
              <Gut g="⏺" color={C.tAmber} />
              <span className="txt" style={{ color: C.tAmberText }}>
                {t.warn1}
              </span>
            </div>
            <div className="w2" style={{ ...row, marginTop: -6 }}>
              <Gut g="" color={C.tAmber} />
              <span className="txt" style={{ color: C.tAmberText }}>
                {t.warn2(C.tAmber)}
              </span>
            </div>
          </div>
          <div className="s2" style={{ position: "absolute", left: 46, right: 46, top: 30, display: "flex", flexDirection: "column", gap: 16, ...hidden }}>
            <div className="divider" style={{ height: 1.5, background: "rgba(255,255,255,0.18)", margin: "6px 0 10px" }} />
            <Prompt>
              <span className="p2-typed">{t.p2}</span>
            </Prompt>
            <div className="r1" style={row}>
              <Gut g="⏺" color="#fff" />
              <span className="txt" style={{ color: C.tText }}>
                {t.r1}
              </span>
            </div>
            <div className="r2" style={{ ...row, marginTop: -6 }}>
              <Gut g="" color="#fff" />
              <span className="txt" style={{ color: C.tText }}>
                {t.r2(C.tAmber)}
              </span>
            </div>
          </div>
        </Window>
      </div>
    </AbsoluteFill>
  );
};

// ---- 2. title + logo (5 – 8 s) -------------------------------------------------------------

const Title: React.FC = () => {
  const t = useT();
  const root = useGsap((tl, q, el) => {
    // Size the highlight to the words it sits behind, measured before the split.
    const base = el.getBoundingClientRect();
    const r = q(".kv")[0].getBoundingClientRect();
    gsap.set(q(".hl-block"), { left: r.left - base.left - 20, top: r.top - base.top + 16, width: r.width + 40, height: r.height - 6 });
    const a = reveal(tl, q(".t1"), 0.08, { duration: 0.75 });
    const b = reveal(tl, q(".t2"), 0.22, { duration: 0.75 });
    tl.fromTo(q(".hl-block"), { scaleX: 0 }, { scaleX: 1, duration: 0.55, ease: "expo.inOut" }, 0.7);
    // out through the same masks, then the lockup
    tl.to([...a.lines, ...b.lines], { yPercent: -110, duration: 0.42, ease: "power3.in", stagger: 0.05 }, 1.75);
    tl.to(q(".hl-block"), { scaleX: 0, transformOrigin: "100% 50%", duration: 0.35, ease: "power3.in" }, 1.75);
    tl.to(q(".light"), { autoAlpha: 1, duration: 0.35, ease: "power2.inOut" }, 2.05);
    tl.fromTo(q(".mark"), { scale: 0.3, autoAlpha: 0, rotate: -12 }, { scale: 1, autoAlpha: 1, rotate: 0, duration: 0.6, ease: "back.out(1.8)" }, 2.1);
    tl.set(q(".word"), { autoAlpha: 1 }, 2.2);
    reveal(tl, q(".word"), 2.2, { type: "chars", stagger: 0.025, duration: 0.55 });
  });
  return (
    <AbsoluteFill>
      <AbsoluteFill style={{ background: "radial-gradient(ellipse 60% 55% at 50% 52%, #17152b 0%, #0b0b0f 70%)" }} />
      <div ref={root} style={{ position: "absolute", inset: 0 }}>
        <AbsoluteFill className="light" style={hidden}>
          <Backdrop />
        </AbsoluteFill>
        <div
          className="hl-block"
          style={{ position: "absolute", borderRadius: 16, background: C.accent, transformOrigin: "0% 50%", transform: "scaleX(0)" }}
        />
        <AbsoluteFill
          style={{ alignItems: "center", justifyContent: "center", fontFamily: SANS, fontWeight: 700, letterSpacing: t.lang === "zh" ? "0em" : "-0.045em", color: "#fff" }}
        >
          <div className="t1" style={{ fontSize: 128, lineHeight: 1.12 }}>
            {t.t1}
          </div>
          <div className="t2" style={{ fontSize: 128, lineHeight: 1.12, marginTop: t.lang === "zh" ? 18 : 0 }}>
            {t.t2a}
            {t.t2a ? " " : ""}
            <span className="kv" style={{ marginLeft: t.t2a ? 22 : 0 }}>
              {t.kv}
            </span>
          </div>
        </AbsoluteFill>
        <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", flexDirection: "row", gap: 44 }}>
          <div className="mark" style={hidden}>
            <Logo size={190} />
          </div>
          <div className="word" style={{ fontFamily: SANS, fontWeight: 700, fontSize: 150, letterSpacing: "-0.05em", color: C.ink, ...hidden }}>
            keymule
          </div>
        </AbsoluteFill>
      </div>
    </AbsoluteFill>
  );
};

// ---- 3. the MCP flow (8 – 17.2 s) -------------------------------------------------------------

// The real keymule dialog (src/consent/dialog.html at 3x: 1320×870).
const DW = 840;
const DH = Math.round((870 / 1320) * DW);
const DX = (1920 - DW) / 2;
const DY = 250;
const dk = DW / 1320;
const FIELD = { x: 222 * dk, y: 548 * dk, w: (1254 - 222) * dk, h: (640 - 548) * dk };
const SAVE = { x: DX + 1178 * dk, y: DY + 788 * dk };
const PILL = { w: 270, h: 56 };

const Flow: React.FC = () => {
  const t = useT();
  const root = useGsap((tl, q) => {
    const chip = q(".chip")[0];
    // Measure every Flip target first, in its final layout.
    const toField = Flip.fit(chip, q(".field-target")[0], { duration: 0.5, ease: "power3.inOut", scale: false }) as gsap.core.Tween;
    const toPill = Flip.fit(chip, q(".pill-target")[0], { duration: 0.42, ease: "power3.inOut", scale: false }) as gsap.core.Tween;
    const toSlot = Flip.fit(chip, q(".slot")[0], { duration: 0.65, ease: "power3.inOut", scale: false }) as gsap.core.Tween;
    gsap.set(q(".l-ok, .l-tool2, .l-res2"), { autoAlpha: 0 });

    typeIn(tl, q(".p-typed")[0], t.fp, 0.0, 55);
    reveal(tl, q(".l-tool .txt"), 0.65);
    reveal(tl, q(".l-res .txt"), 0.9);

    tl.from(q(".dialog"), { y: 120, autoAlpha: 0, duration: 0.6, ease: "power3.out" }, 1.3);
    tl.to(q(".term"), { filter: "brightness(0.5)", duration: 0.5, ease: "power2.out" }, 1.3);

    tl.from(chip, { scale: 0.6, autoAlpha: 0, duration: 0.35, ease: "back.out(2.2)" }, 2.0);
    tl.from(q(".kbd"), { scale: 0.7, autoAlpha: 0, duration: 0.3, ease: "back.out(2)" }, 2.0);
    tl.to(q(".kbd"), { autoAlpha: 0, duration: 0.2 }, 2.7);
    tl.add(toField, 2.45);
    tl.to(chip, { backgroundColor: "rgba(255,255,255,1)", color: C.ink, boxShadow: "0 0 0 0 rgba(0,0,0,0)", duration: 0.3 }, 2.8);
    tl.to(q(".chip-token"), { duration: 0.85, scrambleText: { text: DOTS, chars: SCRAMBLE, speed: 0.9 } }, 3.1);

    // the pointer clicks Save
    tl.fromTo(q(".cursor"), { x: SAVE.x + 260, y: SAVE.y + 170, autoAlpha: 0 }, { x: SAVE.x - 6, y: SAVE.y - 4, autoAlpha: 1, duration: 0.55, ease: "power2.inOut" }, 3.95);
    tl.set(q(".img-hover"), { autoAlpha: 1 }, 4.45);
    tl.to(q(".cursor"), { scale: 0.85, duration: 0.08, yoyo: true, repeat: 1, transformOrigin: "0 0" }, 4.55);
    tl.to(q(".save-ring"), { autoAlpha: 1, scale: 1.15, duration: 0.25, ease: "power2.out" }, 4.55);
    tl.to(q(".save-ring, .cursor"), { autoAlpha: 0, duration: 0.2 }, 4.8);

    // the field folds into a named key and leaves the dialog
    tl.add(toPill, 4.8);
    tl.to(chip, { backgroundColor: C.accent, color: "#fff", borderRadius: 28, duration: 0.3 }, 4.8);
    tl.to(q(".chip-token"), { autoAlpha: 0, duration: 0.15 }, 4.8);
    tl.to(q(".chip-label"), { autoAlpha: 1, duration: 0.2 }, 4.95);
    tl.to(q(".dialog"), { autoAlpha: 0, scale: 0.96, duration: 0.35, ease: "power2.in" }, 5.0);
    tl.to(q(".term"), { filter: "brightness(1)", duration: 0.45 }, 5.05);

    // it lands in the terminal, where the agent can use it by name
    tl.set(q(".l-ok"), { autoAlpha: 1 }, 5.3);
    tl.add(toSlot, 5.3);
    tl.from(q(".ok-rest"), { autoAlpha: 0, x: -20, duration: 0.5, ease: "expo.out" }, 5.85);
    tl.fromTo(chip, { scale: 1.08 }, { scale: 1, duration: 0.4, ease: "back.out(3)" }, 5.95);

    // the agent uses it; the header it never sees scrambles to [redacted]
    tl.set(q(".l-tool2, .l-res2"), { autoAlpha: 1 }, 6.3);
    reveal(tl, q(".l-tool2 .txt"), 6.3);
    reveal(tl, q(".l-res2 .txt"), 6.55);
    tl.to(q(".secret"), { duration: 0.8, scrambleText: { text: "[redacted]", chars: SCRAMBLE, speed: 0.9 }, color: C.accentSoft }, 7.0);

    reveal(tl, q(".caption"), 7.9);
  });

  return (
    <AbsoluteFill>
      <Backdrop />
      <div ref={root} style={{ position: "absolute", inset: 0 }}>
        <Window className="term" title="acme-web — agent" left={190} top={80} width={1540} height={660} size={33}>
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <Prompt>
              <span className="p-typed">{t.fp}</span>
            </Prompt>
            <div className="l-tool" style={row}>
              <Gut g="⏺" color={C.tGreen} />
              <span className="txt" style={{ color: C.tDim }}>
                <b style={{ color: "#fff", fontWeight: 600 }}>keymule</b> · add_credential(service: "github", name: "github-ci")
              </span>
            </div>
            <div className="l-res" style={{ ...row, marginTop: -6 }}>
              <Gut g="" color={C.tDim} />
              <span className="txt" style={{ color: "#b9b9c0" }}>
                {t.wait}
              </span>
            </div>
            <div className="l-ok" style={{ ...row, alignItems: "center" }}>
              <Gut g="✓" color={C.tGreen} />
              <span style={{ color: "#c9f7d8" }}>{t.saved}</span>
              <span className="slot" style={{ display: "inline-block", width: PILL.w, height: PILL.h, margin: "0 14px 0 6px" }} />
              <span className="ok-rest" style={{ color: "#c9f7d8" }}>
                {t.savedRest}
              </span>
            </div>
            <div className="l-tool2" style={row}>
              <Gut g="⏺" color={C.tGreen} />
              <span className="txt" style={{ color: C.tDim }}>
                <b style={{ color: "#fff", fontWeight: 600 }}>keymule</b> · http_request("github-ci", GET /repos/acme/web/actions/runs)
              </span>
            </div>
            <div className="l-res2" style={{ ...row, marginTop: -6 }}>
              <Gut g="" color={C.tDim} />
              <span className="txt" style={{ color: "#b9b9c0" }}>
                ⎿  200 OK · Authorization: Bearer{" "}
                <span className="secret" style={{ color: "#ff9a9a" }}>
                  {TOKEN}
                </span>
              </span>
            </div>
          </div>
        </Window>

        <div
          className="dialog"
          style={{
            position: "absolute",
            left: DX,
            top: DY,
            width: DW,
            height: DH,
            borderRadius: 16,
            overflow: "hidden",
            boxShadow: "0 50px 120px rgba(0,0,0,0.45), 0 0 0 1px rgba(0,0,0,0.12)",
          }}
        >
          <Img src={staticFile("shots/dialog-empty.png")} style={{ position: "absolute", width: DW, height: DH }} />
          <Img className="img-hover" src={staticFile("shots/dialog-hover.png")} style={{ position: "absolute", width: DW, height: DH, ...hidden }} />
          <div
            className="save-ring"
            style={{ position: "absolute", left: 1098 * dk, top: 744 * dk, width: 162 * dk, height: 90 * dk, borderRadius: 10, boxShadow: "0 0 0 6px rgba(99,91,255,0.35)", ...hidden }}
          />
        </div>
        <div className="field-target" style={{ position: "absolute", left: DX + FIELD.x, top: DY + FIELD.y, width: FIELD.w, height: FIELD.h }} />
        <div className="pill-target" style={{ position: "absolute", left: 960 - PILL.w / 2, top: DY + FIELD.y - 96, width: PILL.w, height: PILL.h }} />

        <div
          className="kbd"
          style={{
            position: "absolute",
            left: 960 + 340,
            top: DY - 124,
            fontFamily: SANS,
            fontWeight: 600,
            fontSize: 40,
            color: C.ink,
            background: "#fff",
            borderRadius: 14,
            padding: "8px 20px",
            boxShadow: "0 0 0 1px rgba(0,0,0,0.1), 0 5px 0 rgba(0,0,0,0.12), 0 18px 36px rgba(0,0,0,0.2)",
          }}
        >
          ⌘ V
        </div>

        <div
          className="chip"
          style={{
            position: "absolute",
            left: 960 - 320,
            top: DY - 120,
            width: 640,
            height: 62,
            borderRadius: 10,
            background: C.red,
            color: "#fff",
            boxShadow: "0 18px 40px rgba(255,80,80,0.35)",
            fontFamily: MONO,
            fontSize: 25,
            fontWeight: 500,
            display: "flex",
            alignItems: "center",
            overflow: "hidden",
          }}
        >
          <span className="chip-token" style={{ paddingLeft: 18, whiteSpace: "nowrap" }}>
            {TOKEN}
          </span>
          <span
            className="chip-label"
            style={{
              position: "absolute",
              inset: 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 10,
              fontFamily: SANS,
              fontSize: 27,
              fontWeight: 600,
              ...hidden,
            }}
          >
            <KeyIcon size={24} color="#fff" />
            github-ci
          </span>
        </div>
        <Cursor className="cursor" />
        <Caption text={t.cap1} top={800} />
      </div>
    </AbsoluteFill>
  );
};

// ---- 4. the menu-bar app (17.2 – 21 s) -------------------------------------------------------------

const UW = 1500;
const uk = UW / 2880;
const UX = (1920 - UW) / 2;
const UY = 150;
const ICON = { x: 1560, y: 32 };

const Ui: React.FC = () => {
  const t = useT();
  const root = useGsap((tl, q) => {
    const pill = q(".ui-pill")[0];
    const toRow = Flip.fit(pill, q(".row-target")[0], { duration: 0.6, ease: "power3.inOut", scale: false }) as gsap.core.Tween;
    tl.from(q(".menu"), { autoAlpha: 0, y: -10, scaleY: 0.92, transformOrigin: "50% 0%", duration: 0.25, ease: "power2.out" }, 0.3);
    tl.set(q(".mi-open"), { backgroundColor: "#3b82f6", color: "#fff" }, 0.6);
    tl.to(q(".menu"), { autoAlpha: 0, duration: 0.15 }, 0.88);
    tl.set(q(".iconbox"), { backgroundColor: "rgba(0,0,0,0)" }, 0.88);
    tl.fromTo(
      q(".uiwin"),
      { clipPath: `circle(0px at ${ICON.x - UX}px ${ICON.y - UY}px)` },
      { clipPath: `circle(2200px at ${ICON.x - UX}px ${ICON.y - UY}px)`, duration: 0.75, ease: "power3.inOut" },
      0.95,
    );
    // the named key comes out of the menu-bar icon and settles into the vault
    tl.fromTo(pill, { autoAlpha: 0, scale: 0.5 }, { autoAlpha: 1, scale: 1, duration: 0.25, ease: "back.out(2)" }, 1.75);
    tl.add(toRow, 1.85);
    tl.to(pill, { autoAlpha: 0, duration: 0.2 }, 2.45);
    tl.fromTo(q(".sweep-cred"), { scaleX: 0 }, { scaleX: 1, duration: 0.5, ease: "expo.out" }, 2.35);
    // ...and its uses show up: two new requests, and the count goes from 2 to 4
    tl.to(q(".after"), { autoAlpha: 1, duration: 0.25 }, 2.55);
    tl.fromTo(q(".sweep-act"), { scaleX: 0 }, { scaleX: 1, duration: 0.5, ease: "expo.out" }, 2.6);
    tl.fromTo(q(".stat-ring"), { autoAlpha: 0 }, { autoAlpha: 1, duration: 0.25, yoyo: true, repeat: 1, repeatDelay: 0.5 }, 2.6);
    reveal(tl, q(".caption"), 2.65);
  });
  return (
    <AbsoluteFill>
      <Backdrop />
      <div ref={root} style={{ position: "absolute", inset: 0 }}>
        <div
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            top: 0,
            height: 64,
            background: "rgba(255,255,255,0.75)",
            boxShadow: "0 1px 0 rgba(0,0,0,0.06)",
            fontFamily: SANS,
            fontSize: 26,
            fontWeight: 500,
            color: C.ink,
          }}
        >
          <div
            className="iconbox"
            style={{ position: "absolute", left: ICON.x - 26, top: ICON.y - 24, width: 52, height: 48, borderRadius: 9, background: "rgba(0,0,0,0.12)", display: "grid", placeItems: "center" }}
          >
            <Logo size={32} />
          </div>
          <div style={{ position: "absolute", right: 36, top: 16 }}>{t.clock}</div>
        </div>
        <div
          className="menu"
          style={{
            position: "absolute",
            left: ICON.x - 30,
            top: 72,
            width: 340,
            padding: "8px 0",
            background: "rgba(246,246,248,0.97)",
            borderRadius: 14,
            boxShadow: "0 0 0 1px rgba(0,0,0,0.12), 0 18px 40px rgba(0,0,0,0.18)",
            fontFamily: SANS,
            fontSize: 25,
            color: C.ink,
          }}
        >
          {["Open keymule", "Open in Browser", "-", "✓ Start at Login", "-", "Quit keymule"].map((l, i) =>
            l === "-" ? (
              <div key={i} style={{ height: 1, background: "rgba(0,0,0,0.1)", margin: "6px 16px" }} />
            ) : (
              <div key={i} className={i === 0 ? "mi-open" : undefined} style={{ padding: "6px 18px", margin: "0 8px", borderRadius: 8 }}>
                {l}
              </div>
            ),
          )}
        </div>
        <div
          className="uiwin"
          style={{
            position: "absolute",
            left: UX,
            top: UY,
            width: UW,
            height: 1080 * uk,
            borderRadius: 16,
            overflow: "hidden",
            background: "#fff",
            boxShadow: "0 40px 100px rgba(20,20,40,0.22), 0 0 0 1px rgba(0,0,0,0.1)",
            clipPath: "circle(0px at 0px 0px)",
          }}
        >
          <Img src={staticFile("shots/ov-before.png")} style={{ position: "absolute", width: UW, top: 0 }} />
          <Img className="after" src={staticFile("shots/ov-after.png")} style={{ position: "absolute", width: UW, top: 0, ...hidden }} />
          {/* overview coordinates are in the 2880×1800 screenshot */}
          {[
            ["sweep-cred", 544, 584, 1752, 756],
            ["sweep-act", 1834, 590, 2800, 838],
          ].map(([cls, x0, y0, x1, y1]) => (
            <div
              key={cls as string}
              className={cls as string}
              style={{
                position: "absolute",
                left: (x0 as number) * uk,
                top: (y0 as number) * uk,
                width: ((x1 as number) - (x0 as number)) * uk,
                height: ((y1 as number) - (y0 as number)) * uk,
                background: "rgba(99,91,255,0.12)",
                borderLeft: `4px solid ${C.accent}`,
                transformOrigin: "0% 50%",
                transform: "scaleX(0)",
              }}
            />
          ))}
          <div
            className="stat-ring"
            style={{ position: "absolute", left: 1298 * uk, top: 251 * uk, width: 749 * uk, height: 192 * uk, boxShadow: `inset 0 0 0 3px ${C.accent}`, borderRadius: 4, ...hidden }}
          />
          <div className="row-target" style={{ position: "absolute", left: 652 * uk - 14, top: 630 * uk - 24, width: 200, height: 48 }} />
        </div>
        <Pill
          className="ui-pill"
          name="github-ci"
          style={{ position: "absolute", left: ICON.x - 100, top: 90, width: 200, height: 48, justifyContent: "center", padding: 0, ...hidden }}
        />
        <Caption text={t.cap2} top={830} />
      </div>
    </AbsoluteFill>
  );
};

// ---- 5. days later, a new session (21 – 24.8 s) -----------------------------------------------------

const Reel: React.FC<{ items: string[]; width: number }> = ({ items, width }) => (
  <span style={{ display: "inline-block", height: "1.1em", overflow: "hidden", width }}>
    <span className="reel" style={{ display: "flex", flexDirection: "column" }}>
      {items.map((s) => (
        <span key={s} style={{ height: "1.1em", lineHeight: "1.1em" }}>
          {s}
        </span>
      ))}
    </span>
  </span>
);

const Reuse: React.FC = () => {
  const t = useT();
  const root = useGsap((tl, q) => {
    // measure the key's flight from keymule into the tool call, in final layout
    const pill = q(".rpill")[0];
    const toSlot = Flip.fit(pill, q(".rslot")[0], { duration: 0.55, ease: "power3.inOut", scale: false }) as gsap.core.Tween;

    tl.from(q(".date"), { autoAlpha: 0, y: 30, duration: 0.35, ease: "power3.out" }, 0.0);
    tl.to(q(".reel"), { yPercent: -75, duration: 0.7, ease: "power3.inOut" }, 0.22);
    reveal(tl, q(".later"), 0.32);
    tl.to(q(".datewrap"), { yPercent: -20, autoAlpha: 0, duration: 0.33, ease: "power3.in" }, 1.0);
    tl.from(q(".term2"), { y: 50, autoAlpha: 0, duration: 0.42, ease: "power3.out" }, 1.1);
    typeIn(tl, q(".p-typed")[0], t.p3, 1.3, 60);
    // the agent asks keymule; the argument slot waits empty
    reveal(tl, q(".l1 .txt"), 1.85);
    tl.fromTo(q(".rbadge"), { autoAlpha: 0, scale: 0.6, y: 10 }, { autoAlpha: 1, scale: 1, y: 0, duration: 0.35, ease: "back.out(2)" }, 1.85);
    tl.fromTo(q(".rbadge"), { boxShadow: "0 20px 50px rgba(20,20,40,0.25), 0 0 0 0px rgba(99,91,255,0)" }, { boxShadow: "0 20px 50px rgba(20,20,40,0.25), 0 0 0 10px rgba(99,91,255,0.3)", duration: 0.2, yoyo: true, repeat: 1 }, 2.0);
    // the named key comes out of keymule and lands in the call
    tl.fromTo(pill, { autoAlpha: 0, scale: 0.5 }, { autoAlpha: 1, scale: 1, duration: 0.2, ease: "back.out(2)" }, 2.0);
    tl.add(toSlot, 2.05);
    tl.fromTo(pill, { scale: 1.1 }, { scale: 1, duration: 0.35, ease: "back.out(3)" }, 2.6);
    tl.to(q(".rbadge"), { autoAlpha: 0, scale: 0.9, duration: 0.3, ease: "power2.in" }, 3.3);
    reveal(tl, q(".l2 .txt"), 2.68);
    reveal(tl, q(".caption"), 2.75);
  });
  return (
    <AbsoluteFill>
      <Backdrop />
      <div ref={root} style={{ position: "absolute", inset: 0 }}>
        <AbsoluteFill className="datewrap" style={{ alignItems: "center", justifyContent: "center", gap: 18 }}>
          <div className="date" style={{ fontFamily: SANS, fontWeight: 700, fontSize: 150, letterSpacing: "-0.04em", color: C.ink, display: "flex", gap: 36, lineHeight: "1.1em" }}>
            {t.lang === "zh" ? (
              <>
                <span style={{ marginRight: -36 }}>11月</span>
                <Reel items={["16", "17", "18", "19"]} width={146} />
                <span style={{ marginLeft: -36 }}>日</span>
                <Reel items={["周一", "周二", "周三", "周四"]} width={310} />
              </>
            ) : (
              <>
                <Reel items={["Mon", "Tue", "Wed", "Thu"]} width={330} />
                <Reel items={["16", "17", "18", "19"]} width={180} />
                <span>Nov</span>
              </>
            )}
          </div>
          <div className="later" style={{ fontFamily: SANS, fontWeight: 500, fontSize: 54, color: C.ink2 }}>
            {t.later}
          </div>
        </AbsoluteFill>
        <Window className="term2" title={t.newSession} left={190} top={160} width={1540} height={520} size={34}>
          <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            <Prompt>
              <span className="p-typed">{t.p3}</span>
            </Prompt>
            <div className="l1" style={{ ...row, alignItems: "center" }}>
              <Gut g="⏺" color={C.tGreen} />
              <span className="txt" style={{ color: C.tDim }}>
                <b style={{ color: "#fff", fontWeight: 600 }}>keymule</b> · http_request({" "}
                <span className="rslot" style={{ display: "inline-block", width: 186, height: 44, verticalAlign: "middle", borderRadius: 999, border: "2px dashed rgba(165,160,255,0.7)" }} />{" "}
                , GET …/actions/runs)
              </span>
            </div>
            <div className="l2" style={{ ...row, marginTop: -4 }}>
              <Gut g="" color={C.tDim} />
              <span className="txt" style={{ color: "#b9b9c0" }}>
                {t.flaky}
              </span>
            </div>
          </div>
        </Window>
        {/* keymule, handing over the key by name */}
        <div
          className="rbadge"
          style={{
            position: "absolute",
            left: 1470,
            top: 84,
            height: 76,
            padding: "0 26px 0 16px",
            borderRadius: 22,
            background: "#fff",
            display: "flex",
            alignItems: "center",
            gap: 14,
            fontFamily: SANS,
            fontWeight: 700,
            fontSize: 32,
            letterSpacing: "-0.03em",
            color: C.ink,
            ...hidden,
          }}
        >
          <Logo size={48} />
          keymule
        </div>
        <Pill
          className="rpill"
          name="github-ci"
          size={26}
          style={{ position: "absolute", left: 1470 + 120 - 93, top: 84 + 38 - 22, width: 186, height: 44, justifyContent: "center", padding: 0, ...hidden }}
        />
        <Caption text={t.cap3} top={790} />
      </div>
    </AbsoluteFill>
  );
};

// ---- 6. paste guard (24.8 – 27.8 s) ------------------------------------------------------------------

const Guard: React.FC = () => {
  const t = useT();
  const root = useGsap((tl, q, el) => {
    const chip = q(".gchip")[0];
    // put the flying chip exactly over the pasted key, then measure its flight
    const base = el.getBoundingClientRect();
    const r = q(".g-tok")[0].getBoundingClientRect();
    gsap.set(chip, { left: r.left - base.left, top: r.top - base.top, width: r.width, height: r.height });
    const fly = Flip.fit(chip, q(".badge-target")[0], { duration: 0.6, ease: "power3.inOut", scale: false }) as gsap.core.Tween;

    tl.from(q(".term3"), { y: 40, autoAlpha: 0, duration: 0.35, ease: "power3.out" }, 0);
    typeIn(tl, q(".g-typed")[0], t.gp, 0.15, 80);
    tl.fromTo(q(".g-tok"), { autoAlpha: 0 }, { autoAlpha: 1, duration: 0.01 }, 0.62);
    tl.fromTo(q(".hold"), { autoAlpha: 0, y: 14, scale: 0.97 }, { autoAlpha: 1, y: 0, scale: 1, duration: 0.4, ease: "back.out(1.6)" }, 0.85);
    reveal(tl, q(".hold .hl"), 0.9, { stagger: 0.08 });
    reveal(tl, q(".caption"), 1.0);
    tl.set(q(".y-row"), { autoAlpha: 1 }, 1.55);
    typeIn(tl, q(".y-typed")[0], "y", 1.6, 20);
    // the key leaves the message and goes into keymule
    tl.fromTo(q(".badge"), { autoAlpha: 0, scale: 0.4 }, { autoAlpha: 1, scale: 1, duration: 0.35, ease: "back.out(2)" }, 1.7);
    tl.set(chip, { autoAlpha: 1 }, 1.8);
    tl.set(q(".g-tok"), { autoAlpha: 0 }, 1.8);
    tl.add(fly, 1.8);
    tl.to(chip, { duration: 0.5, scrambleText: { text: "sk-p…al9", chars: SCRAMBLE, speed: 1 } }, 1.8);
    tl.to(chip, { autoAlpha: 0, duration: 0.12 }, 2.38);
    tl.fromTo(q(".badge"), { scale: 1.18 }, { scale: 1, duration: 0.4, ease: "back.out(3)" }, 2.4);
    tl.fromTo(q(".g-saved"), { autoAlpha: 0 }, { autoAlpha: 1, duration: 0.3 }, 2.15);
  });
  return (
    <AbsoluteFill>
      <Backdrop />
      <div ref={root} style={{ position: "absolute", inset: 0 }}>
        <Window className="term3" title="acme-web — agent" left={190} top={130} width={1540} height={560} size={33}>
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <Prompt>
              <span className="g-typed">{t.gp}</span>
              <span style={{ position: "relative" }}>
                <span className="g-tok" style={{ color: "#fff", background: "rgba(255,107,107,0.3)", boxShadow: "0 0 0 2px rgba(255,107,107,0.6)", borderRadius: 5, ...hidden }}>
                  {OPENAI}
                </span>
                <span className="g-saved" style={{ position: "absolute", left: 0, top: 0, whiteSpace: "nowrap", color: C.accentSoft, fontWeight: 600, ...hidden }}>
                  [saved in keymule as `openai`]
                </span>
              </span>
            </Prompt>
            <div
              className="hold"
              style={{ border: "1.5px solid rgba(165,160,255,0.65)", background: "rgba(99,91,255,0.14)", borderRadius: 12, padding: "14px 20px", margin: "2px -18px 6px", lineHeight: 1.5, ...hidden }}
            >
              <div className="hl" style={{ color: "#fff" }}>
                keymule held back your message: it has an OpenAI API key.
              </div>
              <div className="hl" style={{ color: "#c8c8d0", whiteSpace: "pre" }}>
                {"  y    save it as `openai` and send your message without it"}
              </div>
            </div>
            <div className="y-row" style={hidden}>
              <Prompt>
                <span className="y-typed">y</span>
              </Prompt>
            </div>
          </div>
        </Window>
        <div className="badge-target" style={{ position: "absolute", left: 1640 - 60, top: 70 - 20, width: 120, height: 40 }} />
        <div
          className="badge"
          style={{
            position: "absolute",
            left: 1640 - 46,
            top: 24,
            width: 92,
            height: 92,
            borderRadius: 24,
            background: "#fff",
            boxShadow: "0 20px 50px rgba(20,20,40,0.28)",
            display: "grid",
            placeItems: "center",
            ...hidden,
          }}
        >
          <Logo size={60} />
        </div>
        <div
          className="gchip"
          style={{
            position: "absolute",
            fontFamily: MONO,
            fontSize: 33,
            color: "#fff",
            background: C.red,
            borderRadius: 6,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            whiteSpace: "nowrap",
            overflow: "hidden",
            boxShadow: "0 16px 40px rgba(255,80,80,0.4)",
            ...hidden,
          }}
        >
          {OPENAI}
        </div>
        <Caption text={t.cap4} top={790} />
      </div>
    </AbsoluteFill>
  );
};

// ---- 7. end card (27.8 – 30 s) -------------------------------------------------------------------------

const INSTALL = "curl -LsSf https://github.com/Rorogogogo/keymule/releases/latest/download/keymule-installer.sh | sh";

const End: React.FC = () => {
  const t = useT();
  const SERVICES = t.services;
  const root = useGsap((tl, q) => {
    tl.fromTo(q(".mq-a"), { xPercent: 0 }, { xPercent: -25, duration: 2.2, ease: "none" }, 0);
    tl.fromTo(q(".mq-b"), { xPercent: -25 }, { xPercent: 0, duration: 2.2, ease: "none" }, 0);
    tl.from(q(".mark"), { scale: 0.4, autoAlpha: 0, duration: 0.5, ease: "back.out(1.8)" }, 0.0);
    reveal(tl, q(".head"), 0.12, { duration: 0.7 });
    reveal(tl, q(".sub"), 0.32);
    tl.from(q(".cmd"), { y: 24, autoAlpha: 0, duration: 0.5, ease: "expo.out" }, 0.5);
    tl.from(q(".url"), { autoAlpha: 0, duration: 0.4 }, 0.7);
  });
  const chips = (cls: string) => (
    <div className={cls} style={{ display: "flex", gap: 22, whiteSpace: "nowrap", width: "max-content" }}>
      {[...SERVICES, ...SERVICES, ...SERVICES, ...SERVICES].map((s, i) => (
        <span
          key={i}
          style={{ fontFamily: SANS, fontSize: 34, fontWeight: 600, color: "rgba(29,29,31,0.16)", border: "2px solid rgba(29,29,31,0.08)", borderRadius: 999, padding: "10px 26px" }}
        >
          {s}
        </span>
      ))}
    </div>
  );
  return (
    <AbsoluteFill>
      <Backdrop />
      <div ref={root} style={{ position: "absolute", inset: 0 }}>
        <div style={{ position: "absolute", top: 70, left: 0, display: "flex", flexDirection: "column", gap: 22 }}>
          {chips("mq-a")}
          {chips("mq-b")}
        </div>
        <div style={{ position: "absolute", bottom: 50, left: 0 }}>{chips("mq-a")}</div>
        <AbsoluteFill style={{ alignItems: "center", justifyContent: "center" }}>
          <div className="mark" style={{ marginBottom: 34 }}>
            <Logo size={120} />
          </div>
          <div className="head" style={{ fontFamily: SANS, fontWeight: 700, fontSize: 118, letterSpacing: t.lang === "zh" ? "0em" : "-0.045em", color: C.ink }}>
            {t.head}
          </div>
          <div className="sub" style={{ marginTop: 14, fontFamily: SANS, fontWeight: 500, fontSize: 38, color: C.ink2 }}>
            {t.sub}
          </div>
          <div
            className="cmd"
            style={{ marginTop: 50, fontFamily: MONO, fontSize: 27, color: "#fff", background: C.ink, borderRadius: 16, padding: "18px 30px", boxShadow: "0 30px 60px rgba(20,20,40,0.25)" }}
          >
            <span style={{ color: C.ink3 }}>$ </span>
            {INSTALL}
          </div>
          <div className="url" style={{ marginTop: 22, fontFamily: SANS, fontSize: 28, color: C.ink3 }}>
            github.com/Rorogogogo/keymule
          </div>
        </AbsoluteFill>
      </div>
    </AbsoluteFill>
  );
};

// ---- sound -------------------------------------------------------------------------------------------

type Cue = [frame: number, sfx: string, vol?: number];
const CUES: Cue[] = [
  // problem
  [2, "type", 0.9], [22, "paste"], [26, "enter"], [30, "warn"], [80, "whoosh", 0.7], [93, "type", 0.8], [107, "enter"], [110, "warn", 0.5],
  // title, logo
  [213, "chime", 0.8],
  // flow (starts at 240)
  [240, "type"], [258, "enter"], [260, "tick"], [279, "pop"], [300, "paste"], [314, "whoosh", 0.6], [333, "type"], [345, "type", 0.7],
  [377, "click"], [384, "whoosh", 0.6], [418, "chime"], [429, "tick"], [450, "type"], [462, "type", 0.6],
  // ui (starts at 516)
  [525, "click"], [542, "click"], [545, "whoosh", 0.6], [568, "pop", 0.7], [586, "tick"],
  // reuse (starts at 630)
  [637, "tick"], [643, "tick"], [649, "tick"], [660, "whoosh", 0.6], [669, "type"], [684, "enter"], [686, "pop", 0.8], [692, "whoosh", 0.6], [709, "tick"], [711, "chime", 0.5],
  // guard (starts at 744)
  [749, "type", 0.8], [763, "paste"], [767, "enter"], [770, "pop"], [792, "type", 0.5], [795, "enter"], [798, "whoosh", 0.7], [815, "chime"],
  // end
  [830, "whoosh", 0.8],
];

// Duck the music under the dialog so the paste and the click read clearly.
const scoreVolume = (f: number) =>
  0.6 - 0.22 * interpolate(f, [272, 282, 392, 404], [0, 1, 1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });

// ---- the film ------------------------------------------------------------------------------------------

const SCENES: [readonly [number, number], React.FC][] = [
  [T.problem, Problem],
  [T.title, Title],
  [T.flow, Flow],
  [T.ui, Ui],
  [T.reuse, Reuse],
  [T.guard, Guard],
  [T.end, End],
];

export const Launch: React.FC<{ lang?: Lang }> = ({ lang = "en" }) => (
  <LangCtx.Provider value={lang}>
  <AbsoluteFill style={{ background: C.bg }}>
    {SCENES.map(([[a, b], S], i) => (
      <Sequence key={i} from={a} durationInFrames={b - a}>
        <S />
      </Sequence>
    ))}
    <Audio src={staticFile("audio/score.wav")} volume={scoreVolume} />
    {CUES.map(([at, name, vol = 1], i) => (
      <Sequence key={`c${i}`} from={at} durationInFrames={60}>
        <Audio src={staticFile(`audio/${name}.wav`)} volume={vol} />
      </Sequence>
    ))}
  </AbsoluteFill>
  </LangCtx.Provider>
);
