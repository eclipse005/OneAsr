import type { TourStepId } from './types'

/** Esc on settings-* with drawer open: close drawer (tour keeps going). Else exit tour. */
export function tourEscClosesSettings(
  stepId: TourStepId | null,
  settingsOpen: boolean
): boolean {
  return !!stepId?.startsWith('settings-') && settingsOpen
}
