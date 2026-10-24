// Package main — Supersonic DeFi Sentinel API
// File: cmd/api/contracts.go
//
// This file contains all contract- and risk-related HTTP handlers.
// It is compiled as part of package main alongside cmd/api/main.go,
// which owns the global `db *sql.DB` variable and starts the HTTP server on :8090.
//
// Route registration is performed by init() so that no changes to main.go
// are required; Go guarantees all init() functions execute before main().
//
// All handlers:
//   - Apply a 5-second query timeout via context.WithTimeout
//   - Set Content-Type: application/json on every response
//   - Return {"error":"…"} JSON bodies with appropriate HTTP status codes
//   - Validate Ethereum address parameters with the canonical 0x-hex regex
//   - Use r.PathValue("address") (Go 1.22+ stdlib mux)

package main

import (
	"log/slog"
	"sync"
	"github.com/redis/rueidis"
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"log"
	"math"
	"math/big"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// ──────────────────────────────────────────────────────────────────────────────
// Constants & package-level helpers
// ──────────────────────────────────────────────────────────────────────────────

const (
	queryTimeout    = 15 * time.Second
	defaultLimit    = 50
	maxLimit        = 200
	defaultOffset   = 0
	interactionsTop = 20
	riskTop         = 50
	gasFuncTop      = 30
	mevTop          = 50
)

// addrRegex validates a checksummed or lower-case Ethereum address.
var addrRegex = regexp.MustCompile(`^0x[0-9a-fA-F]{40}$`)

var (
	depegCacheRaw []byte
	lastDepegSync time.Time
	healthCacheMu      sync.RWMutex
	cachedMaxBlock     uint64
	cachedMaxTime      time.Time
	lastHealthSync     time.Time
	cachedClickHouseOk bool
	depegCacheMu  sync.RWMutex
)

// ──────────────────────────────────────────────────────────────────────────────
// init — Route registration
// All routes registered here; zero changes to main.go required.
// ──────────────────────────────────────────────────────────────────────────────

func init() {
	// Contract list & detail
	http.HandleFunc("GET /v1/contracts", handleListContracts)
	http.HandleFunc("GET /v1/contracts/{address}", handleGetContract)

	// Contract sub-resources
	http.HandleFunc("GET /v1/contracts/{address}/source", handleContractSource)
	http.HandleFunc("GET /v1/contracts/{address}/cfg", handleContractCFG)
	http.HandleFunc("GET /v1/contracts/{address}/functions", handleContractFunctions)
	http.HandleFunc("GET /v1/contracts/{address}/interactions", handleContractInteractions)
	http.HandleFunc("GET /v1/contracts/{address}/balance-sheet", handleContractBalanceSheet)
	http.HandleFunc("GET /v1/contracts/{address}/gas", handleContractGas)

	// Risk endpoints
	http.HandleFunc("GET /v1/risk/depegs", handleRiskDepegs)
	http.HandleFunc("GET /v1/health", handleHealth)
	http.HandleFunc("GET /v1/risk/mev", handleRiskMEV)
	http.HandleFunc("GET /v1/risk/bridge-solvency", handleRiskBridgeSolvency)
	http.HandleFunc("GET /v1/contracts/{address}/risk-simulation", handleContractRiskSimulation)

	slog.Info("[sentinel] DeFi Sentinel API routes registered — 12 endpoints live")
}

// ──────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ──────────────────────────────────────────────────────────────────────────────

func jsonError(w http.ResponseWriter, status int, msg string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]string{"error": msg})
}

func jsonOK(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(v); err != nil {
		slog.Error("json encode error", "handler", "sentinel", "error", err)
	}
}

func validateAddress(w http.ResponseWriter, addr string) bool {
	if !addrRegex.MatchString(addr) {
		jsonError(w, http.StatusBadRequest, "invalid address: must match ^0x[0-9a-fA-F]{40}$")
		return false
	}
	return true
}

func queryCtx(r *http.Request) (context.Context, context.CancelFunc) {
	return context.WithTimeout(r.Context(), queryTimeout)
}

func intParam(r *http.Request, key string, def, ceil int) int {
	raw := r.URL.Query().Get(key)
	if raw == "" {
		return def
	}
	v, err := strconv.Atoi(raw)
	if err != nil || v < 0 {
		return def
	}
	if ceil > 0 && v > ceil {
		return ceil
	}
	return v
}

// ──────────────────────────────────────────────────────────────────────────────
// 1. GET /v1/contracts
// ──────────────────────────────────────────────────────────────────────────────

type contractSummary struct {
	Address         string    `json:"address"`
	Deployer        string    `json:"deployer"`
	BlockNumber     uint64    `json:"block_number"`
	DeployedAt      time.Time `json:"deployed_at"`
	IsVerified      bool      `json:"is_verified"`
	IsDecompiled    bool      `json:"is_decompiled"`
	IsProxy         bool      `json:"is_proxy"`
	ContractName    *string   `json:"contract_name,omitempty"`
	CompilerVersion *string   `json:"compiler_version,omitempty"`
}

func handleListContracts(w http.ResponseWriter, r *http.Request) {
limit  := intParam(r, "limit", defaultLimit, maxLimit)
offset := intParam(r, "offset", defaultOffset, 0)

var whereClause string
switch r.URL.Query().Get("verified") {
case "true":
whereClause = "WHERE is_verified = 1"
case "false":
whereClause = "WHERE is_verified = 0"
default:
whereClause = ""
}

cacheKey := fmt.Sprintf("cache:contracts:list:%s:%d:%d", r.URL.Query().Get("verified"), limit, offset)

if client := getValkeyClient(); client != nil {
cmd := client.B().Get().Key(cacheKey).Build()
if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
w.Header().Set("Content-Type", "application/json")
w.WriteHeader(http.StatusOK)
_, _ = w.Write([]byte(res))
return
}
}

query := `
SELECT
address, deployer, block_number, deployed_at,
is_verified, is_decompiled, is_proxy,
contract_name, compiler_version
FROM worldchain.world_contracts
FINAL
` + whereClause + `
ORDER BY deployed_at DESC
LIMIT ? OFFSET ?`

ctx, cancel := queryCtx(r)
defer cancel()

rows, err := db.QueryContext(ctx, query, limit, offset)
if err != nil {
slog.Error("query error", "handler", "handleListContracts", "error", err)
jsonError(w, http.StatusInternalServerError, "database query failed")
return
}
defer rows.Close()

results := make([]contractSummary, 0, limit)
for rows.Next() {
var c contractSummary
if err := rows.Scan(
&c.Address, &c.Deployer, &c.BlockNumber, &c.DeployedAt,
&c.IsVerified, &c.IsDecompiled, &c.IsProxy,
&c.ContractName, &c.CompilerVersion,
); err != nil {
slog.Error("scan", "handler", "handleListContracts", "error", err)
jsonError(w, http.StatusInternalServerError, "row scan failed")
return
}
results = append(results, c)
}
if err := rows.Err(); err != nil {
jsonError(w, http.StatusInternalServerError, "iteration failed")
return
}

if data, err := json.Marshal(results); err == nil {
if client := getValkeyClient(); client != nil {
go func(k string, v []byte) {
cmd := client.B().Set().Key(k).Value(string(v)).Ex(1 * time.Hour).Build()
_ = client.Do(context.Background(), cmd)
}(cacheKey, data)
}
w.Header().Set("Content-Type", "application/json")
w.WriteHeader(http.StatusOK)
_, _ = w.Write(data)
return
}

jsonOK(w, results)
}

// ──────────────────────────────────────────────────────────────────────────────
// 2. GET /v1/contracts/{address}
// ──────────────────────────────────────────────────────────────────────────────

type contractDetail struct {
	Address            string    `json:"address"`
	Deployer           string    `json:"deployer"`
	TxHash             string    `json:"tx_hash"`
	BlockNumber        uint64    `json:"block_number"`
	DeployedAt         time.Time `json:"deployed_at"`
	IsVerified         bool      `json:"is_verified"`
	IsDecompiled       bool      `json:"is_decompiled"`
	IsProxy            bool      `json:"is_proxy"`
	ContractName       string    `json:"contract_name"`
	CompilerVersion    string    `json:"compiler_version"`
	LicenseType        string    `json:"license_type"`
	VerificationSource string    `json:"verification_source"`
	ProxyImplAddress   *string   `json:"proxy_impl_address,omitempty"`
	VerifiedAt         *time.Time `json:"verified_at,omitempty"`
}

func handleGetContract(w http.ResponseWriter, r *http.Request) {
addr := strings.ToLower(r.PathValue("address"))
if !validateAddress(w, addr) {
return
}

cacheKey := "cache:contract:get:" + addr

if client := getValkeyClient(); client != nil {
cmd := client.B().Get().Key(cacheKey).Build()
if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
w.Header().Set("Content-Type", "application/json")
cacheHits.Inc()
			w.Header().Set("X-Cache", "HIT")
w.WriteHeader(http.StatusOK)
_, _ = w.Write([]byte(res))
return
}
}

const query = `
SELECT
address, deployer, creator_tx_hash AS tx_hash, block_number, deployed_at,
is_verified, is_decompiled, is_proxy,
contract_name, compiler_version, license_type, verification_source,
proxy_impl_address, verified_at
FROM worldchain.world_contracts
WHERE address = ?
ORDER BY position(verification_source, 'Etherscan') DESC, ingested_at DESC
LIMIT 1`

ctx, cancel := queryCtx(r)
defer cancel()

var c contractDetail
err := db.QueryRowContext(ctx, query, addr).Scan(
&c.Address, &c.Deployer, &c.TxHash, &c.BlockNumber, &c.DeployedAt,
&c.IsVerified, &c.IsDecompiled, &c.IsProxy,
&c.ContractName, &c.CompilerVersion, &c.LicenseType, &c.VerificationSource,
&c.ProxyImplAddress, &c.VerifiedAt,
)
if err == sql.ErrNoRows {
jsonError(w, http.StatusNotFound, "contract not found")
return
}
if err != nil {
slog.Error("query error", "handler", "handleGetContract", "error", err)
jsonError(w, http.StatusInternalServerError, "database query failed")
return
}

data, err := json.Marshal(c)
if err != nil {
jsonError(w, http.StatusInternalServerError, "json marshal failed")
return
}

if client := getValkeyClient(); client != nil {
go func(k string, v []byte) {
cmd := client.B().Set().Key(k).Value(string(v)).Ex(1 * time.Hour).Build()
if err := client.Do(context.Background(), cmd).Error(); err != nil { log.Printf("API-SERVER warn: Valkey cache set failed: %v", err) }
}(cacheKey, data)
}

w.Header().Set("Content-Type", "application/json")
cacheMisses.Inc()
	w.Header().Set("X-Cache", "MISS")
w.WriteHeader(http.StatusOK)
_, _ = w.Write(data)
}

// ──────────────────────────────────────────────────────────────────────────────
// 3. GET /v1/contracts/{address}/source
// ──────────────────────────────────────────────────────────────────────────────

type contractSource struct {
	Address         string `json:"address"`
	SourceCode      string `json:"source_code"`
	ABI             string `json:"abi"`
	Disassembly     string `json:"disassembly"`
	CompilerVersion string `json:"compiler_version"`
	IsVerified      bool   `json:"is_verified"`
	IsDecompiled    bool   `json:"is_decompiled"`
}

func handleContractSource(w http.ResponseWriter, r *http.Request) {
	addr := r.PathValue("address")
	if !validateAddress(w, addr) {
		return
	}

	cacheKey := "cache:contract:source:" + addr
	if client := getValkeyClient(); client != nil {
		cmd := client.B().Get().Key(cacheKey).Build()
		if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(res))
			return
		}
	}

	const query = `
		SELECT address, source_code, abi, disassembly, compiler_version, is_verified, is_decompiled
		FROM worldchain.world_contracts
		FINAL
		WHERE address = ?
		LIMIT 1`

	ctx, cancel := queryCtx(r)
	defer cancel()

	var s contractSource
	err := db.QueryRowContext(ctx, query, addr).Scan(
		&s.Address, &s.SourceCode, &s.ABI, &s.Disassembly,
		&s.CompilerVersion, &s.IsVerified, &s.IsDecompiled,
	)
	if err == sql.ErrNoRows {
		jsonError(w, http.StatusNotFound, "contract not found")
		return
	}
	if err != nil {
		slog.Error("query error", "handler", "handleContractSource", "error", err)
		jsonError(w, http.StatusInternalServerError, "database query failed")
		return
	}
	jsonOK(w, s)
}

// ──────────────────────────────────────────────────────────────────────────────
// 4. GET /v1/contracts/{address}/cfg
// ──────────────────────────────────────────────────────────────────────────────

type contractCFG struct {
	Address  string `json:"address"`
	CFGGraph string `json:"cfg_graph"`
}

func handleContractCFG(w http.ResponseWriter, r *http.Request) {
	addr := r.PathValue("address")
	if !validateAddress(w, addr) {
		return
	}

	cacheKey := "cache:contract:cfg:" + addr
	if client := getValkeyClient(); client != nil {
		cmd := client.B().Get().Key(cacheKey).Build()
		if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(res))
			return
		}
	}

	const query = `
		SELECT address, cfg_graph
		FROM worldchain.world_contracts
		FINAL
		WHERE address = ?
		LIMIT 1`

	ctx, cancel := queryCtx(r)
	defer cancel()

	var c contractCFG
	err := db.QueryRowContext(ctx, query, addr).Scan(&c.Address, &c.CFGGraph)
	if err == sql.ErrNoRows {
		jsonError(w, http.StatusNotFound, "contract not found")
		return
	}
	if err != nil {
		slog.Error("query error", "handler", "handleContractCFG", "error", err)
		jsonError(w, http.StatusInternalServerError, "database query failed")
		return
	}
	jsonOK(w, c)
}

// ──────────────────────────────────────────────────────────────────────────────
// 5. GET /v1/contracts/{address}/functions
// ──────────────────────────────────────────────────────────────────────────────

type contractFunction struct {
	FunctionSelector string `json:"function_selector"`
	FunctionName     string `json:"function_name"`
	Signature        string `json:"signature"`
	Visibility       string `json:"visibility"`
	StateMutability  string `json:"state_mutability"`
	IsPayable        bool   `json:"is_payable"`
	RequiresWorldID  bool   `json:"requires_world_id"`
}

func handleContractFunctions(w http.ResponseWriter, r *http.Request) {
	addr := r.PathValue("address")
	if !validateAddress(w, addr) {
		return
	}

	cacheKey := "cache:contract:funcs:" + addr
	if client := getValkeyClient(); client != nil {
		cmd := client.B().Get().Key(cacheKey).Build()
		if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(res))
			return
		}
	}

	const query = `
		SELECT
			function_selector, function_name, signature,
			visibility, state_mutability, is_payable, requires_world_id
		FROM worldchain.world_contract_functions
		WHERE address = ?
		ORDER BY function_name`

	ctx, cancel := queryCtx(r)
	defer cancel()

	rows, err := db.QueryContext(ctx, query, addr)
	if err != nil {
		slog.Error("query", "handler", "handleContractFunctions", "error", err)
		jsonError(w, http.StatusInternalServerError, "database query failed")
		return
	}
	defer rows.Close()

	funcs := make([]contractFunction, 0, 32)
	for rows.Next() {
		var f contractFunction
		if err := rows.Scan(
			&f.FunctionSelector, &f.FunctionName, &f.Signature,
			&f.Visibility, &f.StateMutability, &f.IsPayable, &f.RequiresWorldID,
		); err != nil {
			slog.Error("scan", "handler", "handleContractFunctions", "error", err)
			jsonError(w, http.StatusInternalServerError, "row scan failed")
			return
		}
		funcs = append(funcs, f)
	}
	if err := rows.Err(); err != nil {
		jsonError(w, http.StatusInternalServerError, "iteration failed")
		return
	}
	jsonOK(w, funcs)
}

// ──────────────────────────────────────────────────────────────────────────────
// 6. GET /v1/contracts/{address}/interactions
// ──────────────────────────────────────────────────────────────────────────────

type contractInteraction struct {
	ToAddress        string `json:"to_address"`
	FunctionSelector string `json:"function_selector"`
	Calls            uint64 `json:"calls"`
	Errors           uint64 `json:"errors"`
}

func handleContractInteractions(w http.ResponseWriter, r *http.Request) {
	addr := r.PathValue("address")
	if !validateAddress(w, addr) {
		return
	}

	cacheKey := "cache:contract:interactions:" + addr
	if client := getValkeyClient(); client != nil {
		cmd := client.B().Get().Key(cacheKey).Build()
		if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(res))
			return
		}
	}

	const query = `
		SELECT
			to_address,
			function_selector,
			sum(call_count)  AS calls,
			sum(error_count) AS errors
		FROM worldchain.world_contract_interactions
		WHERE from_address = ?
		GROUP BY to_address, function_selector
		ORDER BY calls DESC
		LIMIT ?`

	ctx, cancel := queryCtx(r)
	defer cancel()

	rows, err := db.QueryContext(ctx, query, addr, interactionsTop)
	if err != nil {
		slog.Error("query", "handler", "handleContractInteractions", "error", err)
		jsonError(w, http.StatusInternalServerError, "database query failed")
		return
	}
	defer rows.Close()

	interactions := make([]contractInteraction, 0, interactionsTop)
	for rows.Next() {
		var i contractInteraction
		if err := rows.Scan(&i.ToAddress, &i.FunctionSelector, &i.Calls, &i.Errors); err != nil {
			slog.Error("scan", "handler", "handleContractInteractions", "error", err)
			jsonError(w, http.StatusInternalServerError, "row scan failed")
			return
		}
		interactions = append(interactions, i)
	}
	if err := rows.Err(); err != nil {
		jsonError(w, http.StatusInternalServerError, "iteration failed")
		return
	}
	jsonOK(w, interactions)
}

// ──────────────────────────────────────────────────────────────────────────────
// 7. GET /v1/risk/depegs
// ──────────────────────────────────────────────────────────────────────────────

type depegDrift struct {
	TokenAddress string    `json:"token_address"`
	PoolAddress  string    `json:"pool_address"`
	SpotPrice    float64   `json:"spot_price"`
	OraclePrice  float64   `json:"oracle_price"`
	DriftRatio   float64   `json:"drift_ratio"`
	BlockNumber  uint64    `json:"block_number"`
	UpdatedAt    time.Time `json:"updated_at"`
}

func handleRiskDepegs(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")

	depegCacheMu.RLock()
	cached := depegCacheRaw
	depegCacheMu.RUnlock()

	if len(cached) == 0 {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("[]"))
		return
	}

	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(cached)
}

// ──────────────────────────────────────────────────────────────────────────────
// 8. GET /v1/risk/mev
// ──────────────────────────────────────────────────────────────────────────────

type mevActivity struct {
	TxHash      string    `json:"tx_hash"`
	MEVType     string    `json:"mev_type"`
	BlockNumber uint64    `json:"block_number"`
	IngestedAt  time.Time `json:"ingested_at"`
}

func handleRiskMEV(w http.ResponseWriter, r *http.Request) {
	const query = `
		SELECT tx_hash, mev_type, block_number, ingested_at
		FROM worldchain.world_mev_activity
		ORDER BY block_number DESC
		LIMIT ?`

	ctx, cancel := queryCtx(r)
	defer cancel()

	rows, err := db.QueryContext(ctx, query, mevTop)
	if err != nil {
		slog.Error("query", "handler", "handleRiskMEV", "error", err)
		jsonError(w, http.StatusInternalServerError, "database query failed")
		return
	}
	defer rows.Close()

	events := make([]mevActivity, 0, mevTop)
	for rows.Next() {
		var m mevActivity
		if err := rows.Scan(&m.TxHash, &m.MEVType, &m.BlockNumber, &m.IngestedAt); err != nil {
			slog.Error("scan", "handler", "handleRiskMEV", "error", err)
			jsonError(w, http.StatusInternalServerError, "row scan failed")
			return
		}
		events = append(events, m)
	}
	if err := rows.Err(); err != nil {
		jsonError(w, http.StatusInternalServerError, "iteration failed")
		return
	}
	jsonOK(w, events)
}

// ──────────────────────────────────────────────────────────────────────────────
// 9. GET /v1/contracts/{address}/balance-sheet
// ──────────────────────────────────────────────────────────────────────────────

type tokenBalance struct {
	TokenAddress string  `json:"token_address"`
	Balance      float64 `json:"balance"`
}

func handleContractBalanceSheet(w http.ResponseWriter, r *http.Request) {
	addr := r.PathValue("address")
	if !validateAddress(w, addr) {
		return
	}

	cacheKey := "cache:contract:balancesheet:" + addr
	if client := getValkeyClient(); client != nil {
		cmd := client.B().Get().Key(cacheKey).Build()
		if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(res))
			return
		}
	}

	const query = `
		SELECT token_address, sum(balance_change) AS balance
		FROM worldchain.api_token_balances
		WHERE wallet_address = ?
		GROUP BY token_address
		HAVING balance > 0`

	ctx, cancel := queryCtx(r)
	defer cancel()

	rows, err := db.QueryContext(ctx, query, addr)
	if err != nil {
		slog.Error("query", "handler", "handleContractBalanceSheet", "error", err)
		jsonError(w, http.StatusInternalServerError, "database query failed")
		return
	}
	defer rows.Close()

	balances := make([]tokenBalance, 0, 32)
	for rows.Next() {
		var b tokenBalance
		if err := rows.Scan(&b.TokenAddress, &b.Balance); err != nil {
			slog.Error("scan", "handler", "handleContractBalanceSheet", "error", err)
			jsonError(w, http.StatusInternalServerError, "row scan failed")
			return
		}
		balances = append(balances, b)
	}
	if err := rows.Err(); err != nil {
		jsonError(w, http.StatusInternalServerError, "iteration failed")
		return
	}
	jsonOK(w, balances)
}

// ──────────────────────────────────────────────────────────────────────────────
// 10. GET /v1/risk/bridge-solvency
// ──────────────────────────────────────────────────────────────────────────────

type jsonRPCRequest struct {
	JSONRPC string        `json:"jsonrpc"`
	Method  string        `json:"method"`
	Params  []interface{} `json:"params"`
	ID      int           `json:"id"`
}

type jsonRPCResponse struct {
	JSONRPC string          `json:"jsonrpc"`
	Result  string          `json:"result"`
	Error   *jsonRPCError   `json:"error,omitempty"`
	ID      int             `json:"id"`
}

type jsonRPCError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

func callRPC(ctx context.Context, url, method string, params []interface{}) (string, error) {
	reqBody := jsonRPCRequest{
		JSONRPC: "2.0",
		Method:  method,
		Params:  params,
		ID:      1,
	}
	buf := new(bytes.Buffer)
	if err := json.NewEncoder(buf).Encode(reqBody); err != nil {
		return "", err
	}
	req, err := http.NewRequestWithContext(ctx, "POST", url, buf)
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("http status %d", resp.StatusCode)
	}

	var res jsonRPCResponse
	if err := json.NewDecoder(resp.Body).Decode(&res); err != nil {
		return "", err
	}
	if res.Error != nil {
		return "", fmt.Errorf("rpc error: %s", res.Error.Message)
	}
	return res.Result, nil
}

func getBalanceOf(ctx context.Context, rpcURL, token, owner string) (*big.Int, error) {
	ownerClean := strings.TrimPrefix(owner, "0x")
	data := "0x70a08231" + fmt.Sprintf("%064s", ownerClean)
	
	params := []interface{}{
		map[string]string{
			"to":   token,
			"data": data,
		},
		"latest",
	}
	res, err := callRPC(ctx, rpcURL, "eth_call", params)
	if err != nil {
		return nil, err
	}
	res = strings.TrimPrefix(res, "0x")
	val := new(big.Int)
	val.SetString(res, 16)
	return val, nil
}

func getTotalSupply(ctx context.Context, rpcURL, token string) (*big.Int, error) {
	params := []interface{}{
		map[string]string{
			"to":   token,
			"data": "0x18160ddd",
		},
		"latest",
	}
	res, err := callRPC(ctx, rpcURL, "eth_call", params)
	if err != nil {
		return nil, err
	}
	res = strings.TrimPrefix(res, "0x")
	val := new(big.Int)
	val.SetString(res, 16)
	return val, nil
}

func getNativeBalance(ctx context.Context, rpcURL, owner string) (*big.Int, error) {
	params := []interface{}{
		owner,
		"latest",
	}
	res, err := callRPC(ctx, rpcURL, "eth_getBalance", params)
	if err != nil {
		return nil, err
	}
	res = strings.TrimPrefix(res, "0x")
	val := new(big.Int)
	val.SetString(res, 16)
	return val, nil
}

func formatBigInt(val *big.Int, decimals int) string {
	f := new(big.Float).SetInt(val)
	div := new(big.Float).SetFloat64(math.Pow(10, float64(decimals)))
	f.Quo(f, div)
	return fmt.Sprintf("%.4f", f)
}

type tokenSolvency struct {
	TokenSymbol        string `json:"token_symbol"`
	L1TokenAddress     string `json:"l1_token_address"`
	L2TokenAddress     string `json:"l2_token_address"`
	L1BridgeLocked     string `json:"l1_bridge_locked"`
	L2TotalSupply      string `json:"l2_total_supply"`
	SolvencyRatio      string `json:"solvency_ratio"`
	Status             string `json:"status"`
}

type bridgeSolvencyResponse struct {
	Timestamp time.Time       `json:"timestamp"`
	Solvent   bool            `json:"solvent"`
	Tokens    []tokenSolvency `json:"tokens"`
}

func handleRiskBridgeSolvency(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	l1RPC := "http://127.0.0.1:8545"
	l2RPC := "https://worldchain-mainnet.g.alchemy.com/public" // public fallback
	
	// Check if local L2 node is synced, if so use it
	localL2RPC := "http://127.0.0.1:9745"
	if localBlock, err := callRPC(ctx, localL2RPC, "eth_blockNumber", nil); err == nil && localBlock != "0x0" {
		l2RPC = localL2RPC
	}

	bridgeL1 := "0x470458C91978D2d929704489Ad730DC3E3001113"
	portalL1 := "0xd5ec14a83b7d95be1e2ac12523e2dee12cbeea6c"

	tokensToCheck := []struct {
		Symbol   string
		L1Addr   string
		L2Addr   string
		Decimals int
	}{
		{"WLD", "0x163f8c2467924be0ae7b5347228cabf260318753", "0x2cfc85d8e48f8eab294be644d9e25c3030863003", 18},
		{"USDC", "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", "0x79a648f18e7e171a27618c0c1692ecd4e54387de", 6},
		{"USDT", "0xdac17f958d2ee523a2206206994597c13d831ec7", "0xce0000007b008f50d762d155002600004cd6c647", 6},
	}

	var results []tokenSolvency
	allSolvent := true

	// 1. Process ETH Solvency
	l1Eth, err := getNativeBalance(ctx, l1RPC, portalL1)
	if err != nil {
		l1Eth = big.NewInt(0)
	}
	l2Weth, err := getTotalSupply(ctx, l2RPC, "0x4200000000000000000000000000000000000006")
	if err != nil {
		l2Weth = big.NewInt(0)
	}

	ratioEth := 1.0
	if l2Weth.Cmp(big.NewInt(0)) > 0 {
		fL1 := new(big.Float).SetInt(l1Eth)
		fL2 := new(big.Float).SetInt(l2Weth)
		fRatio := new(big.Float).Quo(fL1, fL2)
		ratioEth, _ = fRatio.Float64()
	}

	statusEth := "SOLVENT"
	if l1Eth.Cmp(l2Weth) < 0 {
		statusEth = "INSOLVENT"
		allSolvent = false
	}

	results = append(results, tokenSolvency{
		TokenSymbol:    "ETH",
		L1TokenAddress: "native",
		L2TokenAddress: "0x4200000000000000000000000000000000000006",
		L1BridgeLocked: formatBigInt(l1Eth, 18),
		L2TotalSupply:  formatBigInt(l2Weth, 18),
		SolvencyRatio:  fmt.Sprintf("%.4f", ratioEth),
		Status:         statusEth,
	})

	// 2. Process ERC-20 Tokens
	for _, tk := range tokensToCheck {
		l1Locked, err := getBalanceOf(ctx, l1RPC, tk.L1Addr, bridgeL1)
		if err != nil {
			l1Locked = big.NewInt(0)
		}
		l2Supply, err := getTotalSupply(ctx, l2RPC, tk.L2Addr)
		if err != nil {
			l2Supply = big.NewInt(0)
		}

		ratio := 1.0
		if l2Supply.Cmp(big.NewInt(0)) > 0 {
			fL1 := new(big.Float).SetInt(l1Locked)
			fL2 := new(big.Float).SetInt(l2Supply)
			fRatio := new(big.Float).Quo(fL1, fL2)
			ratio, _ = fRatio.Float64()
		}

		status := "SOLVENT"
		if l1Locked.Cmp(l2Supply) < 0 {
			status = "INSOLVENT"
			allSolvent = false
		}

		results = append(results, tokenSolvency{
			TokenSymbol:    tk.Symbol,
			L1TokenAddress: tk.L1Addr,
			L2TokenAddress: tk.L2Addr,
			L1BridgeLocked: formatBigInt(l1Locked, tk.Decimals),
			L2TotalSupply:  formatBigInt(l2Supply, tk.Decimals),
			SolvencyRatio:  fmt.Sprintf("%.4f", ratio),
			Status:         status,
		})
	}

	jsonOK(w, bridgeSolvencyResponse{
		Timestamp: time.Now().UTC(),
		Solvent:   allSolvent,
		Tokens:    results,
	})
}

// ──────────────────────────────────────────────────────────────────────────────
// 11. GET /v1/contracts/{address}/gas
// ──────────────────────────────────────────────────────────────────────────────

type gasUsage struct {
	FunctionSelector string  `json:"function_selector"`
	CallCount        uint64  `json:"call_count"`
	AvgGasUsed       float64 `json:"avg_gas_used"`
	MinGasUsed       uint64  `json:"min_gas_used"`
	MaxGasUsed       uint64  `json:"max_gas_used"`
	SuccessCount     uint64  `json:"success_count"`
	RevertCount      uint64  `json:"revert_count"`
}

func handleContractGas(w http.ResponseWriter, r *http.Request) {
	addr := r.PathValue("address")
	if !validateAddress(w, addr) {
		return
	}

	cacheKey := "cache:contract:gas:" + addr
	if client := getValkeyClient(); client != nil {
		cmd := client.B().Get().Key(cacheKey).Build()
		if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(res))
			return
		}
	}

	const query = `
		SELECT
			function_selector, call_count, avg_gas_used,
			min_gas_used, max_gas_used, success_count, revert_count
		FROM worldchain.api_gas_by_function
		WHERE address = ?
		ORDER BY call_count DESC
		LIMIT ?`

	ctx, cancel := queryCtx(r)
	defer cancel()

	rows, err := db.QueryContext(ctx, query, addr, gasFuncTop)
	if err != nil {
		slog.Error("query", "handler", "handleContractGas", "error", err)
		jsonError(w, http.StatusInternalServerError, "database query failed")
		return
	}
	defer rows.Close()

	stats := make([]gasUsage, 0, gasFuncTop)
	for rows.Next() {
		var g gasUsage
		if err := rows.Scan(
			&g.FunctionSelector, &g.CallCount, &g.AvgGasUsed,
			&g.MinGasUsed, &g.MaxGasUsed, &g.SuccessCount, &g.RevertCount,
		); err != nil {
			slog.Error("scan", "handler", "handleContractGas", "error", err)
			jsonError(w, http.StatusInternalServerError, "row scan failed")
			return
		}
		stats = append(stats, g)
	}
	if err := rows.Err(); err != nil {
		jsonError(w, http.StatusInternalServerError, "iteration failed")
		return
	}
	jsonOK(w, stats)
}


// ──────────────────────────────────────────────────────────────────────────────
// 12. GET /v1/contracts/{address}/risk-simulation (BFS Contagion Path)
// ──────────────────────────────────────────────────────────────────────────────

type simulationNode struct {
	Address            string   `json:"address"`
	ContractName       string   `json:"contract_name"`
	IsVerified         bool     `json:"is_verified"`
	IsDecompiled       bool     `json:"is_decompiled"`
	IsProxy            bool     `json:"is_proxy"`
	IndividualRisk     float64  `json:"individual_risk"`
	PropagatedRisk     float64  `json:"propagated_risk"`
	Depth              int      `json:"depth"`
	Path               []string `json:"path"`
}

type riskSimulationResponse struct {
	SourceAddress      string           `json:"source_address"`
	Direction          string           `json:"direction"`
	MaxDepth           int              `json:"max_depth"`
	AggregateRiskScore float64          `json:"aggregate_risk_score"`
	Path               []simulationNode `json:"path"`
}

type contractRiskInfo struct {
	Score      float64
	Name       string
	Verified   bool
	Decompiled bool
	Proxy      bool
}

func getIndividualRisk(ctx context.Context, address string) contractRiskInfo {
	var verifiedVal uint8
	var decompiledVal uint8
	var proxyVal uint8
	var name string

	info := contractRiskInfo{
		Score:      0.1,
		Name:       "Unknown",
		Verified:   false,
		Decompiled: false,
		Proxy:      false,
	}

	query := `
		SELECT is_verified, is_decompiled, is_proxy, contract_name
		FROM worldchain.world_contracts
		FINAL
		WHERE address = ?
		LIMIT 1`

	err := db.QueryRowContext(ctx, query, address).Scan(&verifiedVal, &decompiledVal, &proxyVal, &name)
	if err != nil {
		return info
	}

	info.Verified = verifiedVal == 1
	info.Decompiled = decompiledVal == 1
	info.Proxy = proxyVal == 1
	info.Name = name

	score := 0.1
	if !info.Verified {
		score += 0.5
	}
	if info.Decompiled {
		score += 0.2
	}
	if info.Proxy {
		score += 0.15
	}
	if score > 0.95 {
		score = 0.95
	}
	info.Score = score
	return info
}

func handleContractRiskSimulation(w http.ResponseWriter, r *http.Request) {
	addr := r.PathValue("address")
	if !validateAddress(w, addr) {
		return
	}

	cacheKey := "cache:contract:risksim:" + addr
	if client := getValkeyClient(); client != nil {
		cmd := client.B().Get().Key(cacheKey).Build()
		if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(res))
			return
		}
	}

	depthParam := r.URL.Query().Get("depth")
	maxDepth := 3
	if depthParam != "" {
		if d, err := strconv.Atoi(depthParam); err == nil && d > 0 && d <= 5 {
			maxDepth = d
		}
	}

	direction := r.URL.Query().Get("direction")
	if direction != "dependents" {
		direction = "dependencies"
	}

	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	type queueItem struct {
		address string
		depth   int
		path    []string
	}

	queue := []queueItem{{address: addr, depth: 0, path: []string{addr}}}
	visited := map[string]bool{addr: true}
	var pathNodes []simulationNode

	sourceRisk := getIndividualRisk(ctx, addr)
	pathNodes = append(pathNodes, simulationNode{
		Address:        addr,
		ContractName:   sourceRisk.Name,
		IsVerified:     sourceRisk.Verified,
		IsDecompiled:   sourceRisk.Decompiled,
		IsProxy:        sourceRisk.Proxy,
		IndividualRisk: sourceRisk.Score,
		PropagatedRisk: sourceRisk.Score,
		Depth:          0,
		Path:           []string{addr},
	})

	for len(queue) > 0 {
		curr := queue[0]
		queue = queue[1:]

		if curr.depth >= maxDepth {
			continue
		}

		var query string
		if direction == "dependencies" {
			query = `
				SELECT to_address, sum(call_count) as calls
				FROM worldchain.world_contract_interactions
				WHERE from_address = ?
				GROUP BY to_address
				ORDER BY calls DESC
				LIMIT 5`
		} else {
			query = `
				SELECT from_address, sum(call_count) as calls
				FROM worldchain.world_contract_interactions
				WHERE to_address = ?
				GROUP BY from_address
				ORDER BY calls DESC
				LIMIT 5`
		}

		rows, err := db.QueryContext(ctx, query, curr.address)
		if err != nil {
			slog.Error("BFS query err", "handler", "sentinel", "error", err)
			continue
		}

		var children []string
		for rows.Next() {
			var child string
			var calls uint64
			if err := rows.Scan(&child, &calls); err == nil {
				children = append(children, child)
			}
		}
		rows.Close()

		for _, child := range children {
			if visited[child] {
				continue
			}
			visited[child] = true

			riskInfo := getIndividualRisk(ctx, child)
			decay := math.Pow(0.6, float64(curr.depth+1))
			propRisk := riskInfo.Score * decay

			newPath := append([]string{}, curr.path...)
			newPath = append(newPath, child)

			pathNodes = append(pathNodes, simulationNode{
				Address:        child,
				ContractName:   riskInfo.Name,
				IsVerified:     riskInfo.Verified,
				IsDecompiled:   riskInfo.Decompiled,
				IsProxy:        riskInfo.Proxy,
				IndividualRisk: riskInfo.Score,
				PropagatedRisk: propRisk,
				Depth:          curr.depth + 1,
				Path:           newPath,
			})

			queue = append(queue, queueItem{
				address: child,
				depth:   curr.depth + 1,
				path:    newPath,
			})
		}
	}

	aggRisk := sourceRisk.Score
	for _, node := range pathNodes {
		if node.PropagatedRisk > aggRisk {
			aggRisk = node.PropagatedRisk
		}
	}

	jsonOK(w, riskSimulationResponse{
		SourceAddress:      addr,
		Direction:          direction,
		MaxDepth:           maxDepth,
		AggregateRiskScore: math.Round(aggRisk*1000) / 1000,
		Path:               pathNodes,
	})
}

// handleContractCFGRaw serves the raw Graphviz .dot string for direct frontend rendering
func handleContractCFGRaw(w http.ResponseWriter, r *http.Request) {
addr := r.PathValue("address")
if !validateAddress(w, addr) {
return
}

cacheKey := "cache:cfg:raw:" + addr

if client := getValkeyClient(); client != nil {
cmd := client.B().Get().Key(cacheKey).Build()
if res, err := client.Do(r.Context(), cmd).ToString(); err == nil && res != "" {
cacheHits.Inc()
w.Header().Set("Content-Type", "text/vnd.graphviz")
w.Header().Set("Access-Control-Allow-Origin", "*")
w.Header().Set("X-Cache", "HIT")
w.WriteHeader(http.StatusOK)
_, _ = w.Write([]byte(res))
return
}
}

const query = `
SELECT cfg_graph
FROM worldchain.world_contracts
FINAL
WHERE address = ?
ORDER BY position(verification_source, 'Etherscan') DESC, ingested_at DESC
LIMIT 1`

ctx, cancel := queryCtx(r)
defer cancel()

var cfg string
err := db.QueryRowContext(ctx, query, addr).Scan(&cfg)
if err != nil {
jsonError(w, http.StatusNotFound, "cfg not found")
return
}

if client := getValkeyClient(); client != nil {
go func(k string, v string) {
cmd := client.B().Set().Key(k).Value(v).Ex(1 * time.Hour).Build()
if err := client.Do(context.Background(), cmd).Error(); err != nil {
log.Printf("API-SERVER warn: Valkey CFG cache set failed: %v", err)
}
}(cacheKey, cfg)
}

cacheMisses.Inc()
w.Header().Set("Content-Type", "text/vnd.graphviz")
w.Header().Set("Access-Control-Allow-Origin", "*")
w.Header().Set("X-Cache", "MISS")
w.WriteHeader(http.StatusOK)
_, _ = w.Write([]byte(cfg))
}


var (
apiValkeyClient rueidis.Client
apiValkeyOnce   sync.Once
)

func getValkeyClient() rueidis.Client {
apiValkeyOnce.Do(func() {
c, err := rueidis.NewClient(rueidis.ClientOption{
InitAddress: []string{"127.0.0.1:6379"},
})
if err != nil {
log.Printf("API-SERVER warn: Valkey client init failed: %v", err)
return
}
apiValkeyClient = c
})
return apiValkeyClient
}

func refreshDepegCache() {
	if db == nil {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	const query = `
		SELECT
			token_address, pool_address,
			spot_price, oracle_price, drift_ratio,
			block_number, updated_at
		FROM worldchain.api_depeg_drift
		WHERE abs(drift_ratio) > 0.01
		ORDER BY abs(drift_ratio) DESC
		LIMIT 50`

	rows, err := db.QueryContext(ctx, query)
	if err != nil {
		slog.Error("query error", "handler", "depegCacheWorker", "error", err)
		return
	}
	defer rows.Close()

	drifts := make([]depegDrift, 0, riskTop)
	for rows.Next() {
		var d depegDrift
		if err := rows.Scan(
			&d.TokenAddress, &d.PoolAddress,
			&d.SpotPrice, &d.OraclePrice, &d.DriftRatio,
			&d.BlockNumber, &d.UpdatedAt,
		); err != nil {
			slog.Error("scan error", "handler", "depegCacheWorker", "error", err)
			return
		}
		drifts = append(drifts, d)
	}

	payload, err := json.Marshal(drifts)
	if err != nil {
		slog.Error("marshal error", "handler", "depegCacheWorker", "error", err)
		return
	}

	depegCacheMu.Lock()
	depegCacheRaw = payload
	lastDepegSync = time.Now()
	depegCacheMu.Unlock()
}

func startDepegCacheWorker() {
	time.Sleep(2 * time.Second)
	refreshDepegCache()
		refreshHealthCache()

	ticker := time.NewTicker(3 * time.Second)
	for range ticker.C {
		refreshDepegCache()
		refreshHealthCache()
	}
}


type healthResponse struct {
Status       string    `json:"status"`
Timestamp    time.Time `json:"timestamp"`
ClickHouse   string    `json:"clickhouse"`
BlockLagSec  float64   `json:"block_lag_seconds"`
MaxBlock     uint64    `json:"max_block_number"`
CacheReady   bool      `json:"cache_ready"`
}

func refreshHealthCache() {
	if db == nil {
		healthCacheMu.Lock()
		cachedClickHouseOk = false
		healthCacheMu.Unlock()
		return
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	if err := db.PingContext(ctx); err != nil {
		healthCacheMu.Lock()
		cachedClickHouseOk = false
		healthCacheMu.Unlock()
		return
	}

	var maxBlock uint64
	var maxTime time.Time
	err := db.QueryRowContext(ctx, "SELECT max(block_number), max(timestamp) FROM worldchain.world_blocks").Scan(&maxBlock, &maxTime)

	healthCacheMu.Lock()
	defer healthCacheMu.Unlock()
	if err != nil {
		cachedClickHouseOk = false
	} else {
		cachedClickHouseOk = true
		cachedMaxBlock = maxBlock
		cachedMaxTime = maxTime
		lastHealthSync = time.Now()
	}
}

func handleHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")

	healthCacheMu.RLock()
	chOk := cachedClickHouseOk
	maxBlock := cachedMaxBlock
	maxTime := cachedMaxTime
	lastSync := lastHealthSync
	healthCacheMu.RUnlock()

	res := healthResponse{
		Status:    "OK",
		Timestamp: time.Now(),
	}

	if db == nil || !chOk || lastSync.IsZero() || time.Since(lastSync) > 15*time.Second {
		res.Status = "WARN"
		if db == nil {
			res.ClickHouse = "NOT_INITIALIZED"
		} else {
			res.ClickHouse = "UNHEALTHY"
		}
		w.WriteHeader(http.StatusServiceUnavailable)
		_ = json.NewEncoder(w).Encode(res)
		return
	}

	res.ClickHouse = "CONNECTED"
	res.MaxBlock = maxBlock
	res.BlockLagSec = time.Since(maxTime).Seconds()

	depegCacheMu.RLock()
	res.CacheReady = !lastDepegSync.IsZero() && time.Since(lastDepegSync) < 15*time.Second
	depegCacheMu.RUnlock()

	w.WriteHeader(http.StatusOK)
	_ = json.NewEncoder(w).Encode(res)
}
