// The scene's local clock excludes paused/hidden time. Long gaps never replay
// animation missed while the app was suspended.
export function advanceStretchPlayback(elapsed: number, deltaMs: number, running: boolean) {
  if (!running || !Number.isFinite(deltaMs) || deltaMs < 0 || deltaMs > 250) return elapsed
  return elapsed + deltaMs
}
