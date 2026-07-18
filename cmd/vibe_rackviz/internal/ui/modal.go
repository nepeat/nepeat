package ui

import (
	"fmt"
	"strconv"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

type powerAction string

const (
	actionOn    powerAction = "power_on"
	actionOff   powerAction = "power_off"
	actionCycle powerAction = "power_cycle"
)

// outletTarget is one switchable outlet feeding the selected device.
type outletTarget struct {
	PDU        string
	OutletName string
	Outlet     int
	PortName   string // the device-side power port (PSU1, …)
	State      pdu.OutletState
	StateKnown bool
	Resolved   bool // a state query answered (possibly with an error)
}

// modal is the power-action confirmation overlay. Off/cycle require typing
// the outlet number; power-on accepts y when only one outlet is a candidate.
type modal struct {
	Action  powerAction
	Device  string
	Targets []outletTarget
	Input   string
	Err     string
}

var styleModal = lipgloss.NewStyle().
	Border(lipgloss.DoubleBorder()).
	BorderForeground(lipgloss.Color("196")).
	Padding(1, 2)

// buildTargets maps the device's power-port endpoints to switchable outlets
// on configured PDUs. Mapping failures surface as a toast, never a guess.
func (a *App) buildTargets(det *deviceDetail) []outletTarget {
	var targets []outletTarget
	for _, pp := range det.powerPorts {
		for _, ep := range pp.Endpoints {
			pcfg, ok := a.cfg.PDUs[ep.Device.Name]
			if !ok {
				continue
			}
			idx, err := pdu.MapOutlet(ep.Name, pcfg.OutletRegex())
			if err != nil {
				a.errMsg = err.Error()
				continue
			}
			targets = append(targets, outletTarget{
				PDU:        ep.Device.Name,
				OutletName: ep.Name,
				Outlet:     idx,
				PortName:   pp.Name,
			})
		}
	}
	return targets
}

// openModal builds targets from the loaded power-port detail; returns nil cmds
// and a toast error when nothing is actionable.
func (a *App) openModal(action powerAction) tea.Cmd {
	d := a.selectedDevice()
	if d == nil {
		return nil
	}
	det := a.details[d.ID]
	if det == nil || det.loading {
		a.errMsg = "device details not loaded yet"
		return nil
	}
	targets := a.buildTargets(det)
	if len(targets) == 0 {
		if a.errMsg == "" {
			a.errMsg = "no outlet on a configured PDU feeds " + d.Name
		}
		return nil
	}
	var cmds []tea.Cmd
	for _, t := range targets {
		cmds = append(cmds, a.outletStateCmd(t.PDU, t.Outlet))
	}
	a.modal = &modal{Action: action, Device: d.Name, Targets: targets}
	return tea.Batch(cmds...)
}

// handleModalKey processes keys while the modal is open.
func (a *App) handleModalKey(msg tea.KeyPressMsg) (tea.Model, tea.Cmd) {
	m := a.modal
	switch msg.String() {
	case "esc", "ctrl+c", "q":
		a.modal = nil
		return a, nil
	case "backspace":
		if len(m.Input) > 0 {
			m.Input = m.Input[:len(m.Input)-1]
		}
		return a, nil
	case "enter":
		return a.confirmModal()
	case "y":
		// Plain y confirms power-on with exactly one candidate outlet.
		if m.Action == actionOn && len(m.Targets) == 1 && m.Input == "" {
			t := m.Targets[0]
			a.modal = nil
			a.statusLine = fmt.Sprintf("sending %s…", m.Action)
			return a, a.powerActionCmd(m.Action, t.PDU, t.Outlet, m.Device)
		}
	}
	if len(msg.Text) == 1 && msg.Text[0] >= '0' && msg.Text[0] <= '9' {
		m.Input += msg.Text
	}
	return a, nil
}

func (a *App) confirmModal() (tea.Model, tea.Cmd) {
	m := a.modal
	n, err := strconv.Atoi(m.Input)
	if err != nil {
		m.Err = "type the outlet number to confirm"
		return a, nil
	}
	for _, t := range m.Targets {
		if t.Outlet == n {
			a.modal = nil
			a.statusLine = fmt.Sprintf("sending %s…", m.Action)
			return a, a.powerActionCmd(m.Action, t.PDU, t.Outlet, m.Device)
		}
	}
	m.Err = fmt.Sprintf("%d is not a candidate outlet", n)
	m.Input = ""
	return a, nil
}

func (a *App) renderModal() string {
	m := a.modal
	var sb strings.Builder
	sb.WriteString(styleErr.Render(strings.ToUpper(string(m.Action))) + " " + styleTitle.Render(m.Device) + "\n\n")
	for _, t := range m.Targets {
		state := "state unknown"
		if t.StateKnown {
			state = "currently " + t.State.String()
		}
		fmt.Fprintf(&sb, "  outlet %d  %s / %s (%s, %s)\n", t.Outlet, t.PDU, t.OutletName, t.PortName, state)
	}
	sb.WriteString("\n")
	if m.Action == actionOn && len(m.Targets) == 1 {
		sb.WriteString("press y to confirm, esc to cancel")
	} else {
		sb.WriteString("type the outlet number + enter to confirm, esc to cancel")
	}
	if m.Input != "" {
		sb.WriteString("\n> " + m.Input)
	}
	if m.Err != "" {
		sb.WriteString("\n" + styleErr.Render(m.Err))
	}
	return styleModal.Render(sb.String())
}
