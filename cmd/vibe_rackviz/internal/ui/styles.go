package ui

import (
	"charm.land/lipgloss/v2"
)

var (
	styleTitle = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("205"))
	styleDim   = lipgloss.NewStyle().Foreground(lipgloss.Color("240"))
	styleErr   = lipgloss.NewStyle().Foreground(lipgloss.Color("196")).Bold(true)
	styleToast = lipgloss.NewStyle().Foreground(lipgloss.Color("220"))

	stylePane = lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color("240")).
			Padding(0, 1)
	stylePaneFocused = stylePane.
				BorderForeground(lipgloss.Color("205"))

	styleSelected = lipgloss.NewStyle().
			Foreground(lipgloss.Color("0")).
			Background(lipgloss.Color("205")).
			Bold(true)

	styleHelp = lipgloss.NewStyle().Foreground(lipgloss.Color("244"))
)

// Elevation block backgrounds encode live power state:
// green = powered on, red = powered off, gray = no power info.
var (
	stylePowerOn = lipgloss.NewStyle().
			Background(lipgloss.Color("#2e7d32")).
			Foreground(lipgloss.Color("#ffffff"))
	stylePowerOff = lipgloss.NewStyle().
			Background(lipgloss.Color("#b71c1c")).
			Foreground(lipgloss.Color("#ffffff"))
	stylePowerNone = lipgloss.NewStyle().
			Background(lipgloss.Color("#44484d")).
			Foreground(lipgloss.Color("#e0e0e0"))

	dotOn  = lipgloss.NewStyle().Foreground(lipgloss.Color("#4caf50")).Render("●")
	dotOff = lipgloss.NewStyle().Foreground(lipgloss.Color("#f44336")).Render("●")
)
