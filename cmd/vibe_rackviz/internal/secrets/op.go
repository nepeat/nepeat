// Package secrets resolves secret references. A value of the form
// "op://vault/item/field" is fetched via the 1Password CLI; anything else is
// returned as-is.
package secrets

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
)

func Resolve(ref string) (string, error) {
	if !strings.HasPrefix(ref, "op://") {
		return ref, nil
	}
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
