import { useEffect, useId, useRef, useState } from "react";
import { Icon, type IconName } from "./Icon";

export interface SelectMenuOption<Value extends string> {
  value: Value;
  label: string;
  description?: string;
}

interface SelectMenuProps<Value extends string> {
  value: Value;
  options: Array<SelectMenuOption<Value>>;
  label: string;
  icon: IconName;
  onChange: (value: Value) => void;
  align?: "start" | "end";
  className?: string;
}

export function SelectMenu<Value extends string>({
  value,
  options,
  label,
  icon,
  onChange,
  align = "start",
  className = "",
}: SelectMenuProps<Value>) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const menuId = useId();
  const selected = options.find((option) => option.value === value) ?? options[0];

  useEffect(() => {
    if (!open) return;

    const closeOnOutsideClick = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        rootRef.current?.querySelector<HTMLButtonElement>(".select-menu-trigger")?.focus();
      }
    };

    document.addEventListener("pointerdown", closeOnOutsideClick);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsideClick);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  return (
    <div ref={rootRef} className={`select-menu ${className} ${open ? "is-open" : ""}`.trim()}>
      <button
        type="button"
        className="select-menu-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={menuId}
        onClick={() => setOpen((current) => !current)}
      >
        <span className="select-menu-trigger-icon"><Icon name={icon} size={14} /></span>
        <span>{selected?.label ?? label}</span>
        <Icon className="select-menu-chevron" name="chevron" size={12} />
      </button>

      {open && (
        <div id={menuId} className={`select-menu-popover is-${align}`} role="listbox" aria-label={label}>
          <div className="select-menu-heading">
            <span>{label}</span>
            <small>{options.length} 个选项</small>
          </div>
          <div className="select-menu-options">
            {options.map((option) => {
              const active = option.value === value;
              return (
                <button
                  type="button"
                  key={option.value}
                  className={`select-menu-option ${active ? "is-selected" : ""}`}
                  role="option"
                  aria-selected={active}
                  onClick={() => {
                    onChange(option.value);
                    setOpen(false);
                  }}
                >
                  <span className="select-menu-option-copy">
                    <strong>{option.label}</strong>
                    {option.description && <small>{option.description}</small>}
                  </span>
                  <span className="select-menu-check">{active && <Icon name="check" size={14} />}</span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
