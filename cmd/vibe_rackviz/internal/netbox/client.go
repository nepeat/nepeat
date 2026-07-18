package netbox

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/nepeat/nepeat/cmd/vibe_rackviz/internal/proxy"
)

type Client struct {
	base  string
	token string
	http  *http.Client
}

func New(baseURL, token string) *Client {
	// PROXY env overrides the standard HTTP_PROXY/HTTPS_PROXY/NO_PROXY
	// handling (socks5:// works too — net/http dials SOCKS5 natively).
	return &Client{
		base:  strings.TrimRight(baseURL, "/"),
		token: token,
		http:  &http.Client{Timeout: 15 * time.Second, Transport: proxy.HTTPTransport(false)},
	}
}

func (c *Client) get(ctx context.Context, path string, query url.Values, v any) error {
	u := c.base + path
	if len(query) > 0 {
		u += "?" + query.Encode()
	}
	return c.getURL(ctx, u, v)
}

func (c *Client) getURL(ctx context.Context, u string, v any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, u, nil)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Token "+c.token)
	req.Header.Set("Accept", "application/json")
	resp, err := c.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 512))
		return fmt.Errorf("GET %s: %s: %s", u, resp.Status, strings.TrimSpace(string(body)))
	}
	return json.NewDecoder(resp.Body).Decode(v)
}

type page[T any] struct {
	Count   int     `json:"count"`
	Next    *string `json:"next"`
	Results []T     `json:"results"`
}

// paginate fetches every page of a list endpoint, following the next links.
func paginate[T any](ctx context.Context, c *Client, path string, query url.Values) ([]T, error) {
	if query == nil {
		query = url.Values{}
	}
	if query.Get("limit") == "" {
		query.Set("limit", "200")
	}
	var out []T
	next := c.base + path + "?" + query.Encode()
	for next != "" {
		var p page[T]
		if err := c.getURL(ctx, next, &p); err != nil {
			return nil, err
		}
		out = append(out, p.Results...)
		if p.Next == nil {
			break
		}
		next = *p.Next
	}
	return out, nil
}
