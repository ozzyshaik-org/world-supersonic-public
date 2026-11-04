package main

import (
	"net"
	"log/slog"
	"os"
	"os/signal"
	"syscall"
	"runtime/debug"
	"encoding/json"
	"context"
	"github.com/redis/rueidis"
"database/sql"
"fmt"
"log"
"net/http"
"strconv"
"sync"
"time"

_ "github.com/ClickHouse/clickhouse-go/v2"
"github.com/gorilla/websocket"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promauto"
	"github.com/prometheus/client_golang/prometheus/promhttp"
"golang.org/x/time/rate"
)

var upgrader = websocket.Upgrader{CheckOrigin: func(r *http.Request) bool { return true }}

type APIKey struct { Limiter *rate.Limiter }

var (
db           *sql.DB
apiKeysCache = make(map[string]*APIKey)
cacheMutex   sync.RWMutex

// Thread-safe client connection matrix for RAM fan-out broadcast operations
clients   = make(map[*websocket.Conn]chan map[string]interface{})
clientsMu sync.Mutex
)

const (
	// keysHash is the Valkey hash Gatekeeper reads: field = API key, value = its requests per second.
	keysHash = "api:keys"
	// keySyncBatch is the number of keys written per HSET while the replacement hash is built.
	keySyncBatch = 500
)


var (
cacheHits = promauto.NewCounter(prometheus.CounterOpts{
Name: "supersonic_cache_hits_total",
Help: "Total number of sub-millisecond Valkey cache lookups fulfilled directly from RAM.",
})
cacheMisses = promauto.NewCounter(prometheus.CounterOpts{
Name: "supersonic_cache_misses_total",
Help: "Total number of cache misses requiring a read-through fallback to ClickHouse.",
})
rateLimitRejections = promauto.NewCounter(prometheus.CounterOpts{
Name: "supersonic_rate_limit_rejections_total",
Help: "Total number of incoming requests blocked and dropped by the atomic Lua window script.",
})
)

// syncKeys mirrors the active API keys from ClickHouse into the Valkey hash api:keys that Gatekeeper reads, every
// 10 s. A pass that fails changes nothing, so the previous hash stays in place (see syncKeysOnce).
func syncKeys() {
	for {
		if client := getValkeyClient(); client == nil {
			log.Printf("API-SERVER warn: API key sync skipped: Valkey client unavailable")
		} else if err := syncKeysOnce(db, client); err != nil {
			log.Printf("API-SERVER warn: API key sync failed, previous %s hash kept: %v", keysHash, err)
		}
		time.Sleep(10 * time.Second)
	}
}

// activeKey is one active row of worldchain.api_keys.
type activeKey struct {
	key string
	rps int
}

// syncKeysOnce replaces api:keys with the complete set of active keys, or leaves it untouched. Valkey is only written
// after the whole ClickHouse read succeeded (query, every row scan and the iteration). The replacement is built under
// a temporary key and swapped in atomically, so a failure at any step keeps the previous hash. A successful read with
// no active keys deletes api:keys, so Gatekeeper then refuses every key. The pass's context is cancelled on return.
func syncKeysOnce(db *sql.DB, client rueidis.Client) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	keys, err := loadActiveKeys(ctx, db)
	if err != nil {
		return err
	}
	return replaceKeyHash(ctx, client, keys)
}

// loadActiveKeys reads every active key, or returns an error and no keys. FINAL is required: api_keys is a
// ReplacingMergeTree, so a key revoked by inserting a newer row with is_active = 0 keeps its older active row until a
// merge, and a plain WHERE is_active = 1 would still return it.
func loadActiveKeys(ctx context.Context, db *sql.DB) ([]activeKey, error) {
	rows, err := db.QueryContext(ctx, "SELECT api_key, requests_per_second FROM worldchain.api_keys FINAL WHERE is_active = 1")
	if err != nil {
		return nil, fmt.Errorf("query api_keys: %w", err)
	}
	defer rows.Close()
	keys := make([]activeKey, 0)
	for rows.Next() {
		var k activeKey
		if err := rows.Scan(&k.key, &k.rps); err != nil {
			return nil, fmt.Errorf("scan api_keys row: %w", err)
		}
		keys = append(keys, k)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read api_keys: %w", err)
	}
	return keys, nil
}

// swapKeysScript renames the fully built temporary hash (KEYS[1]) over api:keys (KEYS[2]) and clears the expiry the
// temporary key carried, as one atomic step.
var swapKeysScript = rueidis.NewLuaScript(`
redis.call("RENAME", KEYS[1], KEYS[2])
redis.call("PERSIST", KEYS[2])
return 1
`)

// replaceKeyHash makes api:keys hold exactly keys. If a write fails before the swap, it returns an error and api:keys
// is unchanged. (If only the swap's reply is lost, the error is returned although api:keys already holds keys.)
func replaceKeyHash(ctx context.Context, client rueidis.Client, keys []activeKey) error {
	if len(keys) == 0 {
		if err := client.Do(ctx, client.B().Del().Key(keysHash).Build()).Error(); err != nil {
			return fmt.Errorf("delete %s: %w", keysHash, err)
		}
		return nil
	}
	tmp := fmt.Sprintf("%s:sync:%d:%d", keysHash, os.Getpid(), time.Now().UnixNano())
	// Best-effort removal of a partly built temporary hash; its expiry removes it anyway if this fails too.
	discard := func() {
		cleanupCtx, cleanupCancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cleanupCancel()
		_ = client.Do(cleanupCtx, client.B().Del().Key(tmp).Build()).Error()
	}
	for start := 0; start < len(keys); start += keySyncBatch {
		cmd := client.B().Hset().Key(tmp).FieldValue()
		for _, k := range keys[start:min(start+keySyncBatch, len(keys))] {
			cmd = cmd.FieldValue(k.key, strconv.Itoa(k.rps))
		}
		if err := client.Do(ctx, cmd.Build()).Error(); err != nil {
			discard()
			return fmt.Errorf("write %s: %w", tmp, err)
		}
		if start == 0 {
			// A pass that dies from here on (process killed) leaves the temporary hash behind for 60 s at most.
			if err := client.Do(ctx, client.B().Expire().Key(tmp).Seconds(60).Build()).Error(); err != nil {
				discard()
				return fmt.Errorf("expire %s: %w", tmp, err)
			}
		}
	}
	if err := swapKeysScript.Exec(ctx, client, []string{tmp, keysHash}, nil).Error(); err != nil {
		discard()
		return fmt.Errorf("swap %s into %s: %w", tmp, keysHash, err)
	}
	return nil
}


// Centralized Aggregator Worker: Reduces ClickHouse query overhead to O(1) globally
func startAggregator() {
for {
payload := make(map[string]interface{})
cacheKey := "cache:aggregator:payload"
cacheHit := false

if client := getValkeyClient(); client != nil {
cmd := client.B().Get().Key(cacheKey).Build()
if res, err := client.Do(context.Background(), cmd).ToString(); err == nil && res != "" {
if err := json.Unmarshal([]byte(res), &payload); err == nil {
cacheHit = true
}
}
}

if !cacheHit {
// 1. Fetch Latest DEX Swap
var sBlk uint64
var sTx, pool, a0, a1 string
ctxDex, cancelDex := context.WithTimeout(context.Background(), 5*time.Second)
	errDex := db.QueryRowContext(ctxDex, "SELECT block_number, tx_hash, pool_address, amount0, amount1 FROM worldchain.api_dex_swaps WHERE ingested_at >= now() - INTERVAL 2 HOUR ORDER BY block_number DESC LIMIT 1").Scan(&sBlk, &sTx, &pool, &a0, &a1)
	cancelDex()
	if errDex == nil {
payload["swap"] = map[string]interface{}{"block": sBlk, "tx": sTx, "pool": pool, "amount0": a0, "amount1": a1}
}

// 2. Fetch Latest Morpho Borrow/Liquidate
var mBlk uint64
var mTx, market, action, user, assets string
ctxMorpho, cancelMorpho := context.WithTimeout(context.Background(), 5*time.Second)
	errMorpho := db.QueryRowContext(ctxMorpho, "SELECT block_number, tx_hash, market_id, action_type, user_address, assets_amount FROM worldchain.api_morpho_activity WHERE ingested_at >= now() - INTERVAL 2 HOUR ORDER BY block_number DESC LIMIT 1").Scan(&mBlk, &mTx, &market, &action, &user, &assets)
	cancelMorpho()
	if errMorpho == nil {
payload["morpho"] = map[string]interface{}{"block": mBlk, "tx": mTx, "action": action, "user": user, "assets": assets}
}

// 3. Fetch Latest NFT Transfer
var nBlk uint64
var nContract, nFrom, nTo, nTokenId string
ctxNFT, cancelNFT := context.WithTimeout(context.Background(), 5*time.Second)
	errNFT := db.QueryRowContext(ctxNFT, "SELECT block_number, contract_address, from_address, to_address, token_id FROM worldchain.api_nft_transfers WHERE ingested_at >= now() - INTERVAL 2 HOUR ORDER BY block_number DESC LIMIT 1").Scan(&nBlk, &nContract, &nFrom, &nTo, &nTokenId)
	cancelNFT()
	if errNFT == nil {
payload["nft"] = map[string]interface{}{"block": nBlk, "contract": nContract, "from": nFrom, "to": nTo, "tokenId": nTokenId}
}

if len(payload) > 0 {
payload["type"] = "multi_stream"
} else {
payload["type"] = "heartbeat"
payload["status"] = "Syncing Node..."
}

if client := getValkeyClient(); client != nil {
if data, err := json.Marshal(payload); err == nil {
go func(k string, v []byte) {
cmd := client.B().Set().Key(k).Value(string(v)).Ex(2 * time.Second).Build()
_ = client.Do(context.Background(), cmd)
}(cacheKey, data)
}
}
}

	sendToRustEdge(payload)

time.Sleep(1 * time.Second)
}
}

func Gatekeeper(next http.HandlerFunc) http.HandlerFunc {
return func(w http.ResponseWriter, r *http.Request) {
key := r.URL.Query().Get("api_key")
if key == "" {
http.Error(w, "Unauthorized: Missing API Key", 401)
return
}

client := getValkeyClient()
if client == nil {
next(w, r)
return
}

window := time.Now().Unix()
ratelimitKey := fmt.Sprintf("ratelimit:%s:%d", key, window)

luaScript := rueidis.NewLuaScript(`
local limit = redis.call("HGET", "api:keys", ARGV[1])
if not limit then return -1 end
local current = redis.call("INCR", KEYS[1])
if current == 1 then redis.call("EXPIRE", KEYS[1], 2) end
if current > tonumber(limit) then return 0 end
return 1
`)

res, err := luaScript.Exec(r.Context(), client, []string{ratelimitKey}, []string{key}).ToInt64()
if err != nil {
next(w, r)
return
}

if res == -1 {
http.Error(w, "Unauthorized: Invalid or Revoked API Key", 401)
return
} else if res == 0 {
rateLimitRejections.Inc()
		http.Error(w, "Too Many Requests: Rate Limit Exceeded", 429)
return
}

next(w, r)
}
}


func handleWS(w http.ResponseWriter, r *http.Request) {
conn, err := upgrader.Upgrade(w, r, nil)
if err != nil { return }
defer conn.Close()

// Register channel to collect centralized broadcast frames
msgChan := make(chan map[string]interface{}, 50)
clientsMu.Lock()
clients[conn] = msgChan
clientsMu.Unlock()

defer func() {
clientsMu.Lock()
delete(clients, conn)
clientsMu.Unlock()
}()

// Asynchronous reader thread to catch socket closures immediately
go func() {
for {
if _, _, readErr := conn.ReadMessage(); readErr != nil {
return
}
}
}()

// Push real-time memory payloads downstream over the websocket connection
for payload := range msgChan {
if writeErr := conn.WriteJSON(payload); writeErr != nil {
return
}
}
}


func recoveryMiddleware(next http.Handler) http.Handler {
return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
defer func() {
if err := recover(); err != nil {
log.Printf("[PANIC RECOVERY] %v\n%s", err, debug.Stack())
w.Header().Set("Content-Type", "application/json")
w.WriteHeader(http.StatusInternalServerError)
_ = json.NewEncoder(w).Encode(map[string]string{
"error": "internal server panic recovered",
})
}
}()
next.ServeHTTP(w, r)
})
}
func main() {
	slog.SetDefault(slog.New(slog.NewJSONHandler(os.Stdout, &slog.HandlerOptions{Level: slog.LevelInfo})))

var err error
db, err = sql.Open("clickhouse", clickhouseDSN())
if err != nil {
log.Fatalf("[FATAL] ClickHouse DB Connection Failed: %v", err)
}
db.SetMaxOpenConns(128)
db.SetMaxIdleConns(128)
db.SetConnMaxLifetime(5 * time.Minute)
db.SetConnMaxIdleTime(2 * time.Minute)

if err := db.Ping(); err != nil {
log.Fatalf("[FATAL] ClickHouse Ping Verification Failed: %v", err)
}

go syncKeys()
go startAggregator()
	go startSystemdWatchdog()
	go startDepegCacheWorker()

mux := http.DefaultServeMux
mux.Handle("/metrics", promhttp.Handler())
mux.HandleFunc("/ws", Gatekeeper(handleWS))
mux.HandleFunc("GET /v1/contract/{address}/cfg/raw", handleContractCFGRaw)
	mux.HandleFunc("GET /v1/contracts/{address}/cfg/raw", handleContractCFGRaw)
mux.Handle("/", http.FileServer(http.Dir(webDir())))

server := &http.Server{
Addr:              ":8090",
Handler:           recoveryMiddleware(mux),
ReadHeaderTimeout: 5 * time.Second,
ReadTimeout:       15 * time.Second,
WriteTimeout:      30 * time.Second,
IdleTimeout:       60 * time.Second,
}

stop := make(chan os.Signal, 1)
signal.Notify(stop, os.Interrupt, syscall.SIGTERM)

go func() {
log.Println("🛡️  Supersonic Multi-Stream Hardened API Active on :8090")
if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
log.Fatalf("[FATAL] HTTP Listener Failure: %v", err)
}
}()

<-stop
log.Println("[SHUTDOWN] OS Signal Received — Initiating Graceful Shutdown...")

ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
defer cancel()

if err := server.Shutdown(ctx); err != nil {
log.Printf("[SHUTDOWN] Forced Termination Error: %v", err)
} else {
log.Println("[SHUTDOWN] Server Gracefully Stopped — All Sockets Flushed Cleanly")
}
}

func startSystemdWatchdog() {
	socketAddr := os.Getenv("NOTIFY_SOCKET")
	if socketAddr == "" {
		return
	}

	conn, err := net.Dial("unixgram", socketAddr)
	if err != nil {
		slog.Error("Failed to connect to systemd notify socket", "error", err)
		return
	}
	defer conn.Close()

	ticker := time.NewTicker(5 * time.Second)
	defer ticker.Stop()

	slog.Info("Systemd Watchdog Keep-Alive agent active", "socket", socketAddr)

	for range ticker.C {
		_, err := conn.Write([]byte("WATCHDOG=1"))
		if err != nil {
			slog.Error("Failed to transmit watchdog heartbeat", "error", err)
		}
	}
}


var (
	ipcConn   net.Conn
	ipcConnMu sync.Mutex
)

func sendToRustEdge(payload interface{}) {
	rawBytes, err := json.Marshal(payload)
	if err != nil {
		return
	}

	ipcConnMu.Lock()
	defer ipcConnMu.Unlock()

	if ipcConn == nil {
		var dialErr error
		ipcConn, dialErr = net.Dial("unix", ipcSockPath())
		if dialErr != nil {
			return
		}
	}

	_, err = ipcConn.Write(rawBytes)
	if err != nil {
		_ = ipcConn.Close()
		ipcConn = nil
	}
}

// clickhouseDSN builds the ClickHouse connection string from CLICKHOUSE_DSN
// (default: local native-protocol port, database "worldchain").
func clickhouseDSN() string {
	if v := os.Getenv("CLICKHOUSE_DSN"); v != "" {
		return v
	}
	return "clickhouse://127.0.0.1:9000/worldchain"
}

// webDir is the directory served at "/" (WEB_DIR, default ./web).
func webDir() string {
	if v := os.Getenv("WEB_DIR"); v != "" {
		return v
	}
	return "./web"
}

// ipcSockPath is the unix socket shared with the edge service (EDGE_IPC_SOCK).
func ipcSockPath() string {
	if v := os.Getenv("EDGE_IPC_SOCK"); v != "" {
		return v
	}
	return "/tmp/supersonic.sock"
}
