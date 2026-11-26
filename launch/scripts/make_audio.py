"""Original score and sound effects for the keymule launch film (v4), made
from scratch with numpy (no samples). 30 s, 48 kHz stereo.

The idea: the music is made of keys. Percussion is built from keyboard
clicks and lock/latch sounds in a half-time groove; harmony is a warm FM
electric piano over a pumping pad (Db major, neo-soul voicings). The grid is
120 BPM (one beat = 15 frames at 30 fps) so cuts land on beats, but the feel
is half time, with room to breathe.

Sections (seconds), matching the timeline T in src/Launch.tsx:
  0.0 - 2.7   problem: a ticking clock of key clicks, a low uneasy drone
  2.7         new session: a latch "clunk"; the drone drops a semitone
  5.0         title: reverse swell into a wide FM chord bloom
  7.0 - 8.0   logo; a riser into the groove
  8.0 - 20.6  groove: half-time key-click kit, sub bass, FM piano chords
  20.6 - 21.0 tape stop ("days later")
  21.8 - 27.8 groove returns with a bell counter-melody
  27.8        end card: final bloom rings out

Run: python3 -I scripts/make_audio.py  ->  public/audio/*.wav
"""
import os
import wave

import numpy as np

SR = 48000
BPM = 120
BEAT = 60 / BPM  # 0.5 s
DUR = 30.0
OUT = os.path.join(os.path.dirname(__file__), "..", "public", "audio")
rng = np.random.default_rng(23)


def t_axis(sec):
    return np.arange(int(sec * SR)) / SR


def note_hz(n):
    return 440.0 * 2 ** ((n - 69) / 12)


def lp(x, cutoff):
    n = len(x)
    f = np.fft.rfftfreq(n, 1 / SR)
    return np.fft.irfft(np.fft.rfft(x) / np.sqrt(1 + (f / cutoff) ** 4), n)


def hp(x, cutoff):
    n = len(x)
    f = np.fft.rfftfreq(n, 1 / SR)
    return np.fft.irfft(np.fft.rfft(x) / np.sqrt(1 + (cutoff / np.maximum(f, 1e-6)) ** 4), n)


def bp(x, lo, hi):
    return hp(lp(x, hi), lo)


def reverb(x, seconds=2.6, mix=0.25, seed=1, bright=5000):
    """Stereo convolution reverb from decaying noise (mono in, stereo out)."""
    r = np.random.default_rng(seed)
    n = int(seconds * SR)
    env = np.exp(-np.arange(n) / (SR * seconds / 6.5))
    out = []
    for _ in range(2):
        ir = lp(r.standard_normal(n) * env, bright)
        ir /= np.sqrt(np.sum(ir**2))
        size = len(x) + n
        out.append(np.fft.irfft(np.fft.rfft(x, size) * np.fft.rfft(ir, size), size)[: len(x)])
    wet = np.stack(out, axis=1)
    return (1 - mix) * np.stack([x, x], axis=1) + mix * wet


def place(buf, sig, at, gain=1.0, pan=0.0):
    i = int(at * SR)
    if i >= len(buf) or i < 0:
        return
    if sig.ndim == 1:
        l, r = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
        sig = np.stack([sig * l * 1.414, sig * r * 1.414], axis=1)
    end = min(len(buf), i + len(sig))
    buf[i:end] += gain * sig[: end - i]


def env_adsr(n, a=0.01, d=0.1, s=0.7, r=0.3):
    t = np.arange(n) / SR
    total = n / SR
    e = np.ones(n) * s
    e[t < a] = t[t < a] / a
    m = (t >= a) & (t < a + d)
    e[m] = 1 - (1 - s) * (t[m] - a) / d
    m = t > total - r
    e[m] *= np.clip((total - t[m]) / r, 0, 1)
    return e


def tape(x, drive=1.3):
    """Soft saturation."""
    return np.tanh(drive * x) / np.tanh(drive)


# ---- instruments ----------------------------------------------------------------

def fm_piano(n, sec=1.8, vel=1.0):
    """FM electric piano: 1:1 body with a decaying index, plus a 14:1 tine."""
    t = t_axis(sec)
    f = note_hz(n)
    idx = 1.6 * vel * np.exp(-t * 3.2) + 0.25
    body = np.sin(2 * np.pi * f * t + idx * np.sin(2 * np.pi * f * t))
    tine = np.sin(2 * np.pi * f * t + 0.9 * np.exp(-t * 30) * np.sin(2 * np.pi * 14 * f * t)) * np.exp(-t * 18)
    x = (body + 0.35 * tine) * np.exp(-t * 1.4) * np.minimum(1, t / 0.003)
    # chorus: a slightly detuned, delayed copy
    d = int(0.012 * SR)
    det = np.sin(2 * np.pi * f * 1.003 * t + idx * np.sin(2 * np.pi * f * 1.003 * t)) * np.exp(-t * 1.4)
    x[d:] += 0.35 * det[:-d]
    return 0.12 * vel * x


def chord(notes, sec, vel=1.0, strum=0.012):
    out = np.zeros(int((sec + strum * len(notes)) * SR) + 1)
    for i, n in enumerate(sorted(notes)):
        s = fm_piano(n, sec, vel * (0.85 + 0.15 * (i == len(notes) - 1)))
        j = int(i * strum * SR)
        out[j:j + len(s)] += s
    return out


def pad(notes, sec, bright=1800):
    t = t_axis(sec)
    x = np.zeros_like(t)
    for n in notes:
        for det in (-0.07, 0.0, 0.06):
            f = note_hz(n + det)
            x += np.sin(2 * np.pi * f * t + rng.uniform(0, 6.28)) + 0.3 * np.sin(4 * np.pi * f * t)
    x = lp(x, bright) * env_adsr(len(t), a=0.6, d=0.3, s=0.9, r=min(1.0, sec * 0.4))
    return x / (len(notes) * 6)


def pump(n, at0, beat=BEAT, depth=0.6):
    """Sidechain-style volume pump on every beat, from absolute time at0."""
    t = at0 + np.arange(n) / SR
    ph = (t % beat) / beat
    return 1 - depth * np.exp(-ph * 9)


def key_click(vel=1.0, tone=2400):
    """A mechanical key: plastic tick plus a little thock."""
    t = t_axis(0.06)
    x = bp(rng.standard_normal(len(t)), 1500, 9000) * np.exp(-t * 260)
    x += 0.5 * np.sin(2 * np.pi * tone * t) * np.exp(-t * 300)
    x += 0.4 * np.sin(2 * np.pi * 180 * t) * np.exp(-t * 90)
    return 0.22 * vel * x


def latch(vel=1.0):
    """A lock latching: two metal clicks and a body thud."""
    out = np.zeros(int(0.35 * SR))
    for d, g, tone in [(0.0, 1.0, 3100), (0.028, 0.7, 2300)]:
        t = t_axis(0.08)
        c = bp(rng.standard_normal(len(t)), 2000, 10000) * np.exp(-t * 200)
        c += 0.6 * np.sin(2 * np.pi * tone * t) * np.exp(-t * 120)
        i = int(d * SR)
        out[i:i + len(c)] += g * c
    t = t_axis(0.35)
    out += 0.9 * np.sin(2 * np.pi * (70 + 60 * np.exp(-t * 30)) * t) * np.exp(-t * 14)
    return 0.4 * vel * out


def kick_soft():
    t = t_axis(0.5)
    f = 42 + 70 * np.exp(-t * 26)
    x = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t * 6.5)
    return 0.55 * tape(x, 1.5)


def rim():
    t = t_axis(0.12)
    x = np.sin(2 * np.pi * 1700 * t) * np.exp(-t * 90) + 0.6 * bp(rng.standard_normal(len(t)), 1500, 6000) * np.exp(-t * 70)
    return 0.16 * x


def snap():
    """A dry finger-snap / clap hybrid for the backbeat."""
    t = t_axis(0.22)
    n = bp(rng.standard_normal(len(t)), 1000, 8000)
    return 0.2 * n * (np.exp(-t * 40) + 0.4 * np.exp(-t * 12))


def sub(n, sec):
    t = t_axis(sec)
    x = np.sin(2 * np.pi * note_hz(n) * t)
    return 0.2 * tape(x * env_adsr(len(t), a=0.01, d=0.1, s=0.85, r=0.08), 1.2)


def bell(n, sec=1.6, vel=1.0):
    t = t_axis(sec)
    f = note_hz(n)
    x = np.sin(2 * np.pi * f * t + 2.2 * np.exp(-t * 4) * np.sin(2 * np.pi * 3.5 * f * t))
    return 0.07 * vel * x * np.exp(-t * 2.6) * np.minimum(1, t / 0.002)


def drone(n, sec, wobble=0.0):
    t = t_axis(sec)
    f = note_hz(n)
    x = np.sin(2 * np.pi * f * t) + 0.5 * np.sin(2 * np.pi * (f * 1.5 + wobble) * t) + 0.3 * np.sin(2 * np.pi * 2.01 * f * t)
    return 0.12 * lp(x, 700) * env_adsr(len(t), a=0.4, d=0.2, s=0.9, r=0.25)


def reverse_swell(sec=1.4):
    t = t_axis(sec)
    x = bp(rng.standard_normal(len(t)), 800, 9000) * (t / sec) ** 3
    tone = sum(np.sin(2 * np.pi * note_hz(m) * t) for m in (61, 68, 72)) * (t / sec) ** 4
    return 0.08 * x + 0.04 * tone


def glitch(sec=0.8):
    """Granular stutters for the ScrambleText moments."""
    out = np.zeros(int(sec * SR))
    at = 0.0
    while at < sec - 0.04:
        g = rng.uniform(0.012, 0.035)
        t = t_axis(g)
        f = rng.choice([1200, 1800, 2400, 3600, 4800])
        grain = np.sign(np.sin(2 * np.pi * f * t)) * np.hanning(len(t))
        i = int(at * SR)
        out[i:i + len(grain)] += grain * rng.uniform(0.3, 1.0)
        at += g + rng.uniform(0.0, 0.03)
    return 0.05 * lp(out, 7000)


def tape_stop(seg, stop_sec):
    """Slow a stereo segment to a halt over stop_sec (pitch falls with speed)."""
    n_out = int(stop_sec * SR)
    t = np.arange(n_out) / n_out
    pos = np.clip(np.cumsum((1 - t) ** 1.6), 0, len(seg) - 2)
    i = pos.astype(int)
    fr = pos - i
    out = seg[i] * (1 - fr)[:, None] + seg[i + 1] * fr[:, None]
    return out * np.linspace(1, 0.3, n_out)[:, None]


# ---- score ------------------------------------------------------------------------

def score():
    buf = np.zeros((int(DUR * SR), 2))
    # Db major, neo-soul voicings (MIDI)
    Dbmaj9 = [49, 56, 60, 63, 65]      # Db Ab C Eb F
    Bbm9 = [46, 53, 56, 60, 61]        # Bb F Ab C Db
    Gbmaj7s11 = [42, 53, 58, 60, 65]   # Gb F Bb C F
    Ab69 = [44, 51, 53, 58, 60]        # Ab Eb F Bb C
    prog = [(Dbmaj9, 37), (Bbm9, 34), (Gbmaj7s11, 30), (Ab69, 32)]

    # 0-2.7: a clock of key clicks (8ths), an uneasy drone on D (a semitone above Db).
    for i in range(int(2.7 / (BEAT / 2))):
        place(buf, key_click(0.7 + 0.3 * (i % 2 == 0), tone=2600 if i % 2 else 2100), i * BEAT / 2, 1.0, pan=0.3 if i % 2 else -0.3)
    place(buf, drone(38, 2.9, wobble=1.3), 0.0, 0.9)
    # 2.7: a latch clunk; the drone drops to Db, clicks double up.
    place(buf, latch(1.0), 2.7, 1.0)
    place(buf, drone(37, 2.4, wobble=0.6), 2.7, 0.9)
    for i in range(int(2.3 / (BEAT / 4))):
        place(buf, key_click(0.45 + 0.4 * (i % 4 == 0)), 2.7 + i * BEAT / 4, 0.9, pan=rng.uniform(-0.5, 0.5))
    place(buf, reverse_swell(1.5), 3.5)

    # 5.0: the title. A wide FM bloom over a pad.
    place(buf, chord(Dbmaj9 + [68, 72], 3.0, vel=1.0, strum=0.02), 5.0, 1.0)
    place(buf, pad(Dbmaj9, 3.0, bright=2400), 5.0, 0.8)
    place(buf, sub(37, 2.8), 5.0)
    place(buf, latch(0.8), 7.1, 0.6)  # the logo locks into place
    place(buf, reverse_swell(1.0), 7.0)

    def groove(t0, t1, full):
        bar = 0
        t = t0
        while t < t1 - 1e-6:
            ch, root = prog[(bar // 2) % 4]
            two = min(8 * BEAT, t1 - t)
            if bar % 2 == 0:
                p = pad(ch, two + 0.2, bright=2000)
                place(buf, p * pump(len(p), t, depth=0.55), t, 0.7)
                # FM piano on 1, and a lighter stab on the "and" of 3 in the next bar
                place(buf, chord(ch, 1.6, vel=0.8), t, 0.9)
                if t + 6.5 * BEAT < t1:
                    place(buf, chord(ch[1:], 0.9, vel=0.55), t + 6.5 * BEAT, 0.8)
                place(buf, sub(root, 3 * BEAT), t)
                if t + 3.5 * BEAT < t1:
                    place(buf, sub(root + 7, BEAT * 0.9), t + 3.5 * BEAT)
                if t + 4 * BEAT < t1:
                    place(buf, sub(root, min(3.5 * BEAT, t1 - t - 4 * BEAT)), t + 4 * BEAT)
            # half-time kit made of keys: kick on 1, snap + latch on 3, swung 16th clicks
            for b in range(4):
                bt = t + b * BEAT
                if bt >= t1 - 1e-6:
                    break
                if b == 0:
                    place(buf, kick_soft(), bt)
                if b == 2:
                    place(buf, snap(), bt, 1.0, pan=0.1)
                    place(buf, latch(0.35), bt, 0.5)
                if b == 3 and bar % 2 == 1 and bt + BEAT / 2 < t1:
                    place(buf, kick_soft(), bt + BEAT / 2, 0.7)
                for s in range(4):
                    swing = 0.03 if s % 2 else 0.0
                    v = [0.9, 0.35, 0.6, 0.35][s] * (1.0 if full else 0.8)
                    place(buf, key_click(v, tone=2200 + 300 * s), bt + s * BEAT / 4 + swing, 0.8, pan=[-0.4, 0.4, -0.2, 0.2][s])
                if b == 1:
                    place(buf, rim(), bt + 3 * BEAT / 4, 0.8, pan=-0.3)
            if full and bar % 2 == 1:
                mel = [72, 75, 77, 80, 77, 75, 72, 70]
                for j in range(2):
                    place(buf, bell(mel[(bar + j) % len(mel)], 1.4), t + (1.5 + 2 * j) * BEAT, 1.0, pan=0.35)
            t += 4 * BEAT
            bar += 1

    groove(8.0, 21.0, full=False)
    # Glitch grains under the scramble moments (flow 11.1 s and 15.0 s, guard 26.6 s).
    for at, sec in [(11.1, 0.85), (15.0, 0.8), (26.6, 0.5)]:
        place(buf, glitch(sec), at, 1.0, pan=0.2)

    # Tape stop into "days later": 20.6-21.0 slows to a halt.
    a = int(20.6 * SR)
    seg = buf[a:a + int(0.9 * SR)].copy()
    buf[a:] *= 0
    stopped = tape_stop(seg, 0.4)
    buf[a:a + len(stopped)] += stopped
    # the date counter's reel ticks, a breath, then the groove returns
    for i in range(3):
        place(buf, key_click(0.8, tone=3000), 21.23 + i * 0.2, 1.0)
    place(buf, reverse_swell(0.6), 21.2, 0.8)

    groove(21.8, 27.8, full=True)

    # 27.8: the end bloom.
    place(buf, latch(1.0), 27.8, 0.8)
    place(buf, chord(Dbmaj9 + [68, 72, 77], 2.2, vel=1.0, strum=0.025), 27.8, 1.1)
    place(buf, pad(Dbmaj9 + [72], 2.2, bright=3000), 27.8, 0.9)
    place(buf, sub(37, 2.0), 27.8)
    place(buf, bell(80, 1.9), 28.1, 1.0, pan=0.3)
    place(buf, bell(84, 1.6), 28.35, 0.8, pan=-0.3)

    mono = buf.mean(axis=1)
    out = 0.7 * reverb(mono, seconds=2.6, mix=0.22) + 0.3 * buf
    out = tape(out * 1.6, 1.1) / 1.6
    n = int(0.35 * SR)
    out[-n:] *= np.linspace(1, 0, n)[:, None]
    return out


# ---- sound effects ------------------------------------------------------------------

def sfx_type():
    x = np.zeros(int(0.5 * SR))
    at = 0.0
    while at < 0.42:
        k = key_click(rng.uniform(0.6, 1.0), tone=rng.uniform(1900, 2700))
        i = int(at * SR)
        x[i:i + len(k)] += k
        at += rng.uniform(0.045, 0.075)
    return 1.6 * x


def sfx_enter():
    k = 1.8 * key_click(1.0, tone=1500)
    out = np.zeros(int(0.12 * SR))
    out[: len(k)] += k
    return out


def sfx_paste():
    t = t_axis(0.22)
    f = 380 + 600 * np.exp(-t * 30)
    return 0.32 * np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t * 18)


def sfx_warn():
    x = np.zeros(int(0.42 * SR))
    for at in (0.0, 0.2):
        t = t_axis(0.16)
        b = np.sign(np.sin(2 * np.pi * 311 * t)) * 0.5 + np.sin(2 * np.pi * 622 * t) * 0.3
        b = lp(b, 2500) * env_adsr(len(t), a=0.005, d=0.05, s=0.8, r=0.05)
        i = int(at * SR)
        x[i:i + len(b)] += b
    return 0.24 * x


def sfx_whoosh(sec=0.6):
    t = t_axis(sec)
    n = rng.standard_normal(len(t))
    return 0.5 * (lp(n, 4500) - lp(n, 900)) * np.sin(np.pi * np.clip(t / sec, 0, 1)) ** 2


def sfx_pop():
    t = t_axis(0.18)
    f = 520 + 420 * np.exp(-t * 40)
    return 0.55 * np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t * 26)


def sfx_chime():
    """Two FM bell notes in the key (Ab, Db)."""
    a = bell(80, 1.4, 1.0)
    b = bell(85, 1.3, 0.8)
    out = np.zeros(int(1.5 * SR))
    out[: len(a)] += a
    j = int(0.07 * SR)
    out[j:j + len(b)] += b
    return 2.6 * out


def sfx_click():
    t = t_axis(0.06)
    x = np.sin(2 * np.pi * 1600 * t) * np.exp(-t * 120)
    x += 0.5 * hp(rng.standard_normal(len(t)), 4000) * np.exp(-t * 300)
    return 0.55 * x


def sfx_tick():
    return 2.0 * key_click(1.0, tone=2800)


def write(name, x):
    x = np.asarray(x, dtype=np.float64)
    if x.ndim == 1:
        x = np.stack([x, x], axis=1)
    peak = np.max(np.abs(x))
    if peak > 0.98:
        x = x / peak * 0.98
    data = (x * 32767).astype("<i2")
    with wave.open(os.path.join(OUT, name), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(data.tobytes())
    print(f"{name}: {len(x) / SR:.2f}s peak {peak:.2f}")


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    m = score()
    write("score.wav", m / np.max(np.abs(m)) * 0.8)
    for name, fn in [("type", sfx_type), ("enter", sfx_enter), ("paste", sfx_paste),
                     ("warn", sfx_warn), ("whoosh", sfx_whoosh), ("pop", sfx_pop),
                     ("chime", sfx_chime), ("click", sfx_click), ("tick", sfx_tick)]:
        write(f"{name}.wav", fn())
