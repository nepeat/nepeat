package pdu

import (
	"testing"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
)

func TestMapOutlet(t *testing.T) {
	cases := []struct {
		name, pattern string
		want          int
		wantErr       bool
	}{
		{"output6", `(\d+)$`, 6, false},
		{"output24", `(\d+)$`, 24, false},
		{"Outlet 14", `(\d+)$`, 14, false},
		{"PS-B", `(\d+)$`, 0, true},
		{"output0", `(\d+)$`, 0, true},
	}
	for _, c := range cases {
		got, err := MapOutlet(c.name, c.pattern)
		if c.wantErr != (err != nil) {
			t.Errorf("MapOutlet(%q): err=%v, wantErr=%v", c.name, err, c.wantErr)
			continue
		}
		if got != c.want {
			t.Errorf("MapOutlet(%q) = %d, want %d", c.name, got, c.want)
		}
	}
}

func TestNoneDriverRefusesEverything(t *testing.T) {
	c, err := New("dma-pdu-01", configFor("none"))
	if err != nil {
		t.Fatal(err)
	}
	if c.Caps() != 0 {
		t.Errorf("none driver caps = %v, want 0", c.Caps())
	}
	if err := c.PowerOff(nil, 1); err == nil {
		t.Error("none driver PowerOff should error")
	}
}

func TestUnknownDriver(t *testing.T) {
	if _, err := New("x", configFor("frobnicator")); err == nil {
		t.Error("unknown driver should error")
	}
}

func configFor(driver string) config.PDU {
	return config.PDU{Driver: driver}
}
