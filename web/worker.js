// Web Worker that hosts the tigr VM (WebAssembly).
//
// Running the VM here, off the main thread, keeps the page responsive
// and — crucially — lets the page kill a runaway program with
// `worker.terminate()`. The VM has no instruction budget, so an
// infinite loop is only escapable by terminating this worker; `app.js`
// does exactly that and spawns a fresh one.
//
// Protocol:
//   main -> worker : { id, kind: 'eval' | 'run', source }
//                    { id, kind: 'reset' }
//   worker -> main : { kind: 'ready', version }
//                    { id, ok, incomplete, value, output, error, ms }

import init, { WasmRepl, run_program, version } from './pkg/tigr.js';
import { VERSION } from './pkg/meta.js';

let repl = null;

// Load the wasm module, then announce readiness. A persistent REPL
// session backs the console tab; the editor tab uses `run_program`,
// which spins up its own throwaway session per run.
//
// The `?v=VERSION` query cache-busts the wasm: GitHub Pages serves it
// through a CDN that caches each URL independently, so after a release a
// browser could fetch the new JS glue against a stale `tigr_bg.wasm` and
// hit "wasm.<export> is not a function". A version-stamped URL the cache
// has never seen forces a fresh fetch. `import.meta.url` is the worker's
// own location (web/), so the relative path resolves to web/pkg/.
init(new URL(`./pkg/tigr_bg.wasm?v=${VERSION}`, import.meta.url))
  .then(() => {
    repl = new WasmRepl();
    self.postMessage({ kind: 'ready', version: version() });
  })
  .catch((err) => {
    self.postMessage({ kind: 'ready', error: String(err) });
  });

self.onmessage = (e) => {
  const msg = e.data;

  if (msg.kind === 'reset') {
    repl = new WasmRepl();
    self.postMessage({ id: msg.id, ok: true, incomplete: false, value: '', output: '', error: '' });
    return;
  }

  let res;
  // Time the VM call itself — compile + run — so `ms` is execution time,
  // not the postMessage round-trip the main thread would otherwise see.
  const t0 = performance.now();
  try {
    const r = msg.kind === 'run' ? run_program(msg.source) : repl.eval(msg.source);
    // Copy every field out before freeing the wasm-owned struct.
    res = {
      id: msg.id,
      ok: r.ok,
      incomplete: r.incomplete,
      value: r.value,
      output: r.output,
      error: r.error,
      ms: performance.now() - t0,
    };
    r.free();
  } catch (err) {
    // A panic in the VM traps the wasm instance; surface it rather
    // than leaving the request unanswered.
    res = { id: msg.id, ok: false, incomplete: false, value: '', output: '', error: String(err), ms: performance.now() - t0 };
  }
  self.postMessage(res);
};
