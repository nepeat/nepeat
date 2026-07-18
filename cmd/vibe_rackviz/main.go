package main

import (
	"context"
	"flag"
	"fmt"
	"os"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/proxy"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/secrets"
)

func main() {
	configPath := flag.String("config", config.DefaultPath(), "path to config.toml")
	list := flag.Bool("list", false, "print racks and exit (no TUI)")
	rack := flag.String("rack", "", "jump straight to this rack by name")
	dryRun := flag.Bool("dry-run", false, "log power actions instead of executing them")
	flag.Parse()

	cfg, err := config.Load(*configPath)
	if err != nil {
		fatal(err)
	}

	// Validate PROXY up front — a typo must not silently bypass the proxy.
	if _, err := proxy.FromEnv(); err != nil {
		fatal(err)
	}

	if *list {
		if err := runList(cfg); err != nil {
			fatal(err)
		}
		return
	}

	if err := runTUI(cfg, *rack, *dryRun); err != nil {
		fatal(err)
	}
}

func runList(cfg *config.Config) error {
	token, err := secrets.NetBoxToken(cfg.NetBox.TokenOpRef)
	if err != nil {
		return err
	}
	client := netbox.New(cfg.NetBox.URL, token)
	ctx := context.Background()
	status, err := client.Status(ctx)
	if err != nil {
		return err
	}
	racks, err := client.ListRacks(ctx)
	if err != nil {
		return err
	}
	fmt.Printf("NetBox %s @ %s — %d racks\n", status.NetBoxVersion, cfg.NetBox.URL, len(racks))
	for _, r := range racks {
		fmt.Printf("  %-24s %-6s %2dU  %d devices\n", r.Name, r.Site.Name, r.UHeight, r.DeviceCount)
	}
	return nil
}

func fatal(err error) {
	fmt.Fprintln(os.Stderr, "vibe_rackviz:", err)
	os.Exit(1)
}
