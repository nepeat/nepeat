package ui

import (
	"fmt"
	"strings"
)

func (a *App) renderRackList(width int) string {
	a.hit.rackLines = a.hit.rackLines[:0]
	var sb strings.Builder
	if len(a.racks) == 0 {
		sb.WriteString(styleDim.Render("(loading…)"))
		return sb.String()
	}
	inner := width - 3
	for i, r := range a.racks {
		line := fmt.Sprintf("%s %dU", r.Name, r.UHeight)
		line = pad(truncate(line, inner), inner)
		if i == a.rackCursor {
			if a.focus == focusRacks {
				line = styleSelected.Render(line)
			} else {
				line = styleTitle.Render(line)
			}
		}
		sb.WriteString(line + "\n")
		a.hit.rackLines = append(a.hit.rackLines, i)
		if i == a.rackCursor {
			sb.WriteString(styleDim.Render(fmt.Sprintf("  %s · %d devices", r.Site.Name, r.DeviceCount)) + "\n")
			a.hit.rackLines = append(a.hit.rackLines, i)
		}
	}

	if len(a.pduNames) > 0 {
		sb.WriteString(styleDim.Render("── PDUs ──") + "\n")
		a.hit.rackLines = append(a.hit.rackLines, -1)
		for i, name := range a.pduNames {
			idx := len(a.racks) + i
			line := pad(truncate("⚡ "+name, inner), inner)
			if idx == a.rackCursor {
				if a.focus == focusRacks {
					line = styleSelected.Render(line)
				} else {
					line = styleTitle.Render(line)
				}
			}
			sb.WriteString(line + "\n")
			a.hit.rackLines = append(a.hit.rackLines, idx)
		}
	}
	return strings.TrimRight(sb.String(), "\n")
}
