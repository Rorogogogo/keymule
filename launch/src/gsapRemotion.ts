// GSAP inside Remotion: build a paused timeline once (after fonts load),
// then seek it to the current frame on every render. Deterministic output.
import { useLayoutEffect, useRef, useState } from "react";
import { continueRender, delayRender, useCurrentFrame, useVideoConfig } from "remotion";
import { gsap } from "gsap";
import { Flip } from "gsap/Flip";
import { ScrambleTextPlugin } from "gsap/ScrambleTextPlugin";
import { SplitText } from "gsap/SplitText";
import { TextPlugin } from "gsap/TextPlugin";
import { loadFont } from "@remotion/google-fonts/Inter";
import { loadFont as loadMono } from "@remotion/google-fonts/GeistMono";

gsap.registerPlugin(Flip, ScrambleTextPlugin, SplitText, TextPlugin);

const sans = loadFont("normal", { weights: ["400", "500", "600", "700", "800"], subsets: ["latin"] });
const mono = loadMono("normal", { weights: ["400", "500", "600"], subsets: ["latin"] });
// Chinese falls back to the system's PingFang SC (rendered on macOS).
const CJK = '"PingFang SC", "Hiragino Sans GB", sans-serif';
export const SANS = `${sans.fontFamily}, ${CJK}`;
export const MONO = `${mono.fontFamily}, ${CJK}`;

export { gsap, Flip, SplitText };

export type Q = (sel: string) => Element[];

export function useGsap(build: (tl: gsap.core.Timeline, q: Q, root: HTMLDivElement) => void) {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const root = useRef<HTMLDivElement>(null);
  const tl = useRef<gsap.core.Timeline | null>(null);
  const frameRef = useRef(frame);
  frameRef.current = frame;
  const [handle] = useState(() => delayRender("building a GSAP timeline"));
  useLayoutEffect(() => {
    let ctx: gsap.Context | undefined;
    let alive = true;
    // Measuring (SplitText lines, Flip) needs real layout: wait for the fonts
    // and for the scene to have a size (it can mount before it's laid out).
    const laidOut = () =>
      new Promise<void>((resolve) => {
        const check = () => {
          if (!alive) return;
          if (root.current && root.current.getBoundingClientRect().width > 0) resolve();
          else setTimeout(check, 10);
        };
        check();
      });
    Promise.all([sans.waitUntilDone(), mono.waitUntilDone()]).then(laidOut).then(() => {
      if (!alive || !root.current) return;
      const el = root.current;
      ctx = gsap.context(() => {
        const t = gsap.timeline({ paused: true });
        build(t, gsap.utils.selector(el) as Q, el);
        tl.current = t;
        t.seek(frameRef.current / fps, false);
      }, el);
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

/** Lines rise out of a mask, one after another. */
export function reveal(tl: gsap.core.Timeline, targets: Element[], at: number, opts: { type?: "lines" | "words" | "chars"; stagger?: number; duration?: number } = {}) {
  const type = opts.type ?? "lines";
  const split = SplitText.create(targets, { type, mask: type });
  const parts = type === "lines" ? split.lines : type === "words" ? split.words : split.chars;
  tl.from(parts, { yPercent: 110, duration: opts.duration ?? 0.6, ease: "expo.out", stagger: opts.stagger ?? 0.06 }, at);
  return split;
}

/** Type text into an element that starts empty. */
export function typeIn(tl: gsap.core.Timeline, el: Element, text: string, at: number, cps = 40) {
  gsap.set(el, { text: "" });
  tl.to(el, { text: { value: text }, duration: text.length / cps, ease: "none" }, at);
}
