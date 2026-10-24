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

func syncKeys() {
for {
ctxSync, cancelSync := context.WithTimeout(context.Background(), 5*time.Second)
	rows, err := db.QueryContext(ctxSync, "SELECT api_key, requests_per_second FROM worldchain.api_keys WHERE is_active = 1")
	defer cancelSync()
if err == nil {
if client := getValkeyClient(); client != nil {
for rows.Next() {
var key string
var rps int
if err := rows.Scan(&key, &rps); err == nil {
_ = client.Do(context.Background(), client.B().Hset().Key("api:keys").FieldValue().FieldValue(key, fmt.Sprintf("%d", rps)).Build())
}
}
}
rows.Close()
}
time.Sleep(10 * time.Second)
}
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
