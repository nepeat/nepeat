// Package secrets resolves secret references. A value of the form
// "op://vault/item/field" is fetched via the 1Password CLI; anything else is
// returned as-is.
package secrets

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
	"sync"
)

// Successful op reads are memoized per ref: multiple PDUs typically share
// one credential item, and `op read` costs hundreds of milliseconds each.
var (
	cacheMu sync.Mutex
	cache   = map[string]*cacheEntry{}
)

type cacheEntry struct {
	once  sync.Once
	value string
	err   error
}

func Resolve(ref string) (string, error) {
	if !strings.HasPrefix(ref, "op://") {
		return ref, nil
	}
	cacheMu.Lock()
	e, ok := cache[ref]
	if !ok {
		e = &cacheEntry{}
		cache[ref] = e
	}
	cacheMu.Unlock()
	e.once.Do(func() { e.value, e.err = opRead(ref) })
	if e.err != nil {
		// Don't cache failures — a locked vault may unlock later.
		cacheMu.Lock()
		delete(cache, ref)
		cacheMu.Unlock()
	}
	return e.value, e.err
}

func opRead(ref string) (string, error) {
	out, err := exec.Command("op", "read", "-n", ref).Output()
	if err != nil {
		var stderr string
		if ee, ok := err.(*exec.ExitError); ok {
			stderr = strings.TrimSpace(string(ee.Stderr))
		}
		return "", fmt.Errorf("op read %s: %w %s", ref, err, stderr)
	}
	return strings.TrimSpace(string(out)), nil
}

// NetBoxToken returns the API token: NETBOX_TOKEN env wins, else the
// configured op reference is resolved.
func NetBoxToken(tokenOpRef string) (string, error) {
	if env := os.Getenv("NETBOX_TOKEN"); env != "" {
		return env, nil
	}
	return Resolve(tokenOpRef)
}
