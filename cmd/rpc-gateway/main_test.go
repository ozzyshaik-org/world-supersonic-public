package main

import (
	"context"
	"database/sql"
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

// ── API-key lookup and revocation ─────────────────────────────────────────────────────────────────────────────────

// keyConn stands in for ClickHouse in API-key lookups: QueryRow returns info, or fails with err. It records queries.
type keyConn struct {
	driver.Conn
	mu      sync.Mutex
	queries []string
	info    *ApiKeyInfo
	err     error
}

func (c *keyConn) QueryRow(_ context.Context, query string, _ ...any) driver.Row {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.queries = append(c.queries, query)
	return keyRow{info: c.info, err: c.err}
}

func (c *keyConn) set(info *ApiKeyInfo, err error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.info, c.err = info, err
}

func (c *keyConn) recorded() []string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]string(nil), c.queries...)
}

type keyRow struct {
	info *ApiKeyInfo
	err  error
}

func (r keyRow) Err() error { return r.err }
func (r keyRow) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	*dest[0].(*string) = r.info.ApiKey
	*dest[1].(*string) = r.info.ClientName
	*dest[2].(*string) = r.info.Tier
	*dest[3].(*uint32) = r.info.RequestsPerSecond
	*dest[4].(*uint8) = r.info.IsActive
	*dest[5].(*time.Time) = r.info.CreatedAt
	return nil
}
func (r keyRow) ScanStruct(any) error { return errors.New("not used") }

func activeKey(key string) *ApiKeyInfo {
	return &ApiKeyInfo{ApiKey: key, ClientName: "test", Tier: "pro", RequestsPerSecond: 20, IsActive: 1, CreatedAt: time.Unix(1700000000, 0).UTC()}
}

func TestLookupApiKey(t *testing.T) {
	if apiKeyCacheTTL != 10*time.Second {
		t.Fatalf("apiKeyCacheTTL = %s; want 10s", apiKeyCacheTTL)
	}
	db := &keyConn{info: activeKey("sk_live_a")}
	ki, err := lookupApiKey(context.Background(), db, "sk_live_a")
	if err != nil || ki == nil || ki.ApiKey != "sk_live_a" || ki.IsActive != 1 {
		t.Fatalf("lookupApiKey = %+v, %v; want the active key", ki, err)
	}
	if q := db.recorded(); len(q) != 1 || !strings.Contains(q[0], "api_keys FINAL WHERE api_key = ? AND is_active = 1") {
		t.Fatalf("query %q; want FINAL so a key revoked by a newer row is not returned", q)
	}

	db.set(nil, sql.ErrNoRows)
	if ki, err := lookupApiKey(context.Background(), db, "sk_live_a"); !errors.Is(err, sql.ErrNoRows) || ki != nil {
		t.Fatalf("missing or inactive key: %+v, %v; want sql.ErrNoRows", ki, err)
	}
	failure := errors.New("clickhouse unavailable")
	db.set(nil, failure)
	if ki, err := lookupApiKey(context.Background(), db, "sk_live_a"); !errors.Is(err, failure) || ki != nil {
		t.Fatalf("lookup failure: %+v, %v; want the error", ki, err)
	}
}

// authRequest sends one keyed request through the gateway middleware (from a non-loopback client address).
func authRequest(h http.Handler, key string) *httptest.ResponseRecorder {
	req := httptest.NewRequest(http.MethodGet, "/v1/block/latest", nil)
	req.Header.Set("X-API-Key", key)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec
}

func okHandler() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusOK) })
}

func TestApiKeyMiddlewareStatuses(t *testing.T) {
	cache := startValkey(t)
	db := &keyConn{}
	h := newTestGateway(cache, db).ApiKeyAuthAndRateLimitMiddleware(okHandler())

	db.set(nil, sql.ErrNoRows)
	if rec := authRequest(h, "sk_missing"); rec.Code != http.StatusUnauthorized {
		t.Fatalf("missing or inactive key: status %d; want 401", rec.Code)
	}

	db.set(nil, errors.New("clickhouse unavailable"))
	rec := authRequest(h, "sk_lookup_fails")
	if rec.Code != http.StatusServiceUnavailable || rec.Header().Get("Retry-After") != "5" || !strings.Contains(rec.Body.String(), `"status":503`) {
		t.Fatalf("lookup failure: status %d, Retry-After %q, body %q; want 503 with Retry-After 5", rec.Code, rec.Header().Get("Retry-After"), rec.Body.String())
	}
	time.Sleep(300 * time.Millisecond) // a cache write would be asynchronous
	if n, err := cache.Do(context.Background(), cache.B().Exists().Key("worldchain:apikey:sk_lookup_fails").Build()).AsInt64(); err != nil || n != 0 {
		t.Fatalf("a failed lookup was cached (exists=%d, %v)", n, err)
	}

	db.set(activeKey("sk_active"), nil)
	if rec := authRequest(h, "sk_active"); rec.Code != http.StatusOK || rec.Header().Get("X-API-Tier") != "pro" {
		t.Fatalf("active key: status %d, tier %q; want 200, pro", rec.Code, rec.Header().Get("X-API-Tier"))
	}
}

// A revoked key keeps being served from the cache until its entry expires (apiKeyCacheTTL, shortened to 1 s here), and is
// refused after that. This shows the bound comes from the cache TTL; it does not measure production timing.
func TestRevokedKeyRefusedAfterCacheTTL(t *testing.T) {
	saved := apiKeyCacheTTL
	apiKeyCacheTTL = time.Second // whole seconds: the Valkey entry is written with EX
	t.Cleanup(func() { apiKeyCacheTTL = saved })

	cache := startValkey(t)
	db := &keyConn{info: activeKey("sk_revoked")}
	h := newTestGateway(cache, db).ApiKeyAuthAndRateLimitMiddleware(okHandler())
	ctx := context.Background()

	if rec := authRequest(h, "sk_revoked"); rec.Code != http.StatusOK {
		t.Fatalf("active key: status %d; want 200", rec.Code)
	}
	deadline := time.Now().Add(2 * time.Second)
	var pttl int64
	for {
		v, err := cache.Do(ctx, cache.B().Pttl().Key("worldchain:apikey:sk_revoked").Build()).AsInt64()
		if err != nil {
			t.Fatal(err)
		}
		if v > 0 {
			pttl = v
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("the key record was not cached")
		}
		time.Sleep(10 * time.Millisecond)
	}
	if pttl > apiKeyCacheTTL.Milliseconds() {
		t.Fatalf("cache entry PTTL %d ms; want at most apiKeyCacheTTL (%d ms)", pttl, apiKeyCacheTTL.Milliseconds())
	}

	db.set(nil, sql.ErrNoRows) // revoked: with FINAL the lookup finds no active row
	if rec := authRequest(h, "sk_revoked"); rec.Code != http.StatusOK {
		t.Fatalf("right after revocation: status %d; want 200 from the still-valid cache entry", rec.Code)
	}
	for deadline := time.Now().Add(4 * time.Second); ; {
		if rec := authRequest(h, "sk_revoked"); rec.Code == http.StatusUnauthorized {
			break
		} else if rec.Code != http.StatusOK || time.Now().After(deadline) {
			t.Fatalf("after the cache TTL: status %d; want 401", rec.Code)
		}
		time.Sleep(50 * time.Millisecond)
	}
}
