import React from "react";
import {
  AbsoluteFill,
  Easing,
  Interactive,
  Sequence,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { Audio } from "@remotion/media";
import { loadFont } from "@remotion/google-fonts/Inter";
import { loadFont as loadMono } from "@remotion/google-fonts/GeistMono";

const { fontFamily: SANS } = loadFont("normal", {
  weights: ["400", "500", "600", "700"],
  subsets: ["latin"],
});
const { fontFamily: MONO } = loadMono("normal", {
  weights: ["400", "500"],
  subsets: ["latin"],
});

// Quiet, Apple-like palette; the accent is keymule's own.
const C = {
  bg: "#fbfbfd",
  ink: "#1d1d1f",
  ink2: "#6e6e73",
  ink3: "#a1a1a6",
  line: "rgba(0,0,0,0.08)",
  card: "#ffffff",
  accent: "#635bff",
  term: "#1d1d1f",
};

const ease = Easing.bezier(0.16, 1, 0.3, 1);
const inOut = Easing.bezier(0.65, 0, 0.35, 1);
const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;
const ramp = (f: number, a: number, b: number, from = 0, to = 1, e = ease) =>
  interpolate(f, [a, b], [from, to], { ...clamp, easing: e });

// ---- building blocks ------------------------------------------------------------

/** A scene: fades and un-blurs in, drifts slowly closer, fades and blurs out. */
const Scene: React.FC<{ len: number; children: React.ReactNode; name: string; last?: boolean }> = ({
  len,
  children,
  name,
  last = false,
}) => {
  const f = useCurrentFrame();
  // Starts after the previous shot has mostly cleared (shots overlap 10 frames).
  const fadeIn = ramp(f, 7, 20);
  // The end card holds to the last frame.
  const fadeOut = last ? 1 : ramp(f, len - 12, len, 1, 0, inOut);
  return (
    <Interactive.Div
      name={name}
      style={{
        position: "absolute",
        inset: 0,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        opacity: Math.min(fadeIn, fadeOut),
        filter: `blur(${(1 - Math.min(fadeIn, fadeOut)) * 14}px)`,
        scale: interpolate(f, [0, len], [1, 1.035], clamp),
      }}
    >
      {children}
    </Interactive.Div>
  );
};

/** Words rise into place one after another. */
const Rise: React.FC<{
  text: string;
  delay?: number;
  size: number;
  weight?: number;
  color?: string;
  stagger?: number;
}> = ({ text, delay = 0, size, weight = 600, color = C.ink, stagger = 4 }) => {
  const f = useCurrentFrame();
  return (
    <div
      style={{
        fontFamily: SANS,
        fontSize: size,
        fontWeight: weight,
        color,
        letterSpacing: "-0.035em",
        lineHeight: 1.05,
        display: "flex",
        gap: "0.24em",
        justifyContent: "center",
      }}
    >
      {text.split(" ").map((w, i) => {
        const s = delay + i * stagger;
        const p = ramp(f, s, s + 22);
        return (
          <span
            key={i}
            style={{
              display: "inline-block",
              opacity: p,
              translate: `0px ${(1 - p) * 0.35 * size}px`,
              filter: `blur(${(1 - p) * 10}px)`,
            }}
          >
            {w}
          </span>
        );
      })}
    </div>
  );
};

const Logo: React.FC<{ size: number }> = ({ size }) => (
  <svg width={size} height={size} viewBox="0 0 24 24">
    <rect width="24" height="24" rx="6" fill={C.ink} />
    <circle cx="12" cy="9.6" r="3.3" fill="#fff" />
    <path d="M10.3 11.8 9.4 17.6h5.2l-.9-5.8z" fill="#fff" />
  </svg>
);

/** Text typed out over [start, start + chars * speed]. */
const typed = (text: string, f: number, start: number, speed = 1.2) =>
  text.slice(0, Math.max(0, Math.floor((f - start) / speed)));

const Card: React.FC<{ children: React.ReactNode; style?: React.CSSProperties }> = ({
  children,
  style,
}) => (
  <div
    style={{
      background: C.card,
      borderRadius: 32,
      boxShadow: `0 0 0 1px ${C.line}, 0 30px 80px rgba(0,0,0,0.08), 0 8px 24px rgba(0,0,0,0.04)`,
      padding: "44px 52px",
      ...style,
    }}
  >
    {children}
  </div>
);

// ---- scenes -----------------------------------------------------------------------

const S1Need: React.FC = () => (
  <Rise text="Your agents need keys." size={150} delay={8} stagger={5} />
);

const TOKEN = "ghp_8fK2xQ7mN4vR1sT9wL3pZ";

const S2Hide: React.FC = () => {
  const f = useCurrentFrame();
  const bubbleIn = ramp(f, 4, 26);
  return (
    <>
      <Rise text="They shouldn't see them." size={110} delay={2} weight={600} />
      <div
        style={{
          marginTop: 90,
          opacity: bubbleIn,
          translate: `0px ${(1 - bubbleIn) * 40}px`,
          background: "#e9e9eb",
          borderRadius: 44,
          padding: "30px 46px",
          fontFamily: SANS,
          fontSize: 50,
          color: C.ink,
          letterSpacing: "-0.01em",
        }}
      >
        my GitHub token is{" "}
        <span style={{ fontFamily: MONO, fontSize: 46 }}>
          {TOKEN.split("").map((ch, i) => {
            // Left to right, each character dissolves into a dot.
            const s = 46 + i * 1.1;
            const p = ramp(f, s, s + 10);
            return (
              <span
                key={i}
                style={{
                  display: "inline-block",
                  width: "0.6em",
                  textAlign: "center",
                  color: p > 0.5 ? C.accent : C.ink,
                  filter: `blur(${Math.sin(p * Math.PI) * 6}px)`,
                }}
              >
                {p > 0.5 ? "•" : ch}
              </span>
            );
          })}
        </span>
      </div>
    </>
  );
};

const S3Logo: React.FC = () => {
  const f = useCurrentFrame();
  const { fps } = useVideoConfig();
  const pop = spring({ frame: f - 4, fps, config: { damping: 14, stiffness: 120 } });
  const word = ramp(f, 12, 36);
  return (
    <>
      <div style={{ display: "flex", alignItems: "center", gap: 44 }}>
        <div style={{ scale: 0.6 + 0.4 * pop, opacity: Math.min(1, pop * 1.4) }}>
          <Logo size={190} />
        </div>
        <div
          style={{
            fontFamily: SANS,
            fontWeight: 700,
            fontSize: 176,
            letterSpacing: "-0.05em",
            color: C.ink,
            opacity: word,
            translate: `${(1 - word) * -40}px 0px`,
            filter: `blur(${(1 - word) * 12}px)`,
          }}
        >
          keymule
        </div>
      </div>
      <div style={{ marginTop: 56 }}>
        <Rise text="Every key. Never seen." size={64} weight={500} color={C.ink2} delay={34} />
      </div>
    </>
  );
};

const Mono: React.FC<{ children: React.ReactNode; color?: string; size?: number }> = ({
  children,
  color = C.ink,
  size = 38,
}) => (
  <div style={{ fontFamily: MONO, fontSize: size, color, lineHeight: 1.55, whiteSpace: "pre" }}>
    {children}
  </div>
);

const REQ = `http_request(\n  credential: "github",\n  path: "/user"\n)`;

const S4Agent: React.FC = () => {
  const f = useCurrentFrame();
  const left = ramp(f, 14, 34);
  const travel = ramp(f, 76, 96, 0, 1, inOut);
  const right = ramp(f, 94, 112);
  const line = (i: number) => ramp(f, 100 + i * 7, 116 + i * 7);
  return (
    <>
      <Rise text="Your agent asks. keymule signs." size={92} delay={0} stagger={4} />
      <div style={{ display: "flex", alignItems: "center", gap: 40, marginTop: 80 }}>
        <Card style={{ width: 660, height: 330, opacity: left, translate: `0px ${(1 - left) * 30}px` }}>
          <div style={{ fontFamily: SANS, fontSize: 28, fontWeight: 600, color: C.ink3, marginBottom: 18, letterSpacing: "0.02em" }}>
            AGENT
          </div>
          <Mono>{typed(REQ, f, 30, 0.9)}</Mono>
        </Card>
        <div style={{ position: "relative", width: 150, height: 96, display: "flex", alignItems: "center", justifyContent: "center" }}>
          <div style={{ position: "absolute", left: 0, right: 0, top: 47, height: 2, background: C.line }} />
          <div
            style={{
              position: "absolute",
              top: 40,
              left: `${travel * 100}%`,
              width: 16,
              height: 16,
              marginLeft: -8,
              borderRadius: 8,
              background: C.accent,
              opacity: travel > 0 && travel < 1 ? 1 : 0,
            }}
          />
          <div style={{ scale: 1 + Math.sin(Math.PI * travel) * 0.15, zIndex: 1 }}>
            <Logo size={72} />
          </div>
        </div>
        <Card style={{ width: 660, height: 330, opacity: right, translate: `0px ${(1 - right) * 30}px` }}>
          <div style={{ fontFamily: SANS, fontSize: 28, fontWeight: 600, color: C.ink3, marginBottom: 18, letterSpacing: "0.02em" }}>
            RESPONSE
          </div>
          <div style={{ opacity: line(0) }}>
            <Mono color="#1a7f37">HTTP 200 OK</Mono>
          </div>
          <div style={{ opacity: line(1) }}>
            <Mono>"login": "Rorogogogo"</Mono>
          </div>
          <div style={{ opacity: line(2), marginTop: 14 }}>
            <Mono size={30} color={C.ink2}>
              token: <span style={{ color: C.accent }}>[redacted by keymule]</span>
            </Mono>
          </div>
        </Card>
      </div>
    </>
  );
};

const S5Dialog: React.FC = () => {
  const f = useCurrentFrame();
  const win = ramp(f, 10, 32);
  const dots = Math.max(0, Math.min(18, Math.floor((f - 36) / 2.4)));
  const press = f >= 96 && f < 102 ? 0.96 : 1;
  const saved = ramp(f, 102, 114);
  return (
    <>
      <Rise text="Typed once. Into keymule, never the chat." size={84} delay={0} stagger={3} />
      <div
        style={{
          marginTop: 80,
          width: 900,
          opacity: win,
          scale: 0.94 + 0.06 * win,
          background: C.card,
          borderRadius: 30,
          boxShadow: `0 0 0 1px ${C.line}, 0 40px 100px rgba(0,0,0,0.12)`,
          padding: "46px 52px 40px",
          fontFamily: SANS,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 22 }}>
          <Logo size={62} />
          <div>
            <div style={{ fontSize: 38, fontWeight: 600, color: C.ink, letterSpacing: "-0.02em" }}>
              Paste your GitHub token for "github"
            </div>
            <div style={{ fontSize: 28, color: C.ink2, marginTop: 6 }}>
              It will only be sent to api.github.com
            </div>
          </div>
        </div>
        <div
          style={{
            marginTop: 34,
            height: 84,
            borderRadius: 16,
            boxShadow: `inset 0 0 0 2px ${C.accent}`,
            display: "flex",
            alignItems: "center",
            padding: "0 28px",
            fontSize: 44,
            letterSpacing: "0.12em",
            color: C.ink,
          }}
        >
          {"•".repeat(dots)}
          <span style={{ width: 3, height: 46, background: C.accent, opacity: Math.floor(f / 8) % 2 ? 0 : 1, marginLeft: 4 }} />
        </div>
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 18, marginTop: 34 }}>
          <div style={{ fontSize: 30, fontWeight: 500, color: C.ink2, padding: "18px 34px", borderRadius: 14, boxShadow: `inset 0 0 0 1px ${C.line}` }}>
            Cancel
          </div>
          <div
            style={{
              fontSize: 30,
              fontWeight: 600,
              color: "#fff",
              padding: "18px 40px",
              borderRadius: 14,
              background: saved > 0.5 ? "#1a7f37" : C.ink,
              scale: press,
              minWidth: 120,
              textAlign: "center",
            }}
          >
            {saved > 0.5 ? "Saved ✓" : "Save"}
          </div>
        </div>
      </div>
    </>
  );
};

const CMD = "keymule ssh vultr-ssh -- uptime";

const S6Ssh: React.FC = () => {
  const f = useCurrentFrame();
  const pill = ramp(f, 2, 18);
  const term = ramp(f, 16, 36);
  const out = ramp(f, 76, 90);
  const note = ramp(f, 90, 104);
  return (
    <>
      <div
        style={{
          opacity: pill,
          fontFamily: SANS,
          fontSize: 30,
          fontWeight: 600,
          color: C.accent,
          background: "rgba(99,91,255,0.09)",
          padding: "10px 24px",
          borderRadius: 999,
          marginBottom: 28,
        }}
      >
        New in 0.16
      </div>
      <Rise text="SSH logins." size={150} delay={6} />
      <div
        style={{
          marginTop: 70,
          width: 1180,
          opacity: term,
          translate: `0px ${(1 - term) * 40}px`,
          background: C.term,
          borderRadius: 28,
          padding: "34px 46px 42px",
          boxShadow: "0 40px 100px rgba(0,0,0,0.18)",
        }}
      >
        <div style={{ display: "flex", gap: 12, marginBottom: 26 }}>
          {["#ff5f57", "#febc2e", "#28c840"].map((c) => (
            <div key={c} style={{ width: 18, height: 18, borderRadius: 9, background: c }} />
          ))}
        </div>
        <Mono color="#f5f5f7" size={40}>
          <span style={{ color: "#8e8e93" }}>$ </span>
          {typed(CMD, f, 40, 1.05)}
        </Mono>
        <div style={{ opacity: out }}>
          <Mono color="#d1d1d6" size={40}>
            {" up 92 days,  load average: 0.03, 0.08, 0.07"}
          </Mono>
        </div>
        <div style={{ opacity: note, marginTop: 12 }}>
          <Mono color="#8b85ff" size={30}>
            # logged in by keymule · the agent never sees the password
          </Mono>
        </div>
      </div>
    </>
  );
};

const S7Three: React.FC = () => {
  const f = useCurrentFrame();
  const words = ["Tokens.", "Passwords.", "SSH."];
  const sub = ramp(f, 76, 94);
  return (
    <>
      <div style={{ display: "flex", gap: 46 }}>
        {words.map((w, i) => {
          const s = 15 + i * 30; // on the beat
          const p = ramp(f, s, s + 14);
          return (
            <div
              key={w}
              style={{
                fontFamily: SANS,
                fontSize: 150,
                fontWeight: 700,
                letterSpacing: "-0.045em",
                color: C.ink,
                opacity: p,
                translate: `0px ${(1 - p) * 50}px`,
                filter: `blur(${(1 - p) * 12}px)`,
              }}
            >
              {w}
            </div>
          );
        })}
      </div>
      <div
        style={{
          marginTop: 44,
          opacity: sub,
          fontFamily: SANS,
          fontSize: 56,
          fontWeight: 500,
          color: C.ink2,
          letterSpacing: "-0.02em",
        }}
      >
        One vault. Nothing for your agent to leak.
      </div>
    </>
  );
};

const S8End: React.FC = () => {
  const f = useCurrentFrame();
  const { fps } = useVideoConfig();
  const pop = spring({ frame: f - 5, fps, config: { damping: 16, stiffness: 110 } });
  const install = ramp(f, 26, 46);
  const meta = ramp(f, 40, 60);
  return (
    <>
      <div style={{ display: "flex", alignItems: "center", gap: 34, opacity: Math.min(1, pop * 1.3), scale: 0.85 + 0.15 * pop }}>
        <Logo size={140} />
        <div style={{ fontFamily: SANS, fontWeight: 700, fontSize: 130, letterSpacing: "-0.05em", color: C.ink }}>
          keymule
        </div>
      </div>
      <div
        style={{
          marginTop: 70,
          opacity: install,
          translate: `0px ${(1 - install) * 24}px`,
          fontFamily: MONO,
          fontSize: 44,
          color: C.ink,
          background: "#f0f0f3",
          padding: "26px 44px",
          borderRadius: 20,
        }}
      >
        <span style={{ color: C.ink3 }}>$ </span>brew install rorogogogo/tap/keymule
      </div>
      <div
        style={{
          marginTop: 40,
          opacity: meta,
          fontFamily: SANS,
          fontSize: 34,
          color: C.ink3,
          letterSpacing: "-0.01em",
        }}
      >
        Free and open source · macOS and Linux · github.com/Rorogogogo/keymule
      </div>
    </>
  );
};

// ---- timeline ---------------------------------------------------------------------

// [start, end] in frames at 30 fps; neighbours overlap for crossfades.
const SHOTS: Array<[string, number, number, React.FC]> = [
  ["Need keys", 0, 100, S1Need],
  ["Shouldn't see", 90, 200, S2Hide],
  ["Logo", 190, 300, S3Logo],
  ["Agent asks", 290, 455, S4Agent],
  ["Dialog", 445, 575, S5Dialog],
  ["SSH", 565, 695, S6Ssh],
  ["Tokens Passwords SSH", 690, 785, S7Three],
  ["End card", 775, 900, S8End],
];

// Sound effects: [file, frame, volume].
const ticks = (from: number, to: number, every: number, vol = 0.35) =>
  Array.from({ length: Math.floor((to - from) / every) + 1 }, (_, i) => ["tick.wav", from + i * every, vol] as const);

const SFX: ReadonlyArray<readonly [string, number, number]> = [
  ["whoosh.wav", 132, 0.6], // token dissolves
  ["chime.wav", 196, 0.55], // logo
  ...ticks(320, 358, 3), // agent call typed
  ["whoosh.wav", 364, 0.35], // request travels
  ["pop.wav", 384, 0.6], // response
  ...ticks(482, 524, 3, 0.3), // secret typed into the dialog
  ["click.wav", 541, 0.7], // Save
  ["pop.wav", 547, 0.45],
  ...ticks(605, 637, 2, 0.28), // ssh command typed
  ["pop.wav", 641, 0.55], // output
  ["chime.wav", 781, 0.4], // end card
];

export const Launch: React.FC = () => {
  return (
    <AbsoluteFill style={{ background: C.bg }}>
      {SHOTS.map(([name, from, to, Shot]) => (
        <Sequence key={name} name={name} from={from} durationInFrames={to - from}>
          <Scene len={to - from} name={name} last={to === 900}>
            <Shot />
          </Scene>
        </Sequence>
      ))}
      <Audio src={staticFile("audio/score.wav")} volume={0.85} />
      {SFX.map(([file, at, vol], i) => (
        <Sequence key={i} name={`sfx ${file} @${at}`} from={at} layout="none">
          <Audio src={staticFile(`audio/${file}`)} volume={vol} />
        </Sequence>
      ))}
    </AbsoluteFill>
  );
};
