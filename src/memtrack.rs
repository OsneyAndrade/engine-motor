use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;

pub struct Counting;

thread_local! {
    static ATIVO: Cell<bool> = const { Cell::new(false) };
    static ATUAL: Cell<isize> = const { Cell::new(0) };
    static PICO: Cell<isize> = const { Cell::new(0) };
}

fn somar(delta: isize) {
    let _ = ATIVO.try_with(|ativo| {
        if !ativo.get() {
            return;
        }
        let _ = ATUAL.try_with(|atual| {
            let novo = atual.get().saturating_add(delta);
            atual.set(novo);
            let _ = PICO.try_with(|pico| {
                if novo > pico.get() {
                    pico.set(novo);
                }
            });
        });
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = mimalloc::MiMalloc.alloc(layout);
        if !p.is_null() {
            somar(layout.size() as isize);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        somar(-(layout.size() as isize));
        mimalloc::MiMalloc.dealloc(ptr, layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = mimalloc::MiMalloc.alloc_zeroed(layout);
        if !p.is_null() {
            somar(layout.size() as isize);
        }
        p
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = mimalloc::MiMalloc.realloc(ptr, layout, new_size);
        if !p.is_null() {
            somar(new_size as isize - layout.size() as isize);
        }
        p
    }
}

struct Guarda;

impl Drop for Guarda {
    fn drop(&mut self) {
        let _ = ATIVO.try_with(|a| a.set(false));
    }
}

pub fn medir<T>(f: impl FnOnce() -> T) -> (T, usize) {
    ATUAL.with(|a| a.set(0));
    PICO.with(|p| p.set(0));
    let resultado = {
        let _guarda = Guarda;
        ATIVO.with(|a| a.set(true));
        f()
    };
    let pico = PICO.with(|p| p.get()).max(0) as usize;
    (resultado, pico)
}
