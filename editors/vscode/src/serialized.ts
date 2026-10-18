// PURE module - zero `vscode` import, so serialized.test.ts runs in a plain
// node process.

/**
 * Returns a function that runs the tasks it is given one after another, in
 * the order they were asked for, each starting when the one before it has
 * settled whether it resolved or failed. A language client is stopped and
 * started by awaiting, so two restarts asked for in quick succession
 * (toggling `enable` twice, say) would otherwise interleave and leave two
 * clients behind.
 */
export function serialized(): <T>(task: () => Promise<T>) => Promise<T> {
  let tail: Promise<unknown> = Promise.resolve();
  return <T>(task: () => Promise<T>): Promise<T> => {
    const result = tail.then(task);
    tail = result.catch(() => undefined);
    return result;
  };
}
