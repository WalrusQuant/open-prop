/** How long a typed number sits before it asks Rust again. */
export const INPUT_DELAY_MS = 200;

/**
 * Runs `run` after `delay` ms, or right away when `delay` is 0. Returns the cancel, so an
 * `$effect` can hand it back as its cleanup and a newer keystroke drops the older request.
 */
export function later(run: () => void, delay: number): () => void {
  if (delay <= 0) {
    run();
    return () => {};
  }
  const timer = setTimeout(run, delay);
  return () => clearTimeout(timer);
}
