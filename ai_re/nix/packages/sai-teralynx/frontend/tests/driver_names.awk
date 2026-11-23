# Builds the name tables of tests/frontend_driver.cpp from the SAI headers (Makefile:
# build/driver_names.inc). For every "typedef enum _sai_<x>_t { ... } sai_<x>_t;" block:
#   E(sai_<x>_t, <ENUMERATOR>)             every enumerator
#   A(<ATTRIBUTE>, "<@type text>")         every enumerator of an attribute enum (_attr_t)
#                                          whose doc comment carries an @type tag (an alias
#                                          "= <other attribute>" without one: the other's)
# The values are the C compiler's (the .inc is compiled against the same headers).
/^typedef enum _sai_[a-z0-9_]+_t/ {
    inenum = 1
    match($0, /_sai_[a-z0-9_]+_t/)
    etype = substr($0, RSTART + 1, RLENGTH - 1)
    type = ""
    next
}
inenum && /^}/ { inenum = 0; next }
inenum && /@type/ {
    t = $0
    sub(/.*@type[ \t]+/, "", t)
    sub(/[ \t]+$/, "", t)
    type = t
    next
}
inenum && /^[ \t]*SAI_[A-Z0-9_]+[ \t]*(=|,|$)/ {
    n = $0
    sub(/^[ \t]*/, "", n)
    match(n, /^SAI_[A-Z0-9_]+/)
    name = substr(n, 1, RLENGTH)
    printf "E(%s, %s)\n", etype, name
    # an alias without its own tag ("SAI_X_ATTR_OLD = SAI_X_ATTR_NEW,") takes the type of the other
    if (type == "" && match(n, /=[ \t]*SAI_[A-Z0-9_]+[ \t]*,/)) {
        other = substr(n, RSTART, RLENGTH)
        gsub(/[= \t,]/, "", other)
        if (other in types) type = types[other]
    }
    if (etype ~ /_attr_t$/ && type != "") { printf "A(%s, \"%s\")\n", name, type; types[name] = type }
    type = ""
}
