import type { ReactNode } from "react";

import { CloseIcon } from "./Icons";

type BannerProps = {
  /**
   * Two tones, because a brass accent leaves no room for a third: the old info
   * tint was blue and the old warn tint was amber, which is brass's own family.
   * `notice` covers everything that is not an error — urgency comes from the
   * copy and from which action the banner offers, not from a background tint.
   */
  tone: "notice" | "error";
  children: ReactNode;
  actions?: ReactNode;
  onDismiss?: () => void;
};

export function Banner({ tone, children, actions, onDismiss }: BannerProps) {
  return (
    <div
      className={`banner banner--${tone}`}
      role={tone === "error" ? "alert" : "status"}
    >
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
              <CloseIcon size={12} />
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
