import React from "react";
import {
  AbsoluteFill,
  Easing,
  Img,
  Sequence,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
} from "remotion";
import { Audio } from "@remotion/media";
import { loadFont } from "@remotion/google-fonts/Inter";
import { loadFont as loadMono } from "@remotion/google-fonts/GeistMono";

const { fontFamily: SANS } = loadFont("normal", {
  weights: ["400", "500", "600", "700", "800"],
  subsets: ["latin"],
});
const { fontFamily: MONO } = loadMono("normal", {
  weights: ["400", "500", "600"],
  subsets: ["latin"],
});

const C = {
  bg: "#fbfbfd",
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
};

const ease = Easing.bezier(0.16, 1, 0.3, 1);
const inOut = Easing.bezier(0.65, 0, 0.35, 1);
const accel = Easing.bezier(0.7, 0, 0.84, 0);
const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;
const ramp = (f: number, a: number, b: number, from = 0, to = 1, e = ease) =>
  interpolate(f, [a, b], [from, to], { ...clamp, easing: e });
const sp = (f: number, damping = 18, stiffness = 120) =>
  spring({ frame: f, fps: 30, config: { damping, stiffness } });

// ---- timeline (absolute frames, 30 fps; one beat = 15 frames) ------------------
// Keep in step with the section times in scripts/make_audio.py.
const T = {
  hook: [0, 57],
  again: [57, 75],
  forgot: [75, 120],
  env: [120, 150],
  title: [150, 180],
  logo: [180, 210],
  flow: [210, 480],
  ui: [480, 570],
  later: [570, 594],
  reuse: [594, 720],
  guard: [720, 810],
  end: [810, 900],
} as const;

// ---- camera ----------------------------------------------------------------------

type Cam = { x?: number; y?: number; z?: number; rx?: number; ry?: number; rz?: number; s?: number; blur?: number };
const camTransform = (c: Cam) =>
  `perspective(2000px) translate3d(${c.x ?? 0}px, ${c.y ?? 0}px, ${c.z ?? 0}px) rotateX(${c.rx ?? 0}deg) rotateY(${
    c.ry ?? 0
  }deg) rotateZ(${c.rz ?? 0}deg) scale(${c.s ?? 1})`;

/** Soft colour fields behind everything; they drift against the camera. */
const Backdrop: React.FC<{ px?: number; py?: number; dark?: boolean }> = ({ px = 0, py = 0, dark }) => {
  const f = useCurrentFrame();
  const drift = Math.sin(f / 40) * 30;
  return (
    <AbsoluteFill style={{ background: dark ? "#0a0a0c" : "#f4f4f7", overflow: "hidden" }}>
      <div
        style={{
          position: "absolute",
          width: 1300,
          height: 1300,
          left: -260 + px * 0.25 + drift,
          top: -520 + py * 0.25,
          borderRadius: "50%",
          background: dark
            ? "radial-gradient(circle, rgba(99,91,255,0.35), rgba(99,91,255,0) 65%)"
            : "radial-gradient(circle, rgba(165,160,255,0.45), rgba(165,160,255,0) 65%)",
        }}
      />
      <div
        style={{
          position: "absolute",
          width: 1200,
          height: 1200,
          right: -300 - px * 0.18 - drift,
          bottom: -560 - py * 0.18,
          borderRadius: "50%",
          background: dark
            ? "radial-gradient(circle, rgba(255,140,90,0.18), rgba(255,140,90,0) 65%)"
            : "radial-gradient(circle, rgba(255,186,150,0.42), rgba(255,186,150,0) 65%)",
        }}
      />
    </AbsoluteFill>
  );
};

// ---- small pieces ------------------------------------------------------------------

const Logo: React.FC<{ size: number }> = ({ size }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" style={{ display: "block" }}>
    <rect width="24" height="24" rx="6" fill={C.ink} />
    <circle cx="12" cy="9.6" r="3.3" fill="#fff" />
    <path d="M10.3 11.8 9.4 17.6h5.2l-.9-5.8z" fill="#fff" />
  </svg>
);

/** Inline markup: *bold white*, {accent}, [[amber]]. */
const Marked: React.FC<{ text: string; base: string }> = ({ text, base }) => {
  const parts = text.split(/(\*[^*]+\*|\{[^}]+\}|\[\[[^\]]+\]\])/g);
  return (
    <>
      {parts.map((p, i) => {
        if (p.startsWith("*") && p.endsWith("*") && p.length > 1)
          return (
            <span key={i} style={{ color: "#fff", fontWeight: 600 }}>
              {p.slice(1, -1)}
            </span>
          );
        if (p.startsWith("{") && p.endsWith("}"))
          return (
            <span key={i} style={{ color: C.accentSoft, fontWeight: 600 }}>
              {p.slice(1, -1)}
            </span>
          );
        if (p.startsWith("[[") && p.endsWith("]]"))
          return (
            <span key={i} style={{ color: C.tAmber, fontWeight: 600 }}>
              {p.slice(2, -2)}
            </span>
          );
        return (
          <span key={i} style={{ color: base }}>
            {p}
          </span>
        );
      })}
    </>
  );
};

type Line =
  | {
      kind: "prompt";
      at: number;
      text: string;
      cps?: number;
      paste?: { at: number; text: string; gone?: number; replace?: string };
      enter: number;
    }
  | { kind: "tool" | "result" | "text" | "cont" | "warn" | "warncont" | "ok"; at: number; text: string }
  | { kind: "hold"; at: number; lines: string[] };

const TermLine: React.FC<{ line: Line; f: number }> = ({ line, f }) => {
  if (f < line.at) return null;
  const p = ramp(f, line.at, line.at + 7);
  // New lines land with a little lift: they start larger and glowing, then settle.
  const lift = ramp(f, line.at, line.at + 14, 1, 0);
  const wrap: React.CSSProperties = {
    opacity: p,
    translate: `${(1 - p) * -16}px 0px`,
    whiteSpace: "pre-wrap",
    wordBreak: "break-word",
    lineHeight: 1.5,
  };
  const gutter = (glyph: string, color: string) => (
    <span style={{ color, display: "inline-block", width: "1.6em", flex: "none" }}>{glyph}</span>
  );
  const row = (glyph: string, gColor: string, content: React.ReactNode, extra?: React.CSSProperties, glow?: string) => (
    <div
      style={{
        ...wrap,
        display: "flex",
        borderRadius: 10,
        margin: "0 -14px",
        padding: "0 14px",
        background: glow ? `rgba(${glow},${0.16 * lift})` : undefined,
        scale: glow ? `${1 + 0.025 * lift}` : undefined,
        transformOrigin: "0% 50%",
        ...extra,
      }}
    >
      {gutter(glyph, gColor)}
      <div style={{ flex: 1 }}>{content}</div>
    </div>
  );

  switch (line.kind) {
    case "prompt": {
      const cps = line.cps ?? 2;
      const n = Math.max(0, Math.floor((f - line.at) * cps));
      const typed = line.text.slice(0, n);
      const pa = line.paste;
      const pasted = pa && f >= pa.at ? pa.text : "";
      const gone = pa?.gone !== undefined && f >= pa.gone;
      const pasteFlash = pa ? ramp(f, pa.at, pa.at + 10, 1, 0) : 0;
      const live = f < line.enter;
      const caret = live && Math.floor(f / 8) % 2 === 0;
      return (
        <div
          style={{
            whiteSpace: "pre-wrap",
            lineHeight: 1.5,
            display: "flex",
            background: live ? "rgba(255,255,255,0.07)" : "rgba(255,255,255,0.04)",
            border: `1.5px solid ${live ? "rgba(255,255,255,0.18)" : "transparent"}`,
            borderRadius: 12,
            padding: "10px 16px",
            margin: "0 -18px 6px",
          }}
        >
          {gutter(">", C.tDim)}
          <div style={{ flex: 1, color: C.tText, wordBreak: "break-all" }}>
            {typed}
            {pasted && !gone && (
              <span
                style={{
                  color: "#fff",
                  background: `rgba(255,107,107,${0.18 + 0.4 * pasteFlash})`,
                  borderRadius: 4,
                  boxShadow: `0 0 0 2px rgba(255,107,107,${0.25 + 0.5 * pasteFlash})`,
                }}
              >
                {pasted}
              </span>
            )}
            {gone && <span style={{ color: C.accentSoft, fontWeight: 600 }}>{pa?.replace}</span>}
            <span style={{ opacity: caret ? 1 : 0, color: "#fff" }}>▍</span>
          </div>
        </div>
      );
    }
    case "tool":
      return row("⏺", C.tGreen, <Marked text={line.text} base={C.tDim} />, undefined, "74,222,128");
    case "result":
      return row("", C.tDim, <Marked text={`⎿  ${line.text}`} base="#b9b9c0" />, { marginTop: -4 });
    case "text":
      return row("⏺", "#fff", <Marked text={line.text} base={C.tText} />);
    case "cont":
      return row("", "#fff", <Marked text={line.text} base={C.tText} />, { marginTop: -4 });
    case "warn":
      return row("⏺", C.tAmber, <Marked text={line.text} base={C.tAmberText} />, undefined, "245,165,36");
    case "warncont":
      return row("", C.tAmber, <Marked text={line.text} base={C.tAmberText} />, { marginTop: -4 });
    case "ok":
      return row("✓", C.tGreen, <Marked text={line.text} base="#c9f7d8" />, undefined, "74,222,128");
    case "hold": {
      const s = sp(f - line.at, 13, 200);
      return (
        <div
          style={{
            whiteSpace: "pre-wrap",
            lineHeight: 1.5,
            opacity: Math.min(1, s * 1.5),
            scale: `${0.92 + 0.08 * s}`,
            transformOrigin: "50% 0%",
            border: "1.5px solid rgba(165,160,255,0.65)",
            background: "rgba(99,91,255,0.14)",
            boxShadow: `0 0 ${40 * (1 - s) + 20}px rgba(99,91,255,${0.25 + 0.3 * (1 - s)})`,
            borderRadius: 12,
            padding: "14px 20px",
            margin: "4px -18px 8px",
          }}
        >
          {line.lines.map((l, i) => (
            <div key={i} style={{ whiteSpace: "pre" }}>
              <Marked text={l} base={i === 0 ? "#fff" : "#c8c8d0"} />
            </div>
          ))}
        </div>
      );
    }
  }
};

/** A stylised agent terminal in 3D space: dark window, prompt box, tool calls. */
const Term: React.FC<{
  lines: Line[];
  title?: string;
  size?: number;
  width?: number;
  height?: number;
  top?: number;
  cam?: (f: number) => Cam;
  origin?: string;
  dim?: (f: number) => number;
  children?: React.ReactNode;
}> = ({
  lines,
  title = "acme-web — agent",
  size = 32,
  width = 1640,
  height = 790,
  top = 60,
  cam,
  origin = "50% 45%",
  dim,
  children,
}) => {
  const f = useCurrentFrame();
  const c = cam ? cam(f) : {};
  const d = dim ? dim(f) : 0;
  const blur = (c.blur ?? 0) + d * 4;
  return (
    <AbsoluteFill>
      <Backdrop px={-(c.ry ?? 0) * 20 - (c.x ?? 0)} py={(c.rx ?? 0) * 20 - (c.y ?? 0)} />
      <div
        style={{
          position: "absolute",
          left: (1920 - width) / 2,
          top,
          width,
          height,
          borderRadius: 22,
          overflow: "hidden",
          background: C.termBg,
          boxShadow: `0 ${50 + d * 20}px 120px rgba(20,20,40,0.28), 0 0 0 1px rgba(0,0,0,0.25)`,
          transform: camTransform({ ...c, z: (c.z ?? 0) - d * 260 }),
          transformOrigin: origin,
          filter: blur > 0.05 || d ? `blur(${blur}px) brightness(${1 - d * 0.3})` : undefined,
        }}
      >
        <div
          style={{
            height: 48,
            background: C.termBar,
            display: "flex",
            alignItems: "center",
            padding: "0 20px",
            gap: 10,
            borderBottom: "1px solid rgba(255,255,255,0.06)",
          }}
        >
          {["#ff5f57", "#febc2e", "#28c840"].map((col) => (
            <div key={col} style={{ width: 14, height: 14, borderRadius: 7, background: col }} />
          ))}
          <div
            style={{
              flex: 1,
              textAlign: "center",
              fontFamily: SANS,
              fontSize: 21,
              fontWeight: 500,
              color: C.tDim,
              marginRight: 62,
            }}
          >
            {title}
          </div>
        </div>
        <div
          style={{
            padding: "30px 46px",
            fontFamily: MONO,
            fontSize: size,
            display: "flex",
            flexDirection: "column",
            gap: 12,
          }}
        >
          {lines.map((l, i) => (
            <TermLine key={i} line={l} f={f} />
          ))}
        </div>
      </div>
      {children}
    </AbsoluteFill>
  );
};

/** A bold caption under the window; it swings up out of depth. */
const Caption: React.FC<{ text: string; from: number; to: number; y?: number; color?: string }> = ({
  text,
  from,
  to,
  y = 948,
  color = C.ink,
}) => {
  const f = useCurrentFrame();
  if (f < from || f > to) return null;
  const p = ramp(f, from, from + 11);
  const out = ramp(f, to - 6, to, 0, 1, accel);
  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        right: 0,
        top: y - 44,
        display: "flex",
        justifyContent: "center",
        opacity: Math.min(p, 1 - out),
        transform: `perspective(1200px) translateY(${(1 - p) * 40 - out * 20}px) rotateX(${(1 - p) * -60 + out * 40}deg)`,
        transformOrigin: "50% 100%",
        filter: `blur(${(1 - p) * 8 + out * 6}px)`,
      }}
    >
      <div style={{ fontFamily: SANS, fontSize: 58, fontWeight: 700, letterSpacing: "-0.03em", color }}>{text}</div>
    </div>
  );
};

/** Words fly in from depth, one after another. */
const Fly: React.FC<{
  text: string;
  delay?: number;
  size: number;
  weight?: number;
  color?: string;
  stagger?: number;
  accentFrom?: number;
  exitAt?: number;
}> = ({ text, delay = 0, size, weight = 700, color = C.ink, stagger = 3, accentFrom = 99, exitAt }) => {
  const f = useCurrentFrame();
  const ex = exitAt !== undefined ? ramp(f, exitAt, exitAt + 7, 0, 1, accel) : 0;
  return (
    <div
      style={{
        fontFamily: SANS,
        fontSize: size,
        fontWeight: weight,
        color,
        letterSpacing: "-0.04em",
        lineHeight: 1.05,
        display: "flex",
        gap: "0.24em",
        justifyContent: "center",
      }}
    >
      {text.split(" ").map((w, i) => {
        const s = delay + i * stagger;
        const p = sp(f - s, 16, 150);
        const z = (1 - p) * -900 + ex * 700;
        return (
          <span
            key={i}
            style={{
              display: "inline-block",
              opacity: Math.min(1, p * 1.6) * (1 - ex),
              transform: `perspective(1400px) translateZ(${z}px) rotateY(${(1 - p) * (i % 2 ? 25 : -25)}deg)`,
              filter: `blur(${(1 - Math.min(1, p)) * 12 + ex * 10}px)`,
              color: i >= accentFrom ? C.accentSoft : undefined,
            }}
          >
            {w}
          </span>
        );
      })}
    </div>
  );
};

const Cursor: React.FC<{ x: number; y: number; press: number }> = ({ x, y, press }) => (
  <svg
    width={44}
    height={44}
    viewBox="0 0 24 24"
    style={{ position: "absolute", left: x, top: y, scale: `${1 - press * 0.15}`, transformOrigin: "0 0" }}
  >
    <path
      d="M4 2.5 19 13.2l-6.4.9 3.6 7-2.6 1.3-3.6-7.1L4.6 19.6z"
      fill="#111"
      stroke="#fff"
      strokeWidth={1.4}
      strokeLinejoin="round"
    />
  </svg>
);

// ---- scenes ---------------------------------------------------------------------------

// Made-up values in real formats.
const TOKEN = "ghp_7Kq2vX9mZ4pL8wR3nT6yB1cF5hJ0dS";
const OPENAI = "sk-proj-Q2demoX7dEmo4FakeK3yNotReal9";

const Hook: React.FC = () => (
  <Term
    size={46}
    width={1700}
    height={560}
    top={250}
    origin="72% 30%"
    cam={(f) => {
      const settle = sp(f, 20, 90);
      // ends by rushing into the pasted token
      const rush = ramp(f, 46, 57, 0, 1, accel);
      return {
        rx: 18 * (1 - settle) + 3,
        ry: -24 * (1 - settle) - 4 + f * 0.06,
        z: -380 * (1 - settle) + f * 1.5,
        s: 1 + rush * 5,
        blur: rush * 14,
      };
    }}
    lines={[
      { kind: "prompt", at: 0, text: "deploy's broken. here's my token ", cps: 3, paste: { at: 12, text: TOKEN }, enter: 17 },
      { kind: "warn", at: 21, text: "That's a live GitHub token, pasted in plain text." },
      { kind: "warncont", at: 27, text: "[[Revoke it now]] and create a new one." },
    ]}
  />
);

const Again: React.FC = () => {
  const f = useCurrentFrame();
  const s = sp(f, 12, 260);
  const shake = f < 9 ? Math.sin(f * 2.6) * (9 - f) * 2 : 0;
  const ex = ramp(f, 13, 18, 0, 1, accel);
  return (
    <AbsoluteFill>
      <Backdrop />
      <AbsoluteFill style={{ alignItems: "center", justifyContent: "center" }}>
        <div
          style={{
            fontFamily: SANS,
            fontWeight: 800,
            fontSize: 300,
            letterSpacing: "-0.06em",
            color: C.ink,
            transform: `perspective(1400px) translateZ(${(1 - s) * 900 - ex * 1200}px) translateX(${shake}px)`,
            opacity: Math.min(1, s * 2) * (1 - ex),
            filter: `blur(${Math.max(0, 1 - s) * 18 + ex * 10}px)`,
          }}
        >
          Again?
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  );
};

const Forgot: React.FC = () => (
  <Term
    size={46}
    width={1700}
    height={500}
    top={280}
    title="acme-web — agent · new session"
    cam={(f) => {
      const whip = sp(f, 17, 170);
      const out = ramp(f, 38, 45, 0, 1, accel);
      return {
        x: (1 - whip) * 1500 - out * 300,
        ry: (1 - whip) * -40 + 3 - f * 0.08 + out * 30,
        rx: 2 + out * 40,
        z: -60 + f * 1.2 - out * 500,
        blur: (1 - whip) * 16 + out * 10,
      };
    }}
    lines={[
      { kind: "prompt", at: 1, text: "is CI green now?", cps: 2, enter: 11 },
      { kind: "text", at: 15, text: "I don't have a GitHub token in this session." },
      { kind: "cont", at: 21, text: "Could you [[paste it again]]?" },
    ]}
  />
);

const Env: React.FC = () => {
  const f = useCurrentFrame();
  const rows: [string, string][] = [
    ["GITHUB_TOKEN", TOKEN],
    ["OPENAI_API_KEY", OPENAI],
    ["STRIPE_SECRET_KEY", "sk_live_…EXAMPLE"],
    ["DATABASE_URL", "postgres://admin:hunter2@db.acme.dev"],
  ];
  // The file lies like a sheet on a desk; the camera glides along it and lifts away.
  const rise = sp(f, 20, 110);
  const out = ramp(f, 24, 30, 0, 1, accel);
  return (
    <AbsoluteFill>
      <Backdrop py={f * 4} />
      <div
        style={{
          position: "absolute",
          left: 210,
          top: 250,
          width: 1500,
          background: "#fff",
          borderRadius: 22,
          boxShadow: "0 60px 120px rgba(20,20,40,0.18), 0 0 0 1px rgba(0,0,0,0.06)",
          overflow: "hidden",
          transform: camTransform({
            rx: 52 * (1 - rise) + 14 - out * 30,
            rz: -6 * (1 - rise),
            y: 160 * (1 - rise) - f * 3 - out * 200,
            z: -200 + f * 4 + out * 300,
          }),
          transformOrigin: "50% 50%",
          opacity: 1 - out,
          filter: out ? `blur(${out * 10}px)` : undefined,
        }}
      >
        <div style={{ height: 54, background: "#f4f4f6", display: "flex", alignItems: "flex-end", padding: "0 20px" }}>
          <div
            style={{
              background: "#fff",
              padding: "10px 26px",
              borderRadius: "10px 10px 0 0",
              fontFamily: MONO,
              fontSize: 22,
              color: C.ink,
            }}
          >
            .env
          </div>
        </div>
        <div style={{ padding: "26px 0", fontFamily: MONO, fontSize: 32, lineHeight: 1.9 }}>
          {rows.map(([k, v], i) => {
            const h = ramp(f, 4 + i * 4, 10 + i * 4);
            return (
              <div key={k} style={{ display: "flex", padding: "0 30px" }}>
                <span style={{ width: 60, color: C.ink3 }}>{i + 1}</span>
                <span style={{ color: "#0b5cad" }}>{k}</span>
                <span style={{ color: C.ink2 }}>=</span>
                <span
                  style={{
                    color: "#b3261e",
                    background: `rgba(223,27,65,${0.13 * h})`,
                    borderRadius: 6,
                    padding: "0 6px",
                    textDecoration: h > 0.5 ? "underline wavy rgba(223,27,65,0.7)" : "none",
                    textUnderlineOffset: 8,
                  }}
                >
                  {v}
                </span>
              </div>
            );
          })}
        </div>
      </div>
      <Caption text="Plain text. Everywhere." from={6} to={30} y={900} />
    </AbsoluteFill>
  );
};

const Title: React.FC = () => (
  <AbsoluteFill>
    <Backdrop dark />
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", gap: 10 }}>
      <Fly text="Give your agent" size={124} color="#fff" delay={0} exitAt={24} />
      <Fly text="a key vault." size={124} color="#fff" delay={6} accentFrom={1} exitAt={24} />
    </AbsoluteFill>
  </AbsoluteFill>
);

// Logo, then the camera flies through the keyhole into the next scene.
const LOGO = 200;
const WORD_W = 600;
const GROUP_W = LOGO + 44 + WORD_W;
const LOGO_X = (1920 - GROUP_W) / 2;
const LOGO_Y = 540 - LOGO / 2;
const HOLE_X = LOGO_X + LOGO / 2;
const HOLE_Y = LOGO_Y + (9.6 / 24) * LOGO;

const LogoScene: React.FC = () => {
  const f = useCurrentFrame();
  const s = sp(f, 13, 170);
  const w = ramp(f, 5, 16);
  const center = ramp(f, 12, 24, 0, 1, inOut);
  const dive = interpolate(f, [15, 29], [0, Math.log(70)], { ...clamp, easing: Easing.in(Easing.cubic) });
  return (
    <AbsoluteFill>
      <Backdrop />
      <AbsoluteFill
        style={{
          transform: `translate(${center * (960 - HOLE_X)}px, ${center * (540 - HOLE_Y)}px) scale(${Math.exp(dive)})`,
          transformOrigin: `${HOLE_X}px ${HOLE_Y}px`,
        }}
      >
        <div
          style={{
            position: "absolute",
            left: LOGO_X,
            top: LOGO_Y,
            transform: `perspective(1200px) rotateY(${(1 - s) * -70}deg) scale(${0.6 + 0.4 * s})`,
            opacity: Math.min(1, s * 1.5),
          }}
        >
          <Logo size={LOGO} />
        </div>
        <div
          style={{
            position: "absolute",
            left: LOGO_X + LOGO + 44,
            top: 540 - 95,
            fontFamily: SANS,
            fontWeight: 700,
            fontSize: 150,
            lineHeight: "190px",
            letterSpacing: "-0.05em",
            color: C.ink,
            opacity: w * ramp(f, 17, 22, 1, 0),
            translate: `${(1 - w) * -40}px 0px`,
            filter: `blur(${(1 - w) * 10}px)`,
          }}
        >
          keymule
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  );
};

// The real keymule dialog, rendered from src/consent/dialog.html (1320×870 @3x).
const DIALOG_W = 840;
const DIALOG_H = Math.round((870 / 1320) * DIALOG_W);

const Dialog: React.FC<{ from: number; paste: number; hover: number; click: number; to: { x: number; y: number } }> = ({
  from,
  paste,
  hover,
  click,
  to,
}) => {
  const f = useCurrentFrame();
  if (f < from || f > click + 16) return null;
  const s = sp(f - from, 15, 190);
  const fly = ramp(f, click + 2, click + 14, 0, 1, accel);
  const src = f >= hover ? "dialog-hover.png" : f >= paste ? "dialog-filled.png" : "dialog-empty.png";
  const left = (1920 - DIALOG_W) / 2;
  const top = (1080 - DIALOG_H) / 2 - 40;
  const u = DIALOG_W / 660;
  const float = Math.sin((f - from) / 14) * 1.5;
  // The pointer drifts to the field, waits for the paste, then goes to Save.
  const keys = [from + 8, paste - 4, hover - 16, hover];
  const cx = interpolate(f, keys, [left + 760 * u, left + 420 * u, left + 420 * u, left + 586 * u], { ...clamp, easing: inOut });
  const cy = interpolate(f, keys, [top + 520 * u, top + 300 * u, top + 300 * u, top + 392 * u], { ...clamp, easing: inOut });
  const press = f >= click && f < click + 5 ? 1 : 0;
  const kbd = ramp(f, paste - 2, paste + 2) * ramp(f, paste + 11, paste + 16, 1, 0);
  const dx = (to.x - (left + DIALOG_W / 2)) * fly;
  const dy = (to.y - (top + DIALOG_H / 2)) * fly;
  return (
    <>
      <div
        style={{
          position: "absolute",
          left,
          top,
          width: DIALOG_W,
          height: DIALOG_H,
          borderRadius: 16,
          overflow: "hidden",
          boxShadow: "0 60px 140px rgba(0,0,0,0.5), 0 0 0 1px rgba(0,0,0,0.12)",
          transform: `perspective(1600px) translate3d(${dx}px, ${dy}px, ${(1 - s) * -500 + 60}px) rotateX(${
            (1 - s) * 28 + float
          }deg) rotateY(${(1 - s) * -12 + float}deg) scale(${1 - fly * 0.94})`,
          opacity: Math.min(s * 1.6, 1) * (1 - fly * 0.6),
        }}
      >
        <Img src={staticFile(`shots/${src}`)} style={{ width: DIALOG_W, height: DIALOG_H, display: "block" }} />
      </div>
      {kbd > 0 && (
        <div
          style={{
            position: "absolute",
            left: left + DIALOG_W + 30,
            top: top + 250 * u,
            opacity: kbd,
            transform: `perspective(800px) translateZ(${(1 - kbd) * -200}px) rotateY(-14deg)`,
            fontFamily: SANS,
            fontWeight: 600,
            fontSize: 44,
            color: C.ink,
            background: "#fff",
            borderRadius: 14,
            padding: "10px 22px",
            boxShadow: "0 0 0 1px rgba(0,0,0,0.1), 0 6px 0 rgba(0,0,0,0.12), 0 20px 40px rgba(0,0,0,0.25)",
          }}
        >
          ⌘ V
        </div>
      )}
      {f >= from + 8 && fly < 0.2 && <Cursor x={cx} y={cy} press={press} />}
    </>
  );
};

const Flow: React.FC = () => {
  const D = { from: 62, paste: 86, hover: 116, click: 122 };
  return (
    <Term
      origin="50% 62%"
      cam={(f) => {
        const emerge = sp(f, 20, 80);
        // push in on the redacted header, then ease back out
        const push = ramp(f, 166, 186, 0, 1, inOut) * ramp(f, 230, 262, 1, 0.4, inOut);
        return {
          z: (1 - emerge) * -1100 + push * 160,
          rx: (1 - emerge) * 14 + 4 - push * 2,
          ry: interpolate(f, [0, 270], [-7, 6]),
          y: push * -40,
          blur: (1 - emerge) * 6,
        };
      }}
      dim={(f) => ramp(f, D.from - 4, D.from + 6) * ramp(f, D.click, D.click + 12, 1, 0)}
      lines={[
        { kind: "prompt", at: 0, text: "why is CI failing on acme/web?", cps: 1.8, enter: 19 },
        { kind: "tool", at: 26, text: "*keymule* · list_credentials()" },
        { kind: "result", at: 33, text: "No GitHub credential stored yet." },
        { kind: "tool", at: 44, text: '*keymule* · add_credential(service: "github", name: "github-ci")' },
        { kind: "result", at: 51, text: "A keymule dialog is open. Waiting for you…" },
        { kind: "ok", at: 137, text: "Saved {github-ci}. The token went to keymule, not to the agent." },
        { kind: "tool", at: 156, text: '*keymule* · http_request("github-ci", GET /repos/acme/web/actions/runs)' },
        { kind: "result", at: 166, text: "200 OK · Authorization: Bearer {[redacted]}" },
        { kind: "text", at: 190, text: "CI fails in *test:e2e*: PLAYWRIGHT_BROWSERS_PATH isn't set." },
        { kind: "cont", at: 197, text: "Fixed the workflow and pushed." },
      ]}
    >
      <Dialog {...D} to={{ x: 420, y: 500 }} />
      <Caption text="You paste it once, into keymule." from={64} to={134} />
      <Caption text="Your agent uses it. It never sees it." from={168} to={270} />
    </Term>
  );
};

const MenuItem: React.FC<{ label: string; on?: boolean; check?: boolean }> = ({ label, on, check }) => (
  <div
    style={{
      padding: "5px 12px 5px 28px",
      margin: "0 6px",
      borderRadius: 6,
      background: on ? "#3b82f6" : "transparent",
      color: on ? "#fff" : C.ink,
      position: "relative",
    }}
  >
    {check && <span style={{ position: "absolute", left: 10 }}>✓</span>}
    {label}
  </div>
);

const Ui: React.FC = () => {
  const f = useCurrentFrame();
  const menuOpen = f >= 6 && f < 26;
  const hoverOpen = f >= 14;
  const win = sp(f - 24, 17, 120);
  const zoom = interpolate(f, [0, 20, 40], [2.3, 2.3, 1], { ...clamp, easing: inOut });
  const after = ramp(f, 46, 50);
  const lift = sp(f - 48, 14, 140);
  const turn = ramp(f, 40, 90, 0, 1, inOut);
  const W = 1400;
  const k = W / 2880;
  const H = 1800 * k * 0.86;
  const rowsTop = 250 * k;
  const rowsH = 165 * k;
  return (
    <AbsoluteFill>
      <Backdrop px={turn * 200} py={-zoom * 40} />
      <AbsoluteFill style={{ scale: `${zoom}`, transformOrigin: "1910px 0px" }}>
        <div
          style={{
            position: "absolute",
            top: 0,
            left: 0,
            right: 0,
            height: 34,
            background: "rgba(255,255,255,0.72)",
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            gap: 22,
            padding: "0 22px",
            fontFamily: SANS,
            fontSize: 15,
            fontWeight: 500,
            color: C.ink,
          }}
        >
          <div
            style={{
              padding: "3px 6px",
              borderRadius: 5,
              background: menuOpen ? "rgba(0,0,0,0.12)" : "transparent",
              display: "flex",
            }}
          >
            <Logo size={18} />
          </div>
          <span style={{ letterSpacing: 0.2 }}>Mon 16 Nov 9:41</span>
        </div>
        {menuOpen && (
          <div
            style={{
              position: "absolute",
              top: 38,
              left: 1700,
              width: 200,
              padding: "6px 0",
              background: "rgba(246,246,248,0.96)",
              borderRadius: 10,
              boxShadow: "0 0 0 0.5px rgba(0,0,0,0.2), 0 10px 30px rgba(0,0,0,0.18)",
              fontFamily: SANS,
              fontSize: 14,
              transform: `perspective(600px) rotateX(${(1 - sp(f - 6, 16, 260)) * -30}deg)`,
              transformOrigin: "50% 0%",
            }}
          >
            <MenuItem label="Open keymule" on={hoverOpen} />
            <MenuItem label="Open in Browser" />
            <div style={{ height: 1, background: "rgba(0,0,0,0.1)", margin: "5px 12px" }} />
            <MenuItem label="Start at Login" check />
            <div style={{ height: 1, background: "rgba(0,0,0,0.1)", margin: "5px 12px" }} />
            <MenuItem label="Quit keymule" />
          </div>
        )}
      </AbsoluteFill>
      {f >= 24 && (
        // The window swings down from the menu bar, then turns so the new rows lift off it.
        <div
          style={{
            position: "absolute",
            left: (1920 - W) / 2,
            top: 80,
            width: W,
            height: H,
            transformStyle: "preserve-3d",
            transform: camTransform({
              rx: (1 - win) * -40 + turn * 10,
              ry: turn * -16,
              z: (1 - win) * -300 + turn * 40,
              x: turn * 60,
            }),
            transformOrigin: "50% 0%",
            opacity: Math.min(1, win * 1.4),
          }}
        >
          <div
            style={{
              position: "absolute",
              inset: 0,
              borderRadius: 14,
              overflow: "hidden",
              background: "#fff",
              boxShadow: "0 50px 120px rgba(20,20,40,0.25), 0 0 0 1px rgba(0,0,0,0.1)",
            }}
          >
            <Img src={staticFile("shots/act-before.png")} style={{ width: W, position: "absolute", top: 0 }} />
            <Img src={staticFile("shots/act-after.png")} style={{ width: W, position: "absolute", top: 0, opacity: after }} />
          </div>
          {/* the two new rows, lifted toward the camera */}
          <div
            style={{
              position: "absolute",
              left: 520 * k,
              top: rowsTop,
              width: (2820 - 520) * k,
              height: rowsH,
              overflow: "hidden",
              borderRadius: 10,
              background: "#fff",
              opacity: lift > 0.01 ? 1 : 0,
              transform: `translateZ(${lift * 90}px)`,
              boxShadow: `0 ${lift * 30}px ${lift * 60}px rgba(40,30,120,${0.25 * lift}), 0 0 0 ${2 * lift}px rgba(99,91,255,${0.7 * lift})`,
            }}
          >
            <Img
              src={staticFile("shots/act-after.png")}
              style={{ width: W, position: "absolute", left: -520 * k, top: -rowsTop }}
            />
          </div>
        </div>
      )}
      <Caption text="Every use, on the record." from={54} to={90} y={1000} />
    </AbsoluteFill>
  );
};

const Later: React.FC = () => {
  const f = useCurrentFrame();
  const out = ramp(f, 18, 24, 0, 1, accel);
  return (
    <AbsoluteFill>
      <Backdrop px={f * 30} />
      <AbsoluteFill
        style={{
          alignItems: "center",
          justifyContent: "center",
          gap: 18,
          transform: `translateX(${(1 - sp(f, 18, 160)) * 700 - out * 900}px)`,
          filter: `blur(${(1 - sp(f, 18, 160)) * 12 + out * 14}px)`,
        }}
      >
        <Fly text="3 days later." size={130} delay={0} />
        <Fly text="New session." size={60} weight={500} color={C.ink2} delay={5} />
      </AbsoluteFill>
    </AbsoluteFill>
  );
};

const Reuse: React.FC = () => (
  <Term
    title="acme-web — agent · new session"
    origin="50% 40%"
    cam={(f) => {
      const whip = sp(f, 17, 150);
      const beat1 = ramp(f, 62, 76, 0, 1, inOut);
      const beat2 = ramp(f, 92, 106, 0, 1, inOut);
      return {
        x: (1 - whip) * -1500,
        ry: (1 - whip) * 38 + 5 - beat1 * 6 - beat2 * 4,
        rx: 3 + beat1 * 2,
        z: beat1 * 60 + beat2 * 60,
        y: beat1 * -60 + beat2 * -60,
        blur: (1 - whip) * 16,
      };
    }}
    lines={[
      { kind: "prompt", at: 0, text: "CI is red again, take a look", cps: 2, enter: 15 },
      { kind: "tool", at: 20, text: '*keymule* · http_request("github-ci", GET /repos/acme/web/actions/runs)' },
      { kind: "result", at: 27, text: "200 OK" },
      { kind: "text", at: 34, text: "A flaky test in auth.spec.ts. Re-ran it: green." },
      { kind: "tool", at: 66, text: '*keymule* · rotate_credential("github-ci")' },
      { kind: "result", at: 73, text: "New token typed into a keymule dialog. The old one is gone." },
      { kind: "tool", at: 96, text: "*Bash* · keymule ssh vps-sydney -- uptime" },
      { kind: "result", at: 103, text: "up 92 days, load average 0.08  (password never shown)" },
    ]}
  >
    <Caption text="Still there. Nothing to paste." from={22} to={64} />
    <Caption text="Rotate it in one dialog." from={66} to={94} />
    <Caption text="SSH logins too." from={96} to={126} />
  </Term>
);

/** The held key, flying out of the message into keymule. */
const KeyChip: React.FC<{ at: number; from: { x: number; y: number }; to: { x: number; y: number } }> = ({ at, from, to }) => {
  const f = useCurrentFrame();
  if (f < at - 2) return null;
  const t = ramp(f, at, at + 13, 0, 1, inOut);
  const badge = sp(f - at + 4, 12, 200);
  const hit = ramp(f, at + 12, at + 22, 1, 0);
  const x = interpolate(t, [0, 1], [from.x, to.x]);
  const y = interpolate(t, [0, 1], [from.y, to.y]) - Math.sin(t * Math.PI) * 160;
  return (
    <>
      <div
        style={{
          position: "absolute",
          left: to.x - 46,
          top: to.y - 46,
          width: 92,
          height: 92,
          borderRadius: 24,
          background: "#fff",
          boxShadow: `0 20px 50px rgba(20,20,40,0.3), 0 0 0 ${8 * hit}px rgba(99,91,255,${0.35 * hit})`,
          display: "grid",
          placeItems: "center",
          transform: `perspective(800px) translateZ(${(1 - badge) * -300}px) scale(${1 + hit * 0.12})`,
          opacity: Math.min(1, badge * 1.5),
        }}
      >
        <Logo size={60} />
      </div>
      {t < 1 && (
        <div
          style={{
            position: "absolute",
            left: x,
            top: y,
            translate: "-50% -50%",
            fontFamily: MONO,
            fontSize: 30,
            fontWeight: 600,
            color: "#fff",
            background: "rgba(255,107,107,0.92)",
            borderRadius: 10,
            padding: "6px 14px",
            boxShadow: "0 16px 40px rgba(255,80,80,0.4)",
            transform: `perspective(800px) translateZ(${Math.sin(t * Math.PI) * 200}px) rotateZ(${t * 12}deg) scale(${1 - t * 0.6})`,
          }}
        >
          sk-p…al9
        </div>
      )}
    </>
  );
};

const Guard: React.FC = () => (
  <Term
    origin="50% 30%"
    cam={(f) => {
      const enter = sp(f, 20, 110);
      const lean = ramp(f, 22, 40, 0, 1, inOut);
      return {
        z: (1 - enter) * -700 + lean * 70,
        rx: (1 - enter) * -20 + 3 + lean * 3,
        ry: interpolate(f, [0, 90], [-5, 3]),
        blur: (1 - enter) * 8,
      };
    }}
    lines={[
      {
        kind: "prompt",
        at: 0,
        text: "add AI summaries to the report, key: ",
        cps: 3,
        paste: { at: 13, text: OPENAI, gone: 55, replace: "[saved in keymule as `openai`]" },
        enter: 18,
      },
      {
        kind: "hold",
        at: 23,
        lines: [
          "keymule held back your message: it has an OpenAI API key (sk-p…al9).",
          "  y           save it as `openai` and send your message without it",
          "  y <name>    save it under another name",
          "  n           send your message as it was",
        ],
      },
      { kind: "prompt", at: 46, text: "y", cps: 1, enter: 52 },
      { kind: "ok", at: 68, text: "keymule saved `openai` (OpenAI) and sent your message without it." },
    ]}
  >
    <KeyChip at={55} from={{ x: 1300, y: 170 }} to={{ x: 1740, y: 120 }} />
    <Caption text="Pasted it by habit? keymule catches it." from={26} to={90} />
  </Term>
);

const INSTALL = "curl -LsSf https://github.com/Rorogogogo/keymule/releases/latest/download/keymule-installer.sh | sh";

const End: React.FC = () => {
  const f = useCurrentFrame();
  const s = sp(f, 13, 150);
  const pill = sp(f - 18, 18, 130);
  const url = ramp(f, 28, 42);
  return (
    <AbsoluteFill>
      <Backdrop px={-f * 2} />
      <AbsoluteFill
        style={{
          alignItems: "center",
          justifyContent: "center",
          transform: `perspective(2000px) translateZ(${interpolate(f, [0, 90], [-60, 40])}px)`,
        }}
      >
        <div
          style={{
            transform: `perspective(1000px) rotateY(${(1 - s) * 80}deg) scale(${0.6 + 0.4 * s})`,
            opacity: Math.min(1, s * 1.5),
            marginBottom: 40,
          }}
        >
          <Logo size={130} />
        </div>
        <Fly text="Your agent's key vault." size={120} delay={4} stagger={3} />
        <div style={{ height: 22 }} />
        <Fly
          text="Tokens, passwords and SSH logins. Used by your agent, never seen."
          size={38}
          weight={500}
          color={C.ink2}
          delay={10}
          stagger={1}
        />
        <div
          style={{
            marginTop: 60,
            opacity: Math.min(1, pill * 1.5),
            transform: `perspective(1200px) rotateX(${(1 - pill) * 70}deg) translateY(${(1 - pill) * 30}px)`,
            transformOrigin: "50% 0%",
            fontFamily: MONO,
            fontSize: 27,
            color: "#fff",
            background: C.ink,
            borderRadius: 16,
            padding: "18px 30px",
            boxShadow: "0 30px 60px rgba(20,20,40,0.25)",
          }}
        >
          <span style={{ color: C.ink3 }}>$ </span>
          {INSTALL}
        </div>
        <div style={{ marginTop: 26, opacity: url, fontFamily: SANS, fontSize: 28, color: C.ink3 }}>
          github.com/Rorogogogo/keymule
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  );
};

// ---- sound ---------------------------------------------------------------------------

type Cue = [frame: number, sfx: string, vol?: number];
const CUES: Cue[] = [
  // hook, then the rush into the token
  [0, "type", 0.9], [12, "paste"], [17, "enter"], [21, "warn"], [47, "whoosh", 0.9],
  // forgot (whips in from the side)
  [74, "whoosh", 0.8], [77, "type", 0.8], [86, "enter"], [90, "warn", 0.6],
  // .env highlights
  [124, "tick"], [128, "tick"], [132, "tick"], [136, "tick"],
  // logo, then the dive through the keyhole
  [182, "chime", 0.8], [197, "whoosh"],
  // MCP flow (scene starts at 210)
  [212, "type"], [224, "type", 0.7], [229, "enter"], [236, "tick"], [254, "tick"],
  [272, "pop"], [296, "paste"], [332, "click"], [335, "whoosh", 0.6], [347, "chime"], [366, "tick"], [376, "tick"],
  [400, "tick"],
  // UI
  [484, "click"], [502, "click"], [504, "whoosh", 0.7], [528, "pop", 0.7],
  // 3 days later
  [567, "whoosh", 0.8],
  // reuse (whips in from the other side)
  [592, "whoosh", 0.8], [596, "type"], [609, "enter"], [614, "tick"], [621, "tick"], [660, "tick"], [667, "pop", 0.6],
  [690, "tick"],
  // guard
  [720, "type"], [733, "paste"], [738, "enter"], [743, "pop"], [766, "type", 0.5], [772, "enter"],
  [775, "whoosh", 0.6], [787, "chime"],
  // end
  [804, "whoosh"],
];

// Duck the music under the dialog so the paste and the click read clearly.
const scoreVolume = (f: number) => 0.6 - 0.22 * interpolate(f, [262, 272, 350, 362], [0, 1, 1, 0], clamp);

// ---- the film ---------------------------------------------------------------------------

const SCENES: [readonly [number, number], React.FC][] = [
  [T.hook, Hook],
  [T.again, Again],
  [T.forgot, Forgot],
  [T.env, Env],
  [T.title, Title],
  [T.logo, LogoScene],
  [T.flow, Flow],
  [T.ui, Ui],
  [T.later, Later],
  [T.reuse, Reuse],
  [T.guard, Guard],
  [T.end, End],
];

export const Launch: React.FC = () => (
  <AbsoluteFill style={{ background: C.bg }}>
    {SCENES.map(([[a, b], S], i) => (
      <Sequence key={i} from={a} durationInFrames={b - a} premountFor={15}>
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
);
