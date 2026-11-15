/**
 * Brand chrome, mobile rail controls, workflow reminder and transport-gap footer.
 * No external links, CDN calls or deployment claims are present.
 */
import type { JSX } from "react";
import markUrl from "../assets/sqlolla-mark.png";
import type { AnalysisTransport } from "../analysis/types";

/** Header props for desktop and narrow-viewport rail controls. */
export interface HeaderProps {
  readonly transport: AnalysisTransport;
  readonly hasEvidence: boolean;
  readonly onOpenLibrary: () => void;
  readonly onOpenExplain: () => void;
  readonly onOpenSettings: () => void;
  readonly onOpenDetails: () => void;
}

/** Render the local brand mark without external image or font requests. */
function Brand(): JSX.Element {
  return (
    <div className="brand">
      <span className="brand__logo">
        <img src={markUrl} alt="SQLolla pixel-art logo" width="46" height="54" />
      </span>
      <span className="brand__text">
        <span className="brand__name">SQLolla</span>
        <span className="brand__sub">Evidence review · local demo</span>
      </span>
    </div>
  );
}

interface HeaderButtonProps {
  readonly label: string;
  readonly variant?: "library" | "explain" | "";
  readonly disabled?: boolean;
  readonly onClick: () => void;
}

/** Render one labelled header button. */
function HeaderButton({ label, variant = "", disabled = false, onClick }: HeaderButtonProps): JSX.Element {
  return (
    <button type="button" className={`header-button ${variant ? `header-button--${variant}-drawer` : ""}`} onClick={onClick} disabled={disabled}>
      {label}
    </button>
  );
}

/** Render the transport provenance pill. */
function TransportPill({ transport }: { readonly transport: HeaderProps["transport"] }): JSX.Element {
  return (
    <span className="transport-pill">
      <span className="transport-dot" aria-hidden="true" />
      {transport.kind === "fixture" ? "FIXTURE ARCHIVE" : "HOST BRIDGE"}
    </span>
  );
}

/** Render the named header actions. */
function HeaderActions({ transport, hasEvidence, onOpenLibrary, onOpenExplain, onOpenSettings, onOpenDetails }: HeaderProps): JSX.Element {
  return (
    <nav className="header-actions" aria-label="Workspace actions">
      <TransportPill transport={transport} />
      <HeaderButton label="Cases & draft" variant="library" onClick={onOpenLibrary} />
      <HeaderButton label="Inspect triple" disabled={!hasEvidence} onClick={onOpenDetails} />
      <HeaderButton label="Explain" variant="explain" onClick={onOpenExplain} />
      <button type="button" className="icon-button" onClick={onOpenSettings} aria-label="Open AI provider settings" title="AI provider settings">
        ⚙
      </button>
    </nav>
  );
}

/** Render the retro-futurist masthead with a locally bundled logo asset. */
export function Header(props: HeaderProps): JSX.Element {
  return (
    <header className="site-header">
      <div className="site-header__scanline" aria-hidden="true" />
      <a className="skip-link" href="#review-workspace">
        Skip to review workspace
      </a>
      <Brand />
      <div className="header-center" aria-hidden="true">
        <span>BUILD</span>
        <i>→</i>
        <span>CHECK</span>
        <i>→</i>
        <span>REVIEW</span>
        <i>→</i>
        <span>REFINE</span>
      </div>
      <HeaderActions {...props} />
    </header>
  );
}

interface JourneyBarProps {
  readonly draftActive: boolean;
  readonly hasEvidence: boolean;
}

/** Keep the Build → Check → Review → Refine task visible without dashboard metrics. */
export function JourneyBar({ draftActive, hasEvidence }: JourneyBarProps): JSX.Element {
  const steps = [
    { id: "intent", label: "Describe intent", state: "done" },
    { id: "build", label: "Build a query", state: draftActive ? "current" : "idle" },
    { id: "check", label: "Check bounded cases", state: hasEvidence ? "stored" : "blocked" },
    { id: "review", label: "Review & refine", state: "current" },
  ];
  return (
    <div className="journey" aria-label="Review workflow">
      {steps.map((step, index) => (
        <span className={`journey__step journey__step--${step.state}`} key={step.id}>
          <span className="journey__index">0{index + 1}</span>
          {step.label}
        </span>
      ))}
      <span className="journey__note">Explicit finite context · database unchanged</span>
    </div>
  );
}

/** Footer states the actual integration boundary and local data policy. */
export function Footer({ transport }: { readonly transport: AnalysisTransport }): JSX.Element {
  return (
    <footer className="site-footer">
      <div>
        <span className="mono-label">LOCAL WORKSPACE</span>
        <p>{transport.label}. Explicit analysis commands run through the local Rust host; archived examples remain labelled fixtures.</p>
      </div>
      <div>
        <span className="mono-label">DATA POLICY</span>
        <p>Feedback stays in this browser. Evidence is immutable. AI runs only on request, using a key kept in this tab's memory.</p>
      </div>
      <p className="footer-wordmark" aria-hidden="true">
        SQLolla / finite evidence, explicit limits
      </p>
    </footer>
  );
}
