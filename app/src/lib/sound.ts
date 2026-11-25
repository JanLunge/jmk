// A short, quiet tone that says "that key press arrived", for the moments
// when the eyes are on the keyboard and not on the screen.

import { ref, watch } from 'vue'

const MUTE_KEY = 'jmk.sound'

export const muted = ref(localStorage.getItem(MUTE_KEY) === 'off')
watch(muted, (off) => localStorage.setItem(MUTE_KEY, off ? 'off' : 'on'))

let context: AudioContext | undefined

/**
 * `new` for a key seen for the first time, `known` for one pressed again:
 * a lower, duller tone, so a repeated key is heard as one.
 */
export function acknowledge(kind: 'new' | 'known') {
  if (muted.value || typeof AudioContext === 'undefined') return
  try {
    context ??= new AudioContext()
    void context.resume()
    const at = context.currentTime
    const [pitch, level, length] = kind === 'new' ? [1175, 0.09, 0.09] : [587, 0.05, 0.07]

    const tone = context.createOscillator()
    tone.type = 'sine'
    tone.frequency.setValueAtTime(pitch, at)
    // A slight drop in pitch makes it a soft "tock" instead of a test tone.
    tone.frequency.exponentialRampToValueAtTime(pitch * 0.94, at + length)

    // A quick swell and a fade to nothing, so there is no click at either end.
    const volume = context.createGain()
    volume.gain.setValueAtTime(0.0001, at)
    volume.gain.exponentialRampToValueAtTime(level, at + 0.006)
    volume.gain.exponentialRampToValueAtTime(0.0001, at + length)

    tone.connect(volume).connect(context.destination)
    tone.start(at)
    tone.stop(at + length + 0.02)
  } catch {
    // No sound output: the count on screen still says it.
  }
}
