//! Embedding API: drive a persistent tigr VM from a Rust host.
//!
//! [`Session`] is to a *whole program* what [`crate::repl::Repl`] is to
//! an incremental line: a single long-lived VM frame (`start_repl`'s
//! persistent frame-0 wall) holds all top-level bindings, and the host
//! calls into them — `update(dt)`, `draw()`, etc. — frame after frame
//! via [`Session::call`]. The VM instance persists across calls, so
//! game state is just tigr variables.
//!
//! Typical host loop:
//! ```ignore
//! use tigr::embed::*;
//!
//! let mut s = Session::new();
//! s.register_module("Gfx", object(&[
//!     ("rect", native("rect", Arity::Exact(4), gfx_rect)),
//! ]));
//! s.load(game_source).map_err(|e| e.render(&s.sources()))?;
//! loop {
//!     s.call("update", vec![Value::Float(dt)])?;
//!     s.call("draw", vec![])?;
//! }
//! ```
//!
//! The whole surface an embedder needs is re-exported below, so
//! `use tigr::embed::*` is enough — no reaching into `crate::vm::*`.

use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::vm::compiler::Compiler;
use crate::vm::fold;
use crate::vm::lexer::Lexer;
use crate::vm::parser;
use crate::vm::source_map::SourceMap;
use crate::vm::value::Closure;

// Re-exports: everything an embedder needs to build native modules,
// pass/inspect values, and handle errors — so `use tigr::embed::*` is
// enough, with no reaching into `crate::vm`. `Error` (lex/parse/
// compile/runtime) is what `load` returns; `RuntimeError` is what
// `call` returns.
pub use crate::vm::error::{Error, RuntimeError, RuntimeErrorKind};
pub use crate::vm::native_modules::{
    bytes, native, native_blocking, native_frame_wait, native_socket, object,
};
pub use crate::vm::value::{Arity, NativeFn, NativeKind, Value};
pub use crate::vm::vm::Vm;

/// Tigr's shared seeded PRNG, re-exported so a native host module can
/// draw from the *same* stream as the `rand()` builtin and the `Random`
/// module — the stream [`Session::seed_rng`] pins. Drawing from here
/// (rather than a private host generator) keeps a native module's
/// randomness part of the one replayable run. See [`crate::vm::rng`].
pub mod rng {
    pub use crate::vm::rng::{next_below, next_f64, next_u64};
}

/// A persistent tigr program a host can load once and call into many
/// times. See the module docs for the model and a host-loop sketch.
pub struct Session {
    vm: Vm,
    /// Names + slots of every top-level binding `load` has committed,
    /// in declaration order. Mirrors the REPL's `locals`: the compiler
    /// pre-declares these so a later `load` resolves prior names to the
    /// right `LoadLocal` slot, and `binding` maps a name back to a slot.
    bindings: Vec<(String, u8)>,
    /// Shared with the VM so import-time source registration lands here
    /// too; the host borrows it via [`sources`](Session::sources) to
    /// render caret diagnostics for a failed `load`.
    sources: Rc<RefCell<SourceMap>>,
    /// Counts `load` calls, only to label each chunk's source entry.
    load_no: u32,
}

impl Session {
    /// Build an empty session with a persistent frame-0 wall, ready for
    /// [`register_module`](Session::register_module) and
    /// [`load`](Session::load).
    pub fn new() -> Self {
        let sources = Rc::new(RefCell::new(SourceMap::new()));
        let mut vm = Vm::with_source_map(sources.clone());
        vm.start_repl();
        Session { vm, bindings: Vec::new(), sources, load_no: 0 }
    }

    /// Register a host-provided native module under a bare `import`
    /// name. Call **before** [`load`](Session::load)ing any program
    /// that imports it. A host module can never shadow a built-in (core
    /// modules resolve first); see [`Vm::register_module`].
    pub fn register_module(&mut self, name: &str, module: Value) {
        self.vm.register_module(name, module);
    }

    /// Register a host-provided **pure-tigr source** module under a bare
    /// `import` name. `import '<name>'` compiles and evaluates `src` on
    /// first use and caches the result, resolving exactly as the
    /// built-in `Math` / `Array` modules do. Call **before**
    /// [`load`](Session::load)ing any program that imports it. A host
    /// module can never shadow a built-in; see
    /// [`Vm::register_source_module`]. Use this to ship framework
    /// helpers written in tigr — ones that must `wait` or close over
    /// callbacks, which a native module cannot express.
    pub fn register_source_module(&mut self, name: &str, src: &str) {
        self.vm.register_source_module(name, src);
    }

    /// Install a host resolver for *path* imports (`import './player'`,
    /// `import 'a/b'`). The resolver is handed the resolved, normalised,
    /// forward-slashed path and returns the module's source, or `None` for
    /// "not found" (an import error). Bare-name imports (the stdlib and the
    /// `register_*_module` names) never consult it. Use this to serve a
    /// game's sibling `.tg` files from a bundle, so an exported build
    /// resolves a multi-file game the same way a dev filesystem build does;
    /// see [`Vm::set_import_loader`]. Call before loading a program that
    /// imports a path.
    pub fn set_import_loader<F>(&mut self, loader: F)
    where
        F: Fn(&str) -> Option<String> + 'static,
    {
        self.vm.set_import_loader(loader);
    }

    /// Seed the pseudo-random stream that the `Random` module and the
    /// bare `rand()` builtin both draw from (see [`crate::vm::rng`]).
    /// This lets the *host* own the seed — record it at session start
    /// and re-inject the same value to reproduce a run — instead of
    /// relying on game code to call `Random.seed`. Any `u64` works
    /// (`0` included); from this point the stream is deterministic.
    pub fn seed_rng(&self, seed: u64) {
        crate::vm::rng::seed(seed);
    }

    /// Compile and run a whole top-level program against the persistent
    /// frame. Top-level functions and data become live frame-0 slots
    /// that survive across calls. May be invoked more than once; later
    /// loads see the bindings earlier ones declared (append-only — this
    /// is *not* hot-reload, which replaces; see `Session::reload`).
    ///
    /// On a lex/parse/compile/uncaught-runtime error the persistent
    /// frame is left intact (the VM truncates back to the pre-load
    /// snapshot) and no new bindings are committed. Render the returned
    /// [`Error`] against [`sources`](Session::sources).
    pub fn load(&mut self, source: &str) -> Result<(), Error> {
        let label = format!("<host:{}>", self.load_no + 1);
        self.load_labeled(label, source)
    }

    /// Like [`load`](Session::load), but the source is labelled `name`
    /// (e.g. the game's file path) so a rendered diagnostic points at it
    /// — `--> games/hello.tg:50:52` rather than an anonymous `<host:N>`.
    /// Use this when the host knows where the source came from; the
    /// label also rides on the chunk, so a later runtime error in a
    /// callback compiled from this source reports the same `name`.
    pub fn load_named(&mut self, name: &str, source: &str) -> Result<(), Error> {
        self.load_labeled(name.to_owned(), source)
    }

    /// Compile and run one chunk against the live session, returning the
    /// value it produced. The shared engine behind [`load`](Session::load),
    /// which discards the value, and [`eval_line`](Session::eval_line),
    /// which returns it for an interactive console to display. Every chunk
    /// is compiled against the persistent frame-0 bindings, so it resolves
    /// and mutates the same top-level state earlier chunks (and the running
    /// program) declared.
    fn eval_labeled(&mut self, label: String, source: &str) -> Result<Value, Error> {
        self.load_no += 1;
        // The label is the source's name (the game's file path for a host
        // load), so its directory is where this program's relative imports
        // resolve — the same base dir a file-loaded program would get.
        let base_dir = base_dir_from_label(&label);
        let sid = self.sources.borrow_mut().add(label, source);

        let tokens = Lexer::new(source).tokenize().map_err(|mut e| {
            e.source = sid;
            Error::from(e)
        })?;
        let mut program = parser::parse(tokens).map_err(|mut e| {
            e.source = sid;
            Error::from(e)
        })?;
        // Fold to match `run` semantics — the REPL skips this, but a
        // whole program is compiled exactly as `tigr run` would.
        fold::fold_program(&mut program);
        // Host-registered modules are ambient: the game can use them with
        // no `import`. Their global indices follow the built-ins + stdlib,
        // matching the VM's globals vec.
        let host_ambient = self.vm.ambient_host_names().to_vec();
        let (main, new_bindings) = Compiler::compile_repl_with_ambient(
            &program,
            &self.bindings,
            sid,
            &host_ambient,
            base_dir,
        )?;

        let closure = crate::vm::gc::alloc_closure(Closure {
            function: Arc::new(main),
            upvalues: Vec::new(),
        });

        // Stack length expected after a clean run: closure slot 0 plus
        // the existing committed bindings. On uncaught raise the VM
        // truncates to this, discarding the half-introduced bindings.
        let snapshot_len = 1 + self.bindings.len();

        match self.vm.run_repl_line(closure, snapshot_len) {
            Ok(value) => {
                self.bindings.extend(new_bindings);
                Ok(value)
            }
            Err(e) => Err(Error::Runtime(e)),
        }
    }

    fn load_labeled(&mut self, label: String, source: &str) -> Result<(), Error> {
        self.eval_labeled(label, source).map(|_| ())
    }

    /// Evaluate a single line against the live session and return its
    /// value, for an interactive host console (a REPL into a running
    /// program). Lines share the persistent frame-0 scope with the loaded
    /// program exactly as repeated [`load`](Session::load) calls do: a bare
    /// expression returns its value, `x := 1` adds a binding later lines and
    /// the running program can see, and `x = 8` mutates an existing global
    /// the program already reads. On a lex/parse/compile/uncaught-runtime
    /// error the frame is left intact and no binding is committed; render
    /// the returned [`Error`] against [`sources`](Session::sources).
    pub fn eval_line(&mut self, source: &str) -> Result<Value, Error> {
        let label = format!("<console:{}>", self.load_no + 1);
        self.eval_labeled(label, source)
    }

    /// Hot-reload: replace the whole program with `source`, preserving
    /// top-level *data* state (Tier-1). Unlike [`load`](Session::load),
    /// which appends, `reload` swaps the code wholesale — the model
    /// behind a "save the file, see the change, keep playing" workflow.
    ///
    /// 1. The new source is compiled **off to the side first**. On a
    ///    lex/parse/compile error the live VM is left completely
    ///    untouched and the rendered diagnostic is returned as `Err` —
    ///    non-fatal, so the host can show an error overlay and keep
    ///    running the last good program.
    /// 2. Top-level bindings are classified: functions / natives are
    ///    *code* (replaced), everything else is *data* (preserved).
    /// 3. Parked green threads are cancelled
    ///    ([`Vm::cancel_coroutines`]): their frames hold `ip`s into the
    ///    old bytecode and cannot migrate. The new `init`/`update`
    ///    re-spawns whatever sequences it needs.
    /// 4. The new top-level runs fresh, recreating every slot.
    /// 5. Data values whose name still exists (and is still data) are
    ///    carried into the new slots. A name that changed kind
    ///    (data↔fn) takes the new definition.
    /// 6. If the new program defines a callable `on_reload`, it is
    ///    invoked once (a state-shape migration hook).
    ///
    /// Caveat: only *compile* failures are non-fatal. A runtime error
    /// thrown while the new top-level executes (step 4) cannot be rolled
    /// back — the old program's coroutines are already cancelled — and
    /// is returned as `Err`. Tier-2 coexistence (old sequences finishing
    /// on old code) is deliberately not implemented; see the plan.
    pub fn reload(&mut self, source: &str) -> Result<(), String> {
        let label = format!("<reload:{}>", self.load_no + 1);
        self.reload_labeled(label, source)
    }

    /// Like [`reload`](Session::reload), but the source is labelled
    /// `name` (e.g. the game's file path) so the rendered compile
    /// diagnostic, and later runtime errors in the reloaded callbacks,
    /// point at the file rather than an anonymous `<reload:N>`.
    pub fn reload_named(&mut self, name: &str, source: &str) -> Result<(), String> {
        self.reload_labeled(name.to_owned(), source)
    }

    fn reload_labeled(&mut self, label: String, source: &str) -> Result<(), String> {
        self.load_no += 1;
        let base_dir = base_dir_from_label(&label);
        let sid = self.sources.borrow_mut().add(label, source);

        // 1. Compile off to the side, against *empty* prior bindings —
        //    this is a full replacement, so the new program's slots
        //    start fresh at slot 1. Any error leaves the VM untouched.
        let empty: Vec<(String, u8)> = Vec::new();
        let compiled: Result<_, Error> = (|| {
            let tokens = Lexer::new(source).tokenize().map_err(|mut e| {
                e.source = sid;
                Error::from(e)
            })?;
            let mut program = parser::parse(tokens).map_err(|mut e| {
                e.source = sid;
                Error::from(e)
            })?;
            fold::fold_program(&mut program);
            let host_ambient = self.vm.ambient_host_names().to_vec();
            let compiled = Compiler::compile_repl_with_ambient(
                &program, &empty, sid, &host_ambient, base_dir.clone(),
            )?;
            Ok(compiled)
        })();
        let (main, new_bindings) = match compiled {
            Ok(v) => v,
            Err(e) => return Err(e.render(&self.sources.borrow())),
        };

        // 2. Snapshot + classify the old top-level bindings (latest
        //    declaration wins for a redeclared name).
        let mut old: HashMap<String, (Value, bool)> = HashMap::new();
        for (name, slot) in &self.bindings {
            if let Some(v) = self.vm.stack_slot(*slot as usize) {
                let data = is_data(&v);
                old.insert(name.clone(), (v, data));
            }
        }
        // Keep the old data values alive across the new program's run:
        // once the stack is reset they are reachable only from `old`,
        // so the new program's allocations could otherwise collect them.
        let carry_roots: Vec<Value> = old
            .values()
            .filter(|(_, data)| *data)
            .map(|(v, _)| v.clone())
            .collect();
        self.vm.hold_reload_roots(carry_roots);

        // 3 + 4. Cancel parked coroutines, reset the persistent frame,
        //        and run the new top-level fresh.
        self.vm.cancel_coroutines();
        let closure = crate::vm::gc::alloc_closure(Closure {
            function: Arc::new(main),
            upvalues: Vec::new(),
        });
        self.vm.start_repl();
        if let Err(e) = self.vm.run_repl_line(closure, 1) {
            // The new top-level raised. State is already reset, so this
            // is fatal to the session — surface it rendered.
            self.vm.release_reload_roots();
            self.bindings = new_bindings;
            return Err(Error::Runtime(e).render(&self.sources.borrow()));
        }

        // 5. Carry data forward into the new slots.
        let new_map: HashMap<String, u8> =
            new_bindings.iter().cloned().collect();
        for (name, (old_val, data)) in &old {
            if !data {
                continue;
            }
            if let Some(&new_slot) = new_map.get(name) {
                // A name that the new program redefined as code takes
                // the new definition — don't clobber it with old data.
                let new_is_data = self
                    .vm
                    .stack_slot(new_slot as usize)
                    .map(|v| is_data(&v))
                    .unwrap_or(false);
                if new_is_data {
                    self.vm.set_stack_slot(new_slot as usize, old_val.clone());
                }
            }
        }
        self.vm.release_reload_roots();
        self.bindings = new_bindings;

        // 6. Optional state-shape migration hook.
        if self.has_callable("on_reload") {
            if let Some(cb) = self.binding("on_reload") {
                self.vm
                    .call_function(cb, Vec::new())
                    .map_err(|e| Error::Runtime(e).render(&self.sources.borrow()))?;
            }
        }
        Ok(())
    }

    /// Look up a top-level binding's current value by name. Returns
    /// `None` if no such binding was ever `load`ed. When a name was
    /// (re)declared more than once, the latest declaration wins.
    pub fn binding(&self, name: &str) -> Option<Value> {
        let slot = self
            .bindings
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, slot)| *slot as usize)?;
        self.vm.stack_slot(slot)
    }

    /// Call a top-level binding by name with `args`, re-entrantly. The
    /// per-frame entry point for `update`/`draw`-style callbacks. An
    /// uncaught raise inside the callee surfaces as `Err` with the
    /// persistent frame left intact (a later call still works). Errors
    /// if the name is unbound or not callable.
    pub fn call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let callee = self.binding(name).ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::NotCallable(format!("no binding named '{name}'")),
                0,
            )
        })?;
        self.vm.call_function(callee, args)
    }

    /// `true` iff `name` is bound to a callable value (tigr closure or
    /// native). Lets a host probe for optional callbacks before a
    /// frame loop, e.g. skip `update` if the game never defined one.
    pub fn has_callable(&self, name: &str) -> bool {
        matches!(
            self.binding(name),
            Some(Value::Function(_)) | Some(Value::NativeFn(_))
        )
    }

    /// Borrow the source map to render an [`Error`] returned by
    /// [`load`](Session::load): `err.render(&session.sources())`.
    pub fn sources(&self) -> Ref<'_, SourceMap> {
        self.sources.borrow()
    }

    /// Direct access to the underlying VM, for hosts that need the
    /// lower-level driving API (e.g. `call_function` against a value
    /// obtained out of band).
    pub fn vm(&mut self) -> &mut Vm {
        &mut self.vm
    }
}

/// The directory a loaded program's relative imports resolve against,
/// derived from its source label. A label that is a bare name with no
/// directory (`<host:1>`, `game.tg`) yields `None`, matching the prior
/// behaviour where string-loaded source had no base dir; a label that is a
/// path (`games/comet/comet.tg`) yields its parent.
fn base_dir_from_label(label: &str) -> Option<PathBuf> {
    Path::new(label)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(PathBuf::from)
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// Classify a top-level binding for hot-reload: callables (tigr closures
/// and natives) are *code* (replaced on reload); everything else is
/// *data* (carried forward).
fn is_data(v: &Value) -> bool {
    !matches!(v, Value::Function(_) | Value::NativeFn(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(v: &Value) -> i64 {
        match v {
            Value::Int(i) => *i,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    /// A host module exposing a single native that returns a constant.
    /// `import 'Host'; Host.answer()` resolves to it. Run under
    /// `gc-torture` this is also the host_modules GC-root test: the
    /// module Object lives only in `host_modules`, so if `trace_roots`
    /// missed it, the handle would go stale mid-run and panic.
    #[test]
    fn registers_and_imports_host_module() {
        let mut s = Session::new();
        s.register_module(
            "Host",
            object(&[("answer", native("answer", Arity::Exact(0), |_| Ok(Value::Int(42))))]),
        );
        s.load("Host := import 'Host'; result := Host.answer();").expect("load");
        assert_eq!(int(&s.binding("result").expect("result")), 42);
    }

    /// A host module is *ambient*: game code calls it with no `import`,
    /// exactly like stdlib. `Host.answer()` resolves the registered
    /// module via its lazy global slot.
    #[test]
    fn host_module_is_ambient_no_import() {
        let mut s = Session::new();
        s.register_module(
            "Host",
            object(&[("answer", native("answer", Arity::Exact(0), |_| Ok(Value::Int(42))))]),
        );
        s.load("result := Host.answer();").expect("load");
        assert_eq!(int(&s.binding("result").expect("result")), 42);
    }

    /// A host *source* module is ambient too — bare `Num.lerp(...)`.
    #[test]
    fn host_source_module_is_ambient_no_import() {
        let mut s = Session::new();
        s.register_source_module(
            "Num",
            "lerp := fn(a, b, t) { a + (b - a) * t };\n${ lerp: lerp }",
        );
        s.load("mid := Num.lerp(0, 10, 0.5);").expect("load");
        match s.binding("mid").expect("mid") {
            Value::Float(v) => assert_eq!(v, 5.0),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// Index stability: a host module used (resolved, so its `ambient`
    /// slot is cleared) before a second module is registered must keep
    /// its global index, so a later `load` referencing both resolves the
    /// right ones. Catches a layout derived from the mutable table.
    #[test]
    fn host_ambient_indices_stable_after_resolution() {
        let mut s = Session::new();
        s.register_module(
            "A",
            object(&[("v", native("v", Arity::Exact(0), |_| Ok(Value::Int(1))))]),
        );
        // Resolve A (clears its lazy marker), then register B and load
        // code that uses both.
        s.load("a1 := A.v();").expect("load 1");
        s.register_module(
            "B",
            object(&[("v", native("v", Arity::Exact(0), |_| Ok(Value::Int(2))))]),
        );
        s.load("a2 := A.v(); b := B.v();").expect("load 2");
        assert_eq!(int(&s.binding("a2").expect("a2")), 1);
        assert_eq!(int(&s.binding("b").expect("b")), 2);
    }

    /// A host ambient module survives hot-reload: reloaded code can still
    /// reference it without an `import`.
    #[test]
    fn host_ambient_survives_reload() {
        let mut s = Session::new();
        s.register_module(
            "Host",
            object(&[("answer", native("answer", Arity::Exact(0), |_| Ok(Value::Int(7))))]),
        );
        s.load("x := Host.answer();").expect("load");
        s.reload("y := Host.answer() + 1;").expect("reload");
        assert_eq!(int(&s.binding("y").expect("y")), 8);
    }

    /// Host-facing helpers: `Value::get_field` reads an `Object` field,
    /// `Value::set_field` writes one in place (a host updating a caller-owned
    /// object), and `RuntimeErrorKind` is reachable from `embed::*` so a host
    /// can build native errors with the same idiom the stdlib natives use.
    #[test]
    fn object_field_read_and_error_kind() {
        let mut s = Session::new();
        s.load("cfg := ${ title: 'hi', width: 640 };").expect("load");
        let cfg = s.binding("cfg").expect("cfg");
        assert!(matches!(cfg.get_field("width"), Some(Value::Int(640))));
        assert!(matches!(cfg.get_field("title"), Some(Value::Str(_))));
        assert!(cfg.get_field("missing").is_none());
        // A non-object yields None rather than panicking.
        assert!(Value::Int(1).get_field("x").is_none());

        // set_field updates an existing field in place, and the object is a
        // reference value, so the binding the program holds observes it.
        assert!(cfg.set_field("width", Value::Int(800)));
        assert!(matches!(cfg.get_field("width"), Some(Value::Int(800))));
        // It also inserts an absent key, and reports false on a non-object
        // (leaving it untouched) rather than panicking.
        assert!(cfg.set_field("height", Value::Int(600)));
        assert!(matches!(cfg.get_field("height"), Some(Value::Int(600))));
        assert!(!Value::Int(1).set_field("x", Value::Int(0)));

        // `fields` reads every (key, value) pair in insertion order, so a host
        // can consume a declaration map whose keys it does not know ahead of
        // time. A non-object yields None rather than an empty vec.
        let pairs = cfg.fields().expect("object has fields");
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["title", "width", "height"], "insertion order preserved");
        assert!(Value::Int(1).fields().is_none());

        // RuntimeErrorKind is exported for host-built natives.
        let _ = RuntimeError::new(RuntimeErrorKind::TypeMismatch("x".into()), 0);
    }

    /// Host-facing byte helpers: `embed::bytes` builds a `Bytes` value from
    /// an owned block, and `Value::with_bytes` borrows the block back in
    /// place (no clone) for a `Bytes` value, `None` for anything else. The
    /// round trip also crosses the VM: a program-built `Bytes` reads back
    /// through `with_bytes`, and a host-built one is visible to the program.
    #[test]
    fn bytes_build_and_borrow() {
        // Build host-side, borrow back without cloning the whole block.
        let b = bytes(vec![1, 2, 3, 4]);
        let sum = b.with_bytes(|s| s.iter().map(|&x| x as u32).sum::<u32>());
        assert_eq!(sum, Some(10));
        assert_eq!(b.with_bytes(<[u8]>::to_vec), Some(vec![1, 2, 3, 4]));
        // A non-bytes value yields None rather than panicking.
        assert!(Value::Int(1).with_bytes(<[u8]>::to_vec).is_none());

        // A program-built Bytes reads back through with_bytes.
        let mut s = Session::new();
        s.load("blob := Bytes.from_string('hi');").expect("load");
        let blob = s.binding("blob").expect("blob");
        assert_eq!(blob.with_bytes(<[u8]>::to_vec), Some(b"hi".to_vec()));
    }

    /// `Value::with_array` borrows an `Array`'s backing slice in place
    /// (no clone) for an `Array` value, `None` for anything else. The
    /// embedder reads a program-built list of points or numbers without
    /// copying it out of the GC arena first; elements stay `Value`s, so
    /// the closure coerces each one itself.
    #[test]
    fn array_borrow() {
        let mut s = Session::new();

        // A flat number list, the polygon/polyline hot path: read it as f32.
        s.load("pts := [10, 20, 30.5, 40];").expect("load");
        let pts = s.binding("pts").expect("pts");
        let flat = pts.with_array(|xs| {
            xs.iter()
                .map(|v| match v {
                    Value::Int(n) => *n as f32,
                    Value::Float(x) => *x as f32,
                    _ => f32::NAN,
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(flat, Some(vec![10.0, 20.0, 30.5, 40.0]));

        // A list of ${x, y} points: read each element's fields.
        s.load("poly := [${ x: 1, y: 2 }, ${ x: 3, y: 4 }];").expect("load");
        let poly = s.binding("poly").expect("poly");
        let coords = poly.with_array(|xs| {
            xs.iter()
                .filter_map(|v| {
                    let x = v.get_field("x")?;
                    let y = v.get_field("y")?;
                    Some((x, y))
                })
                .count()
        });
        assert_eq!(coords, Some(2));

        // A non-array value yields None rather than panicking.
        assert!(Value::Int(1).with_array(|xs| xs.len()).is_none());
    }

    /// A host *source* module is compiled on first import, resolves
    /// under its bare name, and its exported functions are callable —
    /// the delivery path for pure-tigr framework helpers. Importing it
    /// twice evaluates it only once (the second hits the module cache).
    #[test]
    fn registers_and_imports_host_source_module() {
        let mut s = Session::new();
        s.register_source_module(
            "Num",
            "lerp := fn(a, b, t) { a + (b - a) * t };\n${ lerp: lerp }",
        );
        s.load("Num := import 'Num'; mid := Num.lerp(0, 10, 0.5); again := import 'Num';")
            .expect("load");
        match s.binding("mid").expect("mid") {
            Value::Float(v) => assert_eq!(v, 5.0),
            other => panic!("expected Float, got {other:?}"),
        }
    }

    /// A host source module named after a built-in must NOT shadow it:
    /// the source stdlib resolves first. A bogus `Math` whose `abs`
    /// returns -1 must lose to the real `Math.abs` (returns 5).
    #[test]
    fn host_source_module_does_not_shadow_builtin() {
        let mut s = Session::new();
        s.register_source_module("Math", "${ abs: fn(x) { 0 - 1 } }");
        s.load("Math := import 'Math'; v := Math.abs(0 - 5);").expect("load");
        assert_eq!(int(&s.binding("v").expect("v")), 5);
    }

    /// A host import loader serves a *path* import out of memory instead
    /// of the filesystem — the delivery path for a multi-file game's
    /// sibling `.tg` files in an exported bundle. Imports resolve against
    /// the entry's directory, so `./sub/lib` from `games/g/main.tg` becomes
    /// the normalised key `games/g/sub/lib.tg`; a miss is an import error.
    #[test]
    fn import_loader_serves_path_modules() {
        let mut s = Session::new();
        s.set_import_loader(|key| match key {
            "games/g/sub/lib.tg" => Some("${ answer: 42 }".to_owned()),
            _ => None,
        });
        s.load_named(
            "games/g/main.tg",
            "Lib := import './sub/lib'; result := Lib.answer;",
        )
        .expect("load");
        assert_eq!(int(&s.binding("result").expect("result")), 42);
    }

    /// A relative `import './lib'` resolves against the entry's directory
    /// (taken from its label), normalised, then handed to the loader — so a
    /// string-loaded game's relative imports work the way a file-loaded
    /// one's would.
    #[test]
    fn import_loader_relative_uses_entry_base_dir() {
        let mut s = Session::new();
        s.set_import_loader(|key| (key == "games/g/lib.tg").then(|| "${ v: 7 }".to_owned()));
        s.load_named("games/g/main.tg", "Lib := import './lib'; r := Lib.v;")
            .expect("load");
        assert_eq!(int(&s.binding("r").expect("r")), 7);
    }

    /// A path import the loader cannot resolve is a clean import error, not
    /// a silent fall-through to the filesystem.
    #[test]
    fn import_loader_miss_is_an_error() {
        let mut s = Session::new();
        s.set_import_loader(|_| None);
        assert!(s.load_named("g/main.tg", "X := import './nope';").is_err());
    }

    /// A host module named after a built-in must NOT shadow it: the
    /// built-in resolves first. We register a bogus `Math` whose `abs`
    /// returns -1; the real source-stdlib `Math.abs` (returns 5) must
    /// win.
    #[test]
    fn host_module_does_not_shadow_builtin() {
        let mut s = Session::new();
        s.register_module(
            "Math",
            object(&[("abs", native("abs", Arity::Exact(1), |_| Ok(Value::Int(-1))))]),
        );
        s.load("Math := import 'Math'; v := Math.abs(0 - 5);").expect("load");
        assert_eq!(int(&s.binding("v").expect("v")), 5);
    }

    /// Top-level functions and data become live bindings; `call`
    /// invokes a function, `binding` reads data.
    #[test]
    fn session_bindings_and_call() {
        let mut s = Session::new();
        s.load("dbl := fn(dt){ dt * 2 }; x := 10;").expect("load");
        assert!(s.has_callable("dbl"));
        assert!(!s.has_callable("x"));
        assert_eq!(int(&s.binding("x").expect("x")), 10);
        let r = s.call("dbl", vec![Value::Int(21)]).expect("call dbl");
        assert_eq!(int(&r), 42);
    }

    /// `eval_line` evaluates in the live frame-0 scope: a bare expression
    /// returns its value, an assignment mutates a global the loaded program
    /// already reads, and a `:=` binding is visible to later eval lines.
    /// This is what lets a host console be a REPL into a running program.
    #[test]
    fn eval_line_shares_the_live_scope() {
        let mut s = Session::new();
        s.load("x := 10; bump := fn(){ x = x + 1 };").expect("load");

        // A bare expression returns its value.
        assert_eq!(int(&s.eval_line("x").expect("read x")), 10);

        // An assignment mutates the global the program sees: calling the
        // loaded `bump` now starts from the console-set value.
        s.eval_line("x = 100").expect("assign x");
        assert_eq!(int(&s.binding("x").expect("x")), 100);
        s.call("bump", vec![]).expect("bump");
        assert_eq!(int(&s.binding("x").expect("x")), 101);

        // A new binding from one line is visible to the next.
        s.eval_line("y := x * 2").expect("declare y");
        assert_eq!(int(&s.eval_line("y").expect("read y")), 202);

        // An error leaves the session intact and renders against the sources.
        let err = s.eval_line("nope_unbound").expect_err("unbound");
        assert!(!err.render(&s.sources()).is_empty());
        assert_eq!(int(&s.binding("x").expect("x still 101")), 101);
    }

    /// `seed_rng` pins tigr's shared stream from the host side, so the
    /// `Random` module replays identically without the game seeding it.
    #[test]
    fn host_seeds_rng() {
        let draw = |seed: u64| {
            let mut s = Session::new();
            s.seed_rng(seed);
            s.load("R := import 'Random'; v := R.int(0, 1000000);").expect("load");
            int(&s.binding("v").expect("v"))
        };
        assert_eq!(draw(12345), draw(12345));
        // A different seed almost certainly yields a different draw.
        assert_ne!(draw(1), draw(2));
    }

    /// `load_named` labels the source so a rendered diagnostic points at
    /// the given name (a file path) rather than an anonymous `<host:N>`.
    #[test]
    fn named_source_appears_in_diagnostic() {
        let mut s = Session::new();
        // Two statements without a separating `;` is a parse error.
        let err = s.load_named("games/hello.tg", "x := 1\ny := 2\n").expect_err("parse error");
        let rendered = err.render(&s.sources());
        assert!(
            rendered.contains("games/hello.tg"),
            "diagnostic names the file: {rendered}"
        );
        assert!(!rendered.contains("<host:"), "no anonymous label: {rendered}");
    }

    /// `call_function` is re-entrant: a callee that calls another tigr
    /// function works, and an uncaught raise surfaces as `Err` while
    /// leaving the persistent frame intact (a later call still works).
    #[test]
    fn call_reentrancy_and_error_recovery() {
        let mut s = Session::new();
        s.load(
            "inner := fn(n){ n + 1 }; \
             outer := fn(n){ inner(n) * 10 }; \
             boom := fn(){ raise \"kaboom\" };",
        )
        .expect("load");

        assert_eq!(int(&s.call("outer", vec![Value::Int(4)]).expect("outer")), 50);
        assert!(s.call("boom", vec![]).is_err());
        // Frame-0 survived the uncaught raise — a fresh call still runs.
        assert_eq!(int(&s.call("outer", vec![Value::Int(1)]).expect("outer 2")), 20);
    }

    /// An unbound name is a clean error, not a panic.
    #[test]
    fn call_unbound_name_errors() {
        let mut s = Session::new();
        s.load("x := 1;").expect("load");
        assert!(s.call("nope", vec![]).is_err());
    }

    /// A second `load` sees the first load's bindings, and a failed
    /// load leaves prior bindings intact and callable.
    #[test]
    fn incremental_load_and_failed_load_keeps_state() {
        let mut s = Session::new();
        s.load("base := 100; get := fn(){ base };").expect("load 1");
        // Second load resolves `base` declared by the first.
        s.load("derived := base + 1;").expect("load 2");
        assert_eq!(int(&s.binding("derived").expect("derived")), 101);

        // A syntax-error load fails but does not disturb the session.
        assert!(s.load("broken := fn( {").is_err());
        assert_eq!(int(&s.call("get", vec![]).expect("get still works")), 100);
    }

    // -- Phase C: cooperative timing (wait / drain_ready) ------------

    /// `drain_ready` is the per-frame, non-blocking coroutine pump. A
    /// `go` coroutine that `wait`s parks on the host clock; it resumes
    /// only once `drain_ready` is called with `now` past its wake time.
    /// Crucially, a `drain_ready` with the coroutine still parked must
    /// return promptly rather than block the calling (render) thread.
    #[test]
    fn drain_ready_is_non_blocking_and_clock_driven() {
        let mut s = Session::new();
        s.load("flag := 0; go fn(){ wait(1.0); flag = 1; };").expect("load");

        // Coroutine runs up to `wait(1.0)` and parks. If `drain_ready`
        // blocked on the park this call would hang the test.
        s.vm().drain_ready(0.0).expect("drain @0.0");
        assert_eq!(int(&s.binding("flag").expect("flag")), 0);

        // Clock not yet at the wake time — still parked.
        s.vm().drain_ready(0.5).expect("drain @0.5");
        assert_eq!(int(&s.binding("flag").expect("flag")), 0);

        // Clock reaches the wake time — coroutine resumes and finishes.
        s.vm().drain_ready(1.0).expect("drain @1.0");
        assert_eq!(int(&s.binding("flag").expect("flag")), 1);
    }

    /// A host-provided frame-yield (built with [`native_frame_wait`], as
    /// purr's `GameTime.wait_frame` is) parks until the *next* tick
    /// regardless of the clock value — the per-frame yield. Three drains
    /// at the same `now` should step the coroutine across both yields. It
    /// is a host *module member*, not a language builtin: registered by
    /// the host, reached as `Frame.next()`.
    #[test]
    fn host_frame_wait_steps_one_tick_per_drain() {
        let mut s = Session::new();
        s.register_module(
            "Frame",
            object(&[("next", native_frame_wait("next", Arity::Exact(0)))]),
        );
        s.load(
            "Frame := import 'Frame'; ticks := 0; \
             go fn(){ Frame.next(); ticks = 1; Frame.next(); ticks = 2; };",
        )
        .expect("load");

        s.vm().drain_ready(0.0).expect("drain 1"); // runs to first Frame.next
        assert_eq!(int(&s.binding("ticks").expect("ticks")), 0);
        s.vm().drain_ready(0.0).expect("drain 2"); // wakes, runs to second
        assert_eq!(int(&s.binding("ticks").expect("ticks")), 1);
        s.vm().drain_ready(0.0).expect("drain 3"); // wakes, finishes
        assert_eq!(int(&s.binding("ticks").expect("ticks")), 2);
    }

    /// A host-provided frame-yield raises if used outside a frame drive
    /// (here, a synchronous `call`): there is no "next frame" to resume
    /// on. The session survives for a later call.
    #[test]
    fn host_frame_wait_outside_drain_raises() {
        let mut s = Session::new();
        s.register_module(
            "Frame",
            object(&[("next", native_frame_wait("next", Arity::Exact(0)))]),
        );
        s.load(
            "Frame := import 'Frame'; bad := fn(){ Frame.next() }; \
             ok := fn(){ 7 };",
        )
        .expect("load");
        assert!(s.call("bad", vec![]).is_err());
        // The session survived the raise: a normal call still works.
        assert_eq!(int(&s.call("ok", vec![]).expect("ok")), 7);
    }

    /// GC root proof (run this under `--features gc-torture`): a
    /// `wait`-parked coroutine holds a heap array reachable *only*
    /// through its saved (timer-blocked) stack. A collection that runs
    /// while it is parked — here forced via a `gc()` load — must keep
    /// that array alive, or the resume reads a freed handle. Exercises
    /// `Scheduler::queued()` chaining `timer_blocked` and `trace_roots`.
    #[test]
    fn wait_parked_coroutine_survives_gc() {
        let mut s = Session::new();
        s.load(
            "result := 0; \
             go fn(){ \
                 local := [11, 22, 33]; \
                 wait(0.5); \
                 result = local[0] + local[1] + local[2]; \
             };",
        )
        .expect("load");

        // Coroutine runs to `wait(0.5)` and parks, holding `local`.
        s.vm().drain_ready(0.0).expect("drain @0.0");
        assert_eq!(int(&s.binding("result").expect("result")), 0);

        // Collect while parked: `local` lives only on the parked
        // coroutine's saved stack.
        s.load("gc();").expect("gc");

        // Resume past the wait — `local` must be intact.
        s.vm().drain_ready(0.6).expect("drain @0.6");
        assert_eq!(int(&s.binding("result").expect("result")), 66);
    }

    /// `wait` from a synchronous host `call` (e.g. an `update`-style
    /// callback invoked via `Session::call`, not a frame drain) raises
    /// catchably rather than blocking the host thread on the clock. A
    /// clean `Err`, and the session survives for a later call. (`wait`
    /// *does* work at a program's top level and inside a `go` under a
    /// frame drain — just not on a re-entrant host call.)
    #[test]
    fn wait_on_main_raises_and_session_survives() {
        let mut s = Session::new();
        s.load("bad := fn(){ wait(1) }; ok := fn(){ 7 };").expect("load");
        assert!(s.call("bad", vec![]).is_err());
        // Frame-0 survived: a normal call still works.
        assert_eq!(int(&s.call("ok", vec![]).expect("ok")), 7);
    }

    /// A `drain_ready` with no ready coroutines is a cheap no-op that
    /// leaves the persistent session untouched.
    #[test]
    fn drain_ready_with_nothing_ready_is_noop() {
        let mut s = Session::new();
        s.load("x := 5; get := fn(){ x };").expect("load");
        s.vm().drain_ready(0.0).expect("idle drain");
        s.vm().drain_ready(123.0).expect("idle drain 2");
        assert_eq!(int(&s.call("get", vec![]).expect("get")), 5);
    }

    // -- Phase C3: hot-reload ----------------------------------------

    /// The headline reload contract: top-level *data* is carried across
    /// a reload, top-level *code* is replaced. We bump `count` via the
    /// old `tick`, reload with a `tick` that steps by 10, and confirm
    /// `count` survived (data) while `tick` now runs the new body.
    #[test]
    fn reload_carries_data_replaces_code() {
        let mut s = Session::new();
        s.load("count := 0; tick := fn(){ count = count + 1; count };")
            .expect("load");
        s.call("tick", vec![]).expect("tick 1");
        s.call("tick", vec![]).expect("tick 2");
        assert_eq!(int(&s.binding("count").expect("count")), 2);

        // New code: same `count` declaration, a `tick` that steps by 10.
        s.reload("count := 0; tick := fn(){ count = count + 10; count };")
            .expect("reload");
        // `count` was preserved (the new `count := 0` is overridden by
        // the carried value); `tick` runs the new body.
        assert_eq!(int(&s.binding("count").expect("count after reload")), 2);
        assert_eq!(int(&s.call("tick", vec![]).expect("tick new")), 12);
        assert_eq!(int(&s.binding("count").expect("count final")), 12);
    }

    /// A reload whose source fails to compile is non-fatal: it returns
    /// the rendered diagnostic as `Err` and leaves the live program
    /// running the last good code.
    #[test]
    fn reload_compile_error_is_non_fatal() {
        let mut s = Session::new();
        s.load("count := 7; tick := fn(){ count };").expect("load");
        let err = s.reload("count := 0; tick := fn( {").expect_err("syntax error");
        // A rendered, non-empty diagnostic.
        assert!(!err.is_empty());
        // The session is untouched — last-good code still runs.
        assert_eq!(int(&s.binding("count").expect("count intact")), 7);
        assert_eq!(int(&s.call("tick", vec![]).expect("tick intact")), 7);
    }

    /// A name that changes kind across a reload takes the new
    /// definition: data→code means the old value is dropped, not carried
    /// over the new function.
    #[test]
    fn reload_kind_change_takes_new_definition() {
        let mut s = Session::new();
        s.load("x := 5;").expect("load");
        assert_eq!(int(&s.binding("x").expect("x")), 5);
        // `x` is now a function; the old data value must not clobber it.
        s.reload("x := fn(){ 42 };").expect("reload");
        assert!(s.has_callable("x"));
        assert_eq!(int(&s.call("x", vec![]).expect("call x")), 42);
    }

    /// Reload cancels parked green threads: their frames hold `ip`s into
    /// the old bytecode and cannot migrate. A `wait`-parked coroutine is
    /// gone after reload, and a later `drain_ready` neither resurrects it
    /// nor panics.
    #[test]
    fn reload_cancels_parked_coroutines() {
        let mut s = Session::new();
        s.load("flag := 0; go fn(){ wait(1.0); flag = 1; };").expect("load");
        s.vm().drain_ready(0.0).expect("drain"); // coroutine parks on the timer

        // Reload wipes the parked coroutine; `flag` (data) is carried.
        s.reload("flag := 0;").expect("reload");

        // Past the old wait time — nothing wakes, no panic, flag stays 0.
        s.vm().drain_ready(2.0).expect("drain after reload");
        assert_eq!(int(&s.binding("flag").expect("flag")), 0);
    }

    /// The optional `on_reload` hook runs once after data is carried, so
    /// it observes the *preserved* state, not the new program's initial
    /// value.
    #[test]
    fn reload_runs_on_reload_hook_after_carry() {
        let mut s = Session::new();
        s.load("state := 1;").expect("load");
        s.reload(
            "state := 99; \
             migrated := 0; \
             on_reload := fn(){ migrated = state * 2 };",
        )
        .expect("reload");
        // `state` carried (1, not the new 99); the hook saw the carried
        // value: migrated = 1 * 2.
        assert_eq!(int(&s.binding("state").expect("state")), 1);
        assert_eq!(int(&s.binding("migrated").expect("migrated")), 2);
    }

    /// GC root proof for the carry (run under `--features gc-torture`):
    /// the new top-level allocates heavily before the old data is
    /// carried into its slot. The old value is reachable only from the
    /// reload snapshot during that window, so it must be held as a GC
    /// root (`Vm::hold_reload_roots`) or the carry reads a freed handle.
    #[test]
    fn reload_carried_heap_data_survives_gc() {
        let mut s = Session::new();
        s.load("items := [1, 2, 3];").expect("load");
        // New top-level allocates a lot, then redeclares `items` (data);
        // the old `[1, 2, 3]` is carried over the new `[9, 9]`.
        s.reload(
            "junk := for[] (i, 1..=800) { [i, i] }; \
             items := [9, 9]; \
             total := fn(){ items[0] + items[1] + items[2] };",
        )
        .expect("reload");
        assert_eq!(int(&s.call("total", vec![]).expect("total")), 6);
    }
}
