package ui

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
	"golang.org/x/sync/errgroup"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

// The PDU view: selecting a PDU in the left pane's PDUs section turns the
// middle pane into a vertical outlet list with live state and draw.

type pduOutletRow struct {
	Index  int
	Name   string // NetBox outlet name
	Device string // connected device name, "" when free
	Port   string // device-side power port name
}

type pduViewEntry struct {
	loading bool
	at      time.Time
	rows    []pduOutletRow
	states  map[int]pdu.OutletState
	err     string
}

type pduViewMsg struct {
	PDU     string
	Outlets []netbox.PowerOutlet
	States  map[int]pdu.OutletState
	Err     error
}

// selectedPDUName returns the PDU picked in the left pane, or "" when a rack
// is selected.
func (a *App) selectedPDUName() string {
	if i := a.rackCursor - len(a.racks); i >= 0 && i < len(a.pduNames) {
		return a.pduNames[i]
	}
	return ""
}

func (a *App) leftCount() int { return len(a.racks) + len(a.pduNames) }

// selectPDU loads the outlet view (and readings) for the selected PDU.
func (a *App) selectPDU(name string) tea.Cmd {
	a.outletCursor = 0
	var cmds []tea.Cmd
	if cmd := a.maybeLoadReadingsName(name); cmd != nil {
		cmds = append(cmds, cmd)
	}
	e := a.pduViews[name]
	if e == nil || (!e.loading && time.Since(e.at) >= readingsRefresh) {
		a.pduViews[name] = &pduViewEntry{loading: true}
		cmds = append(cmds, a.loadPDUViewCmd(name, a.outletsCache[name]))
	} else if !e.loading {
		cmds = append(cmds, a.pduDrawCmds(name)...)
	}
	if len(cmds) == 0 {
		return nil
	}
	return tea.Batch(cmds...)
}

func (a *App) loadPDUViewCmd(name string, cached []netbox.PowerOutlet) tea.Cmd {
	return func() tea.Msg {
		c, err := a.controllerFor(name)
		if err != nil {
			return pduViewMsg{PDU: name, Err: err}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
		defer cancel()
		var (
			outlets []netbox.PowerOutlet
			states  map[int]pdu.OutletState
		)
		g, gctx := errgroup.WithContext(ctx)
		g.Go(func() (err error) {
			outlets = cached
			if outlets == nil {
				outlets, err = a.client.PowerOutletsByName(gctx, name)
			}
			return
		})
		g.Go(func() (err error) { states, err = c.OutletStates(gctx, nil); return })
		if err := g.Wait(); err != nil {
			return pduViewMsg{PDU: name, Err: err}
		}
		return pduViewMsg{PDU: name, Outlets: outlets, States: states}
	}
}

// buildPDURows sorts NetBox outlets into display rows by outlet index.
func buildPDURows(outlets []netbox.PowerOutlet, regex string) []pduOutletRow {
	rows := make([]pduOutletRow, 0, len(outlets))
	for _, o := range outlets {
		r := pduOutletRow{Name: o.Name}
		if idx, err := pdu.MapOutlet(o.Name, regex); err == nil {
			r.Index = idx
		}
		if len(o.Endpoints) > 0 {
			r.Device = o.Endpoints[0].Device.Name
			r.Port = o.Endpoints[0].Name
		}
		rows = append(rows, r)
	}
	sort.Slice(rows, func(i, j int) bool {
		if rows[i].Index != rows[j].Index {
			return rows[i].Index < rows[j].Index
		}
		return rows[i].Name < rows[j].Name
	})
	return rows
}

// pduDrawCmds refreshes stale per-outlet W/A readings for every cabled
// outlet of the PDU (shown inline in the outlet list).
func (a *App) pduDrawCmds(name string) []tea.Cmd {
	e := a.pduViews[name]
	if e == nil || e.loading {
		return nil
	}
	var cmds []tea.Cmd
	for _, r := range e.rows {
		if r.Device == "" || r.Index == 0 {
			continue
		}
		key := orKey(name, r.Index)
		if d := a.outletDraw[key]; d != nil && (d.loading || time.Since(d.at) < readingsRefresh) {
			continue
		}
		a.outletDraw[key] = &outletReadingEntry{loading: true}
		cmds = append(cmds, a.outletReadingCmd(name, r.Index))
	}
	return cmds
}

// openOutletMenu opens the power action menu for the outlet under the cursor
// in the PDU view.
func (a *App) openOutletMenu() tea.Cmd {
	name := a.selectedPDUName()
	e := a.pduViews[name]
	if name == "" || e == nil || e.loading || a.outletCursor >= len(e.rows) {
		return nil
	}
	r := e.rows[a.outletCursor]
	if r.Index == 0 {
		a.errMsg = fmt.Sprintf("outlet %q has no mappable index", r.Name)
		return nil
	}
	target := r.Device
	if target == "" {
		target = "(free outlet)"
	}
	m := &actionMenu{device: name + " / " + r.Name}
	m.targets = []outletTarget{{
		PDU:        name,
		OutletName: r.Name,
		Outlet:     r.Index,
		PortName:   target,
	}}
	if st, ok := e.states[r.Index]; ok && st != pdu.StateUnknown {
		m.targets[0].State = st
		m.targets[0].StateKnown = true
		m.targets[0].Resolved = true
		m.rebuild()
	} else {
		m.pending = 1
	}
	a.menu = m
	if m.pending > 0 {
		return a.outletStateCmd(name, r.Index)
	}
	return nil
}

var dotFree = styleDim.Render("◌")

// renderPDUView draws the outlet list. Hit-testing reuses the elevation line
// map: each outlet line's owner is its row index.
func (a *App) renderPDUView(width int) string {
	name := a.selectedPDUName()
	a.hit.elevScroll = 0
	a.hit.elevLines = a.hit.elevLines[:0]
	e := a.pduViews[name]
	if e == nil || e.loading {
		return a.spinner.View() + " reading outlets…"
	}
	if e.err != "" {
		return styleErr.Render(truncate("✗ "+e.err, width-4))
	}

	inner := width - 4
	var sb strings.Builder
	on, off, free := 0, 0, 0
	for i, r := range e.rows {
		dot := dotFree
		switch e.states[r.Index] {
		case pdu.StateOn:
			dot = dotOn
		case pdu.StateOff:
			dot = dotOff
		}
		label := ""
		switch {
		case r.Device != "":
			label = r.Device
			if r.Port != "" {
				label += " · " + r.Port
			}
		default:
			free++
			label = "(free)"
		}
		switch e.states[r.Index] {
		case pdu.StateOn:
			on++
		case pdu.StateOff:
			off++
		}

		draw := ""
		if r.Device != "" {
			switch d := a.outletDraw[orKey(name, r.Index)]; {
			case d == nil:
			case d.loading:
				draw = "…"
			case d.err == "":
				draw = fmt.Sprintf("%.1f W", d.watts)
			}
		}
		line := fmt.Sprintf("%02d ╶─ %s", r.Index, label)
		if draw != "" {
			gap := inner - lipgloss.Width(line) - lipgloss.Width(draw) - 1
			if gap < 1 {
				gap = 1
			}
			line += strings.Repeat(" ", gap) + draw
		}
		line = truncate(line, inner)
		if i == a.outletCursor && a.focus == focusElevation {
			line = styleSelected.Render(pad(line, inner))
		} else if r.Device == "" {
			line = styleDim.Render(line)
		}
		sb.WriteString(" " + dot + " " + line + "\n")
		a.hit.elevLines = append(a.hit.elevLines, i)
	}
	sb.WriteString("\n " + styleDim.Render(fmt.Sprintf("⚡ %d on · %d off · %d free", on, off, free)) + "\n")
	a.hit.elevLines = append(a.hit.elevLines, -1, -1)
	return strings.TrimRight(sb.String(), "\n")
}
