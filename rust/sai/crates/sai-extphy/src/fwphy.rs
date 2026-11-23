//! A scripted, stateful fake of the Evora PHYs' firmware-message interface, for tests (feature
//! `testing`): it **detects interleaving** the way the hardware punishes it.
//!
//! Per PHY, clause 45 device 1: a *conversation* starts with the first write of its owner to the
//! request words (1.a000..=1.a0ff) or the doorbell (1.8222); writing the doorbell with `cmd` arms
//! the ack 1.8221 with `(cmd & 0xff00) | 0x00e3` (what the AS7772 tables poll for: 0x11e3 after
//! 0x1111, 0x12e3 after 0x12xx); reading 1.8221 returns the ack and clears it (clear on read);
//! the conversation ends when its owner reads the last result word 1.a101 (the result words
//! 1.a100..=1.a1ff hold what [`FwPhy::set`] put there). Every other register is plain storage.
//!
//! The owner is the accessing thread together with its *actor* tag ([`as_actor`], default
//! `"lib"`), so one test thread can play a diag tool and the library in turn. An access of
//! anyone else to that PHY while a conversation is open is recorded ([`FwPhy::violations`])
//! and **breaks** the conversation: its ack reads 0x0000 from then on, so the owner's `P 1.8221`
//! fails as on the hardware ("1.8221 read 0x0000").
use crate::{BusStatus, Dev, PhyBus};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::ThreadId;

thread_local! {
    static ACTOR: Cell<&'static str> = const { Cell::new("lib") };
}

/// Runs `f` with this thread's accesses tagged `tag` (the default tag is `"lib"`).
pub fn as_actor<R>(tag: &'static str, f: impl FnOnce() -> R) -> R {
    let before = ACTOR.with(|a| a.replace(tag));
    let r = f();
    ACTOR.with(|a| a.set(before));
    r
}

type Actor = (ThreadId, &'static str);

fn actor() -> Actor {
    (std::thread::current().id(), ACTOR.with(Cell::get))
}

#[derive(Debug)]
struct Conv {
    owner: Actor,
    broken: bool,
}

#[derive(Debug, Default)]
struct Inner {
    regs: BTreeMap<(u16, u16), u16>,
    conv: BTreeMap<u16, Conv>,
    violations: Vec<String>,
    completed: BTreeMap<&'static str, u64>,
    accesses: u64,
}

/// The fake (one bus of PHYs).
#[derive(Debug, Default)]
pub struct FwPhy {
    inner: Mutex<Inner>,
}

/// The fake as the runtime's bus.
#[derive(Clone, Debug, Default)]
pub struct FwBus(pub Arc<FwPhy>);

impl PhyBus for FwBus {
    fn read(&self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusStatus> {
        Ok(self.0.access(phy, dev.get(), reg, None))
    }
    fn write(&self, phy: u16, dev: Dev, reg: u16, value: u16) -> Result<(), BusStatus> {
        self.0.access(phy, dev.get(), reg, Some(value));
        Ok(())
    }
}

const ACK: u16 = 0x8221;
const DOORBELL: u16 = 0x8222;
const LAST_RESULT: u16 = 0xa101;

fn request(reg: u16) -> bool {
    (0xa000..=0xa0ff).contains(&reg)
}

impl FwPhy {
    /// An empty bus.
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets register 1.`reg` of `phy` (a result word, a plain register).
    pub fn set(&self, phy: u16, reg: u16, value: u16) {
        self.lock().regs.insert((phy, reg), value);
    }

    /// Register 1.`reg` of `phy` as stored.
    pub fn get(&self, phy: u16, reg: u16) -> u16 {
        self.lock().regs.get(&(phy, reg)).copied().unwrap_or(0)
    }

    /// The interleavings seen so far (empty: none).
    pub fn violations(&self) -> Vec<String> {
        self.lock().violations.clone()
    }

    /// Conversations completed by actor tag.
    pub fn completed(&self, tag: &str) -> u64 {
        self.lock().completed.get(tag).copied().unwrap_or(0)
    }

    /// Accesses so far.
    pub fn accesses(&self) -> u64 {
        self.lock().accesses
    }

    /// Whether a conversation on `phy` is open.
    pub fn open(&self, phy: u16) -> bool {
        self.lock().conv.contains_key(&phy)
    }

    /// One access (`write`: the value written; the result: the value read, 0 for a write).
    pub fn access(&self, phy: u16, dev: u8, reg: u16, write: Option<u16>) -> u16 {
        let me = actor();
        let mut g = self.lock();
        let s = &mut *g;
        s.accesses += 1;
        if dev != 1 {
            return 0;
        }
        if let Some(c) = s.conv.get_mut(&phy) {
            if c.owner != me {
                c.broken = true;
                s.violations.push(format!(
                    "phy 0x{phy:x}: {} {} 1.{reg:04x} inside the conversation of {}",
                    me.1,
                    if write.is_some() { "W" } else { "R" },
                    c.owner.1
                ));
            }
        }
        if let Some(v) = write {
            if (request(reg) || reg == DOORBELL) && !s.conv.contains_key(&phy) {
                s.conv.insert(
                    phy,
                    Conv {
                        owner: me,
                        broken: false,
                    },
                );
            }
            if reg == DOORBELL {
                let broken = s.conv.get(&phy).is_some_and(|c| c.broken);
                let ack = if broken { 0 } else { (v & 0xff00) | 0x00e3 };
                s.regs.insert((phy, ACK), ack);
            } else {
                s.regs.insert((phy, reg), v);
            }
            return 0;
        }
        let broken = s.conv.get(&phy).is_some_and(|c| c.broken);
        let value = if reg == ACK {
            let ack = s.regs.insert((phy, ACK), 0).unwrap_or(0);
            if broken { 0 } else { ack }
        } else {
            s.regs.get(&(phy, reg)).copied().unwrap_or(0)
        };
        if reg == LAST_RESULT && s.conv.get(&phy).is_some_and(|c| c.owner == me) {
            s.conv.remove(&phy);
            if !broken {
                *s.completed.entry(me.1).or_insert(0) += 1;
            }
        }
        value
    }
}
