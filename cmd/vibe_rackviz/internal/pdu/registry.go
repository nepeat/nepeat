package pdu

import (
	"context"
	"fmt"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/config"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/secrets"
)

type Factory func(name string, cfg config.PDU) (Controller, error)

var registry = map[string]Factory{
	"":     newNone,
	"none": newNone,
	"raritan-json": func(name string, cfg config.PDU) (Controller, error) {
		if cfg.Host == "" {
			return nil, fmt.Errorf("pdu %s: host not set", name)
		}
		if cfg.Username == "" {
			return nil, fmt.Errorf("pdu %s: username not set", name)
		}
		pass := cfg.Password
		if cfg.PasswordOpRef != "" {
			var err error
			pass, err = secrets.Resolve(cfg.PasswordOpRef)
			if err != nil {
				return nil, err
			}
		}
		if pass == "" {
			return nil, fmt.Errorf("pdu %s: no password (password or password_op_ref)", name)
		}
		return newRaritanJSON(name, cfg.Host, cfg.Username, pass, cfg.TLSVerify), nil
	},
	"raritan-snmp": func(name string, cfg config.PDU) (Controller, error) {
		community := cfg.SNMPCommunity
		if cfg.SNMPOpRef != "" {
			var err error
			community, err = secrets.Resolve(cfg.SNMPOpRef)
			if err != nil {
				return nil, err
			}
		}
		if cfg.Host == "" {
			return nil, fmt.Errorf("pdu %s: host not set", name)
		}
		if community == "" {
			return nil, fmt.Errorf("pdu %s: no SNMP community (snmp_community or snmp_op_ref)", name)
		}
		return newRaritan(name, cfg.Host, community), nil
	},
}

// New builds a controller for one configured PDU.
func New(name string, cfg config.PDU) (Controller, error) {
	factory, ok := registry[cfg.Driver]
	if !ok {
		return nil, fmt.Errorf("pdu %s: unknown driver %q", name, cfg.Driver)
	}
	return factory(name, cfg)
}

func newNone(name string, _ config.PDU) (Controller, error) {
	return noneController{name: name}, nil
}

// noneController is the default stub: no capabilities, every call refuses.
type noneController struct{ name string }

func (n noneController) Name() string { return n.name }
func (n noneController) Caps() Caps   { return 0 }
func (n noneController) OutletState(context.Context, int) (OutletState, error) {
	return StateUnknown, errNotConfigured
}
func (n noneController) OutletStates(context.Context, []int) (map[int]OutletState, error) {
	return nil, errNotConfigured
}
func (n noneController) OutletReading(context.Context, int) (PowerReading, error) {
	return PowerReading{}, errNotConfigured
}
func (n noneController) OutletReadings(context.Context, []int) (map[int]PowerReading, error) {
	return nil, errNotConfigured
}
func (n noneController) PowerOn(context.Context, int) error    { return errNotConfigured }
func (n noneController) PowerOff(context.Context, int) error   { return errNotConfigured }
func (n noneController) PowerCycle(context.Context, int) error { return errNotConfigured }
func (n noneController) Readings(context.Context) ([]PowerReading, error) {
	return nil, errNotConfigured
}

var errNotConfigured = fmt.Errorf("PDU control not configured")
