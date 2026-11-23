//! The one model of the process, and the runner that runs its sequences through the C++.
#![allow(unsafe_code, reason = "calling the C++ callbacks: see the crate documentation")]

use crate::ptr::in_slice;
use crate::sys;
use sai_core::oid::RawOid;
use sai_macsec::{Level, Macsec, Options, Params, PortKey, Results, SeqError, SeqRunner};
use sai_macsec_phy::status_error;
use std::collections::BTreeMap;
use std::ffi::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

/// The state of the library: `None` before the first attach and after a detach (then the
/// entry points work on a model without a table).
pub(crate) static STATE: Mutex<Option<State>> = Mutex::new(None);

pub(crate) struct State {
    pub(crate) model: Macsec,
    pub(crate) runner: CRunner,
}

impl State {
    /// A switch without an external MACsec table, logging nowhere (no callbacks yet).
    pub(crate) fn none() -> Self {
        Self {
            model: Macsec::without_table(Options::default()),
            runner: CRunner {
                cb: None,
                names: Vec::new(),
                results: BTreeMap::new(),
            },
        }
    }
}

/// The callbacks of the attach, copied (function pointers only).
pub(crate) type Callbacks = sys::ob_rs_callbacks;

/// [`SeqRunner`] over the C++ `run` callback (`ext_macsec_exec` under the sequence lock).
pub(crate) struct CRunner {
    pub(crate) cb: Option<Callbacks>,
    /// The table's sequences, in the table's order (name order).
    pub(crate) names: Vec<String>,
    /// The result names each sequence reads.
    pub(crate) results: BTreeMap<String, Vec<String>>,
}

/// Receives the words of the results of a run: `ctx` is the `Results` the run lent.
unsafe extern "C" fn put(ctx: *mut c_void, name: *const c_char, name_len: usize, word: u32, value: u16) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: ctx is the &mut Results CRunner::run passed for this run, alive and not
        // otherwise used until the callback returns (the header: put is called during run only).
        let Some(results) = (unsafe { ctx.cast::<Results>().as_mut() }) else {
            return;
        };
        // SAFETY: name_len bytes at name (the callback's contract).
        let Some(name) = (unsafe { in_slice(name.cast::<u8>(), name_len) }) else {
            return;
        };
        if let (Ok(name), Ok(word)) = (std::str::from_utf8(name), usize::try_from(word)) {
            results.set_word(name, word, value);
        }
    }));
}

impl SeqRunner for CRunner {
    fn sequence_names(&self) -> Vec<String> {
        self.names.clone()
    }

    fn has(&self, name: &str) -> bool {
        self.results.contains_key(name)
    }

    fn result_names(&self, name: &str) -> Vec<String> {
        self.results.get(name).cloned().unwrap_or_default()
    }

    fn run(&mut self, port: PortKey, name: &str, params: &Params) -> Result<Results, SeqError> {
        let fail = |why: &str| SeqError {
            status: sai_core::Error::Failure,
            why: why.to_owned(),
        };
        let Some(run) = self.cb.and_then(|cb| cb.run) else {
            return Err(fail("no sequence runner"));
        };
        // the bytes are lent: pointers into the model's wiped storage, for this call only
        let lent: Vec<sys::ob_rs_param> = params
            .names()
            .filter_map(|n| {
                let bytes = params.get(n)?;
                Some(sys::ob_rs_param {
                    name: n.as_ptr().cast(),
                    name_len: n.len(),
                    bytes: bytes.as_ptr(),
                    len: bytes.len(),
                })
            })
            .collect();
        let mut results = Results::new();
        let mut why = [0u8; 512];
        // SAFETY: a C++ function of the library with the header's signature (the attach's
        // contract); every pointer is valid for the call: the name (name.len() bytes), the
        // parameter array and the bytes it points to (borrowed from params), the results
        // (exclusively borrowed until the call returns), why (its length).
        let status = unsafe {
            run(
                port.0,
                name.as_ptr().cast(),
                name.len(),
                lent.as_ptr(),
                lent.len(),
                Some(put),
                (&raw mut results).cast(),
                why.as_mut_ptr().cast(),
                why.len(),
            )
        };
        drop(lent);
        if status == sai_sys::SAI_STATUS_SUCCESS {
            return Ok(results);
        }
        let end = why.iter().position(|b| *b == 0).unwrap_or(why.len());
        Err(SeqError {
            status: status_error(status),
            why: String::from_utf8_lossy(why.get(..end).unwrap_or(&[])).into_owned(),
        })
    }

    fn set_ifg(&mut self, port: PortKey, macsec_bits: u32) -> Result<(), SeqError> {
        let Some(set_ifg) = self.cb.and_then(|cb| cb.set_ifg) else {
            return Ok(());
        };
        // SAFETY: a C++ function of the library with the header's signature; plain values.
        let status = unsafe { set_ifg(port.0, macsec_bits) };
        if status == sai_sys::SAI_STATUS_SUCCESS {
            Ok(())
        } else {
            Err(SeqError {
                status: status_error(status),
                why: format!("inter-frame gap not applied ({status})"),
            })
        }
    }
}

/// The API the C++ logged a line of this text under: the attach's lines are the switch's, the
/// ACL notes the ACL's, everything else MACsec's (sai_macsec.cpp's `OB_*` calls).
pub(crate) fn api_of(text: &str) -> sai_sys::sai_api_t {
    if text.starts_with("external MACsec table:") {
        sai_sys::SAI_API_SWITCH
    } else if text.starts_with("MACsec ACL:") {
        sai_sys::SAI_API_ACL
    } else {
        sai_sys::SAI_API_MACSEC
    }
}

/// The options of a model whose log and ids go to the C++.
pub(crate) fn options(cb: Callbacks) -> Options {
    let log = cb.log.map(|log| -> sai_macsec::Logger {
        Box::new(move |level: Level, text: &str| {
            let level = match level {
                Level::Error => sai_sys::SAI_LOG_LEVEL_ERROR,
                Level::Warn => sai_sys::SAI_LOG_LEVEL_WARN,
                Level::Notice => sai_sys::SAI_LOG_LEVEL_NOTICE,
                Level::Info => sai_sys::SAI_LOG_LEVEL_INFO,
            };
            let (Ok(api), Ok(level)) = (i32::try_from(api_of(text)), i32::try_from(level)) else {
                return;
            };
            // SAFETY: a C++ function of the library with the header's signature; the text is
            // text.len() bytes, alive for the call.
            unsafe { log(api, level, text.as_ptr().cast(), text.len()) };
        })
    });
    let alloc = cb.alloc.map(|alloc| -> sai_macsec::IdAlloc {
        Box::new(move |object_type| {
            let object_type = i32::try_from(object_type).ok()?;
            // SAFETY: a C++ function of the library with the header's signature; a plain value.
            let raw = unsafe { alloc(object_type) };
            (raw != 0).then_some(RawOid(raw))
        })
    });
    Options {
        log,
        alloc,
        ..Options::default()
    }
}
