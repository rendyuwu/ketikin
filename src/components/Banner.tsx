import type { ReactNode } from "react";

type BannerProps = {
  tone: "info" | "warn" | "error";
  children: ReactNode;
  actions?: ReactNode;
  onDismiss?: () => void;
};

export function Banner({ tone, children, actions, onDismiss }: BannerProps) {
  return (
    <div className={`banner banner--${tone}`} role={tone === "info" ? "status" : "alert"}>
      <div className="banner-body">{children}</div>
      {actions || onDismiss ? (
        <div className="banner-actions">
          {actions}
          {onDismiss ? (
            <button
              type="button"
              className="icon-button"
              aria-label="Dismiss"
              onClick={onDismiss}
            >
              <CloseIcon />
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

export function CloseIcon() {
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" focusable="false">
      <path
        d="M3.5 3.5l9 9m0-9l-9 9"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        fill="none"
      />
    </svg>
  );
}
