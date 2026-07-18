package proxy

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"net/url"
	"sync"
	"time"
)

// UDPForwarder bridges a local UDP socket to a target host:port through a
// SOCKS5 UDP ASSOCIATE relay (RFC 1928). gosnmp can't take a custom dialer,
// so it is pointed at LocalPort() on 127.0.0.1 instead of the real target.
//
// Note: the proxy must actually support UDP ASSOCIATE (dante, sing-box,
// xray, …). OpenSSH `ssh -D` is TCP-only and will not work for SNMP.
type UDPForwarder struct {
	control net.Conn     // TCP control connection; relay lives as long as it does
	relay   *net.UDPConn // connected socket to the proxy's UDP relay
	local   *net.UDPConn // socket the SNMP client talks to
	header  []byte       // SOCKS5 datagram header for the target

	mu     sync.Mutex
	client *net.UDPAddr // last local client seen (gosnmp's source port)

	closeOnce sync.Once
}

func (f *UDPForwarder) LocalPort() uint16 {
	return uint16(f.local.LocalAddr().(*net.UDPAddr).Port)
}

func (f *UDPForwarder) Close() {
	f.closeOnce.Do(func() {
		f.local.Close()
		f.relay.Close()
		f.control.Close()
	})
}

// DialUDPVia performs the SOCKS5 handshake + UDP ASSOCIATE against proxyURL
// and starts forwarding between a fresh local UDP socket and target.
func DialUDPVia(proxyURL *url.URL, targetHost string, targetPort uint16) (*UDPForwarder, error) {
	if !IsSOCKS(proxyURL) {
		return nil, fmt.Errorf("proxy %s is not SOCKS5", proxyURL)
	}
	proxyAddr := proxyURL.Host
	if proxyURL.Port() == "" {
		proxyAddr = net.JoinHostPort(proxyURL.Hostname(), "1080")
	}
	control, err := net.DialTimeout("tcp", proxyAddr, 10*time.Second)
	if err != nil {
		return nil, fmt.Errorf("socks5 %s: %w", proxyAddr, err)
	}
	relayAddr, err := handshakeUDPAssociate(control, proxyURL)
	if err != nil {
		control.Close()
		return nil, fmt.Errorf("socks5 %s: %w", proxyAddr, err)
	}
	// The relay reply may bind 0.0.0.0 — substitute the proxy's own host.
	if relayAddr.IP == nil || relayAddr.IP.IsUnspecified() {
		ips, err := net.LookupIP(proxyURL.Hostname())
		if err != nil || len(ips) == 0 {
			control.Close()
			return nil, fmt.Errorf("socks5 %s: cannot resolve relay host: %v", proxyAddr, err)
		}
		relayAddr.IP = ips[0]
	}
	relay, err := net.DialUDP("udp", nil, relayAddr)
	if err != nil {
		control.Close()
		return nil, fmt.Errorf("socks5 relay %s: %w", relayAddr, err)
	}
	local, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		relay.Close()
		control.Close()
		return nil, err
	}

	f := &UDPForwarder{
		control: control,
		relay:   relay,
		local:   local,
		header:  datagramHeader(targetHost, targetPort),
	}
	go f.pumpOut()
	go f.pumpIn()
	// If the proxy drops the control connection the association is dead;
	// closing everything unblocks the pumps and surfaces an error upstream.
	go func() {
		io.Copy(io.Discard, control)
		f.Close()
	}()
	return f, nil
}

// pumpOut: local client → wrap with SOCKS5 header → relay.
func (f *UDPForwarder) pumpOut() {
	buf := make([]byte, 65535)
	for {
		n, addr, err := f.local.ReadFromUDP(buf)
		if err != nil {
			return
		}
		f.mu.Lock()
		f.client = addr
		f.mu.Unlock()
		pkt := append(append([]byte{}, f.header...), buf[:n]...)
		if _, err := f.relay.Write(pkt); err != nil {
			return
		}
	}
}

// pumpIn: relay → strip SOCKS5 header → local client.
func (f *UDPForwarder) pumpIn() {
	buf := make([]byte, 65535)
	for {
		n, err := f.relay.Read(buf)
		if err != nil {
			return
		}
		payload, ok := stripDatagramHeader(buf[:n])
		if !ok {
			continue
		}
		f.mu.Lock()
		client := f.client
		f.mu.Unlock()
		if client == nil {
			continue
		}
		if _, err := f.local.WriteToUDP(payload, client); err != nil {
			return
		}
	}
}

// handshakeUDPAssociate runs method negotiation (+ optional RFC 1929
// user/pass auth) and the UDP ASSOCIATE request; returns the relay endpoint.
func handshakeUDPAssociate(c net.Conn, u *url.URL) (*net.UDPAddr, error) {
	c.SetDeadline(time.Now().Add(10 * time.Second))
	defer c.SetDeadline(time.Time{})

	user := ""
	pass := ""
	if u.User != nil {
		user = u.User.Username()
		pass, _ = u.User.Password()
	}
	methods := []byte{0x00}
	if user != "" {
		methods = append(methods, 0x02)
	}
	if _, err := c.Write(append([]byte{0x05, byte(len(methods))}, methods...)); err != nil {
		return nil, err
	}
	resp := make([]byte, 2)
	if _, err := io.ReadFull(c, resp); err != nil {
		return nil, err
	}
	switch resp[1] {
	case 0x00:
	case 0x02:
		req := []byte{0x01, byte(len(user))}
		req = append(req, user...)
		req = append(req, byte(len(pass)))
		req = append(req, pass...)
		if _, err := c.Write(req); err != nil {
			return nil, err
		}
		auth := make([]byte, 2)
		if _, err := io.ReadFull(c, auth); err != nil {
			return nil, err
		}
		if auth[1] != 0x00 {
			return nil, errors.New("auth rejected")
		}
	default:
		return nil, errors.New("no acceptable auth method")
	}

	// UDP ASSOCIATE from 0.0.0.0:0 (we don't know our source port yet).
	if _, err := c.Write([]byte{0x05, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0}); err != nil {
		return nil, err
	}
	head := make([]byte, 4)
	if _, err := io.ReadFull(c, head); err != nil {
		return nil, err
	}
	if head[1] != 0x00 {
		return nil, fmt.Errorf("associate rejected: code %d", head[1])
	}
	var ip net.IP
	switch head[3] {
	case 0x01:
		b := make([]byte, 4)
		if _, err := io.ReadFull(c, b); err != nil {
			return nil, err
		}
		ip = net.IP(b)
	case 0x04:
		b := make([]byte, 16)
		if _, err := io.ReadFull(c, b); err != nil {
			return nil, err
		}
		ip = net.IP(b)
	case 0x03:
		l := make([]byte, 1)
		if _, err := io.ReadFull(c, l); err != nil {
			return nil, err
		}
		host := make([]byte, l[0])
		if _, err := io.ReadFull(c, host); err != nil {
			return nil, err
		}
		ips, err := net.LookupIP(string(host))
		if err != nil || len(ips) == 0 {
			return nil, fmt.Errorf("cannot resolve relay %q", host)
		}
		ip = ips[0]
	default:
		return nil, fmt.Errorf("bad ATYP %d", head[3])
	}
	portB := make([]byte, 2)
	if _, err := io.ReadFull(c, portB); err != nil {
		return nil, err
	}
	return &net.UDPAddr{IP: ip, Port: int(binary.BigEndian.Uint16(portB))}, nil
}

// datagramHeader builds the RFC 1928 UDP request header for target (always
// ATYP=domain so the proxy does any name resolution).
func datagramHeader(host string, port uint16) []byte {
	h := []byte{0x00, 0x00, 0x00, 0x03, byte(len(host))}
	h = append(h, host...)
	return binary.BigEndian.AppendUint16(h, port)
}

func stripDatagramHeader(pkt []byte) ([]byte, bool) {
	if len(pkt) < 7 || pkt[2] != 0x00 { // fragmented packets unsupported
		return nil, false
	}
	switch pkt[3] {
	case 0x01:
		if len(pkt) < 10 {
			return nil, false
		}
		return pkt[10:], true
	case 0x04:
		if len(pkt) < 22 {
			return nil, false
		}
		return pkt[22:], true
	case 0x03:
		l := int(pkt[4])
		if len(pkt) < 7+l {
			return nil, false
		}
		return pkt[7+l:], true
	}
	return nil, false
}
