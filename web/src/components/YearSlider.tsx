interface Props {
  min: number;
  max: number;
  value: number;
  onChange: (year: number) => void;
  disabled?: boolean;
}

export default function YearSlider({ min, max, value, onChange, disabled }: Props) {
  const clamp = (y: number) => Math.min(max, Math.max(min, y));
  const decades: number[] = [];
  for (let y = Math.ceil(min / 10) * 10; y <= max; y += 10) decades.push(y);
  return (
    <div className="year-slider">
      <div className="year-display">
        <button type="button" className="year-step" onClick={() => onChange(clamp(value - 1))} disabled={disabled} aria-label="One year earlier">
          −
        </button>
        <output className="year-value" data-testid="year-value">
          {value}
        </output>
        <button type="button" className="year-step" onClick={() => onChange(clamp(value + 1))} disabled={disabled} aria-label="One year later">
          +
        </button>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={1}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
        aria-label="Year"
        data-testid="year-slider"
      />
      <div className="year-scale" aria-hidden>
        <span>{min}</span>
        {decades.length <= 8 &&
          decades
            .filter((d) => d - min > (max - min) * 0.08 && max - d > (max - min) * 0.08)
            .map((d) => (
              <span key={d} style={{ left: `${((d - min) / (max - min)) * 100}%` }} className="tick">
                {d}
              </span>
            ))}
        <span>{max}</span>
      </div>
    </div>
  );
}
