package ui

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sync"
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
// each outlet feeds (per NetBox cabling). Outlets rides along so the cabling
// can be cached — re-sweeps after power actions then skip the NetBox call.
type powerStatesMsg struct {
	PDU      string
	ByDevice map[string]pdu.OutletState
	Outlets  []netbox.PowerOutlet
	Err      error
}

type toastClearMsg struct{ gen int }

// loadPowerStatesCmd sweeps one PDU: NetBox outlet→device cabling (cached
// after the first sweep) narrows the driver query to just the outlets that
// actually feed something.
func (a *App) loadPowerStatesCmd(pduName string, pduDeviceID int, cached []netbox.PowerOutlet) tea.Cmd {
	return func() tea.Msg {
		c, err := a.controllerFor(pduName)
		if err != nil {
			return powerStatesMsg{PDU: pduName, Err: err}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
		defer cancel()
		outlets := cached
		if outlets == nil {
			outlets, err = a.client.PowerOutlets(ctx, pduDeviceID)
			if err != nil {
				return powerStatesMsg{PDU: pduName, Err: err}
			}
		}
		regex := a.cfg.PDUs[pduName].OutletRegex()
		var need []int
		for _, o := range outlets {
			if len(o.Endpoints) == 0 {
				continue
			}
			if idx, err := pdu.MapOutlet(o.Name, regex); err == nil {
				need = append(need, idx)
			}
		}
		states, err := c.OutletStates(ctx, need)
		if err != nil {
			return powerStatesMsg{PDU: pduName, Err: err}
		}
		byDev := joinOutletStates(outlets, states, regex)
		return powerStatesMsg{PDU: pduName, ByDevice: byDev, Outlets: outlets}
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

// outletReadingsMsg carries live W/A for a batch of outlets on one PDU.
type outletReadingsMsg struct {
	PDU       string
	Requested []int
	ByOutlet  map[int]pdu.PowerReading
	Err       error
}

func orKey(pduName string, outlet int) string {
	return fmt.Sprintf("%s/%d", pduName, outlet)
}

// outletReadingsCmd fetches draw for several outlets in one shot (performBulk
// on the PX3 JSON-RPC driver, one SNMP session otherwise).
func (a *App) outletReadingsCmd(pduName string, outlets []int) tea.Cmd {
	return func() tea.Msg {
		c, err := a.controllerFor(pduName)
		if err != nil {
			return outletReadingsMsg{PDU: pduName, Requested: outlets, Err: err}
		}
		if c.Caps()&pdu.CapMeter == 0 {
			return outletReadingsMsg{PDU: pduName, Requested: outlets, Err: fmt.Errorf("%s: no metering", pduName)}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		defer cancel()
		byOutlet, err := c.OutletReadings(ctx, outlets)
		return outletReadingsMsg{PDU: pduName, Requested: outlets, ByOutlet: byOutlet, Err: err}
	}
}

// ctrlEntry deduplicates concurrent controller builds for one PDU.
type ctrlEntry struct {
	once sync.Once
	c    pdu.Controller
	err  error
}

// controllerFor builds (and caches) the controller for a configured PDU.
// Distinct PDUs build in parallel; concurrent calls for the same PDU share
// one build (which may block on `op read` the first time). Failed builds are
// retried on the next call.
func (a *App) controllerFor(name string) (pdu.Controller, error) {
	cfg, ok := a.cfg.PDUs[name]
	if !ok {
		return nil, fmt.Errorf("pdu %s not configured", name)
	}
	a.ctrlMu.Lock()
	e, ok := a.controllers[name]
	if !ok {
		e = &ctrlEntry{}
		a.controllers[name] = e
	}
	a.ctrlMu.Unlock()
	e.once.Do(func() { e.c, e.err = pdu.New(name, cfg) })
	if e.err != nil {
		a.ctrlMu.Lock()
		delete(a.controllers, name)
		a.ctrlMu.Unlock()
	}
	return e.c, e.err
}

type prewarmedMsg struct{}

// prewarmControllersCmd resolves credentials and builds every configured
// PDU controller while NetBox data is still loading, so the first power
// sweep doesn't pay the op-read cost.
func (a *App) prewarmControllersCmd() tea.Cmd {
	names := make([]string, 0, len(a.cfg.PDUs))
	for name := range a.cfg.PDUs {
		names = append(names, name)
	}
	if len(names) == 0 {
		return nil
	}
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		defer cancel()
		var wg sync.WaitGroup
		for _, name := range names {
			wg.Add(1)
			go func(name string) {
				defer wg.Done()
				c, err := a.controllerFor(name)
				if err != nil {
					return
				}
				// Open a few connections now (TLS handshakes are the
				// expensive part) so the first sweep reuses warm ones.
				var cw sync.WaitGroup
				for i := 0; i < 3; i++ {
					cw.Add(1)
					go func() {
						defer cw.Done()
						_, _ = c.OutletState(ctx, 1)
					}()
				}
				cw.Wait()
			}(name)
		}
		wg.Wait()
		return prewarmedMsg{}
	}
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
