// Prototype of the v4 motion language: "one key's journey".
// GSAP drives everything; Remotion seeks the paused timeline to each frame.
import React, { useLayoutEffect, useRef, useState } from "react";
import { AbsoluteFill, Img, Sequence, continueRender, delayRender, staticFile, useCurrentFrame, useVideoConfig } from "remotion";
import { Audio } from "@remotion/media";
import { gsap } from "gsap";
import { Flip } from "gsap/Flip";
import { ScrambleTextPlugin } from "gsap/ScrambleTextPlugin";
import { SplitText } from "gsap/SplitText";
import { loadFont } from "@remotion/google-fonts/Inter";
import { loadFont as loadMono } from "@remotion/google-fonts/GeistMono";

gsap.registerPlugin(Flip, ScrambleTextPlugin, SplitText);

const sans = loadFont("normal", { weights: ["400", "500", "600", "700"], subsets: ["latin"] });
const mono = loadMono("normal", { weights: ["400", "500", "600"], subsets: ["latin"] });
const SANS = sans.fontFamily;
const MONO = mono.fontFamily;

const C = {
  ink: "#1d1d1f",
  ink2: "#6e6e73",
  accent: "#635bff",
  accentSoft: "#a5a0ff",
  termBg: "#141416",
  termBar: "#202023",
  tText: "#e8e8ea",
  tDim: "#8b8b92",
  tGreen: "#4ade80",
};

const TOKEN = "ghp_7Kq2vX9mZ4pL8wR3nT6yB1cF5hJ0dS";
const DOTS = "•".repeat(30);
const SCRAMBLE = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz0123456789#$%&*+";

// The real keymule dialog (src/consent/dialog.html at 3x: 1320×870).
const DW = 840;
const DH = Math.round((870 / 1320) * DW);
const DX = (1920 - DW) / 2;
const DY = 250;
const k = DW / 1320;
const FIELD = { x: 222 * k, y: 548 * k, w: (1254 - 222) * k, h: (640 - 548) * k };
const PILL = { w: 270, h: 56 };

const KeyIcon: React.FC<{ size: number; color: string }> = ({ size, color }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth={2.2} strokeLinecap="round" strokeLinejoin="round">
    <path d="M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z" />
    <circle cx="16.5" cy="7.5" r=".5" fill={color} />
  </svg>
);

const line: React.CSSProperties = { display: "flex", lineHeight: 1.5, whiteSpace: "pre-wrap" };
const gut = (g: string, color: string) => (
  <span style={{ color, width: "1.6em", flex: "none", display: "inline-block" }}>{g}</span>
);

/** Build the GSAP timeline once (after fonts load); seek it on every frame. */
function useGsapTimeline(build: (tl: gsap.core.Timeline) => void) {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const root = useRef<HTMLDivElement>(null);
  const tl = useRef<gsap.core.Timeline | null>(null);
  const frameRef = useRef(frame);
  frameRef.current = frame;
  const [handle] = useState(() => delayRender("building the GSAP timeline"));
  useLayoutEffect(() => {
    let ctx: gsap.Context | undefined;
    let alive = true;
    Promise.all([sans.waitUntilDone(), mono.waitUntilDone()]).then(() => {
      if (!alive) return;
      ctx = gsap.context(() => {
        const t = gsap.timeline({ paused: true });
        build(t);
        tl.current = t;
        t.seek(frameRef.current / fps, false);
      }, root);
      continueRender(handle);
    });
    return () => {
      alive = false;
      ctx?.revert();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useLayoutEffect(() => {
    tl.current?.seek(frame / fps, false);
  }, [frame, fps]);
  return root;
}

export const Proto: React.FC = () => {
  const root = useGsapTimeline((tl) => {
    const q = gsap.utils.selector(root);
    const reveal = (sel: string, at: number) => {
      const split = SplitText.create(q(sel), { type: "lines", mask: "lines" });
      tl.from(split.lines, { yPercent: 105, duration: 0.6, ease: "expo.out", stagger: 0.06 }, at);
    };
    const chip = q(".chip")[0];

    // Measure every Flip target first, in its final layout.
    const toField = Flip.fit(chip, q(".field-target")[0], { duration: 0.5, ease: "power3.inOut", scale: false }) as gsap.core.Tween;
    const toPill = Flip.fit(chip, q(".pill-target")[0], { duration: 0.42, ease: "power3.inOut", scale: false }) as gsap.core.Tween;
    const toSlot = Flip.fit(chip, q(".slot")[0], { duration: 0.65, ease: "power3.inOut", scale: false }) as gsap.core.Tween;

    // 0 – terminal: the agent asks keymule for a GitHub credential
    reveal(".l-tool .txt", 0.15);
    reveal(".l-res .txt", 0.45);
    gsap.set(q(".l-ok, .l-tool2, .l-res2"), { autoAlpha: 0 });

    // 0.9 – the real dialog rises; the terminal steps back
    tl.from(q(".dialog"), { y: 120, autoAlpha: 0, duration: 0.6, ease: "power3.out" }, 0.9);
    tl.to(q(".term"), { filter: "brightness(0.5)", duration: 0.5, ease: "power2.out" }, 0.9);

    // 1.6 – the copied token appears (⌘V) and drops into the field
    tl.from(chip, { scale: 0.6, autoAlpha: 0, duration: 0.35, ease: "back.out(2.2)" }, 1.6);
    tl.from(q(".kbd"), { scale: 0.7, autoAlpha: 0, duration: 0.3, ease: "back.out(2)" }, 1.6);
    tl.to(q(".kbd"), { autoAlpha: 0, duration: 0.2 }, 2.3);
    tl.add(toField, 2.1);
    tl.to(chip, { backgroundColor: "rgba(255,255,255,1)", color: C.ink, boxShadow: "0 0 0 0 rgba(0,0,0,0)", duration: 0.3 }, 2.45);

    // 2.75 – keymule takes it: the token scrambles into dots
    tl.to(q(".chip-token"), { duration: 0.85, scrambleText: { text: DOTS, chars: SCRAMBLE, speed: 0.9 } }, 2.75);

    // 4.1 – Save
    tl.set(q(".img-hover"), { autoAlpha: 1 }, 4.0);
    tl.to(q(".save-ring"), { autoAlpha: 1, scale: 1.15, duration: 0.25, ease: "power2.out" }, 4.1);
    tl.to(q(".save-ring"), { autoAlpha: 0, duration: 0.2 }, 4.35);

    // 4.35 – the field folds into a named pill and leaves the dialog
    tl.add(toPill, 4.35);
    tl.to(chip, { backgroundColor: C.accent, color: "#fff", borderRadius: 28, duration: 0.3 }, 4.35);
    tl.to(q(".chip-token"), { autoAlpha: 0, duration: 0.15 }, 4.35);
    tl.to(q(".chip-label"), { autoAlpha: 1, duration: 0.2 }, 4.5);
    tl.to(q(".dialog"), { autoAlpha: 0, scale: 0.96, duration: 0.35, ease: "power2.in" }, 4.55);
    tl.to(q(".term"), { filter: "brightness(1)", duration: 0.45 }, 4.6);

    // 4.85 – the pill lands in the terminal, where the agent can use it by name
    tl.set(q(".l-ok"), { autoAlpha: 1 }, 4.85);
    tl.add(toSlot, 4.85);
    tl.from(q(".ok-rest"), { autoAlpha: 0, x: -20, duration: 0.5, ease: "expo.out" }, 5.4);
    tl.fromTo(chip, { scale: 1.08 }, { scale: 1, duration: 0.4, ease: "back.out(3)" }, 5.5);

    // 5.9 – the agent uses it; the header it never sees scrambles to [redacted]
    tl.set(q(".l-tool2, .l-res2"), { autoAlpha: 1 }, 5.85);
    reveal(".l-tool2 .txt", 5.85);
    reveal(".l-res2 .txt", 6.1);
    tl.to(q(".secret"), { duration: 0.8, scrambleText: { text: "[redacted]", chars: SCRAMBLE, speed: 0.9 }, color: C.accentSoft }, 6.55);

    // 7.4 – caption
    reveal(".caption", 7.4);
  });

  return (
    <AbsoluteFill style={{ background: "#f4f4f7" }}>
      <div ref={root} style={{ position: "absolute", inset: 0 }}>
        <div
          style={{
            position: "absolute",
            width: 1300,
            height: 1300,
            left: -300,
            top: -560,
            borderRadius: "50%",
            background: "radial-gradient(circle, rgba(165,160,255,0.4), rgba(165,160,255,0) 65%)",
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
            background: "radial-gradient(circle, rgba(255,186,150,0.38), rgba(255,186,150,0) 65%)",
          }}
        />

        {/* terminal */}
        <div
          className="term"
          style={{
            position: "absolute",
            left: 190,
            top: 80,
            width: 1540,
            height: 660,
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
            <div style={{ flex: 1, textAlign: "center", fontFamily: SANS, fontSize: 21, color: C.tDim, marginRight: 62 }}>
              acme-web — agent
            </div>
          </div>
          <div style={{ padding: "30px 46px", fontFamily: MONO, fontSize: 33, display: "flex", flexDirection: "column", gap: 14 }}>
            <div style={{ ...line, background: "rgba(255,255,255,0.05)", borderRadius: 12, padding: "10px 16px", margin: "0 -18px 4px" }}>
              {gut(">", C.tDim)}
              <span style={{ color: C.tText }}>why is CI failing on acme/web?</span>
            </div>
            <div className="l-tool" style={line}>
              {gut("⏺", C.tGreen)}
              <span className="txt" style={{ color: C.tDim }}>
                <b style={{ color: "#fff", fontWeight: 600 }}>keymule</b> · add_credential(service: "github", name: "github-ci")
              </span>
            </div>
            <div className="l-res" style={{ ...line, marginTop: -6 }}>
              {gut("", C.tDim)}
              <span className="txt" style={{ color: "#b9b9c0" }}>⎿  A keymule dialog is open. Waiting for you…</span>
            </div>
            <div className="l-ok" style={{ ...line, alignItems: "center" }}>
              {gut("✓", C.tGreen)}
              <span style={{ color: "#c9f7d8" }}>Saved </span>
              <span className="slot" style={{ display: "inline-block", width: PILL.w, height: PILL.h, margin: "0 14px 0 6px" }} />
              <span className="ok-rest" style={{ color: "#c9f7d8" }}>
                the agent never saw the token
              </span>
            </div>
            <div className="l-tool2" style={line}>
              {gut("⏺", C.tGreen)}
              <span className="txt" style={{ color: C.tDim }}>
                <b style={{ color: "#fff", fontWeight: 600 }}>keymule</b> · http_request("github-ci", GET /repos/acme/web/actions/runs)
              </span>
            </div>
            <div className="l-res2" style={{ ...line, marginTop: -6 }}>
              {gut("", C.tDim)}
              <span className="txt" style={{ color: "#b9b9c0" }}>
                ⎿  200 OK · Authorization: Bearer <span className="secret" style={{ color: "#ff9a9a" }}>{TOKEN}</span>
              </span>
            </div>
          </div>
        </div>

        {/* the real keymule dialog */}
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
          <Img className="img-hover" src={staticFile("shots/dialog-hover.png")} style={{ position: "absolute", width: DW, height: DH, opacity: 0, visibility: "hidden" }} />
          <div
            className="save-ring"
            style={{
              position: "absolute",
              left: 1098 * k,
              top: 744 * k,
              width: 162 * k,
              height: 90 * k,
              borderRadius: 10,
              boxShadow: "0 0 0 6px rgba(99,91,255,0.35)",
              opacity: 0,
              visibility: "hidden",
            }}
          />
        </div>
        {/* Flip targets (invisible) */}
        <div className="field-target" style={{ position: "absolute", left: DX + FIELD.x, top: DY + FIELD.y, width: FIELD.w, height: FIELD.h }} />
        <div
          className="pill-target"
          style={{ position: "absolute", left: 960 - PILL.w / 2, top: DY + FIELD.y - 96, width: PILL.w, height: PILL.h }}
        />

        {/* ⌘V */}
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

        {/* the token itself: the one object that travels */}
        <div
          className="chip"
          style={{
            position: "absolute",
            left: 960 - 320,
            top: DY - 120,
            width: 640,
            height: 62,
            borderRadius: 10,
            background: "rgba(255,107,107,0.95)",
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
              opacity: 0,
              visibility: "hidden",
            }}
          >
            <KeyIcon size={24} color="#fff" />
            github-ci
          </span>
        </div>

        <div
          className="caption"
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            top: 800,
            textAlign: "center",
            fontFamily: SANS,
            fontSize: 64,
            fontWeight: 700,
            letterSpacing: "-0.03em",
            color: C.ink,
          }}
        >
          Your agent uses it. It never sees it.
        </div>
      </div>

      {(
        [
          [27, "pop"], [48, "paste"], [63, "whoosh", 0.6], [83, "type"], [95, "type", 0.7], [123, "click"],
          [146, "whoosh", 0.6], [162, "chime"], [197, "type"], [209, "type", 0.7],
        ] as [number, string, number?][]
      ).map(([at, n, v = 1], i) => (
        <Sequence key={i} from={at} durationInFrames={60}>
          <Audio src={staticFile(`audio/${n}.wav`)} volume={v} />
        </Sequence>
      ))}
      <Audio src={staticFile("audio/score.wav")} trimBefore={210} volume={0.45} />
    </AbsoluteFill>
  );
};
