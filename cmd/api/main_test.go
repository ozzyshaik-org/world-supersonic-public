package main

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/gorilla/websocket"
	dto "github.com/prometheus/client_model/go"
	"github.com/redis/rueidis"
)

func TestMain(m *testing.M) {
	// No handler under test may dial a real Valkey on 127.0.0.1:6379. With the address pointed at nothing and the last
	// creation attempt dated in the future, getValkeyClient returns nil without dialing (the contract routes then skip
	// their cache and Gatekeeper refuses with 503) unless a test installs a client with useValkey.
	valkeyAddr = "127.0.0.1:1"
	apiValkeyLastTry = time.Now().Add(1000 * time.Hour)
	os.Exit(m.Run())
}

// useValkey makes getValkeyClient return c (nil: no client) until the test ends.
func useValkey(t *testing.T, c rueidis.Client) {
	t.Helper()
	apiValkeyMu.Lock()
	apiValkeyClient = c
	apiValkeyMu.Unlock()
	t.Cleanup(func() {
		apiValkeyMu.Lock()
		apiValkeyClient = nil
		apiValkeyMu.Unlock()
	})
}

// ── Fake database/sql driver ──────────────────────────────────────────────────────────────────────────────────────

// fakeDB is one scripted ClickHouse: every query records its arguments and returns rows, or fails with queryErr. After
// the rows, Next returns iterErr (io.EOF when nil).
type fakeDB struct {
	mu       sync.Mutex
	queries  []string
	args     [][]any
	queryErr error
	columns  []string
	rows     [][]driver.Value
	iterErr  error
}

func (f *fakeDB) recorded() ([]string, [][]any) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]string(nil), f.queries...), append([][]any(nil), f.args...)
}

var (
	fakeDBsMu sync.Mutex
	fakeDBs   = map[string]*fakeDB{}
	fakeDBSeq int
)

func init() { sql.Register("supersonic-fake", fakeDriver{}) }

// openFakeDB returns a *sql.DB backed by f.
func openFakeDB(t *testing.T, f *fakeDB) *sql.DB {
	t.Helper()
	fakeDBsMu.Lock()
	fakeDBSeq++
	name := fmt.Sprintf("%s#%d", t.Name(), fakeDBSeq)
	fakeDBs[name] = f
	fakeDBsMu.Unlock()
	d, err := sql.Open("supersonic-fake", name)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { d.Close() })
	return d
}

type fakeDriver struct{}

func (fakeDriver) Open(name string) (driver.Conn, error) {
	fakeDBsMu.Lock()
	defer fakeDBsMu.Unlock()
	f, ok := fakeDBs[name]
	if !ok {
		return nil, fmt.Errorf("no fake database %q", name)
	}
	return fakeConn{f}, nil
}

type fakeConn struct{ f *fakeDB }

func (fakeConn) Prepare(string) (driver.Stmt, error) { return nil, errors.New("not supported") }
func (fakeConn) Close() error                        { return nil }
func (fakeConn) Begin() (driver.Tx, error)           { return nil, errors.New("not supported") }

func (c fakeConn) QueryContext(_ context.Context, query string, args []driver.NamedValue) (driver.Rows, error) {
	c.f.mu.Lock()
	defer c.f.mu.Unlock()
	vals := make([]any, len(args))
	for i, a := range args {
		vals[i] = a.Value
	}
	c.f.queries = append(c.f.queries, query)
	c.f.args = append(c.f.args, vals)
	if c.f.queryErr != nil {
		return nil, c.f.queryErr
	}
	return &fakeRows{columns: c.f.columns, rows: c.f.rows, iterErr: c.f.iterErr}, nil
}

type fakeRows struct {
	columns []string
	rows    [][]driver.Value
	iterErr error
	pos     int
}

func (r *fakeRows) Columns() []string { return r.columns }
func (r *fakeRows) Close() error      { return nil }
func (r *fakeRows) Next(dest []driver.Value) error {
	if r.pos >= len(r.rows) {
		if r.iterErr != nil {
			return r.iterErr
		}
		return io.EOF
	}
	copy(dest, r.rows[r.pos])
	r.pos++
	return nil
}

func keyRows(pairs ...any) [][]driver.Value {
	var rows [][]driver.Value
	for i := 0; i < len(pairs); i += 2 {
		rows = append(rows, []driver.Value{pairs[i], pairs[i+1]})
	}
	return rows
}

// ── Fix 1: API key sync ───────────────────────────────────────────────────────────────────────────────────────────

func TestLoadActiveKeys(t *testing.T) {
	cols := []string{"api_key", "requests_per_second"}

	ok := &fakeDB{columns: cols, rows: keyRows("sk_a", int64(2), "sk_b", int64(20))}
	keys, err := loadActiveKeys(context.Background(), openFakeDB(t, ok))
	if err != nil {
		t.Fatal(err)
	}
	if want := []activeKey{{"sk_a", 2}, {"sk_b", 20}}; fmt.Sprint(keys) != fmt.Sprint(want) {
		t.Fatalf("keys = %v; want %v", keys, want)
	}
	if queries, _ := ok.recorded(); len(queries) != 1 || !strings.Contains(queries[0], "api_keys FINAL WHERE is_active = 1") {
		t.Fatalf("query %q; want FINAL so revoked ReplacingMergeTree rows are not returned", queries)
	}

	failures := map[string]*fakeDB{
		"query":     {queryErr: errors.New("clickhouse down")},
		"scan":      {columns: cols, rows: keyRows("sk_a", int64(2), "sk_b", "not-a-number")},
		"iteration": {columns: cols, rows: keyRows("sk_a", int64(2)), iterErr: errors.New("connection reset")},
	}
	for what, f := range failures {
		t.Run(what, func(t *testing.T) {
			keys, err := loadActiveKeys(context.Background(), openFakeDB(t, f))
			if err == nil {
				t.Fatalf("%s failure: keys %v, nil error; want an error and no keys", what, keys)
			}
			if keys != nil {
				t.Fatalf("%s failure returned partial keys %v", what, keys)
			}
		})
	}
}

// requireRedis skips the test when redis-server (Valkey-compatible) is not installed, and returns its path.
func requireRedis(t *testing.T) string {
	t.Helper()
	bin, err := exec.LookPath("redis-server")
	if err != nil {
		t.Skip("redis-server not installed; skipping this Valkey-backed test")
	}
	return bin
}

// freePort returns a local TCP port nothing listens on right now.
func freePort(t *testing.T) int {
	t.Helper()
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	return l.Addr().(*net.TCPAddr).Port
}

// redisServer is a throwaway redis-server on a local port.
type redisServer struct {
	addr string
	cmd  *exec.Cmd
}

// startRedisServer starts redis-server on port and waits until it accepts connections (skips without redis-server).
func startRedisServer(t *testing.T, port int) *redisServer {
	t.Helper()
	bin := requireRedis(t)
	cmd := exec.Command(bin, "--port", fmt.Sprint(port), "--bind", "127.0.0.1", "--save", "", "--appendonly", "no")
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	srv := &redisServer{addr: fmt.Sprintf("127.0.0.1:%d", port), cmd: cmd}
	t.Cleanup(srv.stop)
	for deadline := time.Now().Add(5 * time.Second); ; {
		if c, err := net.Dial("tcp", srv.addr); err == nil {
			c.Close()
			return srv
		}
		if time.Now().After(deadline) {
			t.Fatalf("redis-server on %s did not come up", srv.addr)
		}
		time.Sleep(20 * time.Millisecond)
	}
}

// stop kills the server; calling it again is harmless.
func (s *redisServer) stop() {
	if s.cmd.ProcessState == nil {
		_ = s.cmd.Process.Kill()
		_ = s.cmd.Wait()
	}
}

func newValkeyClient(t *testing.T, addr string) rueidis.Client {
	t.Helper()
	client, err := rueidis.NewClient(rueidis.ClientOption{InitAddress: []string{addr}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(client.Close)
	return client
}

// startValkey runs a throwaway redis-server and returns a client for it (skips without redis-server).
func startValkey(t *testing.T) rueidis.Client {
	t.Helper()
	return newValkeyClient(t, startRedisServer(t, freePort(t)).addr)
}

func hashOf(t *testing.T, client rueidis.Client) map[string]string {
	t.Helper()
	m, err := client.Do(context.Background(), client.B().Hgetall().Key(keysHash).Build()).AsStrMap()
	if err != nil {
		t.Fatal(err)
	}
	return m
}

func seedHash(t *testing.T, client rueidis.Client, fields map[string]string) {
	t.Helper()
	ctx := context.Background()
	if err := client.Do(ctx, client.B().Del().Key(keysHash).Build()).Error(); err != nil {
		t.Fatal(err)
	}
	cmd := client.B().Hset().Key(keysHash).FieldValue()
	for k, v := range fields {
		cmd = cmd.FieldValue(k, v)
	}
	if err := client.Do(ctx, cmd.Build()).Error(); err != nil {
		t.Fatal(err)
	}
}

func TestSyncKeysOnce(t *testing.T) {
	client := startValkey(t)
	ctx := context.Background()
	cols := []string{"api_key", "requests_per_second"}
	previous := map[string]string{"sk_revoked": "5", "sk_kept": "2"}

	// A failed read leaves the previous hash exactly as it was.
	failures := map[string]*fakeDB{
		"query":     {queryErr: errors.New("clickhouse down")},
		"scan":      {columns: cols, rows: keyRows("sk_kept", int64(2), "sk_bad", "not-a-number")},
		"iteration": {columns: cols, rows: keyRows("sk_kept", int64(2)), iterErr: errors.New("connection reset")},
	}
	for what, f := range failures {
		seedHash(t, client, previous)
		if err := syncKeysOnce(openFakeDB(t, f), client); err == nil {
			t.Fatalf("%s failure: syncKeysOnce returned nil; want an error", what)
		}
		if got := hashOf(t, client); fmt.Sprint(got) != fmt.Sprint(previous) {
			t.Fatalf("%s failure changed api:keys to %v; want the previous %v", what, got, previous)
		}
	}

	// A Valkey failure leaves it as well (the context is already cancelled, so every write fails).
	seedHash(t, client, previous)
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if err := replaceKeyHash(cancelled, client, []activeKey{{"sk_new", 9}}); err == nil {
		t.Fatal("replaceKeyHash with a failing Valkey returned nil; want an error")
	}
	if got := hashOf(t, client); fmt.Sprint(got) != fmt.Sprint(previous) {
		t.Fatalf("Valkey failure changed api:keys to %v; want the previous %v", got, previous)
	}

	// A successful read replaces the hash completely: the revoked key is gone, no expiry, no temporary key left.
	seedHash(t, client, previous)
	ok := &fakeDB{columns: cols, rows: keyRows("sk_kept", int64(2), "sk_new", int64(20))}
	if err := syncKeysOnce(openFakeDB(t, ok), client); err != nil {
		t.Fatal(err)
	}
	if got, want := hashOf(t, client), map[string]string{"sk_kept": "2", "sk_new": "20"}; fmt.Sprint(got) != fmt.Sprint(want) {
		t.Fatalf("api:keys = %v; want %v", got, want)
	}
	if ttl, err := client.Do(ctx, client.B().Ttl().Key(keysHash).Build()).AsInt64(); err != nil || ttl != -1 {
		t.Fatalf("api:keys TTL = %d, %v; want -1 (no expiry)", ttl, err)
	}
	if left, err := client.Do(ctx, client.B().Keys().Pattern(keysHash+":sync:*").Build()).AsStrSlice(); err != nil || len(left) != 0 {
		t.Fatalf("temporary keys left behind: %v, %v", left, err)
	}

	// More keys than one HSET batch.
	var many []any
	for i := 0; i < keySyncBatch*2+7; i++ {
		many = append(many, fmt.Sprintf("sk_%04d", i), int64(i%100))
	}
	if err := syncKeysOnce(openFakeDB(t, &fakeDB{columns: cols, rows: keyRows(many...)}), client); err != nil {
		t.Fatal(err)
	}
	if got := len(hashOf(t, client)); got != keySyncBatch*2+7 {
		t.Fatalf("api:keys holds %d keys; want %d", got, keySyncBatch*2+7)
	}

	// A successful read with no active keys deletes the hash.
	if err := syncKeysOnce(openFakeDB(t, &fakeDB{columns: cols}), client); err != nil {
		t.Fatal(err)
	}
	if n, err := client.Do(ctx, client.B().Exists().Key(keysHash).Build()).AsInt64(); err != nil || n != 0 {
		t.Fatalf("api:keys exists = %d, %v after a read with no active keys; want deleted", n, err)
	}
}

// ── Fix 2: /ws fan-out ────────────────────────────────────────────────────────────────────────────────────────────

func clientCount() int {
	clientsMu.Lock()
	defer clientsMu.Unlock()
	return len(clients)
}

func waitClients(t *testing.T, want int) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for clientCount() != want {
		if time.Now().After(deadline) {
			t.Fatalf("%d registered /ws clients; want %d", clientCount(), want)
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func dialWS(t *testing.T, srv *httptest.Server) *websocket.Conn {
	t.Helper()
	conn, _, err := websocket.DefaultDialer.Dial("ws"+strings.TrimPrefix(srv.URL, "http"), nil)
	if err != nil {
		t.Fatal(err)
	}
	return conn
}

func droppedPayloads(t *testing.T) float64 {
	t.Helper()
	var m dto.Metric
	if err := wsDroppedPayloads.Write(&m); err != nil {
		t.Fatal(err)
	}
	return m.GetCounter().GetValue()
}

func TestWSReceivesBroadcastAndUnregistersOnDisconnect(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(handleWS))
	defer srv.Close()
	base := clientCount()

	conn := dialWS(t, srv)
	waitClients(t, base+1)

	broadcastToClients([]byte(`{"type":"heartbeat"}`))
	_ = conn.SetReadDeadline(time.Now().Add(3 * time.Second))
	_, msg, err := conn.ReadMessage()
	if err != nil || string(msg) != `{"type":"heartbeat"}` {
		t.Fatalf("client received %q, %v; want the broadcast payload", msg, err)
	}

	// The client goes away while its handler is idle: the handler must notice, unregister and return.
	conn.Close()
	waitClients(t, base)
}

func TestBroadcastSkipsSlowClient(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(handleWS))
	defer srv.Close()
	base := clientCount()

	fast := dialWS(t, srv)
	defer fast.Close()
	waitClients(t, base+1)

	// A client whose queue is already full.
	slow := new(websocket.Conn)
	full := make(chan []byte, 1)
	full <- []byte("unread")
	clientsMu.Lock()
	clients[slow] = full
	clientsMu.Unlock()
	defer func() {
		clientsMu.Lock()
		delete(clients, slow)
		clientsMu.Unlock()
	}()

	before := droppedPayloads(t)
	done := make(chan struct{})
	go func() {
		broadcastToClients([]byte(`{"type":"multi_stream"}`))
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("broadcastToClients blocked on a client with a full queue")
	}
	if got := droppedPayloads(t) - before; got != 1 {
		t.Fatalf("dropped payloads +%v; want +1 for the slow client", got)
	}
	_ = fast.SetReadDeadline(time.Now().Add(3 * time.Second))
	if _, msg, err := fast.ReadMessage(); err != nil || string(msg) != `{"type":"multi_stream"}` {
		t.Fatalf("other client received %q, %v; want the payload despite the slow client", msg, err)
	}
}

// ── Fix 3: address normalization on the contract routes ──────────────────────────────────────────────────────────

const (
	checksummed = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
	lowercased  = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"
)

func contractMux() *http.ServeMux {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/contracts/{address}", handleGetContract)
	mux.HandleFunc("GET /v1/contracts/{address}/source", handleContractSource)
	mux.HandleFunc("GET /v1/contracts/{address}/cfg", handleContractCFG)
	mux.HandleFunc("GET /v1/contracts/{address}/cfg/raw", handleContractCFGRaw)
	mux.HandleFunc("GET /v1/contracts/{address}/functions", handleContractFunctions)
	mux.HandleFunc("GET /v1/contracts/{address}/interactions", handleContractInteractions)
	mux.HandleFunc("GET /v1/contracts/{address}/balance-sheet", handleContractBalanceSheet)
	mux.HandleFunc("GET /v1/contracts/{address}/gas", handleContractGas)
	mux.HandleFunc("GET /v1/contracts/{address}/risk-simulation", handleContractRiskSimulation)
	return mux
}

var contractRoutes = []string{"", "/source", "/cfg", "/cfg/raw", "/functions", "/interactions", "/balance-sheet", "/gas", "/risk-simulation"}

func TestContractRoutesLowercaseAddresses(t *testing.T) {
	saved := db
	defer func() { db = saved }()
	mux := contractMux()

	for _, input := range []string{checksummed, lowercased, "%20" + checksummed + "%20"} {
		for _, route := range contractRoutes {
			f := &fakeDB{} // no rows: the handlers answer 404 or an empty list
			db = openFakeDB(t, f)
			rec := httptest.NewRecorder()
			mux.ServeHTTP(rec, httptest.NewRequest(http.MethodGet, "/v1/contracts/"+input+route, nil))
			if rec.Code == http.StatusBadRequest || rec.Code == http.StatusServiceUnavailable {
				t.Fatalf("%s%s: status %d for a valid address with no Valkey; want the route to answer without its cache", input, route, rec.Code)
			}
			_, args := f.recorded()
			if len(args) == 0 {
				t.Fatalf("%s%s: no database query", input, route)
			}
			for _, call := range args {
				for _, a := range call {
					if s, ok := a.(string); ok && strings.HasPrefix(strings.ToLower(s), "0x") && s != lowercased {
						t.Fatalf("%s%s: queried with address %q; want %q", input, route, s, lowercased)
					}
				}
			}
		}
	}
}

func TestContractRoutesRejectMalformedAddresses(t *testing.T) {
	saved := db
	defer func() { db = saved }()
	mux := contractMux()

	for _, input := range []string{"0x123", lowercased + "0", "0xzz8da6bf26964af9d7eed9e03e53415d37aa9604", "vitalik.eth"} {
		for _, route := range contractRoutes {
			f := &fakeDB{}
			db = openFakeDB(t, f)
			rec := httptest.NewRecorder()
			mux.ServeHTTP(rec, httptest.NewRequest(http.MethodGet, "/v1/contracts/"+input+route, nil))
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("%s%s: status %d; want 400", input, route, rec.Code)
			}
			if _, args := f.recorded(); len(args) != 0 {
				t.Fatalf("%s%s: %d database queries for a malformed address; want none", input, route, len(args))
			}
		}
	}
}

// ── Gatekeeper: fails closed ──────────────────────────────────────────────────────────────────────────────────────

// gatekeep sends one /ws request through Gatekeeper and reports whether it reached the wrapped handler.
func gatekeep(t *testing.T, key string) (*httptest.ResponseRecorder, bool) {
	t.Helper()
	reached := false
	h := Gatekeeper(func(w http.ResponseWriter, r *http.Request) {
		reached = true
		w.WriteHeader(http.StatusOK)
	})
	target := "/ws"
	if key != "" {
		target += "?api_key=" + key
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	rec := httptest.NewRecorder()
	h(rec, httptest.NewRequest(http.MethodGet, target, nil).WithContext(ctx))
	return rec, reached
}

func gatekeeperUnavailableCount(t *testing.T) float64 {
	t.Helper()
	var m dto.Metric
	if err := gatekeeperUnavailableTotal.Write(&m); err != nil {
		t.Fatal(err)
	}
	return m.GetCounter().GetValue()
}

func wantRefused(t *testing.T, what string, rec *httptest.ResponseRecorder, reached bool) {
	t.Helper()
	if reached {
		t.Fatalf("%s: request passed through Gatekeeper; want it refused", what)
	}
	if rec.Code != http.StatusServiceUnavailable || rec.Header().Get("Retry-After") != "5" {
		t.Fatalf("%s: status %d, Retry-After %q; want 503 with Retry-After 5", what, rec.Code, rec.Header().Get("Retry-After"))
	}
}

func TestGatekeeperMissingKey(t *testing.T) {
	useValkey(t, nil)
	if rec, reached := gatekeep(t, ""); reached || rec.Code != http.StatusUnauthorized {
		t.Fatalf("missing api_key: status %d, passed %v; want 401, refused", rec.Code, reached)
	}
}

func TestGatekeeperFailsClosedWithoutValkey(t *testing.T) {
	useValkey(t, nil)
	before := gatekeeperUnavailableCount(t)
	rec, reached := gatekeep(t, "sk_any")
	wantRefused(t, "no Valkey client", rec, reached)
	if got := gatekeeperUnavailableCount(t) - before; got != 1 {
		t.Fatalf("supersonic_gatekeeper_unavailable_total +%v; want +1", got)
	}
}

func TestGatekeeperFailsClosedWhenCheckFails(t *testing.T) {
	srv := startRedisServer(t, freePort(t))
	client := newValkeyClient(t, srv.addr)
	useValkey(t, client)
	ctx := context.Background()

	// The check script errors (api:keys is not a hash).
	if err := client.Do(ctx, client.B().Set().Key(keysHash).Value("not-a-hash").Build()).Error(); err != nil {
		t.Fatal(err)
	}
	rec, reached := gatekeep(t, "sk_ok")
	wantRefused(t, "check script error", rec, reached)

	// A valid key passes while Valkey is up, and is refused once Valkey is gone.
	seedHash(t, client, map[string]string{"sk_ok": "50"})
	if rec, reached := gatekeep(t, "sk_ok"); !reached || rec.Code != http.StatusOK {
		t.Fatalf("valid key with Valkey up: status %d, passed %v; want 200, passed", rec.Code, reached)
	}
	srv.stop()
	rec, reached = gatekeep(t, "sk_ok")
	wantRefused(t, "Valkey stopped", rec, reached)
}

func TestGatekeeperKeyChecks(t *testing.T) {
	client := startValkey(t)
	useValkey(t, client)
	seedHash(t, client, map[string]string{"sk_ok": "50", "sk_limited": "2"})

	if rec, reached := gatekeep(t, "sk_ok"); !reached || rec.Code != http.StatusOK {
		t.Fatalf("valid key: status %d, passed %v; want 200, passed", rec.Code, reached)
	}
	if rec, reached := gatekeep(t, "sk_unknown"); reached || rec.Code != http.StatusUnauthorized {
		t.Fatalf("unknown key: status %d, passed %v; want 401, refused", rec.Code, reached)
	}
	// 2 requests per second: of 6 sent at once (within at most two one-second windows) at least one is over the limit.
	limited := 0
	for i := 0; i < 6; i++ {
		rec, reached := gatekeep(t, "sk_limited")
		switch {
		case rec.Code == http.StatusTooManyRequests && !reached:
			limited++
		case rec.Code == http.StatusOK && reached:
		default:
			t.Fatalf("limited key request %d: status %d, passed %v; want 200 or 429", i, rec.Code, reached)
		}
	}
	if limited == 0 {
		t.Fatal("no 429 for 6 immediate requests at 2 requests per second")
	}
}

// A Valkey that is unreachable at the first attempt is picked up by a later attempt, no sooner than
// valkeyRetryInterval after the previous one, and concurrent callers end up sharing one client.
func TestGetValkeyClientRetriesThrottled(t *testing.T) {
	requireRedis(t)
	port := freePort(t)
	apiValkeyMu.Lock()
	savedAddr := valkeyAddr
	valkeyAddr = fmt.Sprintf("127.0.0.1:%d", port)
	apiValkeyClient, apiValkeyLastTry, apiValkeyConnecting = nil, time.Time{}, false
	apiValkeyMu.Unlock()
	t.Cleanup(func() {
		apiValkeyMu.Lock()
		defer apiValkeyMu.Unlock()
		if apiValkeyClient != nil {
			apiValkeyClient.Close()
		}
		valkeyAddr, apiValkeyClient = savedAddr, nil
		apiValkeyLastTry = time.Now().Add(1000 * time.Hour)
	})

	if c := getValkeyClient(); c != nil {
		t.Fatal("got a client with nothing listening")
	}
	startRedisServer(t, port)
	if c := getValkeyClient(); c != nil {
		t.Fatal("a new attempt was made before valkeyRetryInterval passed")
	}

	apiValkeyMu.Lock()
	apiValkeyLastTry = time.Now().Add(-valkeyRetryInterval - time.Second)
	apiValkeyMu.Unlock()
	results := make([]rueidis.Client, 32)
	var wg sync.WaitGroup
	for i := range results {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			results[i] = getValkeyClient()
		}(i)
	}
	wg.Wait()
	created := getValkeyClient()
	if created == nil {
		t.Fatal("no client after the retry interval with Valkey up")
	}
	for i, c := range results {
		if c != nil && c != created {
			t.Fatalf("caller %d got a different client; want one shared client", i)
		}
	}
	if err := created.Do(context.Background(), created.B().Ping().Build()).Error(); err != nil {
		t.Fatalf("PING through the created client: %v", err)
	}
}

// The contract routes still use the Valkey cache when a client is available.
func TestContractRouteUsesValkeyCache(t *testing.T) {
	client := startValkey(t)
	useValkey(t, client)
	saved := db
	defer func() { db = saved }()
	f := &fakeDB{}
	db = openFakeDB(t, f)

	const dot = "digraph { a -> b }"
	if err := client.Do(context.Background(), client.B().Set().Key("cache:cfg:raw:"+lowercased).Value(dot).Build()).Error(); err != nil {
		t.Fatal(err)
	}
	rec := httptest.NewRecorder()
	contractMux().ServeHTTP(rec, httptest.NewRequest(http.MethodGet, "/v1/contracts/"+checksummed+"/cfg/raw", nil))
	if rec.Code != http.StatusOK || rec.Header().Get("X-Cache") != "HIT" || rec.Body.String() != dot {
		t.Fatalf("cached cfg/raw: status %d, X-Cache %q, body %q; want 200, HIT, the cached graph", rec.Code, rec.Header().Get("X-Cache"), rec.Body.String())
	}
	if _, args := f.recorded(); len(args) != 0 {
		t.Fatalf("%d database queries for a cache hit; want none", len(args))
	}
}
