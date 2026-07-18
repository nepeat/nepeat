package config

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/BurntSushi/toml"
)

type NetBox struct {
	URL        string `toml:"url"`
	TokenOpRef string `toml:"token_op_ref"`
}

type PDU struct {
	Driver          string `toml:"driver"`
	Host            string `toml:"host"`
	SNMPOpRef       string `toml:"snmp_op_ref"`
	SNMPCommunity   string `toml:"snmp_community"`
	OutletNameRegex string `toml:"outlet_name_regex"`

	// raritan-json driver (JSON-RPC over HTTPS)
	Username      string `toml:"username"`
	Password      string `toml:"password"`
	PasswordOpRef string `toml:"password_op_ref"`
	TLSVerify     bool   `toml:"tls_verify"` // default false: PDUs ship self-signed certs
}

type Config struct {
	NetBox NetBox         `toml:"netbox"`
	PDUs   map[string]PDU `toml:"pdu"`
}

// DefaultPath is XDG-style on every platform (~/.config, not the macOS
// "Application Support" dir that os.UserConfigDir returns on darwin).
func DefaultPath() string {
	if xdg := os.Getenv("XDG_CONFIG_HOME"); xdg != "" {
		return filepath.Join(xdg, "vibe_rackviz", "config.toml")
	}
	if home, err := os.UserHomeDir(); err == nil {
		return filepath.Join(home, ".config", "vibe_rackviz", "config.toml")
	}
	return ""
}

// Load reads the TOML config at path, falling back to defaults if the file
// does not exist. Env vars NETBOX_URL / NETBOX_TOKEN override at resolve time.
func Load(path string) (*Config, error) {
	cfg := &Config{
		NetBox: NetBox{
			URL:        "https://infra.dma.space",
			TokenOpRef: "op://Personal/netbox-dma/apikey",
		},
		PDUs: map[string]PDU{},
	}
	if path == "" {
		return cfg, nil
	}
	if _, err := os.Stat(path); os.IsNotExist(err) {
		return cfg, nil
	}
	if _, err := toml.DecodeFile(path, cfg); err != nil {
		return nil, fmt.Errorf("config %s: %w", path, err)
	}
	if env := os.Getenv("NETBOX_URL"); env != "" {
		cfg.NetBox.URL = env
	}
	return cfg, nil
}

func (p PDU) OutletRegex() string {
	if p.OutletNameRegex != "" {
		return p.OutletNameRegex
	}
	return `(\d+)$`
}
