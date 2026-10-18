import { test } from "node:test";
import * as assert from "node:assert/strict";
import { serialized } from "./serialized";

function pause(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

test("serialized runs a task only after the one before it has finished", async () => {
  const run = serialized();
  const events: string[] = [];
  const first = run(async () => {
    events.push("first starts");
    await pause(20);
    events.push("first ends");
  });
  const second = run(async () => {
    events.push("second starts");
    events.push("second ends");
  });
  await Promise.all([first, second]);
  assert.deepEqual(events, ["first starts", "first ends", "second starts", "second ends"]);
});

test("serialized hands each task's result to its caller", async () => {
  const run = serialized();
  const [a, b] = await Promise.all([run(async () => 1), run(async () => "two")]);
  assert.equal(a, 1);
  assert.equal(b, "two");
});

test("serialized runs the next task after one that failed, and reports the failure to its own caller", async () => {
  const run = serialized();
  const failing = run(async () => {
    throw new Error("first failed");
  });
  const next = run(async () => "next ran");
  await assert.rejects(failing, /first failed/);
  assert.equal(await next, "next ran");
});
