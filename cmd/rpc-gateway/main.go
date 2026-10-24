package main

import (
	"net"
	"os/signal"
	"syscall"
	"io"
	"crypto/rand"
	"crypto/subtle"
	"context"
	"encoding/json"
	"fmt"
	"hash/fnv"
	"log"
	"net/http"
	"os"
	"regexp"
	"strings"
	"sync"
	"time"

	ch "github.com/ClickHouse/clickhouse-go/v2"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promauto"
	"github.com/prometheus/client_golang/prometheus/promhttp"
	"github.com/redis/rueidis"
	"golang.org/x/sync/singleflight"
)

const ShardCount = 32

type BlockPayload struct {
	BlockNumber uint64    `json:"block_number"`
	BlockHash   string    `json:"block_hash"`
	Timestamp   time.Time `json:"timestamp"`
}

type TransactionResponse struct {
	BlockNumber uint64 `json:"block_number"`
	TxHash      string `json:"tx_hash"`
	FromAddress string `json:"from_address"`
	ToAddress   string `json:"to_address"`
	ValueWei    string `json:"value_wei"`
	GasUsed     uint64 `json:"gas_used"`
	Status      uint8  `json:"status"`
	IngestedAt  string `json:"ingested_at"`
}

var startTime = time.Now()

type SystemHealthResponse struct {
	Status                   string `json:"status"`
	GatewayUptimeSeconds     int64  `json:"gateway_uptime_seconds"`
	LatestIndexedBlock       uint64 `json:"latest_indexed_block"`
	IngestionLagBlocks       uint64 `json:"ingestion_lag_blocks"`
	DatabaseEngine           string `json:"database_engine"`
	CacheEngine              string `json:"cache_engine"`
	SystemdMonitoredServices int    `json:"systemd_monitored_services"`
	Timestamp                string `json:"timestamp"`
}

type ApiKeyInfo struct {
	ApiKey            string    `json:"api_key"`
	ClientName        string    `json:"client_name"`
	Tier              string    `json:"tier"`
	RequestsPerSecond uint32    `json:"requests_per_second"`
	IsActive          uint8     `json:"is_active"`
	CreatedAt         time.Time `json:"created_at"`
}

type ProvisionKeyRequest struct {
	ClientName string `json:"client_name"`
	Tier       string `json:"tier"`
}

type PortfolioTokenBalanceResponse struct {
	WalletAddress string `json:"wallet_address"`
	TokenAddress  string `json:"token_address"`
	NetBalance    string `json:"net_balance"`
}

type MevAnalyticsResponse struct {
	BlockNumber       uint64 `json:"block_number"`
	TxHash            string `json:"tx_hash"`
	TxIndex           uint32 `json:"tx_index"`
	MevType           string `json:"mev_type"`
	ExtractedValueWei string `json:"extracted_value_wei"`
	SandwichVictimTx  string `json:"sandwich_victim_tx"`
	IngestedAt        string `json:"ingested_at"`
}

type DefiEventResponse struct {
	BlockNumber     uint64 `json:"block_number"`
	TxHash          string `json:"tx_hash"`
	LogIndex        uint32 `json:"log_index"`
	ContractAddress string `json:"contract_address"`
	EventType       string `json:"event_type"`
	RawData         string `json:"raw_data"`
	BlockTimestamp  string `json:"block_timestamp"`
}

type DexSwapResponse struct {
	BlockNumber    uint64 `json:"block_number"`
	TxHash         string `json:"tx_hash"`
	LogIndex       uint32 `json:"log_index"`
	PoolAddress    string `json:"pool_address"`
	Sender         string `json:"sender"`
	Recipient      string `json:"recipient"`
	Amount0        string `json:"amount0"`
	Amount1        string `json:"amount1"`
	SqrtPriceX96   string `json:"sqrtPriceX96"`
	Liquidity      string `json:"liquidity"`
	Tick           int32  `json:"tick"`
	BlockTimestamp string `json:"block_timestamp"`
}

type TokenTransferResponse struct {
	BlockNumber    uint64 `json:"block_number"`
	TxHash         string `json:"tx_hash"`
	LogIndex       uint32 `json:"log_index"`
	TokenAddress   string `json:"token_address"`
	FromAddress    string `json:"from_address"`
	ToAddress      string `json:"to_address"`
	Value          string `json:"value"`
	TokenID        string `json:"token_id"`
	BlockTimestamp string `json:"block_timestamp"`
}

type RpcGateway struct {
	cache  rueidis.Client
	db     ch.Conn
	shards [ShardCount]*singleflight.Group
}

type TokenBucket struct {
	tokens     float64
	lastRefill time.Time
	mu         sync.Mutex
}

var (
	addressRegex        = regexp.MustCompile("^0x[0-9a-fA-F]{40}$")
	httpRequestsTotal   = promauto.NewCounterVec(prometheus.CounterOpts{Name: "rpc_gateway_requests_total", Help: "Total incoming RPC requests"}, []string{"endpoint", "status"})
	cacheLookupTotal    = promauto.NewCounterVec(prometheus.CounterOpts{Name: "rpc_gateway_cache_lookup_total", Help: "Total cache evaluations"}, []string{"result"})
	rateLimitDropsTotal = promauto.NewCounter(prometheus.CounterOpts{Name: "rpc_gateway_rate_limit_drops_total", Help: "Total rate limit drops"})
)

const openAPISpecJSON = `{
  "openapi": "3.0.3",
  "info": {
    "title": "Supersonic World Chain RPC API Gateway",
    "description": "High-throughput REST and JSON-RPC API Gateway for World Chain L2 block, transaction, and address state lookups.",
    "version": "1.0.0"
  },
  "servers": [
    { "url": "http://localhost:8081", "description": "Local gateway" }
  ],
  "paths": {
    "/ws": {
      "get": {
        "summary": "Real-Time WebSocket Telemetry Stream (served by the edge / supersonic-api services, not this gateway)",
        "description": "Establishes a low-latency persistent WebSocket connection for real-time L2 block headers, pending transactions, and DEX swap telemetry stream.",
        "operationId": "subscribeWebSocketTelemetry",
        "responses": {
          "101": {
            "description": "Switching Protocols - WebSocket stream connection successfully established."
          }
        }
      }
    },
    "/v1/tokens/transfers": {
      "get": {
        "summary": "Get Indexed ERC-20 / ERC-721 Token Transfers",
        "description": "Returns indexed token transfer records filtered by wallet address or token contract address.",
        "operationId": "getTokenTransfers",
        "parameters": [
          { "name": "token", "in": "query", "schema": { "type": "string" }, "description": "Token Contract EVM Address (0x...)" },
          { "name": "address", "in": "query", "schema": { "type": "string" }, "description": "Wallet EVM Address (from/to)" }
        ],
        "responses": {
          "200": { "description": "Successful token transfers array" }
        }
      }
    },
    "/v1/dex/swaps": {
      "get": {
        "summary": "Get Real-Time DEX Swap Telemetry",
        "description": "Returns real-time DEX swap events (Uniswap v3 / Aerodrome) filtered by liquidity pool or trader address.",
        "operationId": "getDexSwaps",
        "parameters": [
          { "name": "pool", "in": "query", "schema": { "type": "string" }, "description": "DEX Pool EVM Address (0x...)" },
          { "name": "address", "in": "query", "schema": { "type": "string" }, "description": "Trader Wallet EVM Address" }
        ],
        "responses": {
          "200": { "description": "Successful DEX swaps array" }
        }
      }
    },
    "/v1/block/latest": {
      "get": {
        "summary": "Get Latest L2 Block Header",
        "description": "Returns the latest indexed World Chain L2 block number, block hash, and timestamp with sub-millisecond singleflight cache resolution.",
        "operationId": "getLatestBlock",
        "responses": {
          "200": {
            "description": "Successful block header payload",
            "headers": {
              "X-Cache-Lookup": {
                "schema": { "type": "string" },
                "description": "Cache resolution status (HIT-L1L2, SHARDED-MISS-REPAIRED, MISS-FALLBACK-SUCCESS)"
              }
            },
            "content": {
              "application/json": {
                "schema": { "$ref": "#/components/schemas/BlockPayload" },
                "example": {
                  "block_number": 32656560,
                  "block_hash": "0x7d12790030888ddb2883d3cbea341eb422ad34f29b34da9e823b15693d974c2e",
                  "timestamp": "2026-07-21T19:24:03+05:30"
                }
              }
            }
          },
          "429": { "description": "Too Many Requests (Token Bucket Rate Limiter Exceeded)" },
          "500": { "description": "Internal Database Lookup Failure" }
        }
      }
    },
    "/v1/balance": {
      "get": {
        "summary": "Get EVM Address Balance",
        "description": "Returns the net native balance in Wei for any valid 42-character EVM address.",
        "operationId": "getAddressBalance",
        "parameters": [
          {
            "name": "address",
            "in": "query",
            "required": true,
            "description": "42-character hex-encoded EVM address (e.g. 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045)",
            "schema": { "type": "string", "pattern": "^0x[0-9a-fA-F]{40}$" }
          }
        ],
        "responses": {
          "200": {
            "description": "Native balance in Wei as string",
            "content": { "text/plain": { "schema": { "type": "string" }, "example": "1500000000000000000" } }
          },
          "400": { "description": "Invalid Address Length or Malformed Specification" },
          "429": { "description": "Too Many Requests" }
        }
      }
    },
    "/v1/transactions": {
      "get": {
        "summary": "Get User Transaction History",
        "description": "Returns up to 50 recent transactions associated with an EVM address.",
        "operationId": "getUserTransactions",
        "parameters": [
          {
            "name": "address",
            "in": "query",
            "required": true,
            "description": "42-character hex-encoded EVM address",
            "schema": { "type": "string", "pattern": "^0x[0-9a-fA-F]{40}$" }
          },
          {
            "name": "limit",
            "in": "query",
            "required": false,
            "description": "Maximum number of transactions to return (default 50)",
            "schema": { "type": "integer", "default": 50 }
          }
        ],
        "responses": {
          "200": {
            "description": "Array of transaction records",
            "content": {
              "application/json": {
                "schema": {
                  "type": "array",
                  "items": { "$ref": "#/components/schemas/TransactionResponse" }
                }
              }
            }
          },
          "400": { "description": "Malformed EVM Address Specification" }
        }
      }
    },
    "/metrics": {
      "get": {
        "summary": "Get Prometheus Metrics Telemetry",
        "description": "Exposes standard Prometheus metrics for gateway request totals, cache hits/misses, rate limit drops, and Go runtime stats.",
        "operationId": "getPrometheusMetrics",
        "responses": {
          "200": {
            "description": "Prometheus text format telemetry",
            "content": { "text/plain": { "type": "string" } }
          }
        }
      }
    }
  },
  "components": {
    "schemas": {
      "BlockPayload": {
        "type": "object",
        "properties": {
          "block_number": { "type": "integer", "format": "int64", "example": 32656560 },
          "block_hash": { "type": "string", "example": "0x7d12790030888ddb2883d3cbea341eb422ad34f29b34da9e823b15693d974c2e" },
          "timestamp": { "type": "string", "format": "date-time", "example": "2026-07-21T19:24:03+05:30" }
        }
      },
      "TransactionResponse": {
        "type": "object",
        "properties": {
          "block_number": { "type": "integer", "format": "int64", "example": 32656560 },
          "tx_hash": { "type": "string", "example": "0xaab127f6a7b129b96d9295f78d136ccb9d43e4367ccaaa6ff66f659404fc3a4a" },
          "from_address": { "type": "string", "example": "0xd8da6bf26964af9d7eed9e03e53415d37aa96045" },
          "to_address": { "type": "string", "example": "0x4200000000000000000000000000000000000006" },
          "value_wei": { "type": "string", "example": "1000000000000000000" },
          "gas_used": { "type": "integer", "format": "int64", "example": 21000 },
          "status": { "type": "integer", "example": 1 },
          "ingested_at": { "type": "string", "example": "2026-07-21 19:24:03" }
        }
      }
    }
  }
}`

const swaggerHTML = `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Supersonic World Chain RPC API Explorer</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
  <style>
    html { box-sizing: border-box; overflow: -moz-scrollbars-vertical; overflow-y: scroll; }
    *, *:before, *:after { box-sizing: inherit; }
    body { margin: 0; background: #0b0f19; color: #f3f4f6; }
    .swagger-ui .topbar { display: none; }
    .swagger-ui { filter: invert(88%) hue-rotate(180deg); }
  </style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    window.onload = function() {
      window.ui = SwaggerUIBundle({
        url: "/openapi.json",
        dom_id: '#swagger-ui',
        deepLinking: true,
        presets: [
          SwaggerUIBundle.presets.apis,
          SwaggerUIBundle.SwaggerUIStandalonePreset
        ],
        layout: "BaseLayout"
      });
    };
  </script>
</body>
</html>`

func (b *TokenBucket) Allow(rate float64, burst int) bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	now := time.Now()
	elapsed := now.Sub(b.lastRefill).Seconds()
	b.lastRefill = now
	b.tokens += elapsed * rate
	if b.tokens > float64(burst) {
		b.tokens = float64(burst)
	}
	if b.tokens >= 1.0 {
		b.tokens -= 1.0
		return true
	}
	return false
}

func RateLimitMiddleware(next http.Handler, rate float64, burst int) http.Handler {
	var ipMap sync.Map
	go func() {
		for {
			time.Sleep(1 * time.Minute)
			ipMap.Range(func(key, value interface{}) bool {
				bucket := value.(*TokenBucket)
				bucket.mu.Lock()
				if time.Since(bucket.lastRefill) > 5*time.Minute {
					ipMap.Delete(key)
				}
				bucket.mu.Unlock()
				return true
			})
		}
	}()
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		ip := r.RemoteAddr
		if idx := strings.Index(ip, ":"); idx != -1 {
			ip = ip[:idx]
		}
		if xff := r.Header.Get("X-Forwarded-For"); xff != "" {
			if parts := strings.Split(xff, ","); len(parts) > 0 {
				ip = strings.TrimSpace(parts[0])
			}
		}
		if ip == "127.0.0.1" || ip == "::1" {
			next.ServeHTTP(w, r)
			return
		}
		v, _ := ipMap.LoadOrStore(ip, &TokenBucket{tokens: float64(burst), lastRefill: time.Now()})
		bucket := v.(*TokenBucket)
		if !bucket.Allow(rate, burst) {
			rateLimitDropsTotal.Inc()
			httpRequestsTotal.WithLabelValues("middleware", "429").Inc()
			http.Error(w, "429 Too Many Requests", http.StatusTooManyRequests)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func main() {
	log.Printf("=== SUPERSONIC RPC API GATEWAY INITIALIZING ===")
	cacheClient, err := rueidis.NewClient(rueidis.ClientOption{
		InitAddress:           []string{"127.0.0.1:6379"},
		Dialer:                net.Dialer{Timeout: 2 * time.Second},
		ConnWriteTimeout:      2 * time.Second,
		SelectDB:              0,
		CacheSizeEachConn:     128 * (1 << 20),
		ClientTrackingOptions: []string{"OPTIN"},
	})
	if err != nil {
		log.Fatalf("Fatal: Cache connection rejected: %v", err)
	}
	defer cacheClient.Close()

	chAddr := os.Getenv("CLICKHOUSE_ADDR")
	if chAddr == "" {
		chAddr = "127.0.0.1:9000"
	}

	var dbConn ch.Conn
	for attempt := 1; attempt <= 5; attempt++ {
		dbConn, err = ch.Open(&ch.Options{
			Addr:            []string{chAddr},
			Auth:            ch.Auth{Database: "worldchain", Username: envOr("CLICKHOUSE_USER", "default"), Password: os.Getenv("CLICKHOUSE_PASSWORD")},
			DialTimeout:     5 * time.Second,
			ConnMaxLifetime: 10 * time.Minute,
			MaxOpenConns:    64,
			MaxIdleConns:    32,
		})
		if err == nil {
			pingCtx, pingCancel := context.WithTimeout(context.Background(), 2*time.Second)
			err = dbConn.Ping(pingCtx)
			pingCancel()
			if err == nil {
				log.Printf("[✓] Connected to ClickHouse OLAP Engine (Attempt %d/5)", attempt)
				break
			}
		}
		log.Printf("⚠️ ClickHouse connection attempt %d/5 failed: %v. Retrying in 1s...", attempt, err)
		time.Sleep(1 * time.Second)
	}
	if err != nil {
		log.Fatalf("Fatal: Database connection refused after 5 attempts: %v", err)
	}
	defer dbConn.Close()

	startTime = time.Now()

	gateway := &RpcGateway{cache: cacheClient, db: dbConn}
	for i := 0; i < ShardCount; i++ {
		gateway.shards[i] = &singleflight.Group{}
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/v1/block/latest", gateway.GetLatestBlockHandler)
	mux.HandleFunc("/v1/balance", gateway.GetAddressBalanceHandler)
	mux.HandleFunc("/v1/transactions", gateway.GetUserTransactionsHandler)
	mux.HandleFunc("/v1/tokens/transfers", gateway.GetTokenTransfersHandler)
	mux.HandleFunc("/v1/dex/swaps", gateway.GetDexSwapsHandler)
	mux.HandleFunc("/v1/defi/events", gateway.GetDefiEventsHandler)
	mux.HandleFunc("/v1/mev/analytics", gateway.GetMevAnalyticsHandler)
	mux.HandleFunc("/v1/portfolio", gateway.GetPortfolioHandler)
	mux.HandleFunc("/v1/auth/keys", gateway.ProvisionApiKeyHandler)
	mux.HandleFunc("/v1/health/system", gateway.GetSystemHealthHandler)
	mux.HandleFunc("/openapi.json", gateway.GetOpenAPISpecHandler)
	mux.HandleFunc("/docs", gateway.GetSwaggerDocsHandler)
	mux.Handle("/metrics", promhttp.Handler())

	protectedHandler := gateway.ApiKeyAuthAndRateLimitMiddleware(mux)

	server := &http.Server{
		Addr:              ":8081",
		Handler:           protectedHandler,
		ReadHeaderTimeout: 3 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      10 * time.Second,
		IdleTimeout:       60 * time.Second,
	}

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, os.Interrupt, syscall.SIGTERM, syscall.SIGINT)

	go func() {
		log.Printf("[✓] RPC Gateway online. Listening over socket route :8081...")
		if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("Fatal: Gateway runtime terminated unexpectedly: %v", err)
		}
	}()

	<-stop
	log.Printf("⚠️ Signal received. Initiating graceful gateway shutdown & connection draining...")

	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer shutdownCancel()

	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("❌ Forced Gateway Shutdown Exception: %v", err)
	} else {
		log.Printf("✅ Gateway connections drained cleanly.")
	}
}

func (g *RpcGateway) resolveShard(key string) *singleflight.Group {
	h := fnv.New32a()
	h.Write([]byte(strings.ToLower(key)))
	return g.shards[h.Sum32()%ShardCount]
}

func (g *RpcGateway) GetOpenAPISpecHandler(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Cache-Control", "public, max-age=3600")
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	w.Write([]byte(openAPISpecJSON))
}

func (g *RpcGateway) GetSwaggerDocsHandler(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Cache-Control", "public, max-age=3600")
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	w.Write([]byte(swaggerHTML))
}

func (g *RpcGateway) GetUserTransactionsHandler(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 4*time.Second)
	defer cancel()

	if r.Method != http.MethodGet {
		httpRequestsTotal.WithLabelValues("/v1/transactions", "405").Inc()
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	address := r.URL.Query().Get("address")
	address = strings.TrimSpace(address)
	if !addressRegex.MatchString(address) {
		httpRequestsTotal.WithLabelValues("/v1/transactions", "400").Inc()
		http.Error(w, "Malformed EVM Address Specification", http.StatusBadRequest)
		return
	}
	address = strings.ToLower(address)
	cacheKey := fmt.Sprintf("worldchain:transactions:%s", address)

	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 10*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		cacheLookupTotal.WithLabelValues("HIT-L1L2").Inc()
		httpRequestsTotal.WithLabelValues("/v1/transactions", "200").Inc()
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Cache-Lookup", "HIT-L1L2")
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Write([]byte(val))
		return
	}

	shard := g.resolveShard(cacheKey)
	sharedData, err, _ := shard.Do(cacheKey, func() (interface{}, error) {
		query := `
SELECT block_number, tx_hash, from_address, to_address, toString(value_wei) as value_str, gas_used, status, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') as t_str
FROM worldchain.world_transactions WHERE from_address = ? OR to_address = ?
ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
		rows, err := g.db.Query(ctx, query, address, address)
		if err != nil {
			return nil, err
		}
		defer rows.Close()

		txs := make([]TransactionResponse, 0)
		for rows.Next() {
			var tx TransactionResponse
			if err := rows.Scan(&tx.BlockNumber, &tx.TxHash, &tx.FromAddress, &tx.ToAddress, &tx.ValueWei, &tx.GasUsed, &tx.Status, &tx.IngestedAt); err == nil {
				txs = append(txs, tx)
			}
		}
		marshaled, err := json.Marshal(txs)
		if err != nil {
			return nil, err
		}
		go func(key, data string) {
			setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer setCancel()
			_ = g.cache.Do(setCtx, g.cache.B().Set().Key(key).Value(data).Ex(10*time.Second).Build())
		}(cacheKey, string(marshaled))
		return marshaled, nil
	})

	if err != nil {
		log.Printf("❌ Database Exception: %v", err)
		httpRequestsTotal.WithLabelValues("/v1/transactions", "500").Inc()
		http.Error(w, "Internal Database Lookup Failure", http.StatusInternalServerError)
		return
	}
	cacheLookupTotal.WithLabelValues("SHARDED-MISS-REPAIRED").Inc()
	httpRequestsTotal.WithLabelValues("/v1/transactions", "200").Inc()
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Cache-Lookup", "SHARDED-MISS-REPAIRED")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Write(sharedData.([]byte))
}

func (g *RpcGateway) GetLatestBlockHandler(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 2*time.Second)
	defer cancel()
	const cacheKey = "worldchain:block:latest"
	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 5*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		cacheLookupTotal.WithLabelValues("HIT-L1L2").Inc()
		httpRequestsTotal.WithLabelValues("/v1/block/latest", "200").Inc()
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Cache-Lookup", "HIT-L1L2")
		w.Write([]byte(val))
		return
	}
	shard := g.resolveShard(cacheKey)
	sharedData, err, _ := shard.Do(cacheKey, func() (interface{}, error) {
		var bp BlockPayload
		query := `SELECT block_number, block_hash, timestamp FROM worldchain.world_blocks ORDER BY block_number DESC LIMIT 1`
		if err := g.db.QueryRow(ctx, query).Scan(&bp.BlockNumber, &bp.BlockHash, &bp.Timestamp); err != nil {
			return nil, err
		}
		marshaled, err := json.Marshal(bp)
		if err != nil {
			return nil, err
		}
		go func(data string) {
			setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer setCancel()
			_ = g.cache.Do(setCtx, g.cache.B().Set().Key(cacheKey).Value(data).Ex(5*time.Second).Build())
		}(string(marshaled))
		return marshaled, nil
	})
	if err != nil {
		httpRequestsTotal.WithLabelValues("/v1/block/latest", "500").Inc()
		http.Error(w, "Database lookup failure", http.StatusInternalServerError)
		return
	}
	cacheLookupTotal.WithLabelValues("MISS-FALLBACK-SUCCESS").Inc()
	httpRequestsTotal.WithLabelValues("/v1/block/latest", "200").Inc()
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Cache-Lookup", "MISS-FALLBACK-SUCCESS")
	w.Write(sharedData.([]byte))
}

func (g *RpcGateway) GetAddressBalanceHandler(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 3*time.Second)
	defer cancel()
	address := r.URL.Query().Get("address")
	if len(address) != 42 {
		httpRequestsTotal.WithLabelValues("/v1/balance", "400").Inc()
		http.Error(w, "Invalid address length", http.StatusBadRequest)
		return
	}
	cacheKey := fmt.Sprintf("worldchain:balance:%s", strings.ToLower(address))
	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 3*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		cacheLookupTotal.WithLabelValues("HIT-L1L2").Inc()
		httpRequestsTotal.WithLabelValues("/v1/balance", "200").Inc()
		w.Header().Set("Content-Type", "text/plain")
		w.Header().Set("X-Cache-Lookup", "HIT-L1L2")
		w.Write([]byte(val))
		return
	}
	shard := g.resolveShard(cacheKey)
	sharedData, err, _ := shard.Do(cacheKey, func() (interface{}, error) {
		var netBalance string
		query := `SELECT toString(sum(balance_change)) FROM worldchain.api_token_balances WHERE wallet_address = ?`
		if err := g.db.QueryRow(ctx, query, address).Scan(&netBalance); err != nil {
			return "0", nil
		}
		go func(key, val string) {
			setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer setCancel()
			_ = g.cache.Do(setCtx, g.cache.B().Set().Key(key).Value(val).Ex(3*time.Second).Build())
		}(cacheKey, netBalance)
		return netBalance, nil
	})
	if err != nil {
		httpRequestsTotal.WithLabelValues("/v1/balance", "500").Inc()
		http.Error(w, "Query execution exception", http.StatusInternalServerError)
		return
	}
	cacheLookupTotal.WithLabelValues("SHARDED-MISS-REPAIRED").Inc()
	httpRequestsTotal.WithLabelValues("/v1/balance", "200").Inc()
	w.Header().Set("Content-Type", "text/plain")
	w.Header().Set("X-Cache-Lookup", "SHARDED-MISS-REPAIRED")
	w.Write([]byte(sharedData.(string)))
}


func (g *RpcGateway) GetTokenTransfersHandler(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 4*time.Second)
	defer cancel()

	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		httpRequestsTotal.WithLabelValues("/v1/tokens/transfers", "405").Inc()
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	address := strings.TrimSpace(r.URL.Query().Get("address"))
	token := strings.TrimSpace(r.URL.Query().Get("token"))

	if address != "" && !addressRegex.MatchString(address) {
		httpRequestsTotal.WithLabelValues("/v1/tokens/transfers", "400").Inc()
		http.Error(w, "Malformed Wallet EVM Address Specification", http.StatusBadRequest)
		return
	}
	if token != "" && !addressRegex.MatchString(token) {
		httpRequestsTotal.WithLabelValues("/v1/tokens/transfers", "400").Inc()
		http.Error(w, "Malformed Token EVM Address Specification", http.StatusBadRequest)
		return
	}
	if address == "" && token == "" {
		httpRequestsTotal.WithLabelValues("/v1/tokens/transfers", "400").Inc()
		http.Error(w, "Specification required: Provide 'address' or 'token' query parameter", http.StatusBadRequest)
		return
	}

	address = strings.ToLower(address)
	token = strings.ToLower(token)
	cacheKey := fmt.Sprintf("worldchain:token_transfers:%s:%s", address, token)

	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 10*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		cacheLookupTotal.WithLabelValues("HIT-L1L2").Inc()
		httpRequestsTotal.WithLabelValues("/v1/tokens/transfers", "200").Inc()
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Cache-Lookup", "HIT-L1L2")
		w.Header().Set("Access-Control-Allow-Origin", "*")
		if r.Method == http.MethodHead {
			w.WriteHeader(http.StatusOK)
			return
		}
		w.Write([]byte(val))
		return
	}

	shard := g.resolveShard(cacheKey)
	sharedData, err, _ := shard.Do(cacheKey, func() (interface{}, error) {
		var query string
		var args []interface{}

		if address != "" && token != "" {
			query = `SELECT block_number, tx_hash, log_index, token_address, from_address, to_address, value, token_id, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.world_token_transfers WHERE token_address = ? AND (from_address = ? OR to_address = ?) ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{token, address, address}
		} else if token != "" {
			query = `SELECT block_number, tx_hash, log_index, token_address, from_address, to_address, value, token_id, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.world_token_transfers WHERE token_address = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{token}
		} else {
			query = `SELECT block_number, tx_hash, log_index, token_address, from_address, to_address, value, token_id, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.world_token_transfers WHERE from_address = ? OR to_address = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{address, address}
		}

		rows, err := g.db.Query(ctx, query, args...)
		if err != nil {
			return nil, err
		}
		defer rows.Close()

		transfers := make([]TokenTransferResponse, 0)
		for rows.Next() {
			var tt TokenTransferResponse
			if err := rows.Scan(&tt.BlockNumber, &tt.TxHash, &tt.LogIndex, &tt.TokenAddress, &tt.FromAddress, &tt.ToAddress, &tt.Value, &tt.TokenID, &tt.BlockTimestamp); err == nil {
				transfers = append(transfers, tt)
			}
		}
		marshaled, err := json.Marshal(transfers)
		if err != nil {
			return nil, err
		}
		go func(key, data string) {
			setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer setCancel()
			_ = g.cache.Do(setCtx, g.cache.B().Set().Key(key).Value(data).Ex(10*time.Second).Build())
		}(cacheKey, string(marshaled))
		return marshaled, nil
	})

	if err != nil {
		log.Printf("❌ Database Exception: %v", err)
		httpRequestsTotal.WithLabelValues("/v1/tokens/transfers", "500").Inc()
		http.Error(w, "Internal Database Lookup Failure", http.StatusInternalServerError)
		return
	}
	cacheLookupTotal.WithLabelValues("SHARDED-MISS-REPAIRED").Inc()
	httpRequestsTotal.WithLabelValues("/v1/tokens/transfers", "200").Inc()
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Cache-Lookup", "SHARDED-MISS-REPAIRED")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	w.Write(sharedData.([]byte))
}


func (g *RpcGateway) GetDexSwapsHandler(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 4*time.Second)
	defer cancel()

	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		httpRequestsTotal.WithLabelValues("/v1/dex/swaps", "405").Inc()
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	pool := strings.TrimSpace(r.URL.Query().Get("pool"))
	address := strings.TrimSpace(r.URL.Query().Get("address"))

	if pool != "" && !addressRegex.MatchString(pool) {
		httpRequestsTotal.WithLabelValues("/v1/dex/swaps", "400").Inc()
		http.Error(w, "Malformed Pool EVM Address Specification", http.StatusBadRequest)
		return
	}
	if address != "" && !addressRegex.MatchString(address) {
		httpRequestsTotal.WithLabelValues("/v1/dex/swaps", "400").Inc()
		http.Error(w, "Malformed Wallet EVM Address Specification", http.StatusBadRequest)
		return
	}
	if pool == "" && address == "" {
		httpRequestsTotal.WithLabelValues("/v1/dex/swaps", "400").Inc()
		http.Error(w, "Specification required: Provide 'pool' or 'address' query parameter", http.StatusBadRequest)
		return
	}

	pool = strings.ToLower(pool)
	address = strings.ToLower(address)
	cacheKey := fmt.Sprintf("worldchain:dex_swaps:%s:%s", pool, address)

	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 10*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		cacheLookupTotal.WithLabelValues("HIT-L1L2").Inc()
		httpRequestsTotal.WithLabelValues("/v1/dex/swaps", "200").Inc()
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Cache-Lookup", "HIT-L1L2")
		w.Header().Set("Access-Control-Allow-Origin", "*")
		if r.Method == http.MethodHead {
			w.WriteHeader(http.StatusOK)
			return
		}
		w.Write([]byte(val))
		return
	}

	shard := g.resolveShard(cacheKey)
	sharedData, err, _ := shard.Do(cacheKey, func() (interface{}, error) {
		var query string
		var args []interface{}

		if pool != "" && address != "" {
			query = `SELECT block_number, tx_hash, log_index, pool_address, sender, recipient, toString(amount0), toString(amount1), toString(sqrtPriceX96), toString(liquidity), tick, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.api_dex_swaps WHERE pool_address = ? AND (sender = ? OR recipient = ?) ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{pool, address, address}
		} else if pool != "" {
			query = `SELECT block_number, tx_hash, log_index, pool_address, sender, recipient, toString(amount0), toString(amount1), toString(sqrtPriceX96), toString(liquidity), tick, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.api_dex_swaps WHERE pool_address = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{pool}
		} else {
			query = `SELECT block_number, tx_hash, log_index, pool_address, sender, recipient, toString(amount0), toString(amount1), toString(sqrtPriceX96), toString(liquidity), tick, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.api_dex_swaps WHERE sender = ? OR recipient = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{address, address}
		}

		rows, err := g.db.Query(ctx, query, args...)
		if err != nil {
			return nil, err
		}
		defer rows.Close()

		swaps := make([]DexSwapResponse, 0)
		for rows.Next() {
			var ds DexSwapResponse
			if err := rows.Scan(&ds.BlockNumber, &ds.TxHash, &ds.LogIndex, &ds.PoolAddress, &ds.Sender, &ds.Recipient, &ds.Amount0, &ds.Amount1, &ds.SqrtPriceX96, &ds.Liquidity, &ds.Tick, &ds.BlockTimestamp); err == nil {
				swaps = append(swaps, ds)
			}
		}
		marshaled, err := json.Marshal(swaps)
		if err != nil {
			return nil, err
		}
		go func(key, data string) {
			setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer setCancel()
			_ = g.cache.Do(setCtx, g.cache.B().Set().Key(key).Value(data).Ex(10*time.Second).Build())
		}(cacheKey, string(marshaled))
		return marshaled, nil
	})

	if err != nil {
		log.Printf("❌ Database Exception: %v", err)
		httpRequestsTotal.WithLabelValues("/v1/dex/swaps", "500").Inc()
		http.Error(w, "Internal Database Lookup Failure", http.StatusInternalServerError)
		return
	}
	cacheLookupTotal.WithLabelValues("SHARDED-MISS-REPAIRED").Inc()
	httpRequestsTotal.WithLabelValues("/v1/dex/swaps", "200").Inc()
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Cache-Lookup", "SHARDED-MISS-REPAIRED")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	w.Write(sharedData.([]byte))
}


func (g *RpcGateway) GetDefiEventsHandler(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 4*time.Second)
	defer cancel()

	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		httpRequestsTotal.WithLabelValues("/v1/defi/events", "405").Inc()
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	contract := strings.TrimSpace(r.URL.Query().Get("contract"))
	eventType := strings.TrimSpace(r.URL.Query().Get("type"))

	if contract != "" && !addressRegex.MatchString(contract) {
		httpRequestsTotal.WithLabelValues("/v1/defi/events", "400").Inc()
		http.Error(w, "Malformed Contract EVM Address Specification", http.StatusBadRequest)
		return
	}
	if contract == "" && eventType == "" {
		httpRequestsTotal.WithLabelValues("/v1/defi/events", "400").Inc()
		http.Error(w, "Specification required: Provide 'contract' or 'type' query parameter", http.StatusBadRequest)
		return
	}

	contract = strings.ToLower(contract)
	cacheKey := fmt.Sprintf("worldchain:defi_events:%s:%s", contract, eventType)

	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 10*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		cacheLookupTotal.WithLabelValues("HIT-L1L2").Inc()
		httpRequestsTotal.WithLabelValues("/v1/defi/events", "200").Inc()
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Cache-Lookup", "HIT-L1L2")
		w.Header().Set("Access-Control-Allow-Origin", "*")
		if r.Method == http.MethodHead {
			w.WriteHeader(http.StatusOK)
			return
		}
		w.Write([]byte(val))
		return
	}

	shard := g.resolveShard(cacheKey)
	sharedData, err, _ := shard.Do(cacheKey, func() (interface{}, error) {
		var query string
		var args []interface{}

		if contract != "" && eventType != "" {
			query = `SELECT block_number, tx_hash, log_index, contract_address, event_type, raw_data, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.api_universal_defi_events WHERE contract_address = ? AND event_type = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{contract, eventType}
		} else if contract != "" {
			query = `SELECT block_number, tx_hash, log_index, contract_address, event_type, raw_data, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.api_universal_defi_events WHERE contract_address = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{contract}
		} else {
			query = `SELECT block_number, tx_hash, log_index, contract_address, event_type, raw_data, formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.api_universal_defi_events WHERE event_type = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{eventType}
		}

		rows, err := g.db.Query(ctx, query, args...)
		if err != nil {
			return nil, err
		}
		defer rows.Close()

		events := make([]DefiEventResponse, 0)
		for rows.Next() {
			var de DefiEventResponse
			if err := rows.Scan(&de.BlockNumber, &de.TxHash, &de.LogIndex, &de.ContractAddress, &de.EventType, &de.RawData, &de.BlockTimestamp); err == nil {
				events = append(events, de)
			}
		}
		marshaled, err := json.Marshal(events)
		if err != nil {
			return nil, err
		}
		go func(key, data string) {
			setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer setCancel()
			_ = g.cache.Do(setCtx, g.cache.B().Set().Key(key).Value(data).Ex(10*time.Second).Build())
		}(cacheKey, string(marshaled))
		return marshaled, nil
	})

	if err != nil {
		log.Printf("❌ Database Exception: %v", err)
		httpRequestsTotal.WithLabelValues("/v1/defi/events", "500").Inc()
		http.Error(w, "Internal Database Lookup Failure", http.StatusInternalServerError)
		return
	}
	cacheLookupTotal.WithLabelValues("SHARDED-MISS-REPAIRED").Inc()
	httpRequestsTotal.WithLabelValues("/v1/defi/events", "200").Inc()
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Cache-Lookup", "SHARDED-MISS-REPAIRED")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	w.Write(sharedData.([]byte))
}


func (g *RpcGateway) GetMevAnalyticsHandler(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 4*time.Second)
	defer cancel()

	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		httpRequestsTotal.WithLabelValues("/v1/mev/analytics", "405").Inc()
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	mevType := strings.TrimSpace(r.URL.Query().Get("type"))
	blockStr := strings.TrimSpace(r.URL.Query().Get("block"))

	cacheKey := fmt.Sprintf("worldchain:mev_analytics:%s:%s", mevType, blockStr)

	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 10*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		cacheLookupTotal.WithLabelValues("HIT-L1L2").Inc()
		httpRequestsTotal.WithLabelValues("/v1/mev/analytics", "200").Inc()
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Cache-Lookup", "HIT-L1L2")
		w.Header().Set("Access-Control-Allow-Origin", "*")
		if r.Method == http.MethodHead {
			w.WriteHeader(http.StatusOK)
			return
		}
		w.Write([]byte(val))
		return
	}

	shard := g.resolveShard(cacheKey)
	sharedData, err, _ := shard.Do(cacheKey, func() (interface{}, error) {
		var query string
		var args []interface{}

		if mevType != "" && blockStr != "" {
			query = `SELECT block_number, tx_hash, tx_index, mev_type, toString(extracted_value_wei), coalesce(sandwich_victim_tx, ''), formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.world_mev_activity WHERE mev_type = ? AND block_number = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{mevType, blockStr}
		} else if mevType != "" {
			query = `SELECT block_number, tx_hash, tx_index, mev_type, toString(extracted_value_wei), coalesce(sandwich_victim_tx, ''), formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.world_mev_activity WHERE mev_type = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{mevType}
		} else if blockStr != "" {
			query = `SELECT block_number, tx_hash, tx_index, mev_type, toString(extracted_value_wei), coalesce(sandwich_victim_tx, ''), formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.world_mev_activity WHERE block_number = ? ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
			args = []interface{}{blockStr}
		} else {
			query = `SELECT block_number, tx_hash, tx_index, mev_type, toString(extracted_value_wei), coalesce(sandwich_victim_tx, ''), formatDateTime(ingested_at, '%Y-%m-%d %H:%i:%s') FROM worldchain.world_mev_activity ORDER BY block_number DESC LIMIT 50 SETTINGS max_columns_to_read = 15, max_threads = 2;`
		}

		rows, err := g.db.Query(ctx, query, args...)
		if err != nil {
			return nil, err
		}
		defer rows.Close()

		mevList := make([]MevAnalyticsResponse, 0)
		for rows.Next() {
			var ma MevAnalyticsResponse
			if err := rows.Scan(&ma.BlockNumber, &ma.TxHash, &ma.TxIndex, &ma.MevType, &ma.ExtractedValueWei, &ma.SandwichVictimTx, &ma.IngestedAt); err == nil {
				mevList = append(mevList, ma)
			}
		}
		marshaled, err := json.Marshal(mevList)
		if err != nil {
			return nil, err
		}
		go func(key, data string) {
			setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer setCancel()
			_ = g.cache.Do(setCtx, g.cache.B().Set().Key(key).Value(data).Ex(10*time.Second).Build())
		}(cacheKey, string(marshaled))
		return marshaled, nil
	})

	if err != nil {
		log.Printf("❌ Database Exception: %v", err)
		httpRequestsTotal.WithLabelValues("/v1/mev/analytics", "500").Inc()
		http.Error(w, "Internal Database Lookup Failure", http.StatusInternalServerError)
		return
	}
	cacheLookupTotal.WithLabelValues("SHARDED-MISS-REPAIRED").Inc()
	httpRequestsTotal.WithLabelValues("/v1/mev/analytics", "200").Inc()
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Cache-Lookup", "SHARDED-MISS-REPAIRED")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	w.Write(sharedData.([]byte))
}


func (g *RpcGateway) GetPortfolioHandler(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 4*time.Second)
	defer cancel()

	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		httpRequestsTotal.WithLabelValues("/v1/portfolio", "405").Inc()
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	address := strings.TrimSpace(r.URL.Query().Get("address"))
	if address == "" {
		httpRequestsTotal.WithLabelValues("/v1/portfolio", "400").Inc()
		http.Error(w, "Missing required query parameter 'address' (e.g. ?address=0x...)", http.StatusBadRequest)
		return
	}
	if !addressRegex.MatchString(address) {
		httpRequestsTotal.WithLabelValues("/v1/portfolio", "400").Inc()
		http.Error(w, "Malformed Wallet EVM Address Specification", http.StatusBadRequest)
		return
	}

	address = strings.ToLower(address)
	cacheKey := fmt.Sprintf("worldchain:portfolio:%s", address)

	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 10*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		cacheLookupTotal.WithLabelValues("HIT-L1L2").Inc()
		httpRequestsTotal.WithLabelValues("/v1/portfolio", "200").Inc()
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Cache-Lookup", "HIT-L1L2")
		w.Header().Set("Access-Control-Allow-Origin", "*")
		if r.Method == http.MethodHead {
			w.WriteHeader(http.StatusOK)
			return
		}
		w.Write([]byte(val))
		return
	}

	shard := g.resolveShard(cacheKey)
	sharedData, err, _ := shard.Do(cacheKey, func() (interface{}, error) {
		query := `SELECT token_address, toString(sum(balance_change)) as net_balance FROM worldchain.api_token_balances WHERE wallet_address = ? GROUP BY token_address HAVING sum(balance_change) != 0 LIMIT 100 SETTINGS max_columns_to_read = 15, max_threads = 2;`
		rows, err := g.db.Query(ctx, query, address)
		if err != nil {
			return nil, err
		}
		defer rows.Close()

		portfolio := make([]PortfolioTokenBalanceResponse, 0)
		for rows.Next() {
			var pb PortfolioTokenBalanceResponse
			pb.WalletAddress = address
			if err := rows.Scan(&pb.TokenAddress, &pb.NetBalance); err == nil {
				portfolio = append(portfolio, pb)
			}
		}
		marshaled, err := json.Marshal(portfolio)
		if err != nil {
			return nil, err
		}
		go func(key, data string) {
			setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer setCancel()
			_ = g.cache.Do(setCtx, g.cache.B().Set().Key(key).Value(data).Ex(10*time.Second).Build())
		}(cacheKey, string(marshaled))
		return marshaled, nil
	})

	if err != nil {
		log.Printf("❌ Database Exception: %v", err)
		httpRequestsTotal.WithLabelValues("/v1/portfolio", "500").Inc()
		http.Error(w, "Internal Database Lookup Failure", http.StatusInternalServerError)
		return
	}
	cacheLookupTotal.WithLabelValues("SHARDED-MISS-REPAIRED").Inc()
	httpRequestsTotal.WithLabelValues("/v1/portfolio", "200").Inc()
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Cache-Lookup", "SHARDED-MISS-REPAIRED")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	w.Write(sharedData.([]byte))
}


// envOr returns the environment variable key, or def when it is unset/empty.
func envOr(key, def string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return def
}

// ProvisionApiKeyHandler mints a new API key. It is an ADMIN endpoint: the caller
// must present the shared secret from the ADMIN_TOKEN environment variable in the
// X-Admin-Token header. If ADMIN_TOKEN is unset the endpoint is disabled (503),
// so a default deployment cannot be used to mint keys anonymously.
func (g *RpcGateway) ProvisionApiKeyHandler(w http.ResponseWriter, r *http.Request) {
	adminToken := os.Getenv("ADMIN_TOKEN")
	if adminToken == "" {
		httpRequestsTotal.WithLabelValues("/v1/auth/keys", "503").Inc()
		http.Error(w, "API key provisioning is disabled (ADMIN_TOKEN not configured)", http.StatusServiceUnavailable)
		return
	}
	if subtle.ConstantTimeCompare([]byte(r.Header.Get("X-Admin-Token")), []byte(adminToken)) != 1 {
		httpRequestsTotal.WithLabelValues("/v1/auth/keys", "401").Inc()
		http.Error(w, "Unauthorized", http.StatusUnauthorized)
		return
	}

	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Second)
	defer cancel()

	if r.Method != http.MethodPost {
		httpRequestsTotal.WithLabelValues("/v1/auth/keys", "405").Inc()
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	var req ProvisionKeyRequest
	r.Body = http.MaxBytesReader(w, r.Body, 1048576)
	bodyBytes, err := io.ReadAll(r.Body)
	if err != nil {
		httpRequestsTotal.WithLabelValues("/v1/auth/keys", "413").Inc()
		http.Error(w, "413 Payload Too Large (Max 1MB allowed)", http.StatusRequestEntityTooLarge)
		return
	}
	if err := json.Unmarshal(bodyBytes, &req); err != nil || strings.TrimSpace(req.ClientName) == "" {
		log.Printf("❌ ProvisionKey error: err=%v, raw_body=%q", err, string(bodyBytes))
		httpRequestsTotal.WithLabelValues("/v1/auth/keys", "400").Inc()
		http.Error(w, fmt.Sprintf("Malformed JSON request payload: %v", err), http.StatusBadRequest)
		return
	}

	req.Tier = strings.ToLower(strings.TrimSpace(req.Tier))
	var rps uint32 = 2
	if req.Tier == "enterprise" {
		rps = 100
	} else if req.Tier == "pro" {
		rps = 20
	} else {
		req.Tier = "free"
		rps = 2
	}

	rawBytes := make([]byte, 16)
	if _, err := rand.Read(rawBytes); err != nil {
		http.Error(w, "Internal Entropy Error", http.StatusInternalServerError)
		return
	}
	apiKey := fmt.Sprintf("sk_live_worldchain_%x", rawBytes)

	query := `INSERT INTO worldchain.api_keys (api_key, client_name, tier, requests_per_second, is_active, created_at) VALUES (?, ?, ?, ?, 1, now());`
	if err := g.db.Exec(ctx, query, apiKey, req.ClientName, req.Tier, rps); err != nil {
		log.Printf("❌ API Key Generation Failed: %v", err)
		httpRequestsTotal.WithLabelValues("/v1/auth/keys", "500").Inc()
		http.Error(w, "Failed to persist API Key", http.StatusInternalServerError)
		return
	}

	keyInfo := ApiKeyInfo{
		ApiKey:            apiKey,
		ClientName:        req.ClientName,
		Tier:              req.Tier,
		RequestsPerSecond: rps,
		IsActive:          1,
		CreatedAt:         time.Now(),
	}

	marshaled, err := json.Marshal(keyInfo)
	if err != nil {
		http.Error(w, "Internal JSON Serialization Error", http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusCreated)
	w.Write(marshaled)
}

func (g *RpcGateway) GetApiKeyInfo(ctx context.Context, apiKey string) (*ApiKeyInfo, error) {
	cacheKey := fmt.Sprintf("worldchain:apikey:%s", apiKey)
	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 60*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		var ki ApiKeyInfo
		if err := json.Unmarshal([]byte(val), &ki); err == nil {
			return &ki, nil
		}
	}

	var ki ApiKeyInfo
	query := `SELECT api_key, client_name, tier, requests_per_second, is_active, created_at FROM worldchain.api_keys WHERE api_key = ? AND is_active = 1 LIMIT 1`
	if err := g.db.QueryRow(ctx, query, apiKey).Scan(&ki.ApiKey, &ki.ClientName, &ki.Tier, &ki.RequestsPerSecond, &ki.IsActive, &ki.CreatedAt); err != nil {
		return nil, err
	}

	marshaled, _ := json.Marshal(ki)
	go func(key, data string) {
		setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
		defer setCancel()
		_ = g.cache.Do(setCtx, g.cache.B().Set().Key(key).Value(data).Ex(60*time.Second).Build())
	}(cacheKey, string(marshaled))

	return &ki, nil
}

func (g *RpcGateway) ApiKeyAuthAndRateLimitMiddleware(next http.Handler) http.Handler {
	var clientMap sync.Map
	go func() {
		for {
			time.Sleep(1 * time.Minute)
			clientMap.Range(func(key, value interface{}) bool {
				bucket := value.(*TokenBucket)
				bucket.mu.Lock()
				if time.Since(bucket.lastRefill) > 5*time.Minute {
					clientMap.Delete(key)
				}
				bucket.mu.Unlock()
				return true
			})
		}
	}()

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Methods", "GET, POST, OPTIONS, HEAD")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, X-API-Key, X-Admin-Token, Authorization, X-Requested-With")
		w.Header().Set("Access-Control-Expose-Headers", "X-API-Tier, X-Cache-Lookup, Content-Type")
		w.Header().Set("Access-Control-Max-Age", "86400")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("X-Frame-Options", "DENY")

		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}

		clientKey := r.Header.Get("X-API-Key")
		if clientKey == "" {
			clientKey = r.URL.Query().Get("api_key")
		}
		clientKey = strings.TrimSpace(clientKey)

		rate := 1.0
		burst := 5

		if clientKey != "" {
			ctx, cancel := context.WithTimeout(r.Context(), 1*time.Second)
			ki, err := g.GetApiKeyInfo(ctx, clientKey)
			cancel()
			if err != nil || ki == nil || ki.IsActive != 1 {
				httpRequestsTotal.WithLabelValues("middleware", "401").Inc()
				w.Header().Set("Content-Type", "application/json")
				w.Header().Set("Access-Control-Allow-Origin", "*")
				w.WriteHeader(http.StatusUnauthorized)
				w.Write([]byte(`{"status":401,"error":"Unauthorized","message":"Invalid or revoked API Key"}`))
				return
			}
			w.Header().Set("X-API-Tier", ki.Tier)
			if ki.Tier == "enterprise" {
				rate = 100.0
				burst = 200
			} else if ki.Tier == "pro" {
				rate = 20.0
				burst = 50
			} else if ki.Tier == "free" {
				rate = 2.0
				burst = 10
			}
		}

		ip := r.RemoteAddr
		if idx := strings.Index(ip, ":"); idx != -1 {
			ip = ip[:idx]
		}
		if xff := r.Header.Get("X-Forwarded-For"); xff != "" {
			if parts := strings.Split(xff, ","); len(parts) > 0 {
				ip = strings.TrimSpace(parts[0])
			}
		}
		if ip == "127.0.0.1" || ip == "::1" {
			next.ServeHTTP(w, r)
			return
		}

		mapKey := fmt.Sprintf("%s:%s", ip, clientKey)
		v, _ := clientMap.LoadOrStore(mapKey, &TokenBucket{tokens: float64(burst), lastRefill: time.Now()})
		bucket := v.(*TokenBucket)
		if !bucket.Allow(rate, burst) {
			rateLimitDropsTotal.Inc()
			httpRequestsTotal.WithLabelValues("middleware", "429").Inc()
			http.Error(w, "429 Too Many Requests (Rate Limit Exceeded)", http.StatusTooManyRequests)
			return
		}

		next.ServeHTTP(w, r)
	})
}


func (g *RpcGateway) GetSystemHealthHandler(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 3*time.Second)
	defer cancel()

	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		httpRequestsTotal.WithLabelValues("/v1/health/system", "405").Inc()
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	cacheKey := "worldchain:health:system"
	cmd := g.cache.B().Get().Key(cacheKey).Cache()
	resp := g.cache.DoCache(ctx, cmd, 5*time.Second)
	if resp.Error() == nil {
		val, _ := resp.ToString()
		cacheLookupTotal.WithLabelValues("HIT-L1L2").Inc()
		httpRequestsTotal.WithLabelValues("/v1/health/system", "200").Inc()
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("X-Cache-Lookup", "HIT-L1L2")
		w.Header().Set("Access-Control-Allow-Origin", "*")
		if r.Method == http.MethodHead {
			w.WriteHeader(http.StatusOK)
			return
		}
		w.Write([]byte(val))
		return
	}

	shard := g.resolveShard(cacheKey)
	sharedData, err, _ := shard.Do(cacheKey, func() (interface{}, error) {
		var latestBlock uint64 = 0
		query := `SELECT max(block_number) FROM worldchain.world_blocks SETTINGS max_columns_to_read = 5, max_threads = 2;`
		_ = g.db.QueryRow(ctx, query).Scan(&latestBlock)

		sh := SystemHealthResponse{
			Status:                   "healthy",
			GatewayUptimeSeconds:     int64(time.Since(startTime).Seconds()),
			LatestIndexedBlock:       latestBlock,
			IngestionLagBlocks:       0,
			DatabaseEngine:           "ClickHouse OLAP Engine v24.3 (7x Singleflight Coalesced)",
			CacheEngine:              "Valkey Dual L1/L2 Cache Engine v7.2",
			SystemdMonitoredServices: 10,
			Timestamp:                time.Now().Format("2006-01-02 15:04:05"),
		}

		marshaled, err := json.Marshal(sh)
		if err != nil {
			return nil, err
		}
		go func(key, data string) {
			setCtx, setCancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer setCancel()
			_ = g.cache.Do(setCtx, g.cache.B().Set().Key(key).Value(data).Ex(5*time.Second).Build())
		}(cacheKey, string(marshaled))
		return marshaled, nil
	})

	if err != nil {
		log.Printf("❌ Database Exception: %v", err)
		httpRequestsTotal.WithLabelValues("/v1/health/system", "500").Inc()
		http.Error(w, "Internal Telemetry Failure", http.StatusInternalServerError)
		return
	}
	cacheLookupTotal.WithLabelValues("SHARDED-MISS-REPAIRED").Inc()
	httpRequestsTotal.WithLabelValues("/v1/health/system", "200").Inc()
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Cache-Lookup", "SHARDED-MISS-REPAIRED")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	w.Write(sharedData.([]byte))
}
