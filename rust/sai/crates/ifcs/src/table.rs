//! The function table: one field per `IFCS_SYMBOLS` entry, each of the header's `<name>_fn` type,
//! filled from `dlsym` answers. Written once by a macro over the list (`ifcs_sys::for_each_symbol`),
//! so a symbol the header gains is in the table without an edit here.
#![allow(unsafe_code)]

use core::ffi::c_void;
use core::ptr::NonNull;

macro_rules! table {
    ($(($name:ident, $ty:ty)),* $(,)?) => {
        /// Every IFCS function this backend may call, resolved.
        #[derive(Clone, Copy)]
        #[allow(dead_code, reason = "every IFCS_SYMBOLS entry is resolved at load (a missing one refuses \
            the library, as the C++); those of areas not ported yet are not called")]
        pub(crate) struct Table {
            $(pub(crate) $name: $ty,)*
        }

        impl Table {
            /// Resolves every entry through `lookup`; the first one it does not find is the error.
            pub(crate) fn resolve(
                mut lookup: impl FnMut(&str) -> Option<NonNull<c_void>>,
            ) -> Result<Self, &'static str> {
                Ok(Self {
                    $($name: {
                        let p = lookup(stringify!($name)).ok_or(stringify!($name))?;
                        // SAFETY: `p` is what dlsym answered for the symbol named like this field,
                        // in libifcs or the tracer that stands in for it. The field's type is the
                        // header's `<name>_fn` typedef of exactly that function (bindgen output of
                        // the generated ifcs_api.h); libifcs 0.12.41 defines the symbol with that
                        // signature: the version contract the header's layout facts and the
                        // "missing symbol (wrong libifcs version?)" refusal stand for. A data
                        // pointer from dlsym is a valid function pointer on these targets (POSIX
                        // dlsym), and an `Option<fn>` has the pointer's size and null niche.
                        unsafe { core::mem::transmute::<*mut c_void, $ty>(p.as_ptr()) }
                    },)*
                })
            }
        }
    };
}

ifcs_sys::for_each_symbol!(table);
