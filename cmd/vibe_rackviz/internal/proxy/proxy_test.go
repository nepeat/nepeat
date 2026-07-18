package proxy

import (
	"encoding/binary"
	"io"
	"net"
	"net/url"
	"testing"
	"time"
)

func TestFromEnv(t *testing.T) {
	cases := []struct {
		env     string
		scheme  string
		wantNil bool
		wantErr bool
	}{
		{"", "", true, false},
		{"socks5://127.0.0.1:1080", "socks5", false, false},
		{"socks5h://jump:1080", "socks5", false, false},
		{"socks://jump:1080", "socks5", false, false},
		{"http://proxy:3128", "http", false, false},
		{"ftp://nope", "", false, true},
	}
	for _, c := range cases {
		t.Setenv("PROXY", c.env)
		u, err := FromEnv()
		if c.wantErr != (err != nil) {
			t.Errorf("FromEnv(%q) err=%v wantErr=%v", c.env, err, c.wantErr)
			continue
		}
		if c.wantErr {
			continue
		}
		if c.wantNil != (u == nil) {
			t.Errorf("FromEnv(%q) = %v, wantNil=%v", c.env, u, c.wantNil)
			continue
		}
		if u != nil && u.Scheme != c.scheme {
			t.Errorf("FromEnv(%q) scheme = %q, want %q", c.env, u.Scheme, c.scheme)
		}
	}
}

// TestUDPForwarderEndToEnd runs a real SOCKS5 UDP ASSOCIATE flow against an
// in-process proxy and UDP echo server.
func TestUDPForwarderEndToEnd(t *testing.T) {
	echoAddr := startUDPEcho(t)
	socksAddr := startMiniSOCKS5(t)

	u, _ := url.Parse("socks5://" + socksAddr)
	fwd, err := DialUDPVia(u, "127.0.0.1", uint16(echoAddr.Port))
	if err != nil {
		t.Fatal(err)
	}
	defer fwd.Close()

	client, err := net.DialUDP("udp", nil, &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: int(fwd.LocalPort())})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	client.SetDeadline(time.Now().Add(5 * time.Second))

	for _, payload := range []string{"meow", "second datagram"} {
		if _, err := client.Write([]byte(payload)); err != nil {
			t.Fatal(err)
		}
		buf := make([]byte, 1024)
		n, err := client.Read(buf)
		if err != nil {
			t.Fatalf("echo read for %q: %v", payload, err)
		}
		if string(buf[:n]) != payload {
			t.Fatalf("echo = %q, want %q", buf[:n], payload)
		}
	}
}

func startUDPEcho(t *testing.T) *net.UDPAddr {
	t.Helper()
	conn, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { conn.Close() })
	go func() {
		buf := make([]byte, 65535)
		for {
			n, addr, err := conn.ReadFromUDP(buf)
			if err != nil {
				return
			}
			conn.WriteToUDP(buf[:n], addr)
		}
	}()
	return conn.LocalAddr().(*net.UDPAddr)
}

// startMiniSOCKS5 implements just enough of RFC 1928: no-auth negotiation and
// UDP ASSOCIATE with a working relay.
func startMiniSOCKS5(t *testing.T) string {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { ln.Close() })
	go func() {
		for {
			c, err := ln.Accept()
			if err != nil {
				return
			}
			go serveSOCKS(t, c)
		}
	}()
	return ln.Addr().String()
}

func serveSOCKS(t *testing.T, c net.Conn) {
	defer c.Close()
	// Greeting.
	head := make([]byte, 2)
	if _, err := io.ReadFull(c, head); err != nil {
		return
	}
	methods := make([]byte, head[1])
	if _, err := io.ReadFull(c, methods); err != nil {
		return
	}
	c.Write([]byte{0x05, 0x00})
	// Associate request (client sends ATYP=1 0.0.0.0:0 → 10 bytes total).
	req := make([]byte, 10)
	if _, err := io.ReadFull(c, req); err != nil {
		return
	}
	if req[1] != 0x03 {
		return
	}
	relay, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		return
	}
	defer relay.Close()
	port := uint16(relay.LocalAddr().(*net.UDPAddr).Port)
	reply := []byte{0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1}
	reply = binary.BigEndian.AppendUint16(reply, port)
	c.Write(reply)

	// Relay loop: unwrap client datagrams → target; wrap replies → client.
	go func() {
		var clientAddr *net.UDPAddr
		targets := map[string]*net.UDPAddr{}
		buf := make([]byte, 65535)
		for {
			n, from, err := relay.ReadFromUDP(buf)
			if err != nil {
				return
			}
			pkt := buf[:n]
			if clientAddr == nil || from.String() == clientAddr.String() {
				// From the forwarder: parse header (expect ATYP=domain).
				clientAddr = from
				if len(pkt) < 5 || pkt[3] != 0x03 {
					continue
				}
				l := int(pkt[4])
				host := string(pkt[5 : 5+l])
				dport := binary.BigEndian.Uint16(pkt[5+l : 7+l])
				dst := &net.UDPAddr{IP: net.ParseIP(host), Port: int(dport)}
				if dst.IP == nil {
					ips, _ := net.LookupIP(host)
					if len(ips) == 0 {
						continue
					}
					dst.IP = ips[0]
				}
				targets[dst.String()] = dst
				relay.WriteToUDP(pkt[7+l:], dst)
			} else {
				// From a target: wrap and send to the client.
				hdr := []byte{0, 0, 0, 0x01}
				hdr = append(hdr, from.IP.To4()...)
				hdr = binary.BigEndian.AppendUint16(hdr, uint16(from.Port))
				relay.WriteToUDP(append(hdr, pkt...), clientAddr)
			}
		}
	}()
	// Hold the control connection open until the client closes it.
	io.Copy(io.Discard, c)
}
