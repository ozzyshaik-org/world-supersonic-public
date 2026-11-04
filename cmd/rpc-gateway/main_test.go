package main

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/http/httptest"
	"os/exec"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/redis/rueidis"
	"golang.org/x/sync/singleflight"
)

const (
	checksummed = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
	lowercased  = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"
)

func TestNormalizeAddress(t *testing.T) {
	valid := []struct{ in, want string }{
		{lowercased, lowercased},
		{checksummed, lowercased},
		{"  " + checksummed + "\n", lowercased},
		{"0xD8DA6BF26964AF9D7EED9E03E53415D37AA96045", lowercased},
	}
	for _, c := range valid {
		got, ok := normalizeAddress(c.in)
		if !ok || got != c.want {
			t.Errorf("normalizeAddress(%q) = %q, %v; want %q, true", c.in, got, ok, c.want)
		}
	}
	malformed := []string{
		"",
		"0x",
		lowercased[:41],  // 39 hex digits
		lowercased + "0", // 41 hex digits
		"0xg8da6bf26964af9d7eed9e03e53415d37aa96045", // not hex
		lowercased[2:],        // no 0x
		"0X" + lowercased[2:], // 0X prefix
		"0xd8da6bf26964af9d7eed9e03 53415d37aa96045", // inner space
		"vitalik.eth",
	}
	for _, in := range malformed {
		if got, ok := normalizeAddress(in); ok {
			t.Errorf("normalizeAddress(%q) = %q, true; want rejected", in, got)
		}
	}
}

// fakeConn stands in for the ClickHouse connection: only QueryRow is implemented (any other method panics through the
// nil embedded interface). It records the arguments of every query.
type fakeConn struct {
	driver.Conn
	mu    sync.Mutex
	args  [][]any
	value string
	err   error
}

func (f *fakeConn) QueryRow(_ context.Context, _ string, args ...any) driver.Row {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.args = append(f.args, args)
	return fakeRow{value: f.value, err: f.err}
}

func (f *fakeConn) set(value string, err error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.value, f.err = value, err
}

func (f *fakeConn) calls() [][]any {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([][]any(nil), f.args...)
}

type fakeRow struct {
	value string
	err   error
}

func (r fakeRow) Err() error { return r.err }
func (r fakeRow) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	*dest[0].(*string) = r.value
	return nil
}
func (r fakeRow) ScanStruct(any) error { return errors.New("not used") }

func TestQueryNetBalance(t *testing.T) {
	db := &fakeConn{value: "1500000000000000000"}
	got, err := queryNetBalance(context.Background(), db, lowercased)
	if err != nil || got != "1500000000000000000" {
		t.Fatalf("queryNetBalance = %q, %v; want the stored value", got, err)
	}
	if args := db.calls(); len(args) != 1 || len(args[0]) != 1 || args[0][0] != lowercased {
		t.Fatalf("query args = %v; want [[%s]]", args, lowercased)
	}

	db.set("", errors.New("clickhouse unavailable"))
	got, err = queryNetBalance(context.Background(), db, lowercased)
	if err == nil {
		t.Fatalf("queryNetBalance on a database error = %q, nil; want the error", got)
	}
	if got != "" {
		t.Fatalf("queryNetBalance on a database error returned balance %q; want none", got)
	}
}

// A malformed address is refused before the cache or the database is used: the gateway here has neither, so touching
// either would panic.
func TestBalanceHandlerRejectsMalformedAddress(t *testing.T) {
	g := &RpcGateway{}
	for _, addr := range []string{"", "0x123", lowercased + "0", "0xzz8da6bf26964af9d7eed9e03e53415d37aa9604"} {
		rec := httptest.NewRecorder()
		g.GetAddressBalanceHandler(rec, httptest.NewRequest(http.MethodGet, "/v1/balance?address="+addr, nil))
		if rec.Code != http.StatusBadRequest {
			t.Errorf("address %q: status %d; want 400", addr, rec.Code)
		}
	}
}

// startValkey runs a throwaway redis-server (Valkey-compatible) on a free local port, or skips the test when the binary
// is not installed. It returns a client configured like the gateway's.
func startValkey(t *testing.T) rueidis.Client {
	t.Helper()
	bin, err := exec.LookPath("redis-server")
	if err != nil {
		t.Skip("redis-server not installed; skipping the cache-backed handler test")
	}
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	port := l.Addr().(*net.TCPAddr).Port
	l.Close()
	cmd := exec.Command(bin, "--port", fmt.Sprint(port), "--bind", "127.0.0.1", "--save", "", "--appendonly", "no")
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = cmd.Process.Kill(); _ = cmd.Wait() })

	addr := fmt.Sprintf("127.0.0.1:%d", port)
	var client rueidis.Client
	for deadline := time.Now().Add(5 * time.Second); ; {
		client, err = rueidis.NewClient(rueidis.ClientOption{
			InitAddress:           []string{addr},
			CacheSizeEachConn:     1 << 20,
			ClientTrackingOptions: []string{"OPTIN"},
		})
		if err == nil {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("redis-server on %s did not come up: %v", addr, err)
		}
		time.Sleep(50 * time.Millisecond)
	}
	t.Cleanup(client.Close)
	return client
}

func newTestGateway(cache rueidis.Client, db driver.Conn) *RpcGateway {
	g := &RpcGateway{cache: cache, db: db}
	for i := range g.shards {
		g.shards[i] = &singleflight.Group{}
	}
	return g
}

func getBalance(g *RpcGateway, addr string) *httptest.ResponseRecorder {
	rec := httptest.NewRecorder()
	g.GetAddressBalanceHandler(rec, httptest.NewRequest(http.MethodGet, "/v1/balance?address="+addr, nil))
	return rec
}

// cachedBalance reads the balance cache key directly from the server (no client-side cache); "" when it is not set.
func cachedBalance(t *testing.T, client rueidis.Client, addr string) string {
	t.Helper()
	v, err := client.Do(context.Background(), client.B().Get().Key("worldchain:balance:"+addr).Build()).ToString()
	if rueidis.IsRedisNil(err) {
		return ""
	}
	if err != nil {
		t.Fatal(err)
	}
	return v
}

func waitCached(t *testing.T, client rueidis.Client, addr, want string) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for cachedBalance(t, client, addr) != want {
		if time.Now().After(deadline) {
			t.Fatalf("cache for %s = %q; want %q", addr, cachedBalance(t, client, addr), want)
		}
		time.Sleep(20 * time.Millisecond)
	}
}

// Checksummed and lowercase forms of one address query the database with the lowercase address and share one cache
// entry.
func TestBalanceHandlerChecksummedAndLowercaseAgree(t *testing.T) {
	cache := startValkey(t)
	db := &fakeConn{value: "42"}
	g := newTestGateway(cache, db)

	rec := getBalance(g, checksummed)
	if rec.Code != http.StatusOK || rec.Body.String() != "42" {
		t.Fatalf("checksummed: status %d body %q; want 200 \"42\"", rec.Code, rec.Body.String())
	}
	if args := db.calls(); len(args) != 1 || args[0][0] != lowercased {
		t.Fatalf("checksummed: query args %v; want one query for %s", args, lowercased)
	}
	waitCached(t, cache, lowercased, "42")

	rec = getBalance(g, lowercased)
	if rec.Code != http.StatusOK || rec.Body.String() != "42" {
		t.Fatalf("lowercase: status %d body %q; want 200 \"42\"", rec.Code, rec.Body.String())
	}
	if got := rec.Header().Get("X-Cache-Lookup"); got != "HIT-L1L2" {
		t.Fatalf("lowercase: X-Cache-Lookup %q; want HIT-L1L2 (the entry the checksummed request stored)", got)
	}
	if n := len(db.calls()); n != 1 {
		t.Fatalf("lowercase: %d database queries in total; want 1", n)
	}
}

// A database error is a 500, and no balance (in particular no "0") is cached for it.
func TestBalanceHandlerDatabaseErrorIsNotCached(t *testing.T) {
	cache := startValkey(t)
	db := &fakeConn{err: errors.New("clickhouse unavailable")}
	g := newTestGateway(cache, db)

	rec := getBalance(g, checksummed)
	if rec.Code != http.StatusInternalServerError {
		t.Fatalf("database error: status %d body %q; want 500", rec.Code, rec.Body.String())
	}
	if strings.TrimSpace(rec.Body.String()) == "0" {
		t.Fatal("database error was reported as a zero balance")
	}
	time.Sleep(300 * time.Millisecond) // the cache write on success is asynchronous; give a wrong one time to land
	if got := cachedBalance(t, cache, lowercased); got != "" {
		t.Fatalf("database error cached balance %q; want nothing cached", got)
	}

	db.set("7", nil)
	rec = getBalance(g, lowercased)
	if rec.Code != http.StatusOK || rec.Body.String() != "7" {
		t.Fatalf("after recovery: status %d body %q; want 200 \"7\"", rec.Code, rec.Body.String())
	}
	waitCached(t, cache, lowercased, "7")
}
