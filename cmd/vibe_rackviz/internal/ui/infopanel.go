package ui

import (
	"fmt"
	"strings"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/netbox"
	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/pdu"
)

func (a *App) renderInfo(width int) string {
	if name := a.selectedPDUName(); name != "" {
		return a.renderPDUInfo(name, width)
	}
	d := a.selectedDevice()
	if d == nil {
		rack := a.currentRack()
		if rack == nil {
			return styleDim.Render("nothing selected")
		}
		return a.renderRackInfo(rack, width)
	}

	inner := width - 2
	var sb strings.Builder
	writeKV(&sb, "Role", d.Role.Name)
	if d.DeviceType.Model != "" {
		writeKV(&sb, "Type", strings.TrimSpace(d.DeviceType.Manufacturer.Name+" "+d.DeviceType.Model))
	}
	writeKV(&sb, "Status", d.Status.Label)
	if d.Serial != "" {
		writeKV(&sb, "Serial", d.Serial)
	}
	if d.PrimaryIP != nil {
		writeKV(&sb, "IP", d.PrimaryIP.Address)
	}
	if d.Position != nil {
		writeKV(&sb, "Position", fmt.Sprintf("U%g (%s)", *d.Position, d.FaceValue()))
	} else if d.Parent != nil {
		writeKV(&sb, "Bay", d.Parent.Name+" / "+d.BayName())
	} else {
		writeKV(&sb, "Position", "0U")
	}
	if d.Description != "" {
		sb.WriteString(styleDim.Render(truncate(d.Description, inner)) + "\n")
	}

	det := a.details[d.ID]
	switch {
	case det == nil:
		sb.WriteString("\n" + styleDim.Render("enter to load details"))
	case det.loading:
		sb.WriteString("\n" + a.spinner.View() + " loading details…")
	default:
		a.writeDetail(&sb, d, det, inner)
	}
	a.writeReadings(&sb, d.Name)
	return strings.TrimRight(sb.String(), "\n")
}

func (a *App) writeDetail(sb *strings.Builder, d *netbox.Device, det *deviceDetail, inner int) {
	if len(det.interfaces) > 0 {
		sb.WriteString("\n" + styleTitle.Render("Network") + "\n")
		for _, iface := range det.interfaces {
			for _, ep := range iface.Endpoints {
				line := fmt.Sprintf("%s → %s %s", iface.Name, ep.Device.Name, shortenPort(ep.Name))
				sb.WriteString(truncate(line, inner) + "\n")
			}
		}
	}
	if len(det.powerPorts) > 0 {
		sb.WriteString("\n" + styleTitle.Render("Power") + "\n")
		var totW, totA float64
		measured, pending := 0, 0
		for _, pp := range det.powerPorts {
			if len(pp.Endpoints) == 0 {
				sb.WriteString(truncate(pp.Name+" → "+styleDim.Render("(not cabled)"), inner) + "\n")
				continue
			}
			for _, ep := range pp.Endpoints {
				line := fmt.Sprintf("%s → %s / %s", pp.Name, ep.Device.Name, ep.Name)
				sb.WriteString(truncate(line, inner) + "\n")
				switch e := a.outletDrawEntry(ep.Device.Name, ep.Name); {
				case e == nil:
				case e.loading:
					pending++
					sb.WriteString(styleDim.Render("  └ measuring…") + "\n")
				case e.err != "":
					sb.WriteString(styleDim.Render(truncate("  └ "+e.err, inner)) + "\n")
				default:
					measured++
					totW += e.watts
					totA += e.amps
					sb.WriteString(truncate(fmt.Sprintf("  └ %.1f W · %.2f A", e.watts, e.amps), inner) + "\n")
				}
			}
		}
		if measured > 0 {
			line := fmt.Sprintf("Total %.1f W · %.2f A", totW, totA)
			if pending > 0 {
				line += " …"
			}
			sb.WriteString(styleTitle.Render(truncate(line, inner)) + "\n")
		}
	}
	if len(det.outlets) > 0 {
		sb.WriteString("\n" + styleTitle.Render(fmt.Sprintf("Outlets (%d)", len(det.outlets))) + "\n")
		for _, o := range det.outlets {
			target := styleDim.Render("(free)")
			if len(o.Endpoints) > 0 {
				target = o.Endpoints[0].Device.Name
			}
			sb.WriteString(truncate(fmt.Sprintf("%-9s %s", o.Name, target), inner) + "\n")
		}
	}
}

// outletDrawEntry resolves the cached W/A reading for one feeding outlet, or
// nil when the outlet's PDU isn't configured / not yet polled.
func (a *App) outletDrawEntry(pduName, outletName string) *outletReadingEntry {
	pcfg, ok := a.cfg.PDUs[pduName]
	if !ok {
		return nil
	}
	idx, err := pdu.MapOutlet(outletName, pcfg.OutletRegex())
	if err != nil {
		return nil
	}
	return a.outletDraw[orKey(pduName, idx)]
}

// writeReadings renders the live power box for configured PDUs.
func (a *App) writeReadings(sb *strings.Builder, deviceName string) {
	if _, configured := a.cfg.PDUs[deviceName]; !configured {
		return
	}
	sb.WriteString("\n" + styleTitle.Render("Power draw") + "\n")
	re := a.readings[deviceName]
	switch {
	case re == nil:
		sb.WriteString(styleDim.Render("(not polled)") + "\n")
	case re.loading:
		sb.WriteString(a.spinner.View() + " polling…\n")
	case re.err != "":
		sb.WriteString(styleErr.Render(re.err) + "\n")
	default:
		for _, r := range re.data {
			fmt.Fprintf(sb, "%s %7.1f W %6.2f A\n", styleDim.Render(fmt.Sprintf("%-6s", r.Label)), r.Watts, r.Amps)
		}
		sb.WriteString(styleDim.Render("as of "+re.at.Format("15:04:05")) + "\n")
	}
}

// renderPDUInfo is the right pane for the PDU view: outlet summary, the
// selected outlet's detail + draw, and the PDU's per-leg readings.
func (a *App) renderPDUInfo(name string, width int) string {
	inner := width - 2
	var sb strings.Builder
	e := a.pduViews[name]
	if e != nil && !e.loading && e.err == "" {
		on, off, free := 0, 0, 0
		for _, r := range e.rows {
			if r.Device == "" {
				free++
			}
			switch e.states[r.Index] {
			case pdu.StateOn:
				on++
			case pdu.StateOff:
				off++
			}
		}
		writeKV(&sb, "Outlets", fmt.Sprintf("%d", len(e.rows)))
		writeKV(&sb, "State", fmt.Sprintf("%d on · %d off · %d free", on, off, free))

		if a.outletCursor >= 0 && a.outletCursor < len(e.rows) {
			r := e.rows[a.outletCursor]
			sb.WriteString("\n" + styleTitle.Render(fmt.Sprintf("Outlet %d · %s", r.Index, r.Name)) + "\n")
			st := "unknown"
			if s, ok := e.states[r.Index]; ok {
				st = s.String()
			}
			writeKV(&sb, "State", st)
			switch {
			case r.Device != "":
				writeKV(&sb, "Feeds", truncate(r.Device+" · "+r.Port, inner-10))
			case r.Desc != "":
				writeKV(&sb, "Feeds", truncate(r.Desc, inner-10))
			default:
				writeKV(&sb, "Feeds", "(free)")
			}
			switch d := a.outletDraw[orKey(name, r.Index)]; {
			case d == nil:
			case d.loading:
				writeKV(&sb, "Draw", "measuring…")
			case d.err != "":
				writeKV(&sb, "Draw", "—")
			default:
				writeKV(&sb, "Draw", fmt.Sprintf("%.1f W · %.2f A", d.watts, d.amps))
			}
		}
	}
	a.writeReadings(&sb, name)
	return strings.TrimRight(sb.String(), "\n")
}

func (a *App) renderRackInfo(rack *netbox.Rack, width int) string {
	var sb strings.Builder
	writeKV(&sb, "Site", rack.Site.Name)
	writeKV(&sb, "Height", fmt.Sprintf("%dU", rack.UHeight))
	writeKV(&sb, "Devices", fmt.Sprintf("%d", rack.DeviceCount))
	if rd := a.rackData[rack.ID]; rd != nil && !rd.loading {
		used := 0
		for _, r := range rd.rows("front") {
			if r.DeviceID != 0 {
				used++
			}
		}
		writeKV(&sb, "Used", fmt.Sprintf("%d/%dU (%d%%)", used, rack.UHeight, used*100/max(rack.UHeight, 1)))
	}
	return strings.TrimRight(sb.String(), "\n")
}

func writeKV(sb *strings.Builder, k, v string) {
	fmt.Fprintf(sb, "%s %s\n", styleDim.Render(fmt.Sprintf("%-9s", k)), v)
}

// shortenPort compresses long Cisco-style interface names for narrow panes.
func shortenPort(name string) string {
	replacements := [][2]string{
		{"TwentyFiveGigE", "Twe"},
		{"TenGigabitEthernet", "Te"},
		{"GigabitEthernet", "Gi"},
		{"FastEthernet", "Fa"},
		{"FortyGigabitEthernet", "Fo"},
		{"HundredGigE", "Hu"},
	}
	for _, r := range replacements {
		if strings.HasPrefix(name, r[0]) {
			return r[1] + strings.TrimPrefix(name, r[0])
		}
	}
	return name
}
