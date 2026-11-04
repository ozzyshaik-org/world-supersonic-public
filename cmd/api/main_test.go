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
	// Use up the Valkey client's sync.Once so no handler under test dials a real Valkey on 127.0.0.1:6379: with a nil
	// client the handlers skip their cache.
	apiValkeyOnce.Do(func() {})
	os.Exit(m.Run())
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

// startValkey runs a throwaway redis-server (Valkey-compatible) on a free local port, or skips the test when the binary
// is not installed.
func startValkey(t *testing.T) rueidis.Client {
	t.Helper()
	bin, err := exec.LookPath("redis-server")
	if err != nil {
		t.Skip("redis-server not installed; skipping the Valkey-backed key sync test")
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
		client, err = rueidis.NewClient(rueidis.ClientOption{InitAddress: []string{addr}})
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
