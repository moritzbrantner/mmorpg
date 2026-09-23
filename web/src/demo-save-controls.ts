import { DemoSaveController, type DemoProgress } from "./demo-save";
import "./demo-save-controls.css";

/** Mount the same commands in character selection and the in-world HUD. */
export function installDemoSaveControls(
  roots: readonly HTMLElement[],
  characterId: string,
  capture: () => DemoProgress,
  restore: (progress: DemoProgress) => void,
): void {
  const controller = new DemoSaveController(characterId, capture, restore, () => window.localStorage);
  const statuses: HTMLElement[] = [];
  let feedbackOperation = 0;
  async function run(action: () => string | Promise<string>) {
    const operation = ++feedbackOperation;
    try {
      const message = await action();
      if (operation === feedbackOperation) statuses.forEach((status) => { status.textContent = message; });
    } catch (error) {
      if (operation !== feedbackOperation) return;
      const message = error instanceof Error ? error.message : "Save operation failed.";
      statuses.forEach((status) => { status.textContent = `${message} Current game and local save were not replaced.`; });
    }
  }

  for (const root of roots) {
    root.dataset.gameSaveControls = "";
    root.classList.add("game-save-controls");
    const actions = document.createElement("div");
    actions.className = "game-save-actions";
    const feedback = document.createElement("p");
    feedback.className = "game-save-status";
    feedback.setAttribute("role", "status");
    feedback.setAttribute("aria-live", "polite");
    feedback.textContent = "Offline progress only. Save locally or export a backup.";
    statuses.push(feedback);

    const button = (label: string, action: () => void) => {
      const element = document.createElement("button");
      element.type = "button";
      element.textContent = label;
      element.addEventListener("click", action);
      actions.append(element);
    };
    button("Save game", () => void run(() => {
      controller.save();
      return "Game saved in this browser. Load game resumes this checkpoint.";
    }));
    button("Load game", () => void run(() => controller.load()
      ? "Game loaded. Position, appearance, and waystone progress restored."
      : "No saved game exists for this character. Appearance-only saves are separate."));
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
        // Let the browser consume the download before releasing its object URL.
        window.setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
      return "Save export requested. This does not change your local save slot.";
    }));

    const fileInput = document.createElement("input");
    fileInput.type = "file";
    fileInput.accept = ".json,application/json";
    fileInput.hidden = true;
    fileInput.setAttribute("aria-label", "Import offline game save");
    button("Import save", () => fileInput.click());
    fileInput.addEventListener("change", () => {
      const file = fileInput.files?.[0];
      fileInput.value = ""; // The same file can be selected again after an error.
      if (!file) return;
      void run(async () => await controller.import(file)
        ? "Imported game restored. Press Save game to keep it in this browser."
        : "Import superseded by a newer save/load action.");
    });
    root.append(actions, fileInput, feedback);
  }
}
