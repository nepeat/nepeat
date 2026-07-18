package main

import (
	"os"

	tea "charm.land/bubbletea/v2"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/ui"
)

func runTUI(cfg *config.Config, jumpRack string, dryRun bool) error {
	// RACKVIZ_DEBUG=/path/to/log enables message tracing via the standard
	// log package (bubbletea owns the terminal, so stderr is useless).
	if path := os.Getenv("RACKVIZ_DEBUG"); path != "" {
		f, err := tea.LogToFile(path, "rackviz")
		if err != nil {
			return err
		}
		defer f.Close()
	}
	app := ui.NewApp(cfg, jumpRack, dryRun)
	_, err := tea.NewProgram(app).Run()
	return err
}
