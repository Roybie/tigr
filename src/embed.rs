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
use std::rc::Rc;
use std::sync::Arc;

use crate::vm::compiler::Compiler;
use crate::vm::error::RuntimeErrorKind;
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
pub use crate::vm::error::{Error, RuntimeError};
pub use crate::vm::native_modules::{native, native_blocking, native_socket, object};
pub use crate::vm::value::{Arity, NativeFn, NativeKind, Value};
pub use crate::vm::vm::Vm;

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
        self.load_no += 1;
        let sid = self
            .sources
            .borrow_mut()
            .add(format!("<host:{}>", self.load_no), source);

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
        let (main, new_bindings) =
            Compiler::compile_repl_with_source(&program, &self.bindings, sid)?;

        let closure = crate::vm::gc::alloc_closure(Closure {
            function: Arc::new(main),
            upvalues: Vec::new(),
        });

        // Stack length expected after a clean run: closure slot 0 plus
        // the existing committed bindings. On uncaught raise the VM
        // truncates to this, discarding the half-introduced bindings.
        let snapshot_len = 1 + self.bindings.len();

        match self.vm.run_repl_line(closure, snapshot_len) {
            Ok(_) => {
                self.bindings.extend(new_bindings);
                Ok(())
            }
            Err(e) => Err(Error::Runtime(e)),
        }
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

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
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

    /// `wait_frame()` parks until the *next* tick regardless of the
    /// clock value — the per-frame yield. Three drains at the same
    /// `now` should step the coroutine across both `wait_frame`s.
    #[test]
    fn wait_frame_steps_one_tick_per_drain() {
        let mut s = Session::new();
        s.load(
            "ticks := 0; \
             go fn(){ wait_frame(); ticks = 1; wait_frame(); ticks = 2; };",
        )
        .expect("load");

        s.vm().drain_ready(0.0).expect("drain 1"); // runs to first wait_frame
        assert_eq!(int(&s.binding("ticks").expect("ticks")), 0);
        s.vm().drain_ready(0.0).expect("drain 2"); // wakes, runs to second
        assert_eq!(int(&s.binding("ticks").expect("ticks")), 1);
        s.vm().drain_ready(0.0).expect("drain 3"); // wakes, finishes
        assert_eq!(int(&s.binding("ticks").expect("ticks")), 2);
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

    /// `wait` is only legal inside a host-driven green thread: calling
    /// it on the main coroutine (e.g. from an `update`-style callback)
    /// raises catchably rather than hanging. A clean `Err`, and the
    /// session survives for a later call.
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
}
