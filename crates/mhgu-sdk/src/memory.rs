//! Access to game-owned memory and the base of the GU main module.
use core::{cell::UnsafeCell, marker::PhantomData, ptr::NonNull};

#[derive(Clone, Copy)]
pub(crate) struct Object<'a>(NonNull<u8>, PhantomData<&'a UnsafeCell<()>>);
impl<'a> Object<'a> {
    // Callers must uphold the live-object and game-thread contract of Monster::from_raw.
    pub(crate) unsafe fn at(addr: u32) -> Option<Self> {
        Some(Self(NonNull::new(addr as *mut u8)?, PhantomData))
    }
    pub(crate) fn from_cell<T>(cell: &'a UnsafeCell<T>) -> Self {
        Self(NonNull::from(cell).cast(), PhantomData)
    }
    // The embedded object must fit in the same live allocation.
    pub(crate) unsafe fn embedded(self, offset: usize) -> Self {
        Self(unsafe { self.0.add(offset) }, PhantomData)
    }
    pub(crate) fn addr(self) -> u32 {
        self.0.as_ptr() as u32
    }
    pub(crate) fn get<T: Copy>(self, field: Field<T>) -> T {
        unsafe { self.0.as_ptr().add(field.0).cast::<T>().read_unaligned() }
    }
    pub(crate) fn set<T>(self, field: Field<T>, value: T) {
        unsafe {
            self.0
                .as_ptr()
                .add(field.0)
                .cast::<T>()
                .write_unaligned(value)
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Field<T>(usize, PhantomData<T>);
impl<T> Field<T> {
    pub(crate) const fn at(offset: usize) -> Self {
        Self(offset, PhantomData)
    }
}

pub(crate) unsafe fn entry(offset: u32) -> usize {
    (unsafe { forge_sys::mem::forge_mem_getMainTextAddr() } + offset) as usize
}

// GU globals contain ARM guest pointers, not host pointers.
pub(crate) unsafe fn singleton<'a>(offset: u32) -> Option<Object<'a>> {
    unsafe { Object::at((entry(offset) as *const u32).read()) }
}
