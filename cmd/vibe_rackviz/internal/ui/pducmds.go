package ui

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"time"

	tea "charm.land/bubbletea/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

const readingsRefresh = 30 * time.Second

type readingsMsg struct {
	PDU      string
	Readings []pdu.PowerReading
	Err      error
}

type readingsTickMsg struct{ PDU string }

type actionResultMsg struct {
	Desc string
	PDU  string
	Err  error
}

// powerStatesMsg carries one PDU's outlet states joined to the device names
// each outlet feeds (per NetBox cabling).
type powerStatesMsg struct {
	PDU      string
	ByDevice map[string]pdu.OutletState
	Err      error
}

type toastClearMsg struct{ gen int }

// loadPowerStatesCmd sweeps one PDU: NetBox outlet→device cabling plus the
// driver's bulk outlet states, joined into device name → powered on/off.
func (a *App) loadPowerStatesCmd(pduName string, pduDeviceID int) tea.Cmd {
	return func() tea.Msg {
		c, err := a.controllerFor(pduName)
		if err != nil {
			return powerStatesMsg{PDU: pduName, Err: err}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
		defer cancel()
		outlets, err := a.client.PowerOutlets(ctx, pduDeviceID)
		if err != nil {
			return powerStatesMsg{PDU: pduName, Err: err}
		}
		states, err := c.OutletStates(ctx)
		if err != nil {
			return powerStatesMsg{PDU: pduName, Err: err}
		}
		byDev := joinOutletStates(outlets, states, a.cfg.PDUs[pduName].OutletRegex())
		return powerStatesMsg{PDU: pduName, ByDevice: byDev}
	}
}

// joinOutletStates maps each fed device to a power state. A device with
// multiple feeds counts as on if any outlet is on, off only if all are off.
func joinOutletStates(outlets []netbox.PowerOutlet, states map[int]pdu.OutletState, regex string) map[string]pdu.OutletState {
	byDev := map[string]pdu.OutletState{}
	for _, o := range outlets {
		if len(o.Endpoints) == 0 {
			continue
		}
		idx, err := pdu.MapOutlet(o.Name, regex)
		if err != nil {
			continue
		}
		st, ok := states[idx]
		if !ok {
			continue
		}
		dev := o.Endpoints[0].Device.Name
		byDev[dev] = combineStates(byDev[dev], st)
	}
	return byDev
}

func combineStates(a, b pdu.OutletState) pdu.OutletState {
	switch {
	case a == pdu.StateOn || b == pdu.StateOn:
		return pdu.StateOn
	case a == pdu.StateOff && b == pdu.StateOff, a == pdu.StateUnknown:
		return b
	case b == pdu.StateUnknown:
		return a
	default:
		return pdu.StateUnknown
	}
}

type outletStateMsg struct {
	PDU    string
	Outlet int
	State  pdu.OutletState
	Err    error
}

// outletReadingMsg is one outlet's live W/A for the device info pane.
type outletReadingMsg struct {
	PDU     string
	Outlet  int
	Reading pdu.PowerReading
	Err     error
}

func orKey(pduName string, outlet int) string {
	return fmt.Sprintf("%s/%d", pduName, outlet)
}

func (a *App) outletReadingCmd(pduName string, outlet int) tea.Cmd {
	return func() tea.Msg {
		c, err := a.controllerFor(pduName)
		if err != nil {
			return outletReadingMsg{PDU: pduName, Outlet: outlet, Err: err}
		}
		if c.Caps()&pdu.CapMeter == 0 {
			return outletReadingMsg{PDU: pduName, Outlet: outlet, Err: fmt.Errorf("%s: no metering", pduName)}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		rd, err := c.OutletReading(ctx, outlet)
		return outletReadingMsg{PDU: pduName, Outlet: outlet, Reading: rd, Err: err}
	}
}

// controllerFor builds (and caches) the controller for a configured PDU.
// Called from tea.Cmd goroutines — guarded by the app mutex; may block on
// `op read` the first time.
func (a *App) controllerFor(name string) (pdu.Controller, error) {
	cfg, ok := a.cfg.PDUs[name]
	if !ok {
		return nil, fmt.Errorf("pdu %s not configured", name)
	}
	a.ctrlMu.Lock()
	defer a.ctrlMu.Unlock()
	if c, ok := a.controllers[name]; ok {
		return c, nil
	}
	c, err := pdu.New(name, cfg)
	if err != nil {
		return nil, err
	}
	a.controllers[name] = c
	return c, nil
}

func (a *App) loadReadingsCmd(pduName string) tea.Cmd {
	return func() tea.Msg {
		c, err := a.controllerFor(pduName)
		if err != nil {
			return readingsMsg{PDU: pduName, Err: err}
		}
		if c.Caps()&pdu.CapMeter == 0 {
			return readingsMsg{PDU: pduName, Err: fmt.Errorf("%s: no metering support", pduName)}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		r, err := c.Readings(ctx)
		return readingsMsg{PDU: pduName, Readings: r, Err: err}
	}
}

func readingsTickCmd(pduName string) tea.Cmd {
	return tea.Tick(readingsRefresh, func(time.Time) tea.Msg {
		return readingsTickMsg{PDU: pduName}
	})
}

func (a *App) outletStateCmd(pduName string, outlet int) tea.Cmd {
	return func() tea.Msg {
		c, err := a.controllerFor(pduName)
		if err != nil {
			return outletStateMsg{PDU: pduName, Outlet: outlet, Err: err}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		st, err := c.OutletState(ctx, outlet)
		return outletStateMsg{PDU: pduName, Outlet: outlet, State: st, Err: err}
	}
}

func (a *App) powerActionCmd(action powerAction, pduName string, outlet int, device string) tea.Cmd {
	return func() tea.Msg {
		desc := fmt.Sprintf("%s %s outlet %d (%s)", action, pduName, outlet, device)
		if a.dryRun {
			logAction("DRY-RUN " + desc)
			return actionResultMsg{Desc: "dry-run: " + desc, PDU: pduName}
		}
		c, err := a.controllerFor(pduName)
		if err != nil {
			return actionResultMsg{Desc: desc, PDU: pduName, Err: err}
		}
		if c.Caps()&pdu.CapSwitch == 0 {
			return actionResultMsg{Desc: desc, PDU: pduName, Err: fmt.Errorf("%s: no switching support", pduName)}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		defer cancel()
		switch action {
		case actionOn:
			err = c.PowerOn(ctx, outlet)
		case actionOff:
			err = c.PowerOff(ctx, outlet)
		case actionCycle:
			err = c.PowerCycle(ctx, outlet)
		}
		if err != nil {
			logAction(fmt.Sprintf("FAIL %s: %v", desc, err))
		} else {
			logAction("OK " + desc)
		}
		return actionResultMsg{Desc: desc, PDU: pduName, Err: err}
	}
}

func logAction(line string) {
	dir, err := os.UserHomeDir()
	if err != nil {
		return
	}
	path := filepath.Join(dir, ".local", "state", "vibe_rackviz")
	if err := os.MkdirAll(path, 0o755); err != nil {
		return
	}
	f, err := os.OpenFile(filepath.Join(path, "actions.log"), os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()
	fmt.Fprintf(f, "%s %s\n", time.Now().Format(time.RFC3339), line)
}
