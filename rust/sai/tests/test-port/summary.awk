# Per-section table of one test_sai run.
# awk -f summary.awk -v plan=F -v events=F -v err=F -v out=F [-v ref=TSV] [-v tsv=OUT] [-v label=rust]
#   plan    plan.awk output (sections in run order, source ranges)
#   events  section_trace events ("" or missing: untraced run, counts from FAIL lines only)
#   err     the run's stderr (FAIL lines, test_port_rust lines, the tracer's @@tp markers)
#   out     the run's stdout ("N checks, M failures" when main() finished)
#   ref     a TSV of the same form as tsv (the C++ reference run) to compare with
#   tsv     where to write: section <TAB> checks <TAB> fails <TAB> state <TAB> first FAIL ("TOTAL" last)
#   sites   "PC <TAB> file:line" of the tracer's fail events (addr2line), optional
#   failures where to write every failing site: section <TAB> file:line <TAB> failures, optional
function readable(f,   l, r) { r = (getline l < f); if (r >= 0) close(f); return r > 0 }
function top() { return sp > 0 ? stk[sp] : "(main)" }
function mtop() { return msp > 0 ? mstk[msp] : "(main)" }
function by_range(fl, ln,   i) {
    for (i = 1; i <= np; i++) if (pfile[order[i]] == fl && ln >= pstart[order[i]] && ln <= pstop[order[i]]) return order[i]
    return "(outside test_* bodies)"
}
function add(name) { if (!(name in known)) { known[name] = 1; order[++np] = name } }
function signame(n) {
    return n == 4 ? "SIGILL" : n == 6 ? "SIGABRT" : n == 7 ? "SIGBUS" : n == 8 ? "SIGFPE" : n == 9 ? "SIGKILL" : n == 11 ? "SIGSEGV" : n == 15 ? "SIGTERM" : "signal " n
}
BEGIN {
    FS = "\t"
    np = 0
    while ((getline l < plan) > 0) {
        split(l, a, "\t")
        add(a[1]); pfile[a[1]] = a[2]; pstart[a[1]] = a[3] + 0; pstop[a[1]] = a[4] + 0; depth[a[1]] = a[5] + 0
    }
    close(plan)
    traced = events != "" && readable(events)
    pc = 0; pf = 0; sp = 0
    if (traced) {
        while ((getline l < events) > 0) {
            n = split(l, a, "\t"); kind = a[1]; c = a[n - 1] + 0; f = a[n] + 0
            if (kind != "timeout" && c >= 0 && f >= 0) {
                cur = top(); if (cur == "(main)") add(cur)
                chk[cur] += c - pc; fl[cur] += f - pf; pc = c; pf = f
            }
            if (kind == "enter") { add(a[2]); stk[++sp] = a[2]; ran[a[2]] = 1; nran++ }
            else if (kind == "leave") { if (sp > 0) sp--; left[a[2]] = 1 }
            else if (kind == "fail") { wsite[++nws] = a[2]; wsec[nws] = top() }
            else if (kind == "nowatch") nowatch = a[2]
            else if (kind == "signal") { sig = a[2]; sig_rip = a[3]; sig_where = a[4]; sig_in = top() }
            else if (kind == "timeout") { timeout = a[2]; timeout_in = top() }
            else if (kind == "exit") { exited = 1; st = a[2] + 0; exit_in = top(); exit_depth = sp }
        }
        close(events)
        total_c = pc; total_f = pf
    }
    if (sites != "") while ((getline l < sites) > 0) { split(l, a, "\t"); site[a[1]] = a[2] }
    for (i = 1; i <= nws; i++) {
        w = (wsite[i] in site) ? site[wsite[i]] : wsite[i]; s = wsec[i]
        if (!(s in wfirst)) wfirst[s] = w
        if (!((s, w) in wcount)) { worder[++nwo] = s SUBSEP w; wsecof[nwo] = s; wsiteof[nwo] = w }
        wcount[s, w]++
    }
    msp = 0
    while ((getline l < err) > 0) {
        if (l ~ /^@@tp enter /) { mstk[++msp] = substr(l, 12); continue }
        if (l ~ /^@@tp leave /) { if (msp > 0) msp--; continue }
        if (l ~ /^FAIL [^:]*:[0-9]+:/) {
            s = substr(l, 6); i = index(s, ":"); fpath = substr(s, 1, i - 1); rest = substr(s, i + 1)
            ln = rest + 0; fb = fpath; sub(/.*\//, "", fb)
            sec = traced ? mtop() : by_range(fb, ln)
            add(sec); flines[sec]++; nfl++
            if (!(sec in first)) first[sec] = fb ":" ln
            continue
        }
        if (l ~ /^test_port_rust: summary /) { split(substr(l, 25), a, " "); ucount[a[1]] = a[2]; continue }
        if (l ~ /^test_port_rust: profile shim: /) { shim[++nshim] = substr(l, 31); continue }
        if (l ~ /^test_port_rust: /) {
            s = substr(l, 17); i = index(s, ": ")
            if (i > 0) { r = substr(s, 1, i - 1); if (!(r in ureason)) { ureq[++nu] = r; ureason[r] = substr(s, i + 2); usec[r] = traced ? mtop() : "?" } }
        }
    }
    close(err)
    finished = 0
    if (out != "") while ((getline l < out) > 0) if (l ~ /^[0-9]+ checks, [0-9]+ failures$/) { split(l, a, " "); finished = 1; fin_c = a[1] + 0; fin_f = a[3] + 0 }
    if (ref != "") while ((getline l < ref) > 0) { split(l, a, "\t"); rc[a[1]] = a[2]; rf[a[1]] = a[3]; has_ref = 1 }

    if (!traced) { total_c = finished ? fin_c : -1; total_f = finished ? fin_f : nfl }
    died = ""
    if (traced && sig != "") died = signame(sig) " in " sig_in " at " sig_rip " (" sig_where ")"
    else if (traced && timeout != "") died = "timeout after " timeout " s in " timeout_in
    else if (traced && exited && exit_depth > 0) died = "exited (status " st ") inside " exit_in
    else if (traced && !exited) died = "no exit seen (tracer lost the test)"
    else if (traced && !finished && st % 128 != 0) died = signame(st % 128) " (no section running)"
    else if (traced && !finished) died = "exited with status " int(st / 256) % 256 " before main() finished" (nran ? "" : " (no section ran: see the stderr head)")
    else if (!traced && !finished) died = "did not finish (untraced: section unknown, see the stderr tail)"

    hdr = sprintf("%-36s %7s %6s  %-22s", "section", "checks", "fails", "first FAIL")
    if (has_ref) hdr = hdr sprintf(" | %7s %6s %7s", "C++ chk", "fails", "d chk")
    print hdr "  state"
    for (i = 1; i <= np; i++) {
        s = order[i]
        if (s == "(main)" && !chk[s] && !flines[s]) continue
        ind = ""; for (d = 0; d < depth[s]; d++) ind = ind "  "
        if (traced) {
            state = !(s in ran) && s != "(main)" ? "not run" : (s == sig_in && sig != "") || (s == timeout_in && timeout != "") || (exited && exit_depth > 0 && s == exit_in) || (!exited && sp > 0 && s == top()) ? "DIED" : fl[s] > 0 ? "fail" : "ok"
            cs = chk[s] + 0; fs = fl[s] + 0
        } else {
            state = flines[s] > 0 ? "fail" : finished ? "ok?" : "?"
            cs = "?"; fs = flines[s] + 0
        }
        if (traced && (s in ran) && fs != flines[s] + 0) state = state " (" flines[s] + 0 " FAIL lines on stderr)"
        if (traced && (s in ran) && fs != flines[s] + 0) nostderr += fs - flines[s]
        row = sprintf("%-36s %7s %6s  %-22s", ind s, cs, fs, (s in wfirst) ? wfirst[s] : (s in first) ? first[s] : "-")
        if (has_ref) row = row sprintf(" | %7s %6s %7s", (s in rc) ? rc[s] : "-", (s in rf) ? rf[s] : "-", (traced && (s in rc)) ? cs - rc[s] : "")
        print row "  " state
        ff = (s in wfirst) ? wfirst[s] : (s in first) ? first[s] : "-"
        if (tsv != "") printf "%s\t%s\t%s\t%s\t%s\n", s, cs, fs, state, ff > tsv
    }
    tc = total_c < 0 ? "?" : total_c
    row = sprintf("%-36s %7s %6s  %-22s", "TOTAL", tc, total_f, "")
    if (has_ref) row = row sprintf(" | %7s %6s %7s", rc["TOTAL"], rf["TOTAL"], (total_c >= 0 && ("TOTAL" in rc)) ? total_c - rc["TOTAL"] : "")
    tstate = died != "" ? "DIED" : (total_f > 0 ? "fail" : "ok")
    print row "  " tstate
    if (tsv != "") { printf "TOTAL\t%s\t%s\t%s\t-\n", tc, total_f, tstate > tsv; close(tsv) }
    print ""
    print "counting: " (traced ? "exact (section_trace: g_checks / g_fail at every section entry and return)" : "UNTRACED: failures by FAIL lines and source line ranges; checks per section unknown")
    if (finished) print "main() finished: " fin_c " checks, " fin_f " failures" (traced && (fin_c != total_c || fin_f != total_f) ? "  (MISMATCH with the traced counters)" : "")
    if (traced) print "failure sites: " (nowatch != "" ? "from FAIL lines only (no hardware watchpoint: " nowatch ")" : "exact (hardware watchpoint on g_fail: " nws + 0 " hits)")
    if (!traced && finished && fin_f > nfl) nostderr = fin_f - nfl
    if (nostderr > 0) print "failures without a FAIL line on the run's stderr: " nostderr " (the suite had fd 2 redirected, e.g. test_macsec's log capture; the watchpoint has their sites)"
    if (died != "") print "died: " died
    if (nwo) {
        print ""
        print "failing checks (section, site, failures):"
        for (i = 1; i <= nwo && i <= 80; i++) printf "  %-30s %-26s %d\n", wsecof[i], wsiteof[i], wcount[wsecof[i], wsiteof[i]]
        if (nwo > 80) print "  ... " nwo - 80 " more sites (failures file)"
    }
    if (failures != "") { for (i = 1; i <= nwo; i++) printf "%s\t%s\t%d\n", wsecof[i], wsiteof[i], wcount[wsecof[i], wsiteof[i]] > failures; if (nwo) close(failures) }
    if (nshim) for (i = 1; i <= nshim; i++) print "profile shim: " shim[i]
    if (nu) {
        print ""
        printf "%-26s %6s  %-28s %s\n", "unanswered request", "calls", "first in section", "first reason"
        for (i = 1; i <= nu; i++) { r = ureq[i]; printf "%-26s %6s  %-28s %s\n", r, (r in ucount) ? ucount[r] : ">=1", usec[r], ureason[r] }
    }
}
