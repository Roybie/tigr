// Playground runtime glue.
//
// Owns all behavior: drives the VM worker, renders results, wires the
// tabs and controls. It binds to the DOM by the `id`s in
// DESIGN_CONTRACT.md and adds no styling of its own — the visual design
// is authored separately and supplies the markup these hooks live on.

import { EXAMPLES } from './examples.js';
import { createEditor } from './editor.js';

// The CodeMirror editor instance — created in main() once #editor is
// bound. Holds the editor-tab program source.
let editor = null;

// --- VM worker manager ----------------------------------------------

// Wraps the Web Worker that hosts the wasm VM. `kill()` terminates a
// runaway run and spawns a fresh worker (which resets the REPL session).
class VM {
  constructor() {
    this.pending = new Map();
    this.nextId = 1;
    this.spawn();
  }

  spawn() {
    this.worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
    this.ready = new Promise((resolve) => { this._markReady = resolve; });
    this.worker.onmessage = (e) => {
      const msg = e.data;
      if (msg.kind === 'ready') { this._markReady(msg); return; }
      const resolve = this.pending.get(msg.id);
      if (resolve) { this.pending.delete(msg.id); resolve(msg); }
    };
  }

  async send(payload) {
    await this.ready;
    const id = this.nextId++;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.worker.postMessage({ id, ...payload });
    });
  }

  // Terminate a stuck run and start over with a clean worker.
  kill() {
    this.worker.terminate();
    for (const resolve of this.pending.values()) {
      resolve({ ok: false, incomplete: false, value: '', output: '', error: '(stopped)' });
    }
    this.pending.clear();
    this.spawn();
  }
}

// --- DOM helpers -----------------------------------------------------

const ui = {};
const HOOKS = [
  'tab-repl', 'tab-editor', 'panel-repl', 'panel-editor',
  'repl-scrollback', 'repl-prompt', 'repl-input', 'repl-reset',
  'editor', 'editor-run', 'editor-run-kbd', 'editor-stop', 'editor-output', 'editor-meta', 'editor-exit', 'editor-examples', 'editor-file',
  'status',
];

function bindUI() {
  for (const id of HOOKS) {
    const node = document.getElementById(id);
    if (!node) console.warn(`playground: missing hook #${id} (see DESIGN_CONTRACT.md)`);
    ui[id.replace(/-/g, '_')] = node;
  }
}

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

function setStatus(text) {
  if (ui.status) ui.status.textContent = text;
}

// Mirror the visual-viewport height into `--vvh`. On mobile this shrinks
// when the on-screen keyboard opens; the REPL panel's CSS height reads
// it so the input row stays above the keyboard. On desktop it just
// tracks the window height, so the var is harmless there.
function syncViewportHeight() {
  const vv = window.visualViewport;
  const h = vv ? vv.height : window.innerHeight;
  document.documentElement.style.setProperty('--vvh', `${h}px`);
}

// The editor pane's filename meta — mirrors the selected example, the
// same way the Output pane's meta line carries the run time.
function setEditorFile(key) {
  if (ui.editor_file) ui.editor_file.textContent = `${key}.tg`;
}

// Remove the design's placeholder `.entry` blocks, leaving any sibling
// chrome (the REPL banner) in place.
function clearDemoEntries(container) {
  if (!container) return;
  container.querySelectorAll('.entry').forEach((e) => e.remove());
}

// Build one result block — the `.entry-*` classes the design styles.
// Output and error are `<pre>` (pre-formatted: error carries a
// caret-underlined source span); input and value are `<div>`.
function renderResult(target, result) {
  const out = (result.output || '').replace(/\n$/, '');
  if (out) target.appendChild(el('pre', 'entry-output', out));
  if (result.ok) {
    target.appendChild(el('div', 'entry-value', result.value));
  } else {
    target.appendChild(el('pre', 'entry-error', result.error));
  }
}

// --- Scratch sandbox -------------------------------------------------

// The "Yours" example slot (data-value="scratch") is a persistent
// sandbox: unlike the bundled examples its contents live in
// localStorage, and the editor autosaves to it while it is selected.
const SCRATCH_KEY = 'tigr.playground.scratch';
const SCRATCH_PLACEHOLDER =
  '// scratch: your sandbox.\n// nothing here yet. type away.\n';

// True while the scratch slot is the selected example — gates autosave
// so loading a bundled example never overwrites the saved sandbox.
let scratchActive = false;
let scratchSaveTimer = null;

// Saved sandbox text, or the placeholder when nothing is stored yet. An
// empty string counts as stored (the user cleared it on purpose).
function loadScratch() {
  try {
    const saved = localStorage.getItem(SCRATCH_KEY);
    return saved !== null ? saved : SCRATCH_PLACEHOLDER;
  } catch {
    return SCRATCH_PLACEHOLDER;
  }
}

function saveScratch(text) {
  try {
    localStorage.setItem(SCRATCH_KEY, text);
  } catch {
    // Storage full or disabled (private mode) — losing autosave is not
    // fatal, so swallow it rather than interrupt editing.
  }
}

// Debounced autosave wired to the editor's onChange. Re-checks
// scratchActive when the timer fires: a pending save must not land
// after the user has switched to a bundled example.
function onEditorChange() {
  if (!scratchActive) return;
  clearTimeout(scratchSaveTimer);
  scratchSaveTimer = setTimeout(() => {
    if (scratchActive) saveScratch(editor.getValue());
  }, 400);
}

// --- Tabs ------------------------------------------------------------

function activateTab(which) {
  const isRepl = which === 'repl';
  ui.tab_repl?.classList.toggle('is-active', isRepl);
  ui.tab_editor?.classList.toggle('is-active', !isRepl);
  ui.tab_repl?.setAttribute('aria-selected', String(isRepl));
  ui.tab_editor?.setAttribute('aria-selected', String(!isRepl));
  ui.panel_repl?.classList.toggle('is-active', isRepl);
  ui.panel_editor?.classList.toggle('is-active', !isRepl);
  // CSS keys the REPL-only keyboard-aware height off this attribute.
  document.body.dataset.tab = which;
  if (isRepl) ui.repl_input?.focus();
}

// --- REPL console ----------------------------------------------------

let replBuffer = '';        // accumulates lines while input is unfinished
const replHistory = [];     // submitted entries, for ↑ / ↓ recall
let replHistoryAt = 0;      // cursor into replHistory; == length means "new line"

function replPrompt() {
  if (ui.repl_prompt) ui.repl_prompt.textContent = replBuffer ? '..>' : 'tigr>';
}

// Grow the REPL input to fit its content (Shift+Enter adds lines) up to
// the CSS max-height, past which it scrolls. Reset height first so it
// shrinks back when lines are removed or the input is cleared.
function autoGrowReplInput() {
  const el = ui.repl_input;
  if (!el) return;
  el.style.height = 'auto';
  el.style.height = `${el.scrollHeight}px`;
}

async function submitReplLine(vm) {
  const line = ui.repl_input.value;
  ui.repl_input.value = '';
  autoGrowReplInput();
  replBuffer += line + '\n';

  setStatus('running…');
  const result = await vm.send({ kind: 'eval', source: replBuffer });
  setStatus('ready');

  // Unfinished input: keep the buffer, stay on the continuation prompt.
  if (!result.ok && result.incomplete) {
    replPrompt();
    return;
  }

  const entry = el('div', 'entry');
  entry.appendChild(el('div', 'entry-input', `tigr> ${replBuffer.replace(/\n$/, '')}`));
  renderResult(entry, result);
  ui.repl_scrollback.appendChild(entry);
  ui.repl_scrollback.scrollTop = ui.repl_scrollback.scrollHeight;

  replHistory.push(replBuffer.replace(/\n$/, ''));
  replHistoryAt = replHistory.length;
  replBuffer = '';
  replPrompt();
}

// ↑ / ↓ walk the submission history — but only when the input is a
// single line, so arrow keys still move the caret in multi-line edits.
function recallHistory(dir) {
  if (ui.repl_input.value.includes('\n')) return false;
  const next = replHistoryAt + dir;
  if (next < 0 || next > replHistory.length) return false;
  replHistoryAt = next;
  ui.repl_input.value = next === replHistory.length ? '' : replHistory[next];
  autoGrowReplInput();
  return true;
}

function resetRepl(vm) {
  vm.send({ kind: 'reset' });
  replBuffer = '';
  clearDemoEntries(ui.repl_scrollback);
  replPrompt();
  setStatus('ready');
  ui.repl_input?.focus();
}

// --- Editor ----------------------------------------------------------

// The runtime the worker measured around the VM call, as the output
// pane's meta line. Sub-millisecond clock resolution is clamped on
// non-isolated origins, so a too-fast run reads "<1 ms" rather than "0".
function formatRuntime(ms) {
  if (typeof ms !== 'number' || !isFinite(ms)) return '';
  if (ms < 1) return 'finished in <1 ms';
  return `finished in ${ms < 10 ? ms.toFixed(1) : Math.round(ms)} ms`;
}

// The output pane's exit pill. The VM has no real exit codes, so this
// maps the run's success flag onto the conventional 0 / 1 — and turns
// red (.is-error) on a raised error.
function setExitPill(ok) {
  const pill = ui.editor_exit;
  if (!pill) return;
  pill.hidden = false;
  pill.classList.toggle('is-error', !ok);
  pill.setAttribute('aria-label', ok ? 'exit code 0' : 'exit code 1');
  const label = pill.querySelector('.exit-pill-label');
  if (label) label.textContent = ok ? 'exit 0' : 'exit 1';
}

async function runProgram(vm) {
  clearDemoEntries(ui.editor_output);
  if (ui.editor_output) ui.editor_output.textContent = '';
  setStatus('running…');
  const result = await vm.send({ kind: 'run', source: editor.getValue() });
  setStatus('ready');
  if (ui.editor_meta) ui.editor_meta.textContent = formatRuntime(result.ms);
  setExitPill(result.ok);
  if (ui.editor_output) {
    const entry = el('div', 'entry');
    renderResult(entry, result);
    ui.editor_output.appendChild(entry);
  }
}

function stopProgram(vm) {
  vm.kill();
  setStatus('stopped — session reset');
  replBuffer = '';
  replPrompt();
}

// The design's examples list is a <nav> of .example-item buttons, each
// with a data-value key into EXAMPLES. Delegate clicks on the nav,
// mirror selection by toggling .is-active (same convention as tabs).
// data-value="scratch" is special — it loads the localStorage sandbox
// rather than a bundled example.
function loadExamples() {
  if (!ui.editor_examples) return;
  ui.editor_examples.addEventListener('click', (e) => {
    const item = e.target.closest('.example-item');
    if (!item || !ui.editor_examples.contains(item)) return;
    const key = item.dataset.value;
    const isScratch = key === 'scratch';
    const src = isScratch ? loadScratch() : EXAMPLES[key];
    if (src === undefined) {
      console.warn(`playground: no example for data-value="${key}"`);
      return;
    }
    ui.editor_examples.querySelectorAll('.example-item.is-active')
      .forEach((n) => n.classList.remove('is-active'));
    item.classList.add('is-active');
    // Drop any pending scratch save before swapping the document, then
    // flip the flag so the setValue below does not autosave the new
    // content over the saved sandbox.
    clearTimeout(scratchSaveTimer);
    scratchActive = isScratch;
    editor.setValue(src);
    setEditorFile(key);
    // On a phone the sidebar overlays the editor — collapse it once a
    // selection is made so the editor is visible again.
    if (isNarrowViewport()) collapseSidebar();
  });
}

// On phones the examples sidebar is an overlay rather than a column.
function isNarrowViewport() {
  return window.matchMedia('(max-width: 720px)').matches;
}
function collapseSidebar() {
  const toggle = document.getElementById('sidebar-toggle');
  if (toggle) toggle.checked = true;
}

// --- Pane splitter ---------------------------------------------------

// Drag the bar between the editor and output panes to re-divide them.
// The `.editor-column` grid carries the two panes plus a 6px splitter
// row; dragging rewrites its `grid-template-rows`.
function setupPaneSplitter() {
  const column = document.querySelector('.editor-column');
  const splitter = document.getElementById('pane-splitter');
  if (!column || !splitter) return;
  const MIN = 90;          // px — neither pane may shrink below this
  let dragging = false;

  splitter.addEventListener('pointerdown', (e) => {
    dragging = true;
    splitter.classList.add('is-dragging');
    splitter.setPointerCapture(e.pointerId);
    document.body.style.userSelect = 'none';
    e.preventDefault();
  });
  splitter.addEventListener('pointermove', (e) => {
    if (!dragging) return;
    const rect = column.getBoundingClientRect();
    // The splitter row is taller on touch layouts — measure it rather
    // than assume the desktop 6px.
    const bar = splitter.offsetHeight || 6;
    const top = Math.max(MIN, Math.min(e.clientY - rect.top, rect.height - MIN - bar));
    column.style.gridTemplateRows = `${top}px ${bar}px 1fr`;
  });
  const end = () => {
    if (!dragging) return;
    dragging = false;
    splitter.classList.remove('is-dragging');
    document.body.style.userSelect = '';
  };
  splitter.addEventListener('pointerup', end);
  splitter.addEventListener('pointercancel', end);
}

// --- Wire-up ---------------------------------------------------------

function main() {
  bindUI();
  const vm = new VM();

  // Keep `--vvh` current and mark the starting tab so the keyboard-aware
  // REPL height has something to read from the first paint.
  document.body.dataset.tab = 'editor';
  syncViewportHeight();
  window.visualViewport?.addEventListener('resize', syncViewportHeight);
  window.visualViewport?.addEventListener('scroll', syncViewportHeight);

  // On a phone the sidebar overlays the editor — start it collapsed so
  // the playground opens onto the editor, not the examples list.
  if (isNarrowViewport()) collapseSidebar();

  // Tabs.
  ui.tab_repl?.addEventListener('click', () => activateTab('repl'));
  ui.tab_editor?.addEventListener('click', () => activateTab('editor'));

  // REPL: Enter submits, Shift+Enter inserts a newline, ↑/↓ recall.
  ui.repl_input?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submitReplLine(vm);
    } else if (e.key === 'ArrowUp') {
      if (recallHistory(-1)) e.preventDefault();
    } else if (e.key === 'ArrowDown') {
      if (recallHistory(1)) e.preventDefault();
    }
  });
  ui.repl_input?.addEventListener('input', autoGrowReplInput);
  ui.repl_reset?.addEventListener('click', () => resetRepl(vm));
  clearDemoEntries(ui.repl_scrollback);
  replPrompt();

  // Editor: CodeMirror mounts into #editor, seeded from the example
  // marked .is-active in the design markup. That is the scratch slot by
  // default, so the playground opens onto the saved sandbox.
  // ⌘/Ctrl+Enter runs.
  if (ui.editor) {
    const active = ui.editor_examples?.querySelector('.example-item.is-active');
    const key = active?.dataset.value;
    scratchActive = key === 'scratch';
    const seed = scratchActive ? loadScratch() : (EXAMPLES[key] ?? EXAMPLES.expressions);
    setEditorFile(scratchActive ? 'scratch' : (EXAMPLES[key] ? key : 'expressions'));
    editor = createEditor(ui.editor, seed, () => runProgram(vm), onEditorChange);
  }
  ui.editor_run?.addEventListener('click', () => runProgram(vm));
  // CodeMirror's Mod-Enter binding is ⌘ on macOS, Ctrl elsewhere — make
  // the Run button's key hint match the platform rather than assume Mac.
  if (ui.editor_run_kbd) {
    const isMac = /Mac|iP(hone|od|ad)/.test(navigator.platform);
    ui.editor_run_kbd.textContent = isMac ? '⌘↵' : 'Ctrl↵';
  }
  ui.editor_stop?.addEventListener('click', () => stopProgram(vm));
  clearDemoEntries(ui.editor_output);
  // Drop the design's placeholder "finished in 12 ms" until a real run.
  if (ui.editor_meta) ui.editor_meta.textContent = '';
  loadExamples();
  setupPaneSplitter();

  setStatus('loading…');
  vm.ready.then((msg) => {
    setStatus(msg && msg.error ? `load failed: ${msg.error}` : 'ready');
  });
}

main();
