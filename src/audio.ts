let ctx: AudioContext | null = null;

/** Must be called from a user gesture (the Start tap) to unlock audio. */
export function initAudio() {
  ctx ??= new AudioContext();
  if (ctx.state === "suspended") {
    void ctx.resume();
  }
}

function beep(freq: number, durMs: number, delayS = 0, gainValue = 0.4) {
  if (!ctx) return;
  const t0 = ctx.currentTime + delayS;
  const osc = ctx.createOscillator();
  const gain = ctx.createGain();
  osc.type = "sine";
  osc.frequency.value = freq;
  gain.gain.setValueAtTime(gainValue, t0);
  gain.gain.exponentialRampToValueAtTime(0.001, t0 + durMs / 1000);
  osc.connect(gain).connect(ctx.destination);
  osc.start(t0);
  osc.stop(t0 + durMs / 1000 + 0.02);
}

export const sounds = {
  prealert: () => beep(880, 120),
  workStart: () => {
    beep(1318, 160);
    beep(1318, 220, 0.2);
  },
  restStart: () => beep(659, 350),
  finished: () => {
    beep(784, 150);
    beep(988, 150, 0.18);
    beep(1318, 350, 0.36);
  },
};

export function vibrate(pattern: number | number[]) {
  navigator.vibrate?.(pattern);
}
