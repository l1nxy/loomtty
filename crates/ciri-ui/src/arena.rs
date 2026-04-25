//! Bump-allocator arena for per-frame element trees.
//!
//! Element trees in ciri-ui are immediate-mode — rebuilt on every chrome
//! cache miss. Backing every node with a `Box<dyn Element>` calls the
//! system allocator dozens to hundreds of times per cache-miss frame.
//! This module replaces that with a chunked bump arena: every
//! `AnyElement::new` does a pointer bump in a 1 MB chunk, a `Drop`
//! fixup is recorded so the value gets dropped on `clear()`, and the
//! whole frame's allocations vanish in one `clear()` at frame end —
//! no per-element free, no fragmentation.
//!
//! Ported from GPUI's `arena.rs` (Apache-2.0). The shape is intentionally
//! identical: the allocator is straightforward chunked bump, an
//! `ArenaBox<T>` is a smart-pointer that validates against an `Rc<Cell<bool>>`
//! so use-after-clear panics rather than corrupts memory, and dyn-trait
//! coercion goes through `ArenaBox::map`.

use std::{
    alloc::{self, handle_alloc_error},
    cell::{Cell, RefCell},
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    ptr::{self, NonNull},
    rc::Rc,
};

struct ArenaElement {
    value: *mut u8,
    drop: unsafe fn(*mut u8),
}

impl Drop for ArenaElement {
    #[inline(always)]
    fn drop(&mut self) {
        unsafe { (self.drop)(self.value) };
    }
}

struct Chunk {
    start: *mut u8,
    end: *mut u8,
    offset: *mut u8,
}

impl Drop for Chunk {
    fn drop(&mut self) {
        unsafe {
            let chunk_size = self.end.offset_from(self.start) as usize;
            let layout = alloc::Layout::from_size_align_unchecked(chunk_size, 1);
            alloc::dealloc(self.start, layout);
        }
    }
}

impl Chunk {
    fn new(chunk_size: NonZeroUsize) -> Self {
        let layout = alloc::Layout::from_size_align(chunk_size.get(), 1).unwrap();
        let start = unsafe { alloc::alloc(layout) };
        if start.is_null() {
            handle_alloc_error(layout);
        }
        let end = unsafe { start.add(chunk_size.get()) };
        Self {
            start,
            end,
            offset: start,
        }
    }

    fn allocate(&mut self, layout: alloc::Layout) -> Option<NonNull<u8>> {
        let aligned = unsafe { self.offset.add(self.offset.align_offset(layout.align())) };
        let next = unsafe { aligned.add(layout.size()) };

        if next <= self.end {
            self.offset = next;
            NonNull::new(aligned)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.offset = self.start;
    }
}

pub struct Arena {
    chunks: Vec<Chunk>,
    elements: Vec<ArenaElement>,
    valid: Rc<Cell<bool>>,
    current_chunk_index: usize,
    chunk_size: NonZeroUsize,
}

impl Drop for Arena {
    fn drop(&mut self) {
        self.clear();
    }
}

impl Arena {
    pub fn new(chunk_size: usize) -> Self {
        let chunk_size = NonZeroUsize::try_from(chunk_size).unwrap();
        Self {
            chunks: vec![Chunk::new(chunk_size)],
            elements: Vec::new(),
            valid: Rc::new(Cell::new(true)),
            current_chunk_index: 0,
            chunk_size,
        }
    }

    pub fn capacity(&self) -> usize {
        self.chunks.len() * self.chunk_size.get()
    }

    /// Drop every element previously allocated and reset the bump
    /// pointers in every chunk. Underlying chunk allocations are
    /// retained — capacity does not shrink.
    ///
    /// Any [`ArenaBox`] that survived past this call will panic on
    /// dereference (the validity flag is invalidated then re-issued).
    pub fn clear(&mut self) {
        self.valid.set(false);
        self.valid = Rc::new(Cell::new(true));
        self.elements.clear();
        for chunk_index in 0..=self.current_chunk_index {
            self.chunks[chunk_index].reset();
        }
        self.current_chunk_index = 0;
    }

    #[inline(always)]
    pub fn alloc<T>(&mut self, f: impl FnOnce() -> T) -> ArenaBox<T> {
        #[inline(always)]
        unsafe fn inner_writer<T, F>(ptr: *mut T, f: F)
        where
            F: FnOnce() -> T,
        {
            unsafe { ptr::write(ptr, f()) };
        }

        unsafe fn drop_in_place<T>(ptr: *mut u8) {
            unsafe { std::ptr::drop_in_place(ptr.cast::<T>()) };
        }

        let layout = alloc::Layout::new::<T>();
        let mut current_chunk = &mut self.chunks[self.current_chunk_index];
        let ptr = if let Some(ptr) = current_chunk.allocate(layout) {
            ptr.as_ptr()
        } else {
            self.current_chunk_index += 1;
            if self.current_chunk_index >= self.chunks.len() {
                self.chunks.push(Chunk::new(self.chunk_size));
                assert_eq!(self.current_chunk_index, self.chunks.len() - 1);
                log::trace!(
                    "increased ciri-ui element arena capacity to {}kb",
                    self.capacity() / 1024,
                );
            }
            current_chunk = &mut self.chunks[self.current_chunk_index];
            if let Some(ptr) = current_chunk.allocate(layout) {
                ptr.as_ptr()
            } else {
                panic!(
                    "Arena chunk_size of {} is too small to allocate {} bytes",
                    self.chunk_size,
                    layout.size()
                );
            }
        };

        unsafe { inner_writer(ptr.cast(), f) };
        self.elements.push(ArenaElement {
            value: ptr,
            drop: drop_in_place::<T>,
        });

        ArenaBox {
            ptr: ptr.cast(),
            valid: self.valid.clone(),
        }
    }
}

/// Smart pointer to a value owned by an [`Arena`]. Cheap to move; not
/// `Clone` (the arena is the sole owner). Deref panics if the arena
/// has been cleared since this box was issued.
pub struct ArenaBox<T: ?Sized> {
    ptr: *mut T,
    valid: Rc<Cell<bool>>,
}

impl<T: ?Sized> ArenaBox<T> {
    /// Coerce the inner pointer through a closure — typically used to
    /// convert `ArenaBox<E>` into `ArenaBox<dyn Trait>` via an unsized
    /// reference reinterpretation.
    #[inline(always)]
    pub fn map<U: ?Sized>(mut self, f: impl FnOnce(&mut T) -> &mut U) -> ArenaBox<U> {
        ArenaBox {
            ptr: f(&mut self),
            valid: self.valid,
        }
    }

    #[track_caller]
    fn validate(&self) {
        assert!(
            self.valid.get(),
            "attempted to dereference an ArenaBox after its Arena was cleared",
        );
    }
}

impl<T: ?Sized> Deref for ArenaBox<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.validate();
        unsafe { &*self.ptr }
    }
}

impl<T: ?Sized> DerefMut for ArenaBox<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.validate();
        unsafe { &mut *self.ptr }
    }
}

// ─── Current-arena scope (thread-local) ────────────────────────────

thread_local! {
    /// Fallback arena. `with_element_arena` falls back to this when no
    /// app-specific arena has been pushed via [`ElementArenaScope`].
    /// Sized for a typical chrome paint pass; grows automatically if
    /// a frame outsizes it.
    static FALLBACK_ELEMENT_ARENA: RefCell<Arena> = RefCell::new(Arena::new(256 * 1024));

    /// Pointer to the currently active arena. Set by
    /// [`ElementArenaScope::enter`], cleared by its `Drop`. Read by
    /// [`with_element_arena`] inside `Element` builders so that the
    /// element tree allocates into whichever arena the host pushed
    /// for this paint pass.
    static CURRENT_ELEMENT_ARENA: Cell<Option<*const RefCell<Arena>>> =
        const { Cell::new(None) };
}

/// Run `f` with a mutable borrow of the current element arena. Use this
/// inside `Element` constructors that need to bump-allocate (e.g.
/// `AnyElement::new`). When no scope is active the thread-local
/// fallback arena is used — that path is intended for tests and
/// hit-test-only paths that build a one-shot tree, query, and drop;
/// the live render path should always run inside an
/// [`ElementArenaScope`] so frames clear at known boundaries.
///
/// Hosts that drive hit-tests outside of any scope must call
/// [`clear_fallback_element_arena`] at a safe boundary (e.g. between
/// frames or between input events) to prevent unbounded growth.
#[inline]
pub fn with_element_arena<R>(f: impl FnOnce(&mut Arena) -> R) -> R {
    CURRENT_ELEMENT_ARENA.with(|current| {
        if let Some(arena_ptr) = current.get() {
            // SAFETY: The pointer was published by an `ElementArenaScope`
            // whose lifetime brackets the current call. The scope's
            // `Drop` un-publishes it before the borrow can dangle.
            let arena_cell = unsafe { &*arena_ptr };
            f(&mut arena_cell.borrow_mut())
        } else {
            FALLBACK_ELEMENT_ARENA.with_borrow_mut(f)
        }
    })
}

/// RAII guard that publishes `arena` as the active element arena for
/// this thread until the guard drops. Nesting is supported: the previous
/// pointer is restored on drop.
pub struct ElementArenaScope {
    previous: Option<*const RefCell<Arena>>,
}

impl ElementArenaScope {
    pub fn enter(arena: &RefCell<Arena>) -> Self {
        let previous = CURRENT_ELEMENT_ARENA.with(|current| {
            let prev = current.get();
            current.set(Some(arena as *const RefCell<Arena>));
            prev
        });
        Self { previous }
    }
}

impl Drop for ElementArenaScope {
    fn drop(&mut self) {
        CURRENT_ELEMENT_ARENA.with(|current| {
            current.set(self.previous);
        });
    }
}

/// Clear the thread-local fallback arena. Hosts must call this at
/// a safe boundary — i.e. when no [`ArenaBox`] issued by the fallback
/// is still in use. Suitable points are the start of each frame's
/// render, or between successive input events. Without this, every
/// hit-test that goes through the fallback path
/// (build_tree-without-scope) accumulates arena entries for the
/// lifetime of the thread.
pub fn clear_fallback_element_arena() {
    FALLBACK_ELEMENT_ARENA.with_borrow_mut(|a| a.clear());
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;

    #[test]
    fn alloc_and_deref() {
        let mut arena = Arena::new(1024);
        let a = arena.alloc(|| 1u64);
        let b = arena.alloc(|| 2u32);
        let c = arena.alloc(|| 3u16);
        let d = arena.alloc(|| 4u8);
        assert_eq!(*a, 1);
        assert_eq!(*b, 2);
        assert_eq!(*c, 3);
        assert_eq!(*d, 4);
    }

    #[test]
    fn clear_drops_and_resets() {
        let dropped = Rc::new(Cell::new(false));
        struct DropGuard(Rc<Cell<bool>>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let mut arena = Arena::new(64);
        arena.alloc(|| DropGuard(dropped.clone()));
        arena.clear();
        assert!(dropped.get());
    }

    #[test]
    fn grow_when_chunk_full() {
        let mut arena = Arena::new(8);
        arena.alloc(|| 1u64);
        arena.alloc(|| 2u64);
        assert_eq!(arena.capacity(), 16);
        arena.alloc(|| 3u32);
        arena.alloc(|| 4u32);
        assert_eq!(arena.capacity(), 24);
    }

    #[test]
    fn alignment_respected() {
        let mut arena = Arena::new(256);
        let x1 = arena.alloc(|| 1u8);
        let x2 = arena.alloc(|| 2u16);
        let x3 = arena.alloc(|| 3u32);
        let x4 = arena.alloc(|| 4u64);
        assert_eq!(*x1, 1);
        assert_eq!(*x2, 2);
        assert_eq!(*x3, 3);
        assert_eq!(*x4, 4);
        assert_eq!(x1.ptr.align_offset(std::mem::align_of_val(&*x1)), 0);
        assert_eq!(x2.ptr.align_offset(std::mem::align_of_val(&*x2)), 0);
    }

    #[test]
    #[should_panic(expected = "attempted to dereference an ArenaBox after its Arena was cleared")]
    fn use_after_clear_panics() {
        let mut arena = Arena::new(16);
        let value = arena.alloc(|| 1u64);
        arena.clear();
        let _ = *value;
    }

    #[test]
    fn map_to_dyn() {
        trait Greet {
            fn greet(&self) -> &'static str;
        }
        struct Hello;
        impl Greet for Hello {
            fn greet(&self) -> &'static str {
                "hi"
            }
        }
        let mut arena = Arena::new(64);
        let boxed = arena.alloc(|| Hello);
        let dyn_boxed: ArenaBox<dyn Greet> = boxed.map(|h| h as &mut dyn Greet);
        assert_eq!(dyn_boxed.greet(), "hi");
    }
}
