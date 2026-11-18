// Dragging something from the picker onto a key.

import type { Action, Basic } from './protocol'

/** What is being dragged: one plain action, or a whole key (see-through). */
export type Dragged = { basic: Basic } | { action: Action }

const TYPE = 'application/x-jmk-key'

export function startDrag(event: DragEvent, dragged: Dragged) {
  event.dataTransfer?.setData(TYPE, JSON.stringify(dragged))
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'copy'
}

/** While something hovers its content is hidden; only its kind can be seen. */
export const carriesKey = (event: DragEvent) => !!event.dataTransfer?.types.includes(TYPE)

export function dropped(event: DragEvent): Dragged | undefined {
  const text = event.dataTransfer?.getData(TYPE)
  return text ? (JSON.parse(text) as Dragged) : undefined
}

/** What a key becomes when `dragged` lands on it: on a hold-tap only the tap half is replaced. */
export function landOn(before: Action | undefined, dragged: Dragged): Action {
  if ('action' in dragged) return dragged.action
  if (before && typeof before === 'object' && 'HoldTap' in before) return { HoldTap: { ...before.HoldTap, tap: dragged.basic } }
  return { Basic: dragged.basic }
}
