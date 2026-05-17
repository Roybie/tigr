//! Hand-written tracing mark-sweep garbage collector (v0.10).
//!
//! Replaces the `Rc<RefCell<...>>` representation of the mutable /
//! potentially-cyclic value types — `Array`, `Object`, `Map`, `Set`,
//! `Iter`, `Closure`, and the upvalue cells — so reference cycles are
//! actually reclaimed (an `Rc` graph leaks them forever).
//!
//! Design:
//!
//! - A `thread_local!` [`Heap`] owns every collectable object in a set
//!   of per-kind [`Arena`]s. A [`Value`] no longer carries the data —
//!   it carries a small `Copy` [`GcRef`] handle (slot index + a
//!   generation counter).
//! - Freeing a slot bumps its generation, so any surviving stale handle
//!   mismatches and **panics loudly** instead of silently corrupting
//!   the heap.
//! - The heap is thread-local, so native-function signatures need not
//!   change to thread a heap parameter through every call.
//! - `GcRef::borrow` / `borrow_mut` hand back RAII guards that deref to
//!   `&T` / `&mut T`, so the existing `.borrow()` call sites port
//!   essentially unchanged.
//!
//! This file builds the heap, the handles, the access guards, the
//! `Trace`/`Marker` mark phase, the sweep, and the collection trigger.

use std::cell::{Ref, RefCell, RefMut};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};

use crate::vm::value::{Closure, IterState, MapKey, Upvalue, Value};

/// Object-count the heap starts (and never drops below) as a collection
/// threshold. A pure count, not a byte total — adequate for a hobby VM.
pub const MIN_THRESHOLD: usize = 1024;

// ---------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------

/// A handle into the heap. `K` is one of the zero-sized kind markers
/// (`ArrayKind`, `ObjectKind`, ...), so `GcRef<ArrayKind>` and
/// `GcRef<ObjectKind>` are distinct types — a mis-typed handle is a
/// compile error, leaving only the generation check at runtime.
pub struct GcRef<K> {
    index: u32,
    generation: u32,
    _kind: PhantomData<fn() -> K>,
}

impl<K> Clone for GcRef<K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K> Copy for GcRef<K> {}

impl<K> PartialEq for GcRef<K> {
    /// Identity equality — the role `Rc::ptr_eq` played before.
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}
impl<K> Eq for GcRef<K> {}

impl<K> std::hash::Hash for GcRef<K> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.index.hash(h);
        self.generation.hash(h);
    }
}

impl<K> std::fmt::Debug for GcRef<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GcRef#{}", self.index)
    }
}

// ---------------------------------------------------------------------
// Access guards
// ---------------------------------------------------------------------

/// Shared-borrow guard. Owns a clone of the slot's `Rc<RefCell<T>>` so
/// the cell stays alive (and at a stable heap address) for the guard's
/// whole life; `inner` is declared first so it drops before `_cell`.
pub struct GcReadGuard<T: 'static> {
    inner: Ref<'static, T>,
    _cell: Rc<RefCell<T>>,
}

impl<T> Deref for GcReadGuard<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}

/// Exclusive-borrow guard. See [`GcReadGuard`] for the ownership story.
pub struct GcWriteGuard<T: 'static> {
    inner: RefMut<'static, T>,
    _cell: Rc<RefCell<T>>,
}

impl<T> Deref for GcWriteGuard<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}
impl<T> DerefMut for GcWriteGuard<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

fn read_guard<T: 'static>(cell: Rc<RefCell<T>>) -> GcReadGuard<T> {
    let borrowed: Ref<'_, T> = cell.borrow();
    // SAFETY: `_cell` keeps the `RefCell<T>` alive at a fixed heap
    // address for the guard's lifetime, and `inner` is dropped before
    // `_cell` (field declaration order). Extending the borrow to
    // `'static` is therefore sound — the referent outlives every use.
    let inner: Ref<'static, T> = unsafe { std::mem::transmute(borrowed) };
    GcReadGuard { inner, _cell: cell }
}

fn write_guard<T: 'static>(cell: Rc<RefCell<T>>) -> GcWriteGuard<T> {
    let borrowed: RefMut<'_, T> = cell.borrow_mut();
    // SAFETY: see `read_guard` — identical argument.
    let inner: RefMut<'static, T> = unsafe { std::mem::transmute(borrowed) };
    GcWriteGuard { inner, _cell: cell }
}

// ---------------------------------------------------------------------
// Arena
// ---------------------------------------------------------------------

enum SlotState<T> {
    /// A live object. The `Rc` is an arena implementation detail — it is
    /// never exposed; handing a clone to a guard preserves the exact
    /// `RefCell` borrow semantics the VM relied on before v0.10.
    Live(Rc<RefCell<T>>),
    /// A reclaimed slot sitting on the free list.
    Free,
}

struct Slot<T> {
    /// Generation of the handle that currently owns this slot. Bumped on
    /// every free so stale handles are detected.
    generation: u32,
    /// Mark bit, set during the GC mark phase, cleared during sweep.
    mark: bool,
    state: SlotState<T>,
}

/// A homogeneous pool of one collectable kind, with a free list of
/// reclaimed slot indices.
struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

impl<T: 'static> Arena<T> {
    fn new() -> Self {
        Arena { slots: Vec::new(), free: Vec::new() }
    }

    /// Install `value` in a fresh or recycled slot; returns
    /// `(index, generation)` for the handle.
    fn alloc(&mut self, value: T) -> (u32, u32) {
        let cell = Rc::new(RefCell::new(value));
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.mark = false;
            slot.state = SlotState::Live(cell);
            (index, slot.generation)
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(Slot { generation: 0, mark: false, state: SlotState::Live(cell) });
            (index, 0)
        }
    }

    /// Resolve a handle to its live cell, cloning the `Rc` out. Panics
    /// loudly on a stale or freed handle rather than corrupting memory.
    fn cell(&self, index: u32, generation: u32) -> Rc<RefCell<T>> {
        let slot = self.slots.get(index as usize).unwrap_or_else(|| {
            panic!("tigr GC: handle index {index} out of range")
        });
        if slot.generation != generation {
            panic!(
                "tigr GC: stale handle (slot {index}: generation {} != handle generation {generation})",
                slot.generation
            );
        }
        match &slot.state {
            SlotState::Live(cell) => Rc::clone(cell),
            SlotState::Free => panic!("tigr GC: use-after-free (slot {index})"),
        }
    }

    /// Set the mark bit for a live slot. Returns `true` if it was newly
    /// marked (so the caller should trace its children). Panics on a
    /// stale or freed handle — a dangling handle reachable from a root
    /// is exactly the bug class the generation counter exists to catch.
    fn mark(&mut self, index: u32, generation: u32) -> bool {
        let slot = self.slots.get_mut(index as usize).unwrap_or_else(|| {
            panic!("tigr GC: mark of out-of-range handle {index}")
        });
        if slot.generation != generation {
            panic!(
                "tigr GC: mark of stale handle (slot {index}: generation {} != handle generation {generation})",
                slot.generation
            );
        }
        if matches!(slot.state, SlotState::Free) {
            panic!("tigr GC: mark of freed handle (slot {index})");
        }
        if slot.mark {
            false
        } else {
            slot.mark = true;
            true
        }
    }

    /// Sweep: reclaim every unmarked live slot, clear marks on the rest.
    /// Returns the number of objects freed.
    fn sweep(&mut self) -> usize {
        let mut freed = 0;
        for (i, slot) in self.slots.iter_mut().enumerate() {
            match &slot.state {
                SlotState::Live(_) => {
                    if slot.mark {
                        slot.mark = false;
                    } else {
                        slot.generation = slot.generation.wrapping_add(1);
                        debug_assert!(
                            slot.generation != 0,
                            "tigr GC: slot {i} generation counter wrapped"
                        );
                        slot.state = SlotState::Free;
                        self.free.push(i as u32);
                        freed += 1;
                    }
                }
                SlotState::Free => {}
            }
        }
        freed
    }
}

// ---------------------------------------------------------------------
// Heap
// ---------------------------------------------------------------------

/// Snapshot of heap counters, surfaced to tigr code via the `gc()`
/// builtin and used by tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeapStats {
    pub live: usize,
    pub collections: u64,
    pub total_allocated: u64,
    pub total_freed: u64,
}

/// The whole managed heap — one [`Arena`] per collectable kind.
pub struct Heap {
    arrays: Arena<Vec<Value>>,
    objects: Arena<IndexMap<Rc<str>, Value>>,
    maps: Arena<IndexMap<MapKey, Value>>,
    sets: Arena<IndexSet<MapKey>>,
    /// v0.13 — `Bytes` buffers. A `Vec<u8>` owns no handles, so this
    /// arena is a GC leaf (marked, never traced).
    bytes: Arena<Vec<u8>>,
    iters: Arena<IterState>,
    closures: Arena<Closure>,
    upvalues: Arena<Upvalue>,
    /// Live object count across all arenas — the collection trigger.
    live: usize,
    /// `live` value at which the next collection fires.
    threshold: usize,
    collections: u64,
    total_allocated: u64,
    total_freed: u64,
}

impl Heap {
    fn new() -> Self {
        Heap {
            arrays: Arena::new(),
            objects: Arena::new(),
            maps: Arena::new(),
            sets: Arena::new(),
            bytes: Arena::new(),
            iters: Arena::new(),
            closures: Arena::new(),
            upvalues: Arena::new(),
            live: 0,
            threshold: MIN_THRESHOLD,
            collections: 0,
            total_allocated: 0,
            total_freed: 0,
        }
    }

    fn stats(&self) -> HeapStats {
        HeapStats {
            live: self.live,
            collections: self.collections,
            total_allocated: self.total_allocated,
            total_freed: self.total_freed,
        }
    }

    /// Sweep every arena; returns the total objects reclaimed.
    fn sweep(&mut self) -> usize {
        self.arrays.sweep()
            + self.objects.sweep()
            + self.maps.sweep()
            + self.sets.sweep()
            + self.bytes.sweep()
            + self.iters.sweep()
            + self.closures.sweep()
            + self.upvalues.sweep()
    }
}

thread_local! {
    /// The per-thread managed heap. A `Vm` runs on a single thread, so
    /// the VM and every native module reach the same heap with no
    /// plumbing.
    static HEAP: RefCell<Heap> = RefCell::new(Heap::new());
}

/// Current heap counters.
pub fn stats() -> HeapStats {
    HEAP.with(|h| h.borrow().stats())
}

// ---------------------------------------------------------------------
// Per-kind glue: kind marker, `alloc_*`, `GcRef::borrow{,_mut}`
// ---------------------------------------------------------------------

macro_rules! gc_kind {
    ($kind:ident, $payload:ty, $arena:ident, $alloc:ident) => {
        /// Zero-sized type-level tag for one collectable kind.
        pub struct $kind;

        impl GcRef<$kind> {
            /// Shared borrow of the heap object. Panics on a stale handle.
            pub fn borrow(self) -> GcReadGuard<$payload> {
                let cell = HEAP.with(|h| h.borrow().$arena.cell(self.index, self.generation));
                read_guard(cell)
            }

            /// Exclusive borrow of the heap object. Panics on a stale handle.
            // Part of every handle's API; closures happen not to mutate.
            #[allow(dead_code)]
            pub fn borrow_mut(self) -> GcWriteGuard<$payload> {
                let cell = HEAP.with(|h| h.borrow().$arena.cell(self.index, self.generation));
                write_guard(cell)
            }
        }

        /// Allocate `value` on the managed heap.
        pub fn $alloc(value: $payload) -> GcRef<$kind> {
            HEAP.with(|h| {
                let mut heap = h.borrow_mut();
                let (index, generation) = heap.$arena.alloc(value);
                heap.live += 1;
                heap.total_allocated += 1;
                GcRef { index, generation, _kind: PhantomData }
            })
        }
    };
}

gc_kind!(ArrayKind, Vec<Value>, arrays, alloc_array);
gc_kind!(ObjectKind, IndexMap<Rc<str>, Value>, objects, alloc_object);
gc_kind!(MapKind, IndexMap<MapKey, Value>, maps, alloc_map);
gc_kind!(SetKind, IndexSet<MapKey>, sets, alloc_set);
gc_kind!(BytesKind, Vec<u8>, bytes, alloc_bytes);
gc_kind!(IterKind, IterState, iters, alloc_iter);
gc_kind!(ClosureKind, Closure, closures, alloc_closure);
gc_kind!(UpvalueKind, Upvalue, upvalues, alloc_upvalue);

// ---------------------------------------------------------------------
// Tracing: the `Trace` trait, the `Marker`, and `collect`
// ---------------------------------------------------------------------

/// A type that owns [`GcRef`] handles the collector must reach.
/// Implemented for [`Value`] and every managed payload type.
pub trait Trace {
    /// Mark every handle directly owned by `self`.
    fn trace(&self, m: &mut Marker);
}

/// A managed object of any kind, queued on the mark worklist.
enum AnyRef {
    Array(GcRef<ArrayKind>),
    Object(GcRef<ObjectKind>),
    Map(GcRef<MapKind>),
    Iter(GcRef<IterKind>),
    Closure(GcRef<ClosureKind>),
    Upvalue(GcRef<UpvalueKind>),
}

/// Drives the mark phase. Carries the heap plus an explicit worklist —
/// marking never recurses, so a deep or cyclic graph cannot overflow
/// the Rust stack.
pub struct Marker<'h> {
    heap: &'h mut Heap,
    worklist: Vec<AnyRef>,
}

impl<'h> Marker<'h> {
    fn new(heap: &'h mut Heap) -> Self {
        Marker { heap, worklist: Vec::new() }
    }

    pub fn mark_array(&mut self, r: GcRef<ArrayKind>) {
        if self.heap.arrays.mark(r.index, r.generation) {
            self.worklist.push(AnyRef::Array(r));
        }
    }
    pub fn mark_object(&mut self, r: GcRef<ObjectKind>) {
        if self.heap.objects.mark(r.index, r.generation) {
            self.worklist.push(AnyRef::Object(r));
        }
    }
    pub fn mark_map(&mut self, r: GcRef<MapKind>) {
        if self.heap.maps.mark(r.index, r.generation) {
            self.worklist.push(AnyRef::Map(r));
        }
    }
    pub fn mark_set(&mut self, r: GcRef<SetKind>) {
        // Sets hold only primitive keys — nothing managed to trace, so
        // marking the slot is the whole job.
        self.heap.sets.mark(r.index, r.generation);
    }
    pub fn mark_bytes(&mut self, r: GcRef<BytesKind>) {
        // A `Vec<u8>` owns no handles — marking the slot is the whole job.
        self.heap.bytes.mark(r.index, r.generation);
    }
    pub fn mark_iter(&mut self, r: GcRef<IterKind>) {
        if self.heap.iters.mark(r.index, r.generation) {
            self.worklist.push(AnyRef::Iter(r));
        }
    }
    pub fn mark_closure(&mut self, r: GcRef<ClosureKind>) {
        if self.heap.closures.mark(r.index, r.generation) {
            self.worklist.push(AnyRef::Closure(r));
        }
    }
    pub fn mark_upvalue(&mut self, r: GcRef<UpvalueKind>) {
        if self.heap.upvalues.mark(r.index, r.generation) {
            self.worklist.push(AnyRef::Upvalue(r));
        }
    }

    /// Drain the worklist, tracing each freshly-marked object.
    fn run(&mut self) {
        while let Some(item) = self.worklist.pop() {
            // Clone the payload `Rc` out so the heap borrow is released
            // before `trace` re-enters the marker.
            match item {
                AnyRef::Array(r) => {
                    let cell = self.heap.arrays.cell(r.index, r.generation);
                    cell.borrow().trace(self);
                }
                AnyRef::Object(r) => {
                    let cell = self.heap.objects.cell(r.index, r.generation);
                    cell.borrow().trace(self);
                }
                AnyRef::Map(r) => {
                    let cell = self.heap.maps.cell(r.index, r.generation);
                    cell.borrow().trace(self);
                }
                AnyRef::Iter(r) => {
                    let cell = self.heap.iters.cell(r.index, r.generation);
                    cell.borrow().trace(self);
                }
                AnyRef::Closure(r) => {
                    let cell = self.heap.closures.cell(r.index, r.generation);
                    cell.borrow().trace(self);
                }
                AnyRef::Upvalue(r) => {
                    let cell = self.heap.upvalues.cell(r.index, r.generation);
                    cell.borrow().trace(self);
                }
            }
        }
    }
}

impl Trace for Value {
    fn trace(&self, m: &mut Marker) {
        match self {
            Value::Array(r) => m.mark_array(*r),
            Value::Object(r) => m.mark_object(*r),
            Value::Map(r) => m.mark_map(*r),
            Value::Set(r) => m.mark_set(*r),
            Value::Bytes(r) => m.mark_bytes(*r),
            Value::Iter(r) => m.mark_iter(*r),
            Value::Function(r) => m.mark_closure(*r),
            Value::Null
            | Value::Bool(_)
            | Value::Int(_)
            | Value::Float(_)
            | Value::Str(_)
            | Value::Range(_)
            | Value::NativeFn(_)
            | Value::BigInt(_) => {}
        }
    }
}

impl Trace for Vec<Value> {
    fn trace(&self, m: &mut Marker) {
        for v in self {
            v.trace(m);
        }
    }
}

impl Trace for IndexMap<Rc<str>, Value> {
    fn trace(&self, m: &mut Marker) {
        for v in self.values() {
            v.trace(m);
        }
    }
}

impl Trace for IndexMap<MapKey, Value> {
    fn trace(&self, m: &mut Marker) {
        for v in self.values() {
            v.trace(m);
        }
    }
}

impl Trace for IterState {
    fn trace(&self, m: &mut Marker) {
        // An iterator keeps its backing collection alive — without this
        // a `for` loop over an otherwise-unreferenced collection would
        // sweep it mid-iteration.
        match self {
            IterState::Array { array, .. } => m.mark_array(*array),
            IterState::Object { object, .. }
            | IterState::IterObject { object, .. } => m.mark_object(*object),
            IterState::Map { map, .. } => m.mark_map(*map),
            IterState::Set { set, .. } => m.mark_set(*set),
            IterState::Bytes { bytes, .. } => m.mark_bytes(*bytes),
            IterState::Range { .. } | IterState::String { .. } => {}
        }
    }
}

impl Trace for Closure {
    fn trace(&self, m: &mut Marker) {
        // `function` is an `Rc<Function>` — immutable, acyclic, unmanaged.
        for up in &self.upvalues {
            m.mark_upvalue(*up);
        }
    }
}

impl Trace for Upvalue {
    fn trace(&self, m: &mut Marker) {
        match self {
            // `Open` lives on the VM value stack, itself a root.
            Upvalue::Open(_) => {}
            Upvalue::Closed(v) => v.trace(m),
        }
    }
}

/// Run one mark-sweep collection. `trace_roots` marks the root set (the
/// caller — the VM — knows where the roots are). Returns the number of
/// objects reclaimed.
pub fn collect(trace_roots: impl FnOnce(&mut Marker)) -> usize {
    HEAP.with(|h| {
        let mut heap = h.borrow_mut();
        {
            let mut marker = Marker::new(&mut heap);
            trace_roots(&mut marker);
            marker.run();
        }
        let freed = heap.sweep();
        heap.live -= freed;
        heap.total_freed += freed as u64;
        heap.collections += 1;
        // Grow the trigger to twice the surviving population so a
        // steadily-growing heap collects O(log n) times, not O(n).
        heap.threshold = (heap.live * 2).max(MIN_THRESHOLD);
        freed
    })
}

/// Whether a collection should run at the next VM safepoint — `true`
/// once the live population reaches the threshold, or always under
/// torture mode.
pub fn should_collect() -> bool {
    torture_enabled() || HEAP.with(|h| {
        let h = h.borrow();
        h.live >= h.threshold
    })
}

// ---------------------------------------------------------------------
// Torture mode (wired into collection in a later phase)
// ---------------------------------------------------------------------

/// When true, the VM collects on every dispatch-loop iteration — a
/// stress mode that turns any missing GC root into an immediate
/// stale-handle panic. Enabled by the `gc-torture` Cargo feature or the
/// `TIGR_GC_TORTURE` environment variable.
pub fn torture_enabled() -> bool {
    #[cfg(feature = "gc-torture")]
    {
        true
    }
    #[cfg(not(feature = "gc-torture"))]
    {
        // Read the environment once per thread, then cache.
        thread_local! {
            static TORTURE: bool = std::env::var_os("TIGR_GC_TORTURE").is_some();
        }
        TORTURE.with(|t| *t)
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_alloc_returns_distinct_indices() {
        let mut a: Arena<i32> = Arena::new();
        let (i0, g0) = a.alloc(10);
        let (i1, g1) = a.alloc(20);
        assert_eq!((i0, g0), (0, 0));
        assert_eq!((i1, g1), (1, 0));
        assert_eq!(*a.cell(i0, g0).borrow(), 10);
        assert_eq!(*a.cell(i1, g1).borrow(), 20);
    }

    #[test]
    fn sweep_reclaims_unmarked_and_clears_marks() {
        let mut a: Arena<i32> = Arena::new();
        let (i0, _) = a.alloc(1);
        let (i1, _) = a.alloc(2);
        a.slots[i0 as usize].mark = true; // keep slot 0
        let freed = a.sweep();
        assert_eq!(freed, 1);
        // mark on the survivor was cleared, ready for the next cycle
        assert!(!a.slots[i0 as usize].mark);
        // slot 1 went onto the free list
        assert_eq!(a.free, vec![i1]);
    }

    #[test]
    fn free_slot_is_recycled_with_bumped_generation() {
        let mut a: Arena<i32> = Arena::new();
        let (i0, g0) = a.alloc(1);
        let freed = a.sweep(); // nothing marked -> slot 0 freed
        assert_eq!(freed, 1);
        let (i1, g1) = a.alloc(2); // recycles slot 0
        assert_eq!(i1, i0);
        assert_eq!(g0, 0);
        assert_eq!(g1, 1, "generation must bump on reuse");
        assert_eq!(*a.cell(i1, g1).borrow(), 2);
    }

    #[test]
    #[should_panic(expected = "stale handle")]
    fn stale_handle_panics() {
        let mut a: Arena<i32> = Arena::new();
        let (i0, g0) = a.alloc(99);
        a.sweep(); // frees slot 0
        a.alloc(123); // recycles it, bumping the generation
        let _ = a.cell(i0, g0); // old handle -> generation mismatch
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn out_of_range_handle_panics() {
        let a: Arena<i32> = Arena::new();
        let _ = a.cell(7, 0);
    }

    #[test]
    fn alloc_and_borrow_through_public_api() {
        let r = alloc_array(vec![Value::Int(1)]);
        r.borrow_mut().push(Value::Int(2));
        let guard = r.borrow();
        assert_eq!(guard.len(), 2);
        assert_eq!(guard[0], Value::Int(1));
        assert_eq!(guard[1], Value::Int(2));
    }

    #[test]
    fn distinct_kinds_have_distinct_handles() {
        let arr = alloc_array(vec![]);
        let set = alloc_set(IndexSet::new());
        // Different concrete `GcRef<K>` types — this only compiles
        // because the kind tag keeps them apart. Identity holds.
        assert_eq!(arr, arr);
        assert_eq!(set, set);
    }

    use crate::vm::chunk::Chunk;
    use crate::vm::value::Function;

    fn test_function() -> Rc<Function> {
        Rc::new(Function {
            arity: 0,
            has_rest: false,
            chunk: Chunk::new(),
            upvalues: Vec::new(),
            name: None,
        })
    }

    #[test]
    fn collect_keeps_rooted_drops_unrooted() {
        let keep = alloc_array(vec![Value::Int(1)]);
        let _drop = alloc_array(vec![Value::Int(2)]);
        // Exactly one root: `keep`. Everything else is unreachable.
        collect(|m| m.mark_array(keep));
        assert_eq!(stats().live, 1);
        assert_eq!(*keep.borrow(), vec![Value::Int(1)]); // no panic ⇒ alive
    }

    #[test]
    fn collect_reclaims_unrooted_object_cycle() {
        let o = alloc_object(IndexMap::new());
        // o.self = o — a reference cycle `Rc` could never reclaim.
        o.borrow_mut().insert(Rc::from("self"), Value::Object(o));
        collect(|_m| {}); // no roots
        assert_eq!(stats().live, 0, "unrooted cycle must be reclaimed");
    }

    #[test]
    fn collect_reclaims_closure_upvalue_cycle() {
        let up = alloc_upvalue(Upvalue::Open(0));
        let cl = alloc_closure(Closure {
            function: test_function(),
            upvalues: vec![up],
        });
        // Close the upvalue over its own closure: cl → up → cl.
        *up.borrow_mut() = Upvalue::Closed(Value::Function(cl));
        collect(|_m| {});
        assert_eq!(stats().live, 0, "closure/upvalue cycle must be reclaimed");
    }

    #[test]
    fn collect_traces_through_iterator_to_backing_array() {
        let arr = alloc_array(vec![Value::Int(7)]);
        let it = alloc_iter(IterState::Array { array: arr, index: 0 });
        // Only the iterator is rooted; the array is reachable solely
        // *through* it. `IterState::trace` must keep the array alive.
        collect(|m| m.mark_iter(it));
        assert_eq!(stats().live, 2);
        assert_eq!(*arr.borrow(), vec![Value::Int(7)]); // no panic ⇒ alive
    }
}
