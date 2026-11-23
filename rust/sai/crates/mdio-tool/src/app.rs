//! The run itself: parse every script, bring up the bus, run or verify, report.

use std::cell::RefCell;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::ops::ControlFlow;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mdio_report::{
    DiagBus, FailClass, Mode, Names, PhyState, Report, ScriptStatus, State, Status, TempStatus, classify_run,
    lock_holder, sd_status, strerror,
};
use mdio_script::{
    BusError, Check, Dev, E_ABORT, E_POLL, E_VERIFY, Failure, Hooks, LoadError, MdioBus, ParseError, ParseErrorKind,
    RunError, RunOptions, Script, Stats, VerifyMode,
};

use crate::chip::{Chip, ChipOpener, OpenFailure};
use crate::cli::{self, ME, Options};
use crate::temp;

/// What a run uses of the process around it.
pub struct Env<'a> {
    /// Standard output: the table or JSON of `--verify-only`.
    pub out: &'a mut dyn Write,
    /// Standard error: everything else (the journal).
    pub err: &'a mut dyn Write,
    /// Set by SIGTERM / SIGINT: the run stops between two script operations.
    pub stop: &'a AtomicBool,
    /// Called once every script has parsed: start setting `stop` on signals (before that a
    /// signal ends the process, as nothing has been touched).
    pub arm_signals: &'a mut dyn FnMut(),
    /// The SDK backend; `None` in a build without it.
    pub opener: Option<&'a mut dyn ChipOpener>,
    /// systemd's `NOTIFY_SOCKET`.
    pub notify_socket: Option<OsString>,
    /// Where the SDK locks are (`/run`): to name the holder when the SDK is busy.
    pub lock_dir: PathBuf,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// The C interpreter's message for a file that is not a script: with the path.
fn load_error(path: &[u8], e: &LoadError) -> Vec<u8> {
    let mut m = path.to_vec();
    match e {
        LoadError::File(e) => m.extend_from_slice(format!(": {}", strerror(e)).as_bytes()),
        LoadError::Syntax(ParseError { line, kind, .. }) => {
            let at = |m: &mut Vec<u8>, text: &str| m.extend_from_slice(format!(":{line}: {text}").as_bytes());
            match *kind {
                ParseErrorKind::Empty => m.extend_from_slice(b": empty file"),
                ParseErrorKind::NotAScript => {
                    m.extend_from_slice(b": not an MDIO script (first line must be 'mdio-script 1')");
                }
                ParseErrorKind::LineTooLong => at(&mut m, "line too long"),
                ParseErrorKind::UnknownOperationWord => at(&mut m, "unknown operation"),
                ParseErrorKind::UnknownOperation(op) => {
                    at(&mut m, "unknown operation '");
                    m.extend_from_slice(&[op, b'\'']);
                }
                ParseErrorKind::BadDelay => at(&mut m, "D <usec up to 60000000>"),
                ParseErrorKind::BadArguments(op) => {
                    at(&mut m, "bad arguments for '");
                    m.push(op);
                    m.extend_from_slice(b"' (clause 45 only: dev 1..1f)");
                }
            }
        }
    }
    m
}

/// A run error's message as C words it: "<path>:<line>: ..." or "<path>: ...".
fn run_error(path: &[u8], message: &str) -> Vec<u8> {
    let mut m = path.to_vec();
    let numbered = message
        .strip_prefix("line ")
        .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    match numbered {
        Some(rest) => m.extend_from_slice(format!(":{rest}").as_bytes()),
        None => m.extend_from_slice(format!(": {message}").as_bytes()),
    }
    m
}

#[allow(
    clippy::cast_precision_loss,
    reason = "C prints (double)delay_us / 1e6; a script's delays stay far below 2^53 us"
)]
fn seconds_of(us: u64) -> f64 {
    us as f64 / 1e6
}

/// The result code and the counters of a run.
fn outcome(run: &Result<Stats, RunError>) -> (i32, Stats) {
    match run {
        Ok(stats) => (0, *stats),
        Err(e) => (e.kind.code(), e.stats),
    }
}

/// The state of one invocation (C: the globals of `mdio_script_main.c`).
struct Ctx<'e, 'a> {
    env: &'e mut Env<'a>,
    opts: Options,
    st: Status,
    report: Report,
    t0: Instant,
    next_temp: Option<Instant>,
    next_status: Option<Instant>,
}

impl Ctx<'_, '_> {
    /// One line on stderr, with the tool's name.
    fn say(&mut self, text: &[u8]) {
        let mut line = format!("{ME}: ").into_bytes();
        line.extend_from_slice(text);
        line.push(b'\n');
        let _ = self.env.err.write_all(&line);
    }

    fn notify(&self, text: &[u8]) {
        sd_status(self.env.notify_socket.as_deref(), text);
    }

    fn status_flush(&mut self) {
        self.st.seconds = self.t0.elapsed().as_secs_f64();
        let Some(file) = self.opts.status_file.clone() else {
            return;
        };
        if let Err(e) = self.st.write(Path::new(&file)) {
            let mut m = b"cannot write ".to_vec();
            m.extend_from_slice(file.as_bytes());
            m.extend_from_slice(format!(": {}", strerror(&e)).as_bytes());
            self.say(&m);
        }
    }

    /// This board's hint for the class, when one was given (the last one wins).
    fn board_hint(&mut self, class: FailClass) {
        for (cls, text) in &self.opts.hints {
            if cls == class.as_str().as_bytes() {
                self.st.set_hint(text);
            }
        }
    }

    /// The run is over: fill in the summary, write the files, tell systemd and the log.
    fn finish(&mut self, state: State, class: Option<FailClass>) {
        self.st.state = state;
        self.st.fail_class = class;
        self.st.finished = unix_now();
        if let Some(class) = class {
            self.board_hint(class);
        }
        // The duration first: the summary quotes it (as the C tool, mdio_script_main.c finish()).
        self.st.seconds = self.t0.elapsed().as_secs_f64();
        self.st.summarize();
        self.status_flush();
        let mut line = if state == State::Ok {
            b"ok: ".to_vec()
        } else {
            if let Some(r) = self.st.report.as_ref().filter(|r| !r.phys().is_empty()) {
                if r.count(PhyState::Ok) != r.phys().len() {
                    let mut text =
                        format!("{ME}: PHY registers read at {}:\n", self.st.report_when.unwrap_or("")).into_bytes();
                    r.table(&mut text, self.st.names.as_ref());
                    let _ = self.env.err.write_all(&text);
                }
            }
            format!("FAILED ({}): ", class.map_or(state.as_str(), FailClass::as_str)).into_bytes()
        };
        line.extend_from_slice(self.st.summary());
        self.say(&line);
        if !self.st.hint().is_empty() {
            let mut hint = b"what to do: ".to_vec();
            hint.extend_from_slice(self.st.hint());
            self.say(&hint);
        }
        self.notify(&line);
    }

    /// Classifies what the verification hooks collected and attaches it to the status.
    fn attach_report(&mut self, when: &'static str) {
        self.report.classify();
        self.st.report = Some(self.report.clone());
        self.st.report_when = Some(when);
    }

    fn temp_paths(&self) -> Option<(PathBuf, Option<PathBuf>)> {
        let out = self.opts.temp_out.as_ref()?;
        Some((PathBuf::from(out), self.opts.temp_prom.as_ref().map(PathBuf::from)))
    }

    fn temp_remove(&self) {
        if let Some((out, prom)) = self.temp_paths() {
            temp::remove(&out, prom.as_deref());
        }
    }

    /// Reads the monitors and publishes them, exactly as openbcm-asic-temp and syncd do.
    fn publish_temp(&mut self, chip: &RefCell<Box<dyn Chip>>) {
        let Some((out, prom)) = self.temp_paths() else { return };
        let started = Instant::now();
        let read = chip.borrow_mut().temperatures();
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut t = self.st.temp.unwrap_or_default();
        if ms > t.max_ms {
            t.max_ms = ms;
        }
        let good = read.as_ref().ok().filter(|r| temp::plausible(&r.current));
        let Some(r) = good else {
            if t.failed == 0 {
                let (code, n) = match &read {
                    Ok(r) => ("ok".to_owned(), r.current.len()),
                    Err(text) => (text.clone(), 0),
                };
                let implausible = if read.is_ok() && n > 0 {
                    ", implausible value"
                } else {
                    ""
                };
                self.say(
                    format!("ASIC temperature read failed: {code} ({n} monitors{implausible}); files removed")
                        .as_bytes(),
                );
            }
            t.failed = t.failed.saturating_add(1);
            self.st.temp = Some(t);
            temp::remove(&out, prom.as_deref());
            return;
        };
        if let Err((path, e)) = temp::write(&out, prom.as_deref(), self.opts.unit, &r.current, &r.peak, unix_now()) {
            if t.reads == 0 {
                let mut m = b"cannot write ".to_vec();
                m.extend_from_slice(path.as_os_str().as_bytes());
                m.extend_from_slice(format!(": {}", strerror(&e)).as_bytes());
                self.say(&m);
            }
        }
        t.last_c = f64::from(temp::hottest(&r.current).unwrap_or(0)) / 10.0;
        if t.reads == 0 {
            let mut m = format!(
                "ASIC temperature: {} monitors, max {:.1} C; every {} s to ",
                r.current.len(),
                t.last_c,
                self.opts.temp_interval
            )
            .into_bytes();
            m.extend_from_slice(out.as_os_str().as_bytes());
            m.extend_from_slice(b" while the script runs");
            self.say(&m);
        }
        t.reads = t.reads.saturating_add(1);
        self.st.temp = Some(t);
    }
}

/// The chip as the script's bus, shared with the temperature reads of the tick.
struct SharedBus<'c>(&'c RefCell<Box<dyn Chip>>);

impl MdioBus for SharedBus<'_> {
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusError> {
        self.0.borrow_mut().read(phy, dev, reg)
    }
    fn write(&mut self, phy: u16, dev: Dev, reg: u16, val: u16) -> Result<(), BusError> {
        self.0.borrow_mut().write(phy, dev, reg, val)
    }
}

/// A bus for verification sections: reads go through, a write is refused.
struct ReadOnly<B>(B);

impl<B: MdioBus> MdioBus for ReadOnly<B> {
    fn read(&mut self, phy: u16, dev: Dev, reg: u16) -> Result<u16, BusError> {
        self.0.read(phy, dev, reg)
    }
    fn write(&mut self, _phy: u16, _dev: Dev, _reg: u16, _val: u16) -> Result<(), BusError> {
        Err(BusError) // verification sections have no writes
    }
}

/// Hooks of a read-only pass: every check goes into the report.
struct Collect<'r>(&'r mut Report);

impl Hooks for Collect<'_> {
    fn verify(&mut self, check: &Check<'_>) {
        self.0.add(check);
    }
}

/// Hooks of the run: log lines, the tick between two operations, the verification report.
struct RunHooks<'c, 'e, 'a> {
    ctx: &'c mut Ctx<'e, 'a>,
    chip: &'c RefCell<Box<dyn Chip>>,
    script: usize,
}

impl Hooks for RunHooks<'_, '_, '_> {
    fn log(&mut self, msg: &str) {
        self.ctx.say(msg.as_bytes());
    }

    fn wants_tick(&self) -> bool {
        true
    }

    /// Between two script operations, in the thread that runs the script.
    fn tick(&mut self, progress: &Stats) -> ControlFlow<()> {
        if self.ctx.env.stop.load(Ordering::Relaxed) {
            return ControlFlow::Break(());
        }
        let now = Instant::now();
        if self.ctx.opts.temp_out.is_some() && self.ctx.next_temp.is_none_or(|at| now >= at) {
            self.ctx.publish_temp(self.chip);
            let interval = u64::try_from(self.ctx.opts.temp_interval).unwrap_or(5);
            self.ctx.next_temp = Some(now + std::time::Duration::from_secs(interval));
        }
        if self.ctx.next_status.is_none_or(|at| now >= at) {
            if let Some(s) = self.ctx.st.scripts.get_mut(self.script) {
                s.run = *progress;
            }
            self.ctx.st.summarize();
            self.ctx.status_flush();
            self.ctx.st.set_summary(b""); // rebuilt each time while running
            self.ctx.next_status = Some(now + std::time::Duration::from_secs(5));
        }
        ControlFlow::Continue(())
    }

    fn verify(&mut self, check: &Check<'_>) {
        self.ctx.report.add(check);
    }
}

/// Runs the tool on the arguments after the program name; the exit code.
pub fn run(program: &OsStr, args: &[OsString], env: &mut Env<'_>) -> u8 {
    let opts = match cli::parse(program, args, env.err) {
        Ok(opts) => opts,
        Err(cli::Exit(code)) => return code,
    };
    let mut opts = opts;
    if opts.verify {
        opts.status_file = None; // a look at the PHYs is not a run: the last run's record stays
    }
    let mode = if opts.verify { Mode::Verify } else { Mode::Run };
    let mut ctx = Ctx {
        env,
        st: Status::new(mode, unix_now(), std::process::id()),
        opts,
        report: Report::new(),
        t0: Instant::now(),
        next_temp: None,
        next_status: None,
    };
    if let Some(names) = ctx.opts.names.clone() {
        match Names::load(Path::new(&names)) {
            Ok(names) => ctx.st.names = Some(names),
            Err(e) => {
                let mut m = e.0;
                m.extend_from_slice(b" (names ignored)");
                ctx.say(&m);
            }
        }
    }
    match parse_all(&mut ctx) {
        Ok(scripts) => act(&mut ctx, &scripts),
        Err(code) => code,
    }
}

/// 1. Every script must parse before anything is touched.
fn parse_all(ctx: &mut Ctx<'_, '_>) -> Result<Vec<Script>, u8> {
    let (check, verify) = (ctx.opts.check, ctx.opts.verify);
    let paths = ctx.opts.scripts.clone();
    // The status lists every script from the start (C: the paths are filled in before the parse).
    ctx.st.scripts = paths.iter().map(|p| ScriptStatus::new(p, Stats::default())).collect();
    let mut scripts = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let script = match Script::load(Path::new(path)) {
            Ok(script) => script,
            Err(e) => {
                if let (LoadError::Syntax(pe), Some(s)) = (&e, ctx.st.scripts.get_mut(i)) {
                    s.parsed = pe.stats;
                }
                let mut summary = load_error(path.as_bytes(), &e);
                if !(check || verify) {
                    summary.extend_from_slice(b"; nothing was written to any PHY");
                }
                ctx.st.fail_script = Some(path.as_bytes().to_vec());
                ctx.st.set_summary(&summary);
                if check {
                    let summary = ctx.st.summary().to_vec();
                    ctx.say(&summary);
                } else {
                    ctx.finish(State::Failed, classify_run(e.code(), None, None));
                }
                return Err(5);
            }
        };
        let p = *script.stats();
        if let Some(s) = ctx.st.scripts.get_mut(i) {
            s.parsed = p;
        }
        if check || !verify {
            let mut m = path.as_bytes().to_vec();
            m.extend_from_slice(
                format!(
                    ": {} writes, {} reads ({} polls), {} delays ({:.1} s), {} steps; verification: {} reads from line {}",
                    p.writes,
                    p.reads,
                    p.polls,
                    p.delays,
                    seconds_of(p.delay_us),
                    p.labels,
                    p.verify_checks,
                    p.verify_line
                )
                .as_bytes(),
            );
            ctx.say(&m);
        }
        scripts.push(script);
    }
    Ok(scripts)
}

/// Everything after the parse pass.
fn act(ctx: &mut Ctx<'_, '_>, scripts: &[Script]) -> u8 {
    if ctx.opts.check {
        return 0;
    }
    if ctx.opts.verify && scripts.iter().all(|s| s.stats().verify_line == 0) {
        ctx.say(b"no script has a verification section (assertions at its end): nothing to read");
        return 5;
    }
    (ctx.env.arm_signals)();

    // 2. Verification only, through syncd: no SDK in this process.
    if let (true, Some(socket)) = (ctx.opts.verify, ctx.opts.diag_socket.clone()) {
        let mut line = b"through syncd's diag socket ".to_vec();
        line.extend_from_slice(socket.as_bytes());
        ctx.st.set_sdk(&line);
        let mut bus = DiagBus::new(PathBuf::from(socket));
        verify_pass(ctx, scripts, &mut bus);
        let io_error = bus.last_error().map(<[u8]>::to_vec);
        return verify_report(ctx, io_error.as_deref());
    }

    if ctx.env.opener.is_none() {
        ctx.say(b"built without the SDK: --check and --verify-only --diag-socket only");
        return 1;
    }
    let verify = ctx.opts.verify;
    let config = ctx.opts.config.clone().unwrap_or_default();
    if !verify {
        ctx.status_flush();
        ctx.notify(b"bringing up the SDK (init soc, init misc)");
    }
    let Some(opener) = ctx.env.opener.as_mut() else {
        return 1;
    };
    let (chip, line) = match opener.open(ctx.opts.unit, &config) {
        Ok(up) => up,
        Err(e) => return open_failed(ctx, &e),
    };
    ctx.st.set_sdk(line.as_bytes());
    ctx.say(line.as_bytes());

    if verify {
        let chip = RefCell::new(chip);
        verify_pass(ctx, scripts, &mut ReadOnly(SharedBus(&chip)));
        return verify_report(ctx, None);
    }
    run_scripts(ctx, scripts, chip)
}

/// The SDK did not come up: say why, classified.
fn open_failed(ctx: &mut Ctx<'_, '_>, e: &crate::chip::OpenError) -> u8 {
    let busy = e.failure == OpenFailure::Busy;
    let class = match e.failure {
        OpenFailure::Busy => FailClass::SdkBusy,
        OpenFailure::Config => FailClass::SdkConfig,
        OpenFailure::MdioClock => FailClass::MdioClock,
        OpenFailure::Init => FailClass::SdkInit,
    };
    let summary = if busy {
        let lock = ctx.env.lock_dir.join(format!("openbcm-unit{}.lock", ctx.opts.unit));
        match lock_holder(&lock) {
            Some(holder) => {
                let mut m = b"the SDK lock ".to_vec();
                m.extend_from_slice(lock.as_os_str().as_bytes());
                m.extend_from_slice(format!(" is held by pid {} (", holder.pid).as_bytes());
                m.extend_from_slice(if holder.comm.is_empty() { b"?" } else { &holder.comm });
                m.extend_from_slice(b"): that process owns the switch chip; nothing was written to any PHY");
                m
            }
            None => format!("{}; nothing was written to any PHY", e.message).into_bytes(),
        }
    } else {
        format!("{} ({}); nothing was written to any PHY", e.message, e.code_text).into_bytes()
    };
    ctx.st.set_summary(&summary);
    if ctx.opts.verify {
        let summary = ctx.st.summary().to_vec();
        ctx.say(&summary);
        if busy {
            ctx.say(b"while syncd owns the chip, read through it: --verify-only --diag-socket SOCKET");
        }
    } else {
        ctx.finish(State::Failed, Some(class));
    }
    if busy { 3 } else { 4 }
}

/// Reads the verification section of every script that has one: reads only.
fn verify_pass(ctx: &mut Ctx<'_, '_>, scripts: &[Script], bus: &mut dyn MdioBus) {
    for (i, script) in scripts.iter().enumerate() {
        if script.stats().verify_line == 0 {
            continue;
        }
        let run = script.run(
            bus,
            &mut Collect(&mut ctx.report),
            RunOptions {
                verify: VerifyMode::Only,
            },
        );
        if let Some(s) = ctx.st.scripts.get_mut(i) {
            s.started = true;
            (s.result, s.run) = outcome(&run);
        }
    }
}

/// The end of `--verify-only`: the table or the JSON on stdout.
fn verify_report(ctx: &mut Ctx<'_, '_>, io_error: Option<&[u8]>) -> u8 {
    ctx.attach_report("now");
    ctx.st.finished = unix_now();
    ctx.st.seconds = ctx.t0.elapsed().as_secs_f64();
    let nphy = ctx.report.phys().len();
    let bad = nphy.saturating_sub(ctx.report.count(PhyState::Ok));
    let io_bad = ctx.report.count(PhyState::Io);
    ctx.st.state = if bad > 0 { State::Failed } else { State::Ok };
    if bad > 0 {
        let failed = ctx.st.scripts.iter().rfind(|s| s.started && s.result != 0);
        if let Some(path) = failed.map(|s| s.path.clone()) {
            ctx.st.fail_script = Some(path);
        }
        ctx.st.fail_class = classify_run(E_VERIFY, None, Some(&ctx.report));
        if let Some(class) = ctx.st.fail_class {
            ctx.board_hint(class);
        }
    }
    ctx.st.summarize();
    let mut text = Vec::new();
    if ctx.opts.json {
        ctx.st.json(&mut text);
    } else {
        ctx.report.table(&mut text, ctx.st.names.as_ref());
        if bad > 0 {
            text.extend_from_slice(
                format!(
                    "{bad} of {nphy} PHYs are not as the script expects ({}): ",
                    ctx.st.fail_class.map_or("", FailClass::as_str)
                )
                .as_bytes(),
            );
            text.extend_from_slice(ctx.st.summary());
            text.push(b'\n');
            if !ctx.st.hint().is_empty() {
                text.extend_from_slice(b"what to do: ");
                text.extend_from_slice(ctx.st.hint());
                text.push(b'\n');
            }
        } else {
            text.extend_from_slice(format!("{nphy} of {nphy} PHYs as the script expects\n").as_bytes());
        }
    }
    let _ = ctx.env.out.write_all(&text);
    let _ = ctx.env.out.flush();
    if let (true, Some(e)) = (io_bad > 0, io_error) {
        ctx.say(e);
    }
    if bad > 0 { 2 } else { 0 }
}

/// 3. The run.
fn run_scripts(ctx: &mut Ctx<'_, '_>, scripts: &[Script], chip: Box<dyn Chip>) -> u8 {
    let chip = RefCell::new(chip);
    if ctx.opts.temp_out.is_some() {
        ctx.st.temp = Some(TempStatus::default());
        // The first read after a reset returns a garbage peak (the SDK discards one too).
        let _ = chip.borrow_mut().temperatures();
    }
    ctx.notify(b"running the script");
    for (i, script) in scripts.iter().enumerate() {
        let parsed = *script.stats();
        let path = ctx
            .opts
            .scripts
            .get(i)
            .map(|p| p.as_bytes().to_vec())
            .unwrap_or_default();
        let started = Instant::now();
        if let Some(s) = ctx.st.scripts.get_mut(i) {
            s.started = true;
        }
        ctx.st.fail = Failure::default();
        let run = script.run(
            &mut SharedBus(&chip),
            &mut RunHooks {
                ctx: &mut *ctx,
                chip: &chip,
                script: i,
            },
            RunOptions {
                verify: VerifyMode::Report,
            },
        );
        let (code, stats) = outcome(&run);
        let seconds = started.elapsed().as_secs_f64();
        if let Some(s) = ctx.st.scripts.get_mut(i) {
            (s.result, s.run, s.seconds) = (code, stats, seconds);
        }
        let mut m = path.clone();
        m.extend_from_slice(
            format!(
                ": {} in {seconds:.1} s: {} of {} operations, {} writes, {} reads ({} polls, {} re-reads), {} expected \
                 values differed",
                if code == 0 { "done" } else { "FAILED" },
                stats.ops,
                parsed.ops,
                stats.writes,
                stats.reads,
                stats.polls,
                stats.poll_rereads,
                stats.differs
            )
            .as_bytes(),
        );
        ctx.say(&m);
        let Err(e) = run else { continue };
        ctx.say(&run_error(&path, &e.message));
        ctx.st.fail = e.failure;
        ctx.st.fail_script = Some(path);
        let stopped = ctx.env.stop.load(Ordering::Relaxed);
        let when = if code == E_POLL && parsed.verify_line != 0 && !stopped {
            // Post-mortem, reads only: does the PHY still answer?
            ctx.report.reset();
            let _ = script.run(
                &mut ReadOnly(SharedBus(&chip)),
                &mut Collect(&mut ctx.report),
                RunOptions {
                    verify: VerifyMode::Only,
                },
            );
            "after the failure (reads only)"
        } else {
            "the end of the script"
        };
        if ctx.report.phys().is_empty() {
            ctx.st.report_when = Some(when);
        } else {
            ctx.attach_report(when);
        }
        ctx.temp_remove();
        let class = classify_run(code, Some(ctx.st.fail.phy), ctx.st.report.as_ref());
        let aborted = code == E_ABORT;
        ctx.finish(if aborted { State::Interrupted } else { State::Failed }, class);
        return if aborted { 7 } else { 2 };
    }
    ctx.temp_remove();
    if !ctx.report.phys().is_empty() {
        ctx.attach_report("the end of the script");
    }
    ctx.finish(State::Ok, None);
    0
}
