// Package proxy resolves proxy configuration from the environment.
//
// The standard HTTP_PROXY / HTTPS_PROXY / NO_PROXY variables are honored for
// NetBox HTTP calls via http.ProxyFromEnvironment. A PROXY variable overrides
// them for HTTP and, when it is a SOCKS5 proxy, is also used to relay SNMP
// datagrams (see DialUDPVia).
package proxy

import (
	"crypto/tls"
	"fmt"
	"net/http"
	"net/url"
	"os"
)

// HTTPTransport clones the default transport (which honors HTTP_PROXY /
// HTTPS_PROXY / NO_PROXY) and applies the PROXY override when set.
// tlsInsecure skips certificate verification — PDUs ship self-signed certs.
func HTTPTransport(tlsInsecure bool) *http.Transport {
	tr := http.DefaultTransport.(*http.Transport).Clone()
	if u, err := FromEnv(); err == nil && u != nil {
		tr.Proxy = http.ProxyURL(u)
	}
	if tlsInsecure {
		tr.TLSClientConfig = &tls.Config{InsecureSkipVerify: true}
	}
	return tr
}

// FromEnv parses the PROXY environment variable. Returns nil when unset.
// "socks"/"socks5h" schemes are normalized to "socks5".
func FromEnv() (*url.URL, error) {
	raw := os.Getenv("PROXY")
	if raw == "" {
		return nil, nil
	}
	u, err := url.Parse(raw)
	if err != nil {
		return nil, fmt.Errorf("PROXY %q: %w", raw, err)
	}
	switch u.Scheme {
	case "socks", "socks5h":
		u.Scheme = "socks5"
	case "socks5", "http", "https":
	default:
		return nil, fmt.Errorf("PROXY %q: unsupported scheme %q", raw, u.Scheme)
	}
	if u.Host == "" {
		return nil, fmt.Errorf("PROXY %q: missing host", raw)
	}
	return u, nil
}

// IsSOCKS reports whether the proxy can carry SNMP (UDP over SOCKS5).
func IsSOCKS(u *url.URL) bool {
	return u != nil && u.Scheme == "socks5"
}
