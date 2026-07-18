package ui

import (
	tea "charm.land/bubbletea/v2"
)

// Hit-test geometry captured during render. Screen rows: 0 = header,
// 1 = pane top border, 2 = pane title, 3+ = pane content lines.
const (
	mouseBodyTop    = 1 // first body row (pane border)
	mouseContentTop = 2 // first pane content row (title line)
)

type hitState struct {
	paneX      [3]int
	paneW      [3]int
	rackLines  []int // rack-list content line → rack index (-1 none)
	elevLines  []int // unscrolled elevation line → block index (-1 none)
	elevScroll int
	overlayX   int // popup position in body coordinates; overlayW==0 → none
	overlayY   int
	overlayW   int
	overlayH   int
	menuLines  []int // menu popup content line → item index (-1 none)
}

func (a *App) inOverlay(x, y int) bool {
	return a.hit.overlayW > 0 &&
		x >= a.hit.overlayX && x < a.hit.overlayX+a.hit.overlayW &&
		y-mouseBodyTop >= a.hit.overlayY && y-mouseBodyTop < a.hit.overlayY+a.hit.overlayH
}

// paneAt returns which pane covers column x, or -1.
func (a *App) paneAt(x int) int {
	for i := 0; i < 3; i++ {
		if x >= a.hit.paneX[i] && x < a.hit.paneX[i]+a.hit.paneW[i] {
			return i
		}
	}
	return -1
}

func (a *App) handleClick(m tea.Mouse) (tea.Model, tea.Cmd) {
	a.errMsg = ""
	a.toast = ""

	// Popups swallow clicks: outside closes, menu items activate.
	if a.modal != nil {
		if !a.inOverlay(m.X, m.Y) {
			a.modal = nil
		}
		return a, nil
	}
	if a.menu != nil {
		if !a.inOverlay(m.X, m.Y) {
			a.menu = nil
			return a, nil
		}
		// Popup content starts after the border and one padding row.
		line := m.Y - mouseBodyTop - a.hit.overlayY - 2
		if line >= 0 && line < len(a.hit.menuLines) && a.hit.menuLines[line] >= 0 {
			a.menu.cursor = a.hit.menuLines[line]
			return a.handleMenuKey(tea.KeyPressMsg{Code: tea.KeyEnter})
		}
		return a, nil
	}

	pane := a.paneAt(m.X)
	if pane < 0 {
		return a, nil
	}
	a.focus = focusArea(pane)
	line := m.Y - mouseContentTop - 1 // skip the title line

	switch focusArea(pane) {
	case focusRacks:
		if line >= 0 && line < len(a.hit.rackLines) && a.hit.rackLines[line] >= 0 {
			if a.rackCursor != a.hit.rackLines[line] {
				a.rackCursor = a.hit.rackLines[line]
				return a, a.selectRack()
			}
		}
	case focusElevation:
		idx := a.hit.elevScroll + line
		if line >= 0 && idx >= 0 && idx < len(a.hit.elevLines) && a.hit.elevLines[idx] >= 0 {
			bi := a.hit.elevLines[idx]
			if bi == a.devCursor {
				// Clicking the selected device again opens the action menu.
				return a, a.openMenu()
			}
			a.devCursor = bi
			return a, a.selectDevice()
		}
	}
	return a, nil
}

func (a *App) handleWheel(m tea.Mouse) (tea.Model, tea.Cmd) {
	delta := 0
	switch m.Button {
	case tea.MouseWheelUp:
		delta = -1
	case tea.MouseWheelDown:
		delta = 1
	default:
		return a, nil
	}
	if a.modal != nil {
		return a, nil
	}
	if a.menu != nil {
		if n := len(a.menu.items); n > 0 {
			a.menu.cursor = clamp(a.menu.cursor+delta, 0, n-1)
		}
		return a, nil
	}
	if a.paneAt(m.X) == int(focusRacks) {
		a.focus = focusRacks
	} else {
		a.focus = focusElevation
	}
	return a, a.moveCursor(delta)
}
