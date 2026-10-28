import { describe, expect, it } from 'vitest'
import { tourEscClosesSettings } from './tourEsc'

describe('tourEscClosesSettings', () => {
  it('设置步且抽屉开着时 Esc 只关设置，不退出引导', () => {
    expect(tourEscClosesSettings('settings-models', true)).toBe(true)
    expect(tourEscClosesSettings('settings-hotwords', true)).toBe(true)
    expect(tourEscClosesSettings('settings-output', true)).toBe(true)
  })

  it('非设置步或设置已关时 Esc 退出引导', () => {
    expect(tourEscClosesSettings('add-or-drop', true)).toBe(false)
    expect(tourEscClosesSettings('task-row', true)).toBe(false)
    expect(tourEscClosesSettings('settings-models', false)).toBe(false)
    expect(tourEscClosesSettings(null, true)).toBe(false)
    expect(tourEscClosesSettings('done', false)).toBe(false)
  })
})
