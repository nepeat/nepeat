# The sections of tests/test_sai.cpp in run order, from the source (no build needed):
# every "static void test_X()" definition (test_sai.cpp and the .inc files) with its line range,
# walked from main() through the test_*() calls in each body (nested calls nest).
# Out: name <TAB> file <TAB> first line <TAB> last line <TAB> depth
FNR == 1 { fname = FILENAME; sub(/.*\//, "", fname) }
/^static void test_[A-Za-z0-9_]+\(\)$/ { n = $3; sub(/\(\).*/, "", n); cur = n; file[n] = fname; start[n] = FNR; next }
/^int main\(\)/ { cur = "main"; file[cur] = fname; start[cur] = FNR; next }
cur != "" && /^}/ { stop[cur] = FNR; cur = ""; next }
cur != "" && /^[ \t]+test_[A-Za-z0-9_]+\(\);/ { c = $1; sub(/\(\);.*/, "", c); calls[cur] = calls[cur] + 1; call[cur, calls[cur]] = c }
function emit(f, d,   i) {
    if (d >= 0) printf "%s\t%s\t%d\t%d\t%d\n", f, file[f], start[f], stop[f], d
    for (i = 1; i <= calls[f]; i++) emit(call[f, i], d + 1)
}
END { if (!("main" in start)) { print "plan.awk: no main()" > "/dev/stderr"; exit 1 } emit("main", -1) }
