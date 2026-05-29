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
//                    { id, kind: 'lint', source }
//                    { id, kind: 'reset' }
//   worker -> main : { kind: 'ready', version, catalog }
//                    { id, ok, incomplete, value, output, error, ms }
//                    { id, diagnostics }   (reply to 'lint')

import init, { WasmRepl, run_program, version, catalog_json, diagnostics } from './pkg/tigr.js';
import { BUILD_ID } from './pkg/meta.js';

let repl = null;

// Load the wasm module, then announce readiness. A persistent REPL
// session backs the console tab; the editor tab uses `run_program`,
// which spins up its own throwaway session per run.
//
// The `?v=BUILD_ID` query cache-busts the wasm: GitHub Pages serves it
// through a CDN that caches each URL independently, so a browser could
// fetch the new JS glue against a stale `tigr_bg.wasm` and hit
// "wasm.<export> is not a function". BUILD_ID embeds a checksum of the
// wasm (see build.sh), so any rebuild gives a URL the cache has never
// seen and forces a fresh fetch. `import.meta.url` is the worker's own
// location (web/), so the relative path resolves to web/pkg/.
init(new URL(`./pkg/tigr_bg.wasm?v=${BUILD_ID}`, import.meta.url))
  .then(() => {
    repl = new WasmRepl();
    // The catalog is static (it doesn't depend on user code), so ship it
    // once now; the main thread builds completions from it synchronously,
    // with no per-keystroke round-trip.
    self.postMessage({
      kind: 'ready',
      version: version(),
      catalog: JSON.parse(catalog_json()),
    });
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

  // Static-check the source (no execution) and return diagnostics for the
  // editor's linter. Never throws into the request: a parse hiccup just
  // yields no diagnostics.
  if (msg.kind === 'lint') {
    let diags = [];
    try {
      diags = JSON.parse(diagnostics(msg.source));
    } catch (_err) {
      diags = [];
    }
    self.postMessage({ id: msg.id, diagnostics: diags });
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
