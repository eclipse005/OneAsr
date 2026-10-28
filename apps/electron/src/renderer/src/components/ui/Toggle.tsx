/** Switch control — styles via global `.toggle` / `.toggle.on`. */
export function Toggle({
  on,
  onChange,
  'aria-label': ariaLabel
}: {
  on: boolean
  onChange: (v: boolean) => void
  'aria-label'?: string
}) {
  return (
    <button
      type="button"
      className={`toggle ${on ? 'on' : ''}`}
      role="switch"
      aria-checked={on}
      aria-label={ariaLabel}
      onClick={() => onChange(!on)}
    />
  )
}
