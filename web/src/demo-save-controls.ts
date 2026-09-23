import { hatOption } from "./character-customization";
import { DemoSaveController, demoSaveKey, type DemoProgress } from "./demo-save";
import "./demo-save-controls.css";

/** Mount one shared command boundary in selection and the in-world HUD. */
export function installDemoSaveControls(
  roots: readonly HTMLElement[],
  characterId: string,
  capture: () => DemoProgress,
  restore: (progress: DemoProgress) => void,
) {
  const controller = new DemoSaveController(characterId, capture, restore, () => window.localStorage);
  const statuses: HTMLElement[] = [];
  const summaries: HTMLElement[] = [];
  const loadButtons: HTMLButtonElement[] = [];
  let feedbackOperation = 0;

  function refresh() {
    let message: string;
    let canLoad = false;
    try {
      const checkpoint = controller.inspect();
      canLoad = checkpoint !== null;
      message = checkpoint
        ? `Local checkpoint · ${hatOption(checkpoint.appearance.hat).name} · Position ${checkpoint.position.x.toFixed(1)}, ${checkpoint.position.z.toFixed(1)} · Waystone ${checkpoint.waystoneActive ? "active" : "inactive"}`
        : "No local checkpoint. Save your game or import a backup to continue later.";
    } catch {
      message = "Local checkpoint unavailable or invalid. Import/export remain available; existing saved bytes are untouched.";
    }
    for (const summary of summaries) summary.textContent = message;
    for (const button of loadButtons) button.disabled = !canLoad;
  }

  async function run(action: () => string | Promise<string>) {
    const operation = ++feedbackOperation;
    try {
      const message = await action();
      if (operation !== feedbackOperation) return;
      refresh();
      for (const status of statuses) status.textContent = message;
    } catch (error) {
      if (operation !== feedbackOperation) return;
      refresh();
      const message = error instanceof Error ? error.message : "Save operation failed.";
      for (const status of statuses) status.textContent = `${message} Current game and local save were not replaced.`;
    }
  }

  for (const root of roots) {
    root.classList.add("game-save-controls");
    const summary = document.createElement("p");
    summary.className = "game-save-summary";
    summaries.push(summary);
    const actions = document.createElement("div");
    actions.className = "game-save-actions";
    const feedback = document.createElement("p");
    feedback.className = "game-save-status";
    feedback.setAttribute("role", "status");
    feedback.setAttribute("aria-live", "polite");
    feedback.textContent = "Manual saves only. Returning to characters pauses your current session.";
    statuses.push(feedback);

    const button = (label: string, action: () => void) => {
      const element = document.createElement("button");
      element.type = "button";
      element.textContent = label;
      element.addEventListener("click", action);
      actions.append(element);
      return element;
    };
    button("Save game", () => void run(() => {
      controller.save();
      return "Game saved in this browser. Load game resumes this checkpoint.";
    }));
    loadButtons.push(button("Load game", () => void run(() => controller.load()
      ? "Game loaded. Position, appearance, and waystone progress restored."
      : "No saved game exists for this character. Appearance-only saves are separate.")));
    button("Export save", () => void run(() => {
      const raw = controller.export();
      const url = URL.createObjectURL(new Blob([raw], { type: "application/json" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = "mmorpg-greyhaven-save.json";
      document.body.append(link);
      try {
        link.click();
      } finally {
        link.remove();
        window.setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
      return "Save export requested. Your local checkpoint has not changed.";
    }));

    const fileInput = document.createElement("input");
    fileInput.type = "file";
    fileInput.accept = ".json,application/json";
    fileInput.hidden = true;
    fileInput.setAttribute("aria-label", "Import offline game save");
    button("Import save", () => fileInput.click());
    fileInput.addEventListener("change", () => {
      const file = fileInput.files?.[0];
      fileInput.value = "";
      if (!file) return;
      for (const status of statuses) status.textContent = "Reading and validating save…";
      void run(async () => await controller.import(file)
        ? "Imported game restored. Press Save game to keep it in this browser."
        : "Import superseded by a newer action.");
    });
    root.append(summary, actions, fileInput, feedback);
  }

  // Queries only on mount, commands, selection entry, or a relevant other-tab change.
  // Never serialize or access localStorage from the render/movement loop.
  window.addEventListener("storage", (event) => {
    if (event.key === null || event.key === demoSaveKey(characterId)) refresh();
  });
  refresh();
  return {
    refresh,
    cancelPending() {
      controller.cancelPending();
      feedbackOperation += 1;
      for (const status of statuses) status.textContent = "Current session retained. Save game keeps a checkpoint; no automatic save was made.";
    },
  };
}
