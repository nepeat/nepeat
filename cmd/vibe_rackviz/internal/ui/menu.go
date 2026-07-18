package ui

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

// actionMenu is the submenu opened by enter on a device row. Opening it
// polls the outlet states; actions that would be no-ops are hidden (all
// outlets on → no "Power on", all off → no "Power off"). Unknown states
// (unreachable PDU, mixed) leave every action visible.
type actionMenu struct {
	device  string
	targets []outletTarget
	pending int // outstanding outlet-state queries
	items   []menuItem
	cursor  int
	note    string
}

type menuItem struct {
	label  string
	action powerAction
}

var styleMenu = lipgloss.NewStyle().
	Border(lipgloss.RoundedBorder()).
	BorderForeground(lipgloss.Color("205")).
	Padding(1, 2)

func (a *App) openMenu() tea.Cmd {
	d := a.selectedDevice()
	if d == nil {
		return nil
	}
	det := a.details[d.ID]
	if det == nil || det.loading {
		// Open the menu as soon as the in-flight detail load lands.
		a.pendingMenu = true
		a.statusLine = loadingDetailsStatus
		return a.selectDevice()
	}
	m := &actionMenu{device: d.Name}
	m.targets = a.buildTargets(det)
	if len(m.targets) == 0 {
		m.note = "no outlet on a configured PDU feeds this device"
		a.menu = m
		return nil
	}
	m.pending = len(m.targets)
	a.menu = m
	var cmds []tea.Cmd
	for _, t := range m.targets {
		cmds = append(cmds, a.outletStateCmd(t.PDU, t.Outlet))
	}
	return tea.Batch(cmds...)
}

// applyState records one outlet-state answer; when all are in, the item
// list is built with no-op actions gated out.
func (m *actionMenu) applyState(msg outletStateMsg) {
	for i := range m.targets {
		t := &m.targets[i]
		if t.PDU == msg.PDU && t.Outlet == msg.Outlet && !t.Resolved {
			t.Resolved = true
			if msg.Err == nil && msg.State != pdu.StateUnknown {
				t.State = msg.State
				t.StateKnown = true
			}
			m.pending--
			break
		}
	}
	if m.pending == 0 && len(m.items) == 0 {
		m.rebuild()
	}
}

func (m *actionMenu) rebuild() {
	allOn := len(m.targets) > 0
	allOff := len(m.targets) > 0
	for _, t := range m.targets {
		if !(t.StateKnown && t.State == pdu.StateOn) {
			allOn = false
		}
		if !(t.StateKnown && t.State == pdu.StateOff) {
			allOff = false
		}
	}
	m.items = nil
	if !allOn {
		m.items = append(m.items, menuItem{"Power on", actionOn})
	}
	if !allOff {
		m.items = append(m.items, menuItem{"Power off", actionOff})
	}
	m.items = append(m.items, menuItem{"Power cycle", actionCycle})
	m.cursor = 0
}

func (a *App) handleMenuKey(msg tea.KeyPressMsg) (tea.Model, tea.Cmd) {
	m := a.menu
	switch msg.String() {
	case "esc", "q", "ctrl+c":
		a.menu = nil
		return a, nil
	case "j", "down":
		if len(m.items) > 0 {
			m.cursor = clamp(m.cursor+1, 0, len(m.items)-1)
		}
		return a, nil
	case "k", "up":
		if len(m.items) > 0 {
			m.cursor = clamp(m.cursor-1, 0, len(m.items)-1)
		}
		return a, nil
	case "enter":
		if m.note != "" {
			a.menu = nil
			return a, nil
		}
		if len(m.items) == 0 {
			return a, nil // states still loading
		}
		action := m.items[m.cursor].action
		a.menu = nil
		// The menu already carries the resolved targets — hand them to the
		// confirmation modal and refresh their states.
		a.modal = &modal{Action: action, Device: m.device, Targets: m.targets}
		var cmds []tea.Cmd
		for _, t := range m.targets {
			cmds = append(cmds, a.outletStateCmd(t.PDU, t.Outlet))
		}
		return a, tea.Batch(cmds...)
	}
	return a, nil
}

func (a *App) renderMenu() string {
	m := a.menu
	// lines/owners built in parallel: owners maps each popup content line to
	// the menu-item index it activates on click (-1 for none).
	var lines []string
	var owners []int
	add := func(line string, owner int) {
		lines = append(lines, line)
		owners = append(owners, owner)
	}
	add(styleTitle.Render(m.device), -1)
	add("", -1)
	switch {
	case m.note != "":
		add(styleDim.Render(m.note), -1)
		add("", -1)
		add(styleHelp.Render("esc to close"), -1)
	default:
		for _, t := range m.targets {
			state := "?"
			if t.StateKnown {
				state = t.State.String()
			}
			add(styleDim.Render(fmt.Sprintf("outlet %d  %s/%s — %s", t.Outlet, t.PDU, t.OutletName, state)), -1)
		}
		add("", -1)
		if len(m.items) == 0 {
			add(a.spinner.View()+" checking outlet states…", -1)
			add("", -1)
			add(styleHelp.Render("esc to close"), -1)
			break
		}
		for i, item := range m.items {
			line := "  " + item.label + "  "
			if i == m.cursor {
				line = styleSelected.Render(line)
			}
			add(line, i)
		}
		add("", -1)
		add(styleHelp.Render("enter select · esc close"), -1)
	}
	a.hit.menuLines = owners
	return styleMenu.Render(strings.Join(lines, "\n"))
}
