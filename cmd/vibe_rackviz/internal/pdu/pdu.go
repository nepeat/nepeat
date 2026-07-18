// Package pdu abstracts switched/metered PDU control so the UI can gate
// features on capabilities rather than vendors.
package pdu

import (
	"context"
	"fmt"
	"regexp"
	"strconv"
)

type Caps uint8

const (
	CapSwitch Caps = 1 << iota // outlet on/off/cycle
	CapMeter                   // power readings
)

type OutletState int

const (
	StateUnknown OutletState = iota
	StateOn
	StateOff
)

func (s OutletState) String() string {
	switch s {
	case StateOn:
		return "on"
	case StateOff:
		return "off"
	}
	return "unknown"
}

// PowerReading is one measurement line, e.g. one inlet leg or the total.
type PowerReading struct {
	Label string // "L1", "L2", "Total"
	Watts float64
	Amps  float64
}

type Controller interface {
	Name() string
	Caps() Caps
	OutletState(ctx context.Context, outlet int) (OutletState, error)
	// OutletStates returns outlet states in one sweep. A non-empty outlets
	// list narrows the query to just those outlets (drivers may ignore the
	// hint and return more).
	OutletStates(ctx context.Context, outlets []int) (map[int]OutletState, error)
	// OutletReading returns one outlet's live draw (outlet-metered PDUs).
	OutletReading(ctx context.Context, outlet int) (PowerReading, error)
	// OutletReadings returns live draw for many outlets in as few round
	// trips as the backend allows.
	OutletReadings(ctx context.Context, outlets []int) (map[int]PowerReading, error)
	PowerOn(ctx context.Context, outlet int) error
	PowerOff(ctx context.Context, outlet int) error
	PowerCycle(ctx context.Context, outlet int) error
	Readings(ctx context.Context) ([]PowerReading, error)
}

// MapOutlet extracts the backend outlet index from a NetBox outlet name using
// the configured regex (first capture group, default trailing digits).
func MapOutlet(name, pattern string) (int, error) {
	re, err := regexp.Compile(pattern)
	if err != nil {
		return 0, fmt.Errorf("outlet regex %q: %w", pattern, err)
	}
	m := re.FindStringSubmatch(name)
	if len(m) < 2 {
		return 0, fmt.Errorf("outlet %q does not match %q", name, pattern)
	}
	n, err := strconv.Atoi(m[1])
	if err != nil || n < 1 {
		return 0, fmt.Errorf("outlet %q: bad index %q", name, m[1])
	}
	return n, nil
}
