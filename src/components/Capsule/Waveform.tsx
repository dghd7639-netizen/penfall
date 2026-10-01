import { useEffect, useRef } from 'react'
import { useReducedMotion } from 'framer-motion'
import { useAppStore } from '../../stores/appStore'

const WIDTH = 64
const HEIGHT = 20
const POINTS = 24
const MID = HEIGHT / 2
const MAX_SWING = 8

/** Zig-zags around the centre line; each point swings further the louder that moment was. */
function tracePath(values: number[]): string {
  const step = WIDTH / (values.length - 1)
  return values
    .map((value, i) => {
      const y = MID + (i % 2 === 0 ? -value : value) * MAX_SWING
      return `${i === 0 ? 'M' : 'L'}${(i * step).toFixed(2)} ${y.toFixed(2)}`
    })
    .join(' ')
}

/**
 * Live input level drawn as a scrolling line: the microphone volume is sampled over time and
 * the line swings further from the centre the louder you speak.
 */
export function Waveform() {
  const pathRef = useRef<SVGPathElement | null>(null)
  const history = useRef<number[]>(Array.from({ length: POINTS }, () => 0))
  const reduced = useReducedMotion()

  useEffect(() => {
    const path = pathRef.current
    if (!path) return
    if (reduced) {
      path.setAttribute('d', `M0 ${MID} L${WIDTH} ${MID}`)
      return
    }

    let frame = 0
    let last = 0
    const animate = (now: number) => {
      // About 24 samples per second keeps the line calm instead of jittery.
      if (now - last >= 42) {
        last = now
        const volume = useAppStore.getState().audioVolume
        history.current.push(Math.max(0, Math.min(1, volume)))
        history.current.shift()
        path.setAttribute('d', tracePath(history.current))
      }
      frame = requestAnimationFrame(animate)
    }
    frame = requestAnimationFrame(animate)
    return () => cancelAnimationFrame(frame)
  }, [reduced])

  return (
    <svg width={WIDTH} height={HEIGHT} viewBox={`0 0 ${WIDTH} ${HEIGHT}`} aria-hidden="true">
      <path
        ref={pathRef}
        d={`M0 ${MID} L${WIDTH} ${MID}`}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
        className="text-white/85"
      />
    </svg>
  )
}
