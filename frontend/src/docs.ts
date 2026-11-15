/** Progressive enhancement for the static documentation page. */
const clipboardStatus = document.querySelector<HTMLElement>("#clipboard-status");

/** Select the exact example if the browser refuses clipboard access. */
function selectExample(example: HTMLElement): void {
  const selection = window.getSelection();
  if (!selection) return;
  const range = document.createRange();
  range.selectNodeContents(example);
  selection.removeAllRanges();
  selection.addRange(range);
}

/** Copy plain text and leave a visible, accessible result on the selected button. */
async function copyExample(button: HTMLButtonElement, example: HTMLElement): Promise<void> {
  let message = "Copied";
  try {
    await navigator.clipboard.writeText(example.textContent ?? "");
  } catch {
    selectExample(example);
    message = "Text selected. Use your keyboard to copy.";
  }
  button.textContent = message === "Copied" ? message : "Selected";
  if (clipboardStatus) clipboardStatus.textContent = message;
}

/** Enhance only copy buttons with existing source elements. */
function enableCopy(): void {
  for (const button of document.querySelectorAll<HTMLButtonElement>("button[data-copy]")) {
    const example = document.getElementById(button.dataset.copy ?? "");
    if (!example) continue;
    button.hidden = false;
    button.addEventListener("click", () => void copyExample(button, example));
  }
}

/** Match the active guide to direct links and in-page navigation. */
function updateGuide(): void {
  const target = document.getElementById(window.location.hash.slice(1));
  const section = target?.closest("section[aria-labelledby]")?.getAttribute("aria-labelledby") ?? "quick-start";
  for (const link of document.querySelectorAll<HTMLAnchorElement>(".guide-nav a")) {
    if (link.hash === `#${section}`) link.setAttribute("aria-current", "location");
    else link.removeAttribute("aria-current");
  }
}

enableCopy();
updateGuide();
window.addEventListener("hashchange", updateGuide);
