// CodeMirror 6 editor for the playground — syntax highlighting, line
// numbers, and tigr-aware autocomplete.
//
// Loaded buildless from esm.sh. Versions are pinned exactly and every
// import carries a matching `?deps=` so esm.sh rewrites each package's
// shared peers (`@codemirror/state`, `@codemirror/view`,
// `@codemirror/language`, `@lezer/highlight`) to one build. That single
// instance is essential: two copies of `@codemirror/state` make
// CodeMirror reject extensions, and two `@lezer/highlight` copies give
// `tags.*` distinct identities so highlighting silently does nothing.

// NB: `codemirror@6` must be pinned exactly — the npm `codemirror`
// package has a stray `6.65.7` publish that is actually CodeMirror 5,
// so a `@6` range resolves to it. `6.0.2` is the real CM6 meta package.
import { EditorView, basicSetup }
  from 'https://esm.sh/codemirror@6.0.2?deps=@codemirror/state@6.6.0,@codemirror/view@6.43.0,@codemirror/language@6.12.3,@lezer/highlight@1.2.3';
import { keymap }
  from 'https://esm.sh/@codemirror/view@6.43.0?deps=@codemirror/state@6.6.0';
import { Prec }
  from 'https://esm.sh/@codemirror/state@6.6.0';
import { indentWithTab }
  from 'https://esm.sh/@codemirror/commands@6.10.3?deps=@codemirror/state@6.6.0,@codemirror/view@6.43.0';
import { StreamLanguage, LanguageSupport, HighlightStyle, syntaxHighlighting, indentUnit }
  from 'https://esm.sh/@codemirror/language@6.12.3?deps=@codemirror/state@6.6.0,@codemirror/view@6.43.0,@lezer/highlight@1.2.3';
import { linter }
  from 'https://esm.sh/@codemirror/lint@6.8.5?deps=@codemirror/state@6.6.0,@codemirror/view@6.43.0';
import { tags as t }
  from 'https://esm.sh/@lezer/highlight@1.2.3';

// --- tigr language ---------------------------------------------------

const KEYWORDS = new Set([
  'fn', 'if', 'else', 'for', 'while', 'break', 'continue', 'return',
  'import', 'try', 'catch', 'raise', 'match', 'spawn', 'select',
  'parallel', 'go', 'yield', 'gen',
]);
const ATOMS = new Set(['true', 'false', 'null']);

// A StreamLanguage tokenizer — good enough for highlighting. It mirrors
// `src/vm/lexer.rs`: `// …` and `/* … */` comments, `'…'` interpolating
// and `"…"` raw strings, `0x/0o/0b` and decimal numbers.
const tigrStream = StreamLanguage.define({
  name: 'tigr',
  startState: () => ({ block: false }),
  token(stream, state) {
    if (state.block) {
      while (!stream.eol()) {
        if (stream.match('*/')) { state.block = false; break; }
        stream.next();
      }
      return 'comment';
    }
    if (stream.eatSpace()) return null;

    if (stream.match('//')) { stream.skipToEnd(); return 'comment'; }
    if (stream.match('/*')) {
      state.block = true;
      while (!stream.eol()) {
        if (stream.match('*/')) { state.block = false; break; }
        stream.next();
      }
      return 'comment';
    }

    const ch = stream.peek();

    // Strings: '…' (escapes + {interpolation}) and "…" (raw).
    if (ch === "'" || ch === '"') {
      const raw = ch === '"';
      stream.next();
      let c;
      while ((c = stream.next()) != null) {
        if (c === ch) break;
        if (!raw && c === '\\') stream.next();
      }
      return 'string';
    }

    // `.` — a range/spread operator (`..`, `..=`, `...`), a leading-dot
    // float (`.5`), or member-access punctuation. The dot run must be
    // resolved here: tokenizing `1..101` one dot at a time strands the
    // stream on the second `.`, where the digit-anchored number regex
    // matches nothing — and a non-advancing token() crashes CodeMirror.
    if (ch === '.') {
      if (stream.match(/^\.\.\.|^\.\.=?/)) return 'operator';
      if (/\d/.test(stream.string[stream.pos + 1] || '')) {
        stream.match(/^\.[\d_]+([eE][+-]?\d+)?/);
        return 'number';
      }
      stream.next();
      return 'punctuation';
    }

    // Numbers: radix or decimal, with `_` group separators.
    if (/\d/.test(ch)) {
      if (stream.match(/^0[xXoObB][0-9a-fA-F_]+/)) return 'number';
      stream.match(/^\d[\d_]*(\.[\d_]+)?([eE][+-]?\d+)?/);
      return 'number';
    }

    // Identifiers / keywords.
    if (/[A-Za-z_]/.test(ch)) {
      const word = stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)[0];
      if (KEYWORDS.has(word)) return 'keyword';
      if (ATOMS.has(word)) return 'atom';
      return 'variableName';
    }

    if ('+-*/%<>=!&|^~?:'.includes(ch)) { stream.next(); return 'operator'; }
    if ('()[]{},;.'.includes(ch)) { stream.next(); return 'punctuation'; }

    stream.next();
    return null;
  },
  languageData: {
    commentTokens: { line: '//', block: { open: '/*', close: '*/' } },
    autocomplete: tigrCompletions,
  },
  // Map the token strings returned above directly to highlight tags,
  // rather than relying on StreamLanguage's classic-name fallback
  // (which has no `punctuation` / `variableName` entry).
  tokenTable: {
    comment: t.comment,
    string: t.string,
    number: t.number,
    keyword: t.keyword,
    atom: t.atom,
    variableName: t.variableName,
    operator: t.operator,
    punctuation: t.punctuation,
  },
});

// --- completion ------------------------------------------------------
//
// The stdlib catalog (module members, builtins) arrives from the wasm
// build once it loads (app.js calls `setCatalog`), so suggestions match
// what the language server gives an editor. Until then, completion falls
// back to keywords/atoms plus a static builtin/module list.

let CATALOG = null;

/** Receive the stdlib catalog (see wasm `catalog_json`). */
export function setCatalog(cat) {
  CATALOG = cat;
}

// Static fallback used before the catalog loads.
const FALLBACK_BUILTINS = ['print', 'str', 'num', 'int', 'float', 'bool',
  'floor', 'ceil', 'rand', 'type', 'gc', 'join'];
const FALLBACK_MODULES = ['Iter', 'Array', 'Map', 'Set', 'String', 'Math',
  'Object', 'LocalChannel', 'Test', 'JSON', 'Random', 'Bytes', 'BigInt',
  'Path', 'Time', 'DateTime'];

// CodeMirror completion source. Two modes:
//   * after `Receiver.` — offer that module's members (the headline case);
//   * otherwise — keywords, atoms, module names, and builtins.
function tigrCompletions(context) {
  // Member access: `<receiver>.<partial>`, partial possibly empty.
  const dotted = context.matchBefore(/[A-Za-z_][A-Za-z0-9_]*\.[A-Za-z0-9_]*/);
  if (dotted) {
    const dot = dotted.text.lastIndexOf('.');
    const receiver = dotted.text.slice(0, dot);
    const mod = CATALOG && CATALOG.modules[receiver];
    if (!mod) return null; // unknown receiver — no noise
    return {
      from: dotted.from + dot + 1,
      options: mod.members.map((m) => ({
        label: m.name,
        type: m.constant ? 'constant' : 'function',
        detail: m.signature,
        info: m.doc || undefined,
      })),
    };
  }

  // Identifier position.
  const word = context.matchBefore(/[A-Za-z_][A-Za-z0-9_]*/);
  if (!word && !context.explicit) return null;
  return { from: word ? word.from : context.pos, options: identifierOptions() };
}

function identifierOptions() {
  const out = [];
  for (const k of KEYWORDS) out.push({ label: k, type: 'keyword' });
  for (const a of ATOMS) out.push({ label: a, type: 'constant' });
  if (CATALOG) {
    for (const name of Object.keys(CATALOG.modules)) out.push({ label: name, type: 'class' });
    for (const b of CATALOG.builtins) {
      out.push({ label: b.name, type: 'function', detail: b.signature, info: b.doc || undefined });
    }
  } else {
    for (const b of FALLBACK_BUILTINS) out.push({ label: b, type: 'function' });
    for (const m of FALLBACK_MODULES) out.push({ label: m, type: 'class' });
  }
  return out;
}

// --- theme -----------------------------------------------------------
//
// Colors are the design's CSS custom properties (styles.css), so the
// editor tracks the page's light / dark palette automatically.

const tigrHighlight = HighlightStyle.define([
  { tag: t.keyword, color: 'var(--accent-ink)', fontWeight: '500' },
  { tag: t.atom, color: 'var(--accent-ink)' },
  { tag: t.string, color: 'var(--ok)' },
  { tag: t.number, color: 'var(--accent-ink)' },
  { tag: t.comment, color: 'var(--ink-3)', fontStyle: 'italic' },
  { tag: t.operator, color: 'var(--ink-2)' },
  { tag: t.punctuation, color: 'var(--ink-3)' },
  { tag: t.variableName, color: 'var(--ink)' },
]);

const tigrTheme = EditorView.theme({
  '&': { color: 'var(--ink)', backgroundColor: 'transparent', height: '100%' },
  '.cm-scroller': {
    fontFamily: 'var(--font-mono)', fontSize: '13.5px', lineHeight: '1.65',
  },
  '.cm-content': { caretColor: 'var(--accent)' },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--accent)' },
  '.cm-gutters': {
    backgroundColor: 'transparent', color: 'var(--ink-4)', border: 'none',
  },
  '.cm-activeLine': {
    backgroundColor: 'color-mix(in oklch, var(--accent) 7%, transparent)',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'transparent', color: 'var(--ink-2)',
  },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection': {
    backgroundColor: 'color-mix(in oklch, var(--accent) 26%, transparent)',
  },
  '&.cm-focused': { outline: 'none' },
}, { dark: window.matchMedia('(prefers-color-scheme: dark)').matches });

// --- public API ------------------------------------------------------

// Mount a CodeMirror editor into `parent`. `onRun` fires on Mod-Enter;
// `onChange` fires after any edit to the document. `lint`, if given, is an
// async `(sourceText) => [{ from, to, message }]` (the worker's static
// check) that drives inline error squiggles.
// Returns a small handle so app.js need not know CodeMirror internals.
export function createEditor(parent, doc, onRun, onChange, lint) {
  // `linter` re-checks on a debounce after edits. Offsets come back in the
  // same UTF-16 units CodeMirror uses, but the doc may have changed while
  // the worker was busy, so clamp every range into the current document.
  const tigrLinter = lint
    ? [linter(async (view) => {
        const diags = await lint(view.state.doc.toString());
        const len = view.state.doc.length;
        return diags.map((d) => {
          const from = Math.max(0, Math.min(d.from, len));
          const to = Math.max(from, Math.min(d.to, len));
          return { from, to, severity: 'error', source: 'tigr', message: d.message };
        });
      }, { delay: 400 })]
    : [];

  const view = new EditorView({
    parent,
    doc,
    extensions: [
      basicSetup,
      new LanguageSupport(tigrStream),
      syntaxHighlighting(tigrHighlight),
      tigrTheme,
      indentUnit.of('  '),
      ...tigrLinter,
      // Prec.highest so this Mod-Enter binding beats basicSetup's
      // default keymap, which already maps Mod-Enter to `insertBlankLine`
      // (without this it loses the precedence race and just adds a line).
      Prec.highest(keymap.of([
        indentWithTab,
        { key: 'Mod-Enter', preventDefault: true, run: () => { onRun?.(); return true; } },
      ])),
      EditorView.updateListener.of((u) => { if (u.docChanged) onChange?.(); }),
    ],
  });

  return {
    getValue: () => view.state.doc.toString(),
    setValue: (text) => view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
    }),
    focus: () => view.focus(),
  };
}
