package ui

import (
	"strings"
	"testing"

	"charm.land/lipgloss/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
)

func TestPadAndCenterPad(t *testing.T) {
	p := pad("abc", 10)
	if lipgloss.Width(p) != 10 {
		t.Errorf("pad width = %d, want 10", lipgloss.Width(p))
	}
	if !strings.HasSuffix(p, " ") {
		t.Error("pad must end with NBSP (whitespace-restyle workaround)")
	}
	c := centerPad("abc", 11)
	if lipgloss.Width(c) != 11 {
		t.Errorf("centerPad width = %d, want 11", lipgloss.Width(c))
	}
	if !strings.HasPrefix(c, "    abc") {
		t.Errorf("centerPad not centered: %q", c)
	}
	// Exact-width input passes through untouched.
	if got := pad("abcde", 5); got != "abcde" {
		t.Errorf("pad exact = %q", got)
	}
}

// Regression: lipgloss v2's Style.Width re-wraps lines and strips styling
// from trailing whitespace, clipping block backgrounds at the text edge.
// renderPane must keep the styled padding intact all the way to the border.
func TestBlockBackgroundSurvivesPane(t *testing.T) {
	cfg, _ := config.Load("")
	app := NewApp(cfg, "", false)

	body := stylePowerNone.Render(centerPad("dma-core-a-1", 67))
	line := " 37 " + body
	pane := app.renderPane(focusElevation, 74, 6, "TITLE", line)
	for _, l := range strings.Split(pane, "\n") {
		if !strings.Contains(l, "dma-core-a-1") {
			continue
		}
		// The style reset must come after the padded region ends (the NBSP),
		// not right after the device name.
		name := strings.Index(l, "dma-core-a-1")
		nbsp := strings.Index(l, " ")
		reset := strings.Index(l[name:], "\x1b[m")
		if nbsp < 0 {
			t.Fatalf("styled padding lost (no NBSP): %q", l)
		}
		if reset >= 0 && name+reset < nbsp {
			t.Fatalf("style reset before padding end — background clipped: %q", l)
		}
		return
	}
	t.Fatal("block line not found in pane")
}
