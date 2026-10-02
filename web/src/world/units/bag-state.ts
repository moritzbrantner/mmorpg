import type { WorldCommand } from "../../command-wire";
import type { InventorySlot, StatTotals, ZoneSnapshot } from "../../replication";

/** Retains received sheets; intents never modify this presentation cache. */
export class BagState {
  #identity: string | null = null;
  #tick = -1n;
  #revision = 0n;
  #sheetRevision = 0n;
  #slots: readonly InventorySlot[] | null = null;
  #equipment: readonly (number | null)[] | null = null;
  #stats: StatTotals | null = null;
  #dead = false;
  #feedback = "";
  #sentRevision: bigint | null = null;

  get slots(): readonly InventorySlot[] | null { return this.#slots; }
  /** Item IDs by equipment slot, received with the bag under the same revision. */
  get equipment(): readonly (number | null)[] | null { return this.#equipment; }
  get stats(): StatTotals | null { return this.#stats; }
  get ready(): boolean { return this.#slots !== null && this.#sheetRevision === this.#revision; }
  get canMove(): boolean { return this.ready && !this.#dead; }
  get feedback(): string { return this.#feedback; }
  get revision(): bigint { return this.#revision; }
  get status(): string {
    if (!this.ready) {
      return "Waiting for your bag…";
    }
    if (this.#dead) {
      return "You cannot move items while dead.";
    }
    return "Select a stack, then its destination.";
  }

  reset(snapshot: ZoneSnapshot | null = null): void {
    this.#identity = snapshot ? `${snapshot.zoneId}:${snapshot.contentRevision}:${snapshot.viewerId}` : null;
    this.#tick = -1n;
    this.#revision = 0n;
    this.#sheetRevision = 0n;
    this.#slots = null;
    this.#equipment = null;
    this.#stats = null;
    this.#dead = false;
    this.#feedback = "";
    this.#sentRevision = null;
  }

  update(snapshot: ZoneSnapshot): boolean {
    const identity = `${snapshot.zoneId}:${snapshot.contentRevision}:${snapshot.viewerId}`;
    if ((this.#identity !== null && this.#identity !== identity) || snapshot.tick <= this.#tick ||
        snapshot.inventoryRevision < this.#revision) {
      return false;
    }
    this.#identity = identity;
    this.#tick = snapshot.tick;
    this.#revision = snapshot.inventoryRevision;
    this.#dead = snapshot.viewer.dead;
    if (snapshot.inventory !== null) {
      this.#slots = snapshot.inventory.map((stack) => stack ? { ...stack } : null);
      this.#equipment = snapshot.equipment ? [...snapshot.equipment] : null;
      this.#stats = snapshot.stats ? { ...snapshot.stats } : null;
      this.#sheetRevision = snapshot.inventoryRevision;
      if (this.#sentRevision !== null && this.#sheetRevision > this.#sentRevision) {
        this.#feedback = "Bag updated.";
        this.#sentRevision = null;
      }
    }
    for (const event of snapshot.events) {
      if (event.kind !== "error") {
        continue;
      }
      if (event.code === "invalid-inventory-move") {
        this.#feedback = "That bag move was refused. Your items are unchanged.";
      }
      if (event.code === "inventory-full") {
        this.#feedback = "That stack is full. Your items are unchanged.";
      }
      if (event.code === "you-are-dead") {
        this.#feedback = "You cannot move items while dead.";
      }
      if (event.code === "too-many-intents") {
        this.#feedback = "Too many actions. Try the bag move again.";
      }
      if (["invalid-inventory-move", "inventory-full", "you-are-dead", "too-many-intents"].includes(event.code)) {
        this.#sentRevision = null;
      }
    }
    return true;
  }

  move(source: number, destination: number, quantity: number): WorldCommand | null {
    const stack = this.#slots?.[source];
    if (!this.canMove || !Number.isInteger(source) || source < 0 || source >= 16 ||
        !Number.isInteger(destination) || destination < 0 || destination >= 16 ||
        !stack || !Number.isInteger(quantity) || quantity < 1 || quantity > stack.quantity) {
      return null;
    }
    this.#feedback = "Move sent. Waiting for the zone.";
    this.#sentRevision = this.#revision;
    return { kind: "move-item", source, destination, quantity };
  }
}
