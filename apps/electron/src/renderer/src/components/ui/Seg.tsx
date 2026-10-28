/** Connected segmented control (exclusive) — styles via `.seg` / `.seg-item`. */
export function Seg({
  options,
  value,
  onChange
}: {
  options: { id: string; label: string }[]
  value: string
  onChange: (id: string) => void
}) {
  return (
    <div className="seg" role="group">
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          className={`seg-item ${value === o.id ? 'active' : ''}`}
          aria-pressed={value === o.id}
          onClick={() => onChange(o.id)}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}
