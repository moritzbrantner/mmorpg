const A = { op: "always" };
function w(e) {
  return "key" in e;
}
function ee(e) {
  return w(e) ? "keyboard" : e.device === "mouseButton" || e.device === "wheel" ? "mouse" : "gamepad";
}
function I(e) {
  if (w(e))
    return ["keyboard", e.key.kind, e.key.value].join(":");
  switch (e.device) {
    case "mouseButton":
      return ["mouseButton", e.button].join(":");
    case "wheel":
      return ["wheel", e.direction].join(":");
    case "gamepadButton":
      return ["gamepadButton", e.gamepad ?? "any", e.button, e.threshold].join(":");
    case "gamepadAxis":
      return [
        "gamepadAxis",
        e.gamepad ?? "any",
        e.axis,
        e.direction,
        e.threshold,
        e.deadzone
      ].join(":");
  }
}
function C(e, n) {
  const t = e ?? A;
  switch (t.op) {
    case "always":
      return !0;
    case "context":
      return n.has(t.id);
    case "not":
      return !C(t.expr, n);
    case "all":
      return t.exprs.every((i) => C(i, n));
    case "any":
      return t.exprs.some((i) => C(i, n));
  }
}
function B(e) {
  const n = e ?? A;
  switch (n.op) {
    case "always":
      return 0;
    case "context":
      return 1;
    case "not":
      return B(n.expr);
    case "all":
      return n.exprs.reduce((t, i) => t + B(i), 0);
    case "any":
      return n.exprs.length === 0 ? 0 : Math.min(...n.exprs.map((t) => B(t)));
  }
}
function j(e, n, t) {
  if (n.length === 0)
    return { kind: "none" };
  const i = [], s = [];
  for (const d of e)
    !C(d.when, t) || n.length > d.sequence.length || n.every((c, l) => R(c, d.sequence[l])) && (n.length === d.sequence.length ? i.push(d) : s.push(d));
  if (s.length > 0)
    return {
      kind: "pending",
      exactBindingIds: i.map((d) => d.id).sort(),
      continuationBindingIds: s.map((d) => d.id).sort()
    };
  if (i.length === 0)
    return { kind: "none" };
  const r = i.map(L).sort(oe)[0], o = i.filter((d) => H(L(d), r)).sort((d, c) => d.id.localeCompare(c.id));
  return new Set(o.map((d) => d.action)).size > 1 ? { kind: "ambiguous", bindingIds: o.map((d) => d.id) } : { kind: "resolved", bindingId: o[0].id, action: o[0].action };
}
function W(e) {
  const n = [];
  for (const [t, i] of ne(e)) {
    const s = e[t], r = e[i], o = ae(s.sequence, r.sequence);
    if (o === "separate")
      continue;
    const a = de(s.when, r.when);
    if (a.kind === "disjoint")
      continue;
    let d;
    a.kind === "unknown" ? d = o === "exact" ? "potentialExact" : "potentialPrefix" : o === "prefix" ? d = "chordPrefix" : s.action === r.action && ce(s.when, r.when) && (s.priority ?? 0) === (r.priority ?? 0) ? d = "duplicate" : H(L(s), L(r)) ? d = "ambiguousExact" : d = "overrideExact";
    const c = {
      leftBindingId: s.id,
      rightBindingId: r.id,
      kind: d
    };
    a.kind === "overlap" && a.witnessContexts.length > 0 && (c.witnessContexts = a.witnessContexts), n.push(c);
  }
  return n;
}
function ne(e) {
  const n = J(), t = [];
  return e.forEach((i, s) => {
    for (const r of te(n, i.sequence))
      t.push([r, s]);
    ie(n, i.sequence, s);
  }), t.sort(([i, s], [r, o]) => i - r || s - o), t;
}
function te(e, n) {
  const t = /* @__PURE__ */ new Set();
  let i = e;
  if (n.length === 0) {
    for (const s of e.subtreeIndices)
      t.add(s);
    return t;
  }
  for (const s of e.terminalIndices)
    t.add(s);
  for (let s = 0; s < n.length; s += 1) {
    const r = i.children.get(Y(n[s]));
    if (!r)
      return t;
    i = r;
    const o = s === n.length - 1 ? i.subtreeIndices : i.terminalIndices;
    for (const a of o)
      t.add(a);
  }
  return t;
}
function ie(e, n, t) {
  let i = e;
  i.subtreeIndices.push(t);
  for (const s of n) {
    const r = Y(s);
    let o = i.children.get(r);
    o || (o = J(), i.children.set(r, o)), i = o, i.subtreeIndices.push(t);
  }
  i.terminalIndices.push(t);
}
function J() {
  return { children: /* @__PURE__ */ new Map(), terminalIndices: [], subtreeIndices: [] };
}
function Y(e) {
  if (w(e))
    return JSON.stringify([
      "keyboard",
      e.key.kind,
      e.key.value,
      !!e.modifiers?.ctrl,
      !!e.modifiers?.alt,
      !!e.modifiers?.shift,
      !!e.modifiers?.meta,
      !!e.modifiers?.altGraph
    ]);
  switch (e.device) {
    case "mouseButton":
      return JSON.stringify([
        "mouseButton",
        e.button,
        !!e.modifiers?.ctrl,
        !!e.modifiers?.alt,
        !!e.modifiers?.shift,
        !!e.modifiers?.meta,
        !!e.modifiers?.altGraph
      ]);
    case "wheel":
      return JSON.stringify([
        "wheel",
        e.direction,
        !!e.modifiers?.ctrl,
        !!e.modifiers?.alt,
        !!e.modifiers?.shift,
        !!e.modifiers?.meta,
        !!e.modifiers?.altGraph
      ]);
    case "gamepadButton":
      return JSON.stringify([
        "gamepadButton",
        e.gamepad ?? null,
        e.button,
        e.threshold
      ]);
    case "gamepadAxis":
      return JSON.stringify([
        "gamepadAxis",
        e.gamepad ?? null,
        e.axis,
        e.direction,
        e.threshold,
        e.deadzone
      ]);
  }
}
function se(e, n) {
  const t = new Map(e.map((s) => [s.id, structuredClone(s)])), i = [];
  return n.patches.forEach((s, r) => {
    switch (s.op) {
      case "add":
        t.has(s.binding.id) ? i.push({ patchIndex: r, kind: "addCollision", bindingId: s.binding.id }) : t.set(s.binding.id, structuredClone(s.binding));
        break;
      case "remove":
        t.delete(s.bindingId) || i.push({ patchIndex: r, kind: "missingBinding", bindingId: s.bindingId });
        break;
      case "replace":
        s.binding.id !== s.bindingId ? i.push({
          patchIndex: r,
          kind: "replacementIdMismatch",
          bindingId: s.bindingId
        }) : t.has(s.bindingId) ? t.set(s.bindingId, structuredClone(s.binding)) : i.push({ patchIndex: r, kind: "missingBinding", bindingId: s.bindingId });
        break;
    }
  }), {
    bindings: [...t.values()].sort((s, r) => s.id.localeCompare(r.id)),
    diagnostics: i
  };
}
function R(e, n) {
  if (w(e) || w(n))
    return w(e) && w(n) && re(e, n);
  if (e.device !== n.device)
    return !1;
  switch (e.device) {
    case "mouseButton":
      return n.device === "mouseButton" && e.button === n.button && q(e.modifiers, n.modifiers);
    case "wheel":
      return n.device === "wheel" && e.direction === n.direction && q(e.modifiers, n.modifiers);
    case "gamepadButton":
      return n.device === "gamepadButton" && e.button === n.button && e.threshold === n.threshold && e.gamepad === n.gamepad;
    case "gamepadAxis":
      return n.device === "gamepadAxis" && e.axis === n.axis && e.direction === n.direction && e.threshold === n.threshold && e.deadzone === n.deadzone && e.gamepad === n.gamepad;
  }
}
function re(e, n) {
  return e.key.kind === n.key.kind && e.key.value === n.key.value && q(e.modifiers, n.modifiers);
}
function q(e, n) {
  return !!e?.ctrl == !!n?.ctrl && !!e?.alt == !!n?.alt && !!e?.shift == !!n?.shift && !!e?.meta == !!n?.meta && !!e?.altGraph == !!n?.altGraph;
}
function L(e) {
  return [e.priority ?? 0, B(e.when)];
}
function H(e, n) {
  return e[0] === n[0] && e[1] === n[1];
}
function oe(e, n) {
  return n[0] - e[0] || n[1] - e[1];
}
function ae(e, n) {
  if (e.length === n.length && e.every((s, r) => R(s, n[r])))
    return "exact";
  const t = Math.min(e.length, n.length);
  return Array.from({ length: t }, (s, r) => r).every((s) => R(e[s], n[s])) ? "prefix" : "separate";
}
function de(e, n) {
  const t = [.../* @__PURE__ */ new Set([...K(e), ...K(n)])].sort();
  if (t.length > 16)
    return { kind: "unknown", contextCount: t.length };
  const i = 2 ** t.length;
  for (let s = 0; s < i; s += 1) {
    const r = /* @__PURE__ */ new Set();
    if (t.forEach((o, a) => {
      (s & 2 ** a) !== 0 && r.add(o);
    }), C(e, r) && C(n, r))
      return { kind: "overlap", witnessContexts: [...r].sort() };
  }
  return { kind: "disjoint" };
}
function K(e) {
  const n = e ?? A;
  switch (n.op) {
    case "always":
      return [];
    case "context":
      return [n.id];
    case "not":
      return K(n.expr);
    case "all":
    case "any":
      return n.exprs.flatMap(K);
  }
}
function ce(e, n) {
  return JSON.stringify(e ?? A) === JSON.stringify(n ?? A);
}
function ue(e, n, t, i) {
  return le(e, n, t, i).resolution;
}
function le(e, n, t, i) {
  const { contexts: s, depthByContext: r, barrier: o } = he(t, i), a = {
    activeContexts: [...s].sort(),
    contextStack: i.map(ge),
    ...o ? { barrier: o } : {}
  };
  if (n.length === 0)
    return { resolution: { kind: "none" }, ...a, candidates: [] };
  const d = [], c = [], l = [...e].sort((u, g) => u.id.localeCompare(g.id));
  for (const u of l) {
    const g = {
      bindingId: u.id,
      action: u.action,
      priority: u.priority ?? 0,
      specificity: B(u.when)
    };
    if (!C(u.when, s)) {
      d.push({ ...g, match: "none", status: "inactiveContext" });
      continue;
    }
    if (n.length > u.sequence.length) {
      d.push({ ...g, match: "none", status: "inputLongerThanBinding" });
      continue;
    }
    if (!n.every((m, b) => R(m, u.sequence[b]))) {
      d.push({ ...g, match: "none", status: "sequenceMismatch" });
      continue;
    }
    const y = N(u.when, r), x = n.length === u.sequence.length ? "exact" : "continuation";
    if (o && y < o.depth) {
      d.push({
        ...g,
        match: x,
        status: "blockedByModal",
        ownerDepth: y
      });
      continue;
    }
    const M = d.length;
    d.push({
      ...g,
      match: x,
      status: "lowerContextLayer",
      ownerDepth: y
    }), c.push({ binding: u, traceIndex: M, depth: y, match: x });
  }
  if (c.length === 0)
    return { resolution: { kind: "none" }, ...a, candidates: d };
  const h = Math.max(...c.map((u) => u.depth)), f = c.filter((u) => u.depth === h), p = f.map((u) => u.binding), v = j(p, n, s);
  for (const u of f) {
    const g = d[u.traceIndex];
    switch (v.kind) {
      case "pending":
        g.status = u.match === "exact" ? "pendingExact" : "pendingContinuation";
        break;
      case "ambiguous":
        g.status = v.bindingIds.includes(u.binding.id) ? "ambiguousWinner" : "lowerRank";
        break;
      case "resolved": {
        if (u.binding.id === v.bindingId) {
          g.status = "winner";
          break;
        }
        const y = p.find((x) => x.id === v.bindingId);
        g.status = y && fe(u.binding, y) && u.binding.action === y.action ? "equivalentWinner" : "lowerRank";
        break;
      }
      case "none":
        g.status = "lowerRank";
        break;
    }
  }
  return { resolution: v, ...a, candidates: d };
}
function he(e, n) {
  const t = new Set(e), i = /* @__PURE__ */ new Map();
  let s;
  return n.forEach((r, o) => {
    t.add(r.id), i.set(r.id, o), r.blocksLower && (s = { id: r.id, depth: o });
  }), { contexts: t, depthByContext: i, ...s ? { barrier: s } : {} };
}
function fe(e, n) {
  return (e.priority ?? 0) === (n.priority ?? 0) && B(e.when) === B(n.when);
}
function N(e, n, t = !0) {
  const i = e ?? { op: "always" };
  switch (i.op) {
    case "always":
      return -1;
    case "context":
      return t ? n.get(i.id) ?? -1 : -1;
    case "not":
      return N(i.expr, n, !t);
    case "all":
    case "any":
      return i.exprs.reduce((s, r) => Math.max(s, N(r, n, t)), -1);
  }
}
function pe(e) {
  return e.blocksLower ? { id: e.id, blocksLower: !0 } : { id: e.id };
}
function ge(e) {
  return pe(e);
}
function F(e) {
  const n = e.actions.map((c) => structuredClone(c)).sort((c, l) => E(c.id, l.id) || E(c.title, l.title)), t = new Set(n.map((c) => c.id)), i = new Map(n.map((c) => [c.id, [...c.allowedDevices ?? []]])), s = [];
  for (const [c, l] of z(n.map((h) => h.id)))
    l > 1 && s.push({ kind: "duplicateActionId", actionId: c });
  const r = n.flatMap((c) => [...c.defaults ?? []].sort((l, h) => E(l.id, h.id)));
  for (const [c, l] of z(r.map((h) => h.id)))
    l > 1 && s.push({ kind: "duplicateBindingId", bindingId: c });
  for (const c of n) {
    const l = [...c.defaults ?? []].sort((h, f) => E(h.id, f.id));
    c.id.length === 0 && s.push({ kind: "emptyActionId", actionId: c.id });
    for (const h of l)
      h.action !== c.id && s.push({
        kind: "defaultActionMismatch",
        actionId: c.id,
        bindingId: h.id
      }), V(h, t, i, void 0, s);
  }
  const o = /* @__PURE__ */ new Map();
  for (const c of r)
    o.has(c.id) || o.set(c.id, structuredClone(c));
  const a = [...o.values()].sort((c, l) => E(c.id, l.id)), d = W(a.filter((c) => $(c, t)));
  return {
    baseBindings: a,
    diagnostics: s,
    conflicts: d,
    knownActions: t,
    allowedDevicesByAction: i
  };
}
function P(e, n) {
  const t = e.diagnostics.map(me);
  if (!n || n.patches.length === 0) {
    const a = e.baseBindings.map((d) => structuredClone(d));
    return {
      valid: t.length === 0,
      effectiveBindings: a,
      diagnostics: t,
      conflicts: e.conflicts.map(be)
    };
  }
  const i = se(e.baseBindings, n), s = i.bindings, r = i.diagnostics.map((a) => ({
    kind: xe(a.kind),
    bindingId: a.bindingId,
    patchIndex: a.patchIndex
  }));
  n.patches.forEach((a, d) => {
    (a.op === "add" || a.op === "replace") && V(a.binding, e.knownActions, e.allowedDevicesByAction, d, r);
  }), r.sort((a, d) => (a.patchIndex ?? Number.MAX_SAFE_INTEGER) - (d.patchIndex ?? Number.MAX_SAFE_INTEGER)), t.push(...r);
  const o = W(s.filter((a) => $(a, e.knownActions)));
  return { valid: t.length === 0, effectiveBindings: s, diagnostics: t, conflicts: o };
}
function Ae(e, n) {
  return P(F(e), n);
}
function z(e) {
  const n = /* @__PURE__ */ new Map();
  for (const t of e)
    n.set(t, (n.get(t) ?? 0) + 1);
  return [...n.entries()].sort(([t], [i]) => E(t, i));
}
function V(e, n, t, i, s) {
  const r = {
    actionId: e.action,
    bindingId: e.id,
    ...i === void 0 ? {} : { patchIndex: i }
  };
  e.id.length === 0 && s.push({ kind: "emptyBindingId", ...r }), n.has(e.action) || s.push({ kind: "unknownAction", ...r }), e.sequence.length === 0 && s.push({ kind: "emptySequence", ...r });
  const o = t.get(e.action) ?? [];
  e.sequence.forEach((a, d) => {
    o.includes(ee(a)) || s.push({ kind: "defaultDeviceNotAllowed", ...r, strokeIndex: d });
    const c = Z(a);
    c && s.push({ kind: c, ...r, strokeIndex: d });
  });
}
function me(e) {
  return { ...e };
}
function be(e) {
  return e.witnessContexts ? { ...e, witnessContexts: [...e.witnessContexts] } : { ...e };
}
function Z(e) {
  if (w(e))
    return e.key.kind === "logical" && !ve(e.key.value) ? "invalidLogicalKey" : e.key.kind === "physical" && !ye(e.key.value) ? "invalidPhysicalKey" : void 0;
  switch (e.device) {
    case "mouseButton":
      return T(e.button, 0, 31) ? void 0 : "invalidMouseButton";
    case "wheel":
      return ["up", "down", "left", "right"].includes(e.direction) ? void 0 : "invalidWheelDirection";
    case "gamepadButton":
      return X(e.gamepad) ? T(e.button, 0, 255) ? G(e.threshold, 1, 100) ? void 0 : "invalidThreshold" : "invalidGamepadButton" : "invalidGamepadIndex";
    case "gamepadAxis":
      return X(e.gamepad) ? T(e.axis, 0, 31) ? G(e.threshold, 1, 100) ? !G(e.deadzone, 0, 99) || e.deadzone >= e.threshold ? "invalidDeadzone" : void 0 : "invalidThreshold" : "invalidGamepadAxis" : "invalidGamepadIndex";
  }
}
function $(e, n) {
  return e.id.length > 0 && n.has(e.action) && e.sequence.length > 0 && e.sequence.every((t) => Z(t) === void 0);
}
function ve(e) {
  return e.length > 0 && e !== "Unidentified" && !/[\u0000-\u001F\u007F]/u.test(e);
}
function ye(e) {
  return e !== "Unidentified" && /^[A-Za-z][A-Za-z0-9]*$/u.test(e);
}
function T(e, n, t) {
  return Number.isInteger(e) && e >= n && e <= t;
}
function G(e, n, t) {
  return T(e, n, t);
}
function X(e) {
  return e === void 0 || T(e, 0, 15);
}
function E(e, n) {
  return e < n ? -1 : e > n ? 1 : 0;
}
function xe(e) {
  switch (e) {
    case "addCollision":
      return "profileAddCollision";
    case "missingBinding":
      return "profileMissingBinding";
    case "replacementIdMismatch":
      return "profileReplacementIdMismatch";
  }
}
const we = {
  setTimeout(e, n) {
    return globalThis.setTimeout(e, n);
  },
  clearTimeout(e) {
    globalThis.clearTimeout(e);
  }
};
class Me {
  registry;
  compiledRegistry;
  profile;
  report;
  getActiveContexts;
  getContextStack;
  chordTimeoutMs;
  consumePolicy;
  retryOnChordMismatch;
  scheduler;
  onDispatch;
  onDecision;
  pending = [];
  pendingExactBindingIds = [];
  timer;
  active = /* @__PURE__ */ new Map();
  pressedInputs = /* @__PURE__ */ new Set();
  constructor(n) {
    this.registry = structuredClone(n.registry), this.compiledRegistry = F(this.registry), this.profile = n.profile ? structuredClone(n.profile) : void 0, this.getActiveContexts = n.getActiveContexts, this.getContextStack = n.getContextStack, this.chordTimeoutMs = n.chordTimeoutMs ?? 1e3, this.consumePolicy = n.consumePolicy ?? "matched", this.retryOnChordMismatch = n.retryOnChordMismatch ?? !0, this.scheduler = n.scheduler ?? we, this.onDispatch = n.onDispatch, this.onDecision = n.onDecision, this.report = P(this.compiledRegistry, this.profile);
  }
  get validationReport() {
    return structuredClone(this.report);
  }
  get effectiveBindings() {
    return structuredClone(this.report.effectiveBindings);
  }
  get pendingSequence() {
    return structuredClone(this.pending);
  }
  get hasPendingChord() {
    return this.pending.length > 0;
  }
  updateConfiguration(n, t) {
    const i = this.reset("configurationChanged");
    return this.registry = structuredClone(n), this.compiledRegistry = F(this.registry), this.profile = t ? structuredClone(t) : void 0, this.report = P(this.compiledRegistry, this.profile), i;
  }
  updateProfile(n) {
    const t = this.reset("profileChanged");
    return this.profile = n ? structuredClone(n) : void 0, this.report = P(this.compiledRegistry, this.profile), t;
  }
  handleKeyDown(n, t = {}) {
    return this.handleInputDown(n, t);
  }
  handleInputDown(n, t = {}) {
    const i = t.repeat ?? !1, s = I(n);
    this.pressedInputs.add(s);
    const r = this.contextStack(), o = this.contexts(r);
    if (!this.report.valid)
      return this.emit(this.decision("invalidConfiguration", [n], o, [], !1, { reason: "invalidConfiguration" }));
    if (i && this.pending.length > 0)
      return this.emit(this.decision("repeatSuppressed", structuredClone(this.pending), o, [], this.shouldConsume(!0, !1), { reason: "repeatSuppressed" }));
    const a = [...this.pending, structuredClone(n)], d = this.resolve(a, o, r);
    if (d.kind === "none" && this.pending.length > 0) {
      const c = structuredClone(this.pending);
      return this.clearPending(), this.retryOnChordMismatch ? this.processFreshStroke(n, i, o, r, c) : this.emit(this.decision("cancelled", a, o, [], this.shouldConsume(!0, !1), { reason: "chordMismatch", cancelledSequence: c }, d));
    }
    return this.finishInputDown(a, n, i, o, d);
  }
  handleKeyUp(n) {
    return this.handleInputUp(n);
  }
  handleInputUp(n) {
    const t = this.contexts(this.contextStack()), i = I(n);
    if (this.pressedInputs.delete(i), !this.report.valid)
      return this.emit(this.decision("invalidConfiguration", [n], t, [], !1, { reason: "invalidConfiguration" }));
    const s = this.active.get(i) ?? [];
    if (this.active.delete(i), s.length === 0)
      return this.emit(this.decision("none", [n], t, [], !1, { reason: "unmatched" }));
    const r = s.slice().sort((o, a) => o.bindingId.localeCompare(a.bindingId)).map((o) => ({
      action: o.action,
      bindingId: o.bindingId,
      phase: "release",
      repeat: !1,
      reason: "keyUp",
      sequence: structuredClone(o.sequence),
      activeContexts: t
    }));
    return this.emit(this.decision("released", [n], t, r, this.shouldConsume(!0, !0), {
      reason: "keyReleased",
      bindingIds: r.map((o) => o.bindingId)
    }));
  }
  cancelChord(n = "explicit") {
    const t = this.contexts(this.contextStack()), i = structuredClone(this.pending);
    return this.clearPending(), this.emit(this.decision("cancelled", i, t, [], !1, { reason: "chordCancelled", cancelledSequence: i, resetReason: n }));
  }
  reset(n = "explicit") {
    const t = this.contexts(this.contextStack()), i = structuredClone(this.pending);
    this.clearPending(), this.pressedInputs.clear();
    const s = [...this.active.values()].flat().sort((r, o) => r.bindingId.localeCompare(o.bindingId)).map((r) => ({
      action: r.action,
      bindingId: r.bindingId,
      phase: "release",
      repeat: !1,
      reason: "reset",
      sequence: structuredClone(r.sequence),
      activeContexts: t
    }));
    return this.active.clear(), this.emit(this.decision("reset", i, t, s, !1, { reason: "reset", resetReason: n }));
  }
  processFreshStroke(n, t, i, s, r) {
    const o = [structuredClone(n)], a = this.resolve(o, i, s);
    return this.finishInputDown(o, n, t, i, a, r);
  }
  finishInputDown(n, t, i, s, r, o) {
    if (r.kind === "none")
      return this.clearPending(), this.emit(this.decision("none", n, s, [], !1, {
        reason: o ? "chordMismatch" : "unmatched",
        ...o ? { cancelledSequence: o } : {}
      }, r));
    if (r.kind === "pending")
      return this.pending = structuredClone(n), this.pendingExactBindingIds = [...r.exactBindingIds], this.scheduleTimeout(), this.emit(this.decision("pending", n, s, [], this.shouldConsume(!0, !1), {
        reason: "pendingChord",
        bindingIds: r.exactBindingIds,
        continuationBindingIds: r.continuationBindingIds,
        ...o ? { cancelledSequence: o } : {}
      }, r));
    if (this.clearPending(), r.kind === "ambiguous")
      return this.emit(this.decision("ambiguous", n, s, [], this.shouldConsume(!0, !1), {
        reason: "ambiguous",
        bindingIds: r.bindingIds,
        ...o ? { cancelledSequence: o } : {}
      }, r));
    const a = this.registry.actions.find((c) => c.id === r.action)?.repeatPolicy ?? "never";
    if (i && a !== "allow")
      return this.emit(this.decision("repeatSuppressed", n, s, [], this.shouldConsume(!0, !1), {
        reason: "repeatSuppressed",
        bindingIds: [r.bindingId],
        ...o ? { cancelledSequence: o } : {}
      }, r));
    const d = {
      action: r.action,
      bindingId: r.bindingId,
      phase: i ? "repeat" : "press",
      repeat: i,
      reason: n.length > 1 ? "chord" : "direct",
      sequence: structuredClone(n),
      activeContexts: s
    };
    return i || this.activate(d, t), this.emit(this.decision("dispatched", n, s, [d], this.shouldConsume(!0, !0), {
      reason: "resolved",
      bindingIds: [r.bindingId],
      ...o ? { cancelledSequence: o } : {}
    }, r));
  }
  scheduleTimeout() {
    this.timer !== void 0 && this.scheduler.clearTimeout(this.timer), this.timer = this.scheduler.setTimeout(() => {
      this.timer = void 0, this.flushPendingTimeout();
    }, this.chordTimeoutMs);
  }
  flushPendingTimeout() {
    if (this.pending.length === 0 || !this.report.valid)
      return;
    const n = structuredClone(this.pending), t = new Set(this.pendingExactBindingIds);
    this.pending = [], this.pendingExactBindingIds = [];
    const i = this.contextStack(), s = this.contexts(i), r = this.report.effectiveBindings.filter((a) => t.has(a.id) && a.sequence.length === n.length), o = this.resolve(n, s, i, r);
    if (o.kind === "resolved") {
      const a = {
        action: o.action,
        bindingId: o.bindingId,
        phase: "press",
        repeat: !1,
        reason: "timeout",
        sequence: n,
        activeContexts: s
      }, d = n.at(-1);
      d && this.pressedInputs.has(I(d)) && this.activate(a, d), this.emit(this.decision("dispatched", n, s, [a], !1, { reason: "timeoutResolved", bindingIds: [o.bindingId] }, o));
      return;
    }
    if (o.kind === "ambiguous") {
      this.emit(this.decision("ambiguous", n, s, [], !1, { reason: "timeoutAmbiguous", bindingIds: o.bindingIds }, o));
      return;
    }
    this.emit(this.decision("cancelled", n, s, [], !1, { reason: "timeoutExpired", cancelledSequence: n }, o));
  }
  resolve(n, t, i, s = this.report.effectiveBindings) {
    const r = new Set(t);
    return i ? ue(s, n, r, i) : j(s, n, r);
  }
  activate(n, t) {
    const i = I(t), s = this.active.get(i) ?? [];
    s.some((r) => r.bindingId === n.bindingId) || (s.push({
      action: n.action,
      bindingId: n.bindingId,
      sequence: structuredClone(n.sequence),
      triggerKey: i,
      activeContexts: [...n.activeContexts]
    }), this.active.set(i, s));
  }
  contextStack() {
    return this.getContextStack?.().map((n) => n.blocksLower ? { id: n.id, blocksLower: !0 } : { id: n.id });
  }
  contexts(n) {
    const t = new Set(this.getActiveContexts());
    for (const i of n ?? [])
      t.add(i.id);
    return [...t].sort();
  }
  clearPending() {
    this.pending = [], this.pendingExactBindingIds = [], this.timer !== void 0 && (this.scheduler.clearTimeout(this.timer), this.timer = void 0);
  }
  shouldConsume(n, t) {
    switch (this.consumePolicy) {
      case "never":
        return !1;
      case "matched":
        return n;
      case "dispatched":
        return t;
    }
  }
  decision(n, t, i, s, r, o, a) {
    return {
      kind: n,
      sequence: structuredClone(t),
      activeContexts: [...i],
      ...a ? { resolution: structuredClone(a) } : {},
      dispatches: structuredClone(s),
      consumed: r,
      explanation: structuredClone(o)
    };
  }
  emit(n) {
    for (const t of n.dispatches)
      this.onDispatch?.(structuredClone(t));
    return this.onDecision?.(structuredClone(n)), n;
  }
}
const Ie = /* @__PURE__ */ new Set(["Alt", "AltGraph", "Control", "Meta", "Shift"]);
function Ce(e, n = {}) {
  const { mode: t = "logical", altGraph: i = "distinct", ignoreComposing: s = !0, ignoreModifierOnly: r = !0, respectDefaultPrevented: o = !0 } = n;
  if (s && e.isComposing || o && e.defaultPrevented || r && Ie.has(e.key) || e.key === "Unidentified" || e.key === "Process")
    return null;
  const a = e.getModifierState?.("AltGraph") ?? e.key === "AltGraph", d = {
    ctrl: e.ctrlKey,
    alt: e.altKey,
    shift: e.shiftKey,
    meta: e.metaKey,
    altGraph: a
  };
  return a && i === "distinct" && (d.ctrl = !1, d.alt = !1), {
    key: t === "physical" ? { kind: "physical", value: e.code } : { kind: "logical", value: ke(e.key) },
    modifiers: d
  };
}
function _(e) {
  return e.defaultPrevented || !Number.isInteger(e.button) || e.button < 0 ? null : {
    device: "mouseButton",
    button: e.button,
    modifiers: Q(e)
  };
}
function Be(e) {
  if (e.defaultPrevented)
    return null;
  const n = Math.abs(e.deltaX), t = Math.abs(e.deltaY);
  return n === 0 && t === 0 ? null : {
    device: "wheel",
    direction: t >= n ? e.deltaY < 0 ? "up" : "down" : e.deltaX < 0 ? "left" : "right",
    modifiers: Q(e)
  };
}
function ke(e) {
  return e === " " ? "Space" : e === "Esc" ? "Escape" : e.length === 1 ? e.toLowerCase() : e;
}
function U(e) {
  if (typeof e != "object" || e === null)
    return !1;
  const n = e, t = n.tagName?.toUpperCase();
  if (t === "INPUT" || t === "TEXTAREA" || t === "SELECT" || n.isContentEditable)
    return !0;
  const i = n.role ?? n.getAttribute?.("role");
  return i === "textbox" || i === "searchbox" || i === "combobox";
}
function De(e, n = {}) {
  const t = globalThis, i = n.keyTarget ?? t.window;
  if (!i)
    throw new Error("attachKeyboardRuntime requires a keyTarget outside a browser environment");
  const s = n.focusTarget ?? t.window, r = n.visibilityTarget ?? t.document, o = n.ignoreTextEntry ?? !1, a = n.resetOnBlur ?? !0, d = n.resetOnHidden ?? !0, c = n.resetOnDetach ?? !0, l = /* @__PURE__ */ new Map(), h = (m, b) => {
    b.consumed && (m.preventDefault?.(), n.stopPropagation && m.stopPropagation?.());
  }, f = () => typeof n.mode == "function" ? n.mode() : n.mode ?? "logical", p = (m, b = {}) => Ce(m, {
    ...n.keyboardOptions,
    mode: f(),
    ...b
  }), v = (m) => m.code || m.key, u = (m) => {
    const b = m;
    if (o && U(b.target))
      return;
    const S = v(b), D = l.get(S), k = D ?? p(b);
    if (!k)
      return;
    D || l.set(S, structuredClone(k));
    const O = e.handleKeyDown(k, { repeat: !!b.repeat });
    h(b, O);
  }, g = (m) => {
    const b = m, S = v(b), D = l.get(S);
    l.delete(S);
    const k = D ?? p(b, {
      ignoreComposing: !1,
      respectDefaultPrevented: !1
    });
    if (!k)
      return;
    const O = e.handleKeyUp(k);
    h(b, O);
  }, y = (m) => {
    l.clear(), e.reset(m);
  }, x = () => {
    a && y("blur");
  }, M = () => {
    d && (r?.hidden === !0 || r?.visibilityState === "hidden") && y("hidden");
  };
  return i.addEventListener("keydown", u), i.addEventListener("keyup", g), s?.addEventListener("blur", x), r?.addEventListener("visibilitychange", M), () => {
    i.removeEventListener("keydown", u), i.removeEventListener("keyup", g), s?.removeEventListener("blur", x), r?.removeEventListener("visibilitychange", M), l.clear(), c && e.reset("detached");
  };
}
function Pe(e, n = {}) {
  const t = globalThis, i = n.target ?? t.window;
  if (!i)
    throw new Error("attachMouseRuntime requires a target outside a browser environment");
  const s = n.ignoreTextEntry ?? !1, r = n.resetOnDetach ?? !0, o = /* @__PURE__ */ new Map(), a = (h, f) => {
    f.consumed && (h.preventDefault?.(), n.stopPropagation && h.stopPropagation?.());
  }, d = (h) => {
    const f = h;
    if (s && U(f.target) || (n.respectDefaultPrevented ?? !0) && f.defaultPrevented)
      return;
    const p = _({ ...f, defaultPrevented: !1 });
    p && (o.set(f.button, structuredClone(p)), a(f, e.handleInputDown(p)));
  }, c = (h) => {
    const f = h, p = o.get(f.button) ?? _({ ...f, defaultPrevented: !1 });
    o.delete(f.button), p && a(f, e.handleInputUp(p));
  }, l = (h) => {
    const f = h;
    if (s && U(f.target) || (n.respectDefaultPrevented ?? !0) && f.defaultPrevented)
      return;
    const p = Be({ ...f, defaultPrevented: !1 });
    if (!p)
      return;
    const v = e.handleInputDown(p);
    e.handleInputUp(p), a(f, v);
  };
  return i.addEventListener("mousedown", d), i.addEventListener("mouseup", c), i.addEventListener("wheel", l, { passive: !1 }), () => {
    i.removeEventListener("mousedown", d), i.removeEventListener("mouseup", c), i.removeEventListener("wheel", l, { passive: !1 }), o.clear(), r && e.reset("mouseDetached");
  };
}
function Re(e, n = {}) {
  const t = globalThis, i = n.getGamepads ?? (() => t.navigator?.getGamepads?.() ?? []), s = n.scheduler ?? Te(t), r = n.resetOnDetach ?? !0, o = Se(e.effectiveBindings), a = /* @__PURE__ */ new Map();
  let d = !1, c;
  const l = () => {
    if (d)
      return;
    const h = i();
    for (const f of o) {
      const p = I(f), v = a.has(p), u = Ee(f, h, v);
      if (!v && u)
        a.set(p, structuredClone(f)), e.handleInputDown(f);
      else if (v && !u) {
        const g = a.get(p);
        a.delete(p), g && e.handleInputUp(g);
      }
    }
    c = s.requestFrame(l);
  };
  return c = s.requestFrame(l), () => {
    d = !0, c !== void 0 && s.cancelFrame(c);
    for (const h of a.values())
      e.handleInputUp(h);
    a.clear(), r && e.reset("gamepadDetached");
  };
}
function Ee(e, n, t = !1) {
  const i = n.filter((s) => s !== null && s.connected !== !1 && (e.gamepad === void 0 || s.index === e.gamepad));
  return e.device === "gamepadButton" ? i.some((s) => {
    const r = s.buttons[e.button];
    return !!r && (r.pressed === !0 || r.value * 100 >= e.threshold);
  }) : i.some((s) => {
    const r = s.axes[e.axis];
    if (!Number.isFinite(r) || !(e.direction === "positive" ? r > 0 : r < 0))
      return !1;
    const a = Math.abs(r) * 100;
    return t ? a > e.deadzone : a >= e.threshold;
  });
}
function Se(e) {
  const n = /* @__PURE__ */ new Map();
  for (const t of e)
    for (const i of t.sequence)
      "device" in i && (i.device === "gamepadButton" || i.device === "gamepadAxis") && n.set(I(i), structuredClone(i));
  return [...n.values()].sort((t, i) => I(t).localeCompare(I(i)));
}
function Te(e) {
  return e.requestAnimationFrame && e.cancelAnimationFrame ? {
    requestFrame: (n) => e.requestAnimationFrame(n),
    cancelFrame: (n) => e.cancelAnimationFrame(n)
  } : {
    requestFrame: (n) => globalThis.setTimeout(n, 16),
    cancelFrame: (n) => globalThis.clearTimeout(n)
  };
}
function Q(e) {
  const n = e.getModifierState?.("AltGraph") ?? !1;
  return {
    ctrl: n ? !1 : e.ctrlKey,
    alt: n ? !1 : e.altKey,
    shift: e.shiftKey,
    meta: e.metaKey,
    altGraph: n
  };
}
export {
  Me as InputRuntimeController,
  W as analyzeConflicts,
  se as applyProfile,
  Re as attachGamepadRuntime,
  De as attachKeyboardRuntime,
  Pe as attachMouseRuntime,
  Ce as keyboardEventToStroke,
  Ae as validateRegistry
};
