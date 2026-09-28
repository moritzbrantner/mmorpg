import { test } from "node:test";
import assert from "node:assert/strict";
import { CharacterTurntable, PREVIEW_FRONT_YAW, wrapDegrees } from "../src/character-turntable";

const close = (actual: number, expected: number) => assert.ok(Math.abs(actual - expected) < 1e-10, `${actual} != ${expected}`);

test("starts facing the selection camera and stays still until user input", () => {
  const model = new CharacterTurntable();
  assert.equal(model.degrees, 0);
  assert.equal(model.yaw, PREVIEW_FRONT_YAW);
  const length = Math.hypot(4.5, 7.1);
  close(-Math.sin(model.yaw), 4.5 / length);
  close(-Math.cos(model.yaw), 7.1 / length);
});

test("left/right wrap through full revolutions and reset restores front", () => {
  const model = new CharacterTurntable();
  model.turn(-15);
  assert.equal(model.degrees, 345);
  model.turn(30);
  assert.equal(model.degrees, 15);
  for (let i = 0; i < 10000; i++) model.turn(360);
  assert.equal(model.degrees, 15);
  model.reset();
  assert.equal(model.yaw, PREVIEW_FRONT_YAW);
  assert.equal(wrapDegrees(-0), 0);
  for (const bad of [NaN, Infinity, -Infinity]) assert.throws(() => wrapDegrees(bad));
});

test("drag is relative to the original yaw and surface width, not event frequency", () => {
  const model = new CharacterTurntable();
  model.turn(15);
  assert.equal(model.begin(3, 100, 400), true);
  for (const x of [125, 160, 175, 200]) model.move(3, x);
  assert.equal(model.degrees, 105);
  assert.equal(model.end(3), true);
  assert.equal(model.degrees, 105);
  assert.equal(model.pointerId, null);
});

test("other pointers cannot hijack, finish, or cancel an active drag", () => {
  const model = new CharacterTurntable();
  model.begin(1, 0, 200);
  assert.equal(model.begin(2, 0, 200), false);
  assert.equal(model.move(2, 99), false);
  assert.equal(model.end(2), false);
  assert.equal(model.cancel(2), false);
  model.move(1, 100);
  assert.equal(model.degrees, 180);
  assert.equal(model.pointerId, 1);
});

test("cancel rolls back the active drag; stale moves cannot change the preview", () => {
  const model = new CharacterTurntable();
  model.turn(45);
  model.begin(1, 0, 400);
  model.move(1, 100);
  assert.equal(model.degrees, 135);
  assert.equal(model.cancel(1), true);
  assert.equal(model.degrees, 45);
  assert.equal(model.move(1, 200), false);
  assert.equal(model.end(1), false);
});

test("reset and button input retire an active pointer before applying their own intent", () => {
  const model = new CharacterTurntable();
  model.turn(30);
  model.begin(1, 0, 400);
  model.move(1, 100);
  model.turn(15);
  assert.equal(model.degrees, 45);
  assert.equal(model.pointerId, null);
  model.begin(2, 0, 400);
  model.move(2, 100);
  model.reset();
  assert.equal(model.degrees, 0);
  assert.equal(model.move(2, 200), false);
});

test("rejects invalid geometry and pointer input without poisoning finite rotation", () => {
  const model = new CharacterTurntable();
  for (const width of [0, -1, Infinity, NaN]) assert.equal(model.begin(1, 0, width), false);
  assert.equal(model.begin(1, NaN, 400), false);
  model.begin(1, 0, 400);
  assert.equal(model.move(1, Infinity), false);
  assert.equal(model.degrees, 0);
});
