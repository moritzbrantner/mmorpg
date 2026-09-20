export const INPUT_BINDINGS_BUNDLE_URL =
  "https://moritzbrantner.github.io/input-bindings/input-bindings-browser.js";

const CONTEXT_ID = "mmorpgTechDemo";

const physical = (id, action, code) => ({
  id,
  action,
  sequence: [{ key: { kind: "physical", value: code }, modifiers: {} }],
  when: { op: "context", id: CONTEXT_ID },
  priority: 0,
});

const movementAction = (id, title, codes) => ({
  id,
  title,
  categoryPath: ["Tech demo", "Movement"],
  repeatPolicy: "allow",
  allowedDevices: ["keyboard"],
  defaults: codes.map((code) => physical(`${id}.${code}`, id, code)),
  provenance: { source: "mmorpg/pages-tech-demo", version: "1" },
});

export const DEMO_INPUT_REGISTRY = Object.freeze({
  actions: [
    movementAction("demo.move.forward", "Move forward", ["KeyW", "ArrowUp"]),
    movementAction("demo.move.backward", "Move backward", ["KeyS", "ArrowDown"]),
    movementAction("demo.move.left", "Move left", ["KeyA", "ArrowLeft"]),
    movementAction("demo.move.right", "Move right", ["KeyD", "ArrowRight"]),
    {
      id: "demo.interact",
      title: "Interact",
      categoryPath: ["Tech demo", "Character"],
      repeatPolicy: "never",
      allowedDevices: ["keyboard"],
      defaults: [physical("demo.interact.KeyE", "demo.interact", "KeyE")],
      provenance: { source: "mmorpg/pages-tech-demo", version: "1" },
    },
  ],
});

const MOVEMENT = Object.freeze({
  "demo.move.forward": [0, -1],
  "demo.move.backward": [0, 1],
  "demo.move.left": [-1, 0],
  "demo.move.right": [1, 0],
});

export function attachDemoInputBindings({ target, onMovement, onInteract, onUnavailable }) {
  let disposed = false;
  let detachRuntime = () => {};
  const activeMovement = new Set();

  const publishMovement = () => {
    let x = 0;
    let z = 0;
    for (const action of activeMovement) {
      const direction = MOVEMENT[action];
      x += direction[0];
      z += direction[1];
    }
    onMovement(Math.max(-1, Math.min(1, x)), Math.max(-1, Math.min(1, z)));
  };

  const ready = import(INPUT_BINDINGS_BUNDLE_URL).then(
    ({ InputRuntimeController, attachKeyboardRuntime }) => {
      if (disposed) return;
      const controller = new InputRuntimeController({
        registry: DEMO_INPUT_REGISTRY,
        getActiveContexts: () => new Set([CONTEXT_ID]),
        consumePolicy: "dispatched",
        onDispatch: (dispatch) => {
          if (dispatch.action in MOVEMENT) {
            if (dispatch.phase === "release") activeMovement.delete(dispatch.action);
            else activeMovement.add(dispatch.action);
            publishMovement();
            return;
          }
          if (dispatch.action === "demo.interact" && dispatch.phase === "press") onInteract();
        },
      });

      detachRuntime = attachKeyboardRuntime(controller, {
        keyTarget: target,
        focusTarget: window,
        visibilityTarget: document,
        ignoreTextEntry: true,
        mode: "physical",
      });
      target.dataset.inputBindings = "ready";
    },
    (error) => {
      console.error("Failed to load shared input-bindings runtime", error);
      target.dataset.inputBindings = "unavailable";
      onUnavailable?.(error);
    },
  );

  return {
    ready,
    destroy() {
      disposed = true;
      activeMovement.clear();
      onMovement(0, 0);
      detachRuntime();
    },
  };
}
