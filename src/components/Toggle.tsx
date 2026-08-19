type ToggleProps = {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  description?: string;
  disabled?: boolean;
};

export function Toggle({
  label,
  checked,
  onChange,
  description,
  disabled = false,
}: ToggleProps) {
  return (
    <label className="toggle" data-disabled={disabled || undefined}>
      <span className="toggle-text">
        <span className="toggle-label">{label}</span>
        {description ? (
          <span className="toggle-description">{description}</span>
        ) : null}
      </span>
      <input
        type="checkbox"
        className="toggle-input"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="toggle-track" aria-hidden="true">
        <span className="toggle-thumb" />
      </span>
    </label>
  );
}
