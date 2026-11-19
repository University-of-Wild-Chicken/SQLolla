/** Win9x-style window chrome shared by the workbench's panels. Purely presentational. */
import { useId, type JSX, type ReactNode } from "react";
import markUrl from "../assets/sqlolla-mark.png";

/** Props for one titled window; the title is the region's accessible name. */
export interface WindowProps {
  readonly title: ReactNode;
  readonly children: ReactNode;
  readonly id?: string;
  readonly className?: string;
  /** Optional text shown at the right of the title bar, such as a status or hint. */
  readonly meta?: ReactNode;
}

/** Render a labelled region with a title bar (icon and heading) above a body. */
export function Window({ title, children, id, className = "", meta }: WindowProps): JSX.Element {
  const headingId = useId();
  return (
    <section className={`win ${className}`} id={id} aria-labelledby={headingId}>
      <div className="win__title">
        <img className="win__icon" src={markUrl} alt="" width="14" height="18" />
        <h2 id={headingId}>{title}</h2>
        {meta ? <span className="win__meta">{meta}</span> : null}
      </div>
      <div className="win__body">{children}</div>
    </section>
  );
}
