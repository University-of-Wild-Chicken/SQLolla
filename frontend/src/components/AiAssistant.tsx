/**
 * Plain-language purpose, connection status and per-action model labels for optional AI help.
 * These components render only local settings; none of them sends a request.
 */
import type { JSX } from "react";
import { liveTarget } from "../ai/config";
import type { AiProviderSettings } from "../ai/summary";

/** The three places AI helps, each paired with the deterministic check that decides. */
const JOBS = [
  {
    name: "Draft",
    where: "Query editor · Build with AI",
    text: "Describe a database or question in plain words. AI proposes a schema and SQL; SQLolla compiles it locally before you can use it.",
  },
  {
    name: "Explain",
    where: "Beside a result",
    text: "Ask why a case happened or what to check next. Every AI claim links to the evidence fact it cites.",
  },
  {
    name: "Refine",
    where: "Review bins",
    text: "Mark cases Yes or No. AI proposes corrected SQL, and SQLolla re-checks it against every labeled case and your original rule.",
  },
] as const;

/** Render the three AI jobs as a numbered list. */
export function AiJobs(): JSX.Element {
  return (
    <ol className="ai-jobs">
      {JOBS.map((job) => (
        <li key={job.name}>
          <strong>{job.name}</strong>
          <span className="ai-jobs__where">{job.where}</span>
          <p>{job.text}</p>
        </li>
      ))}
    </ol>
  );
}

/** State the authority boundary in one sentence wherever AI is introduced. */
export function AiRule(): JSX.Element {
  return (
    <p className="ai-rule">
      <strong>AI proposes. SQLolla decides.</strong> AI output is untrusted text until the deterministic compiler and solver check it. Your key stays in this
      tab’s memory, and nothing is sent until you press an AI button.
    </p>
  );
}

interface IntroProps {
  readonly onConnect: () => void;
  readonly onSkip: () => void;
}

/** First-visit explanation of what AI does here; query analysis stays fully usable without it. */
export function AiIntro({ onConnect, onSkip }: IntroProps): JSX.Element {
  return (
    <section className="win ai-intro" aria-labelledby="ai-intro-heading">
      <div className="win__title">
        <h2 id="ai-intro-heading">AI assistant (optional)</h2>
      </div>
      <div className="win__body">
        <p>SQLolla checks queries without AI. Bring your own OpenAI, Anthropic or Gemini key, choose a model, and AI can help with three steps:</p>
        <AiJobs />
        <AiRule />
        <div className="ai-intro__actions">
          <button type="button" className="primary-button" onClick={onConnect}>
            Connect AI provider…
          </button>
          <button type="button" className="ghost-button" onClick={onSkip}>
            Continue without AI
          </button>
        </div>
      </div>
    </section>
  );
}

/** Name the provider and exact model an AI action will use, or nothing when AI is not connected. */
export function AiTargetNote({ settings }: { readonly settings: AiProviderSettings }): JSX.Element | null {
  const target = liveTarget(settings);
  if (!target) return null;
  return (
    <p className="ai-target">
      Uses {target.label} · <code>{target.model}</code>
    </p>
  );
}

/** Short connection text for status bars; never includes the key. */
export function aiStatusText(settings: AiProviderSettings): string {
  const target = liveTarget(settings);
  return target ? `AI: ${target.label} · ${target.model}` : "AI: not connected";
}
