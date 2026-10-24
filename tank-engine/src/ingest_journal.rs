//! C1 v2: the durable ingest journal of the live engine, and the per-block ownership of its writers.
//!
//! Directory `/var/lib/tank-engine/ingest-pending` (created by the deploy pack, owner supersonic, mode 0755; the
//! doctor check `tank_ingest_pending` alerts on any file there older than 5 minutes):
//!
//! * `<n>.block` — the block's source data behind a self-checking header (kind, block, the `ingested_at` every row
//!   gets, length, FNV-1a 64): for a streamed block the protobuf exactly as the ExEx sent it, for a gap block the
//!   same message rebuilt from the core rows the raw heal stored, so a resumed entry rebuilds byte-identical rows. A
//!   payload missing or corrupt under its entry is rebuilt the same way into that entry, which keeps every table's
//!   state (C1 v3, `Journal::restore_payload`); a corrupt file is moved aside as `<n>.block.corrupt[.<ms>]`;
//! * `<n>.json` — the entry: kind (`stream`, `derive`, or JSON-only `heal`), which tables the block has rows for,
//!   which are done, and every send's query_id and terminal evidence in its current token generation;
//! * `gap-<lo>-<hi>.json` — a gap claim: heights the stream skipped, kept until every chunk of at most 10,000 heights
//!   is acknowledged in `world_sequence_gaps` (the raw heal's queue);
//! * `*.next` — temporary files of an atomic replace (write, fsync, rename, fsync dir), removed at process start.
//!
//! Every insert of a block's rows into a table carries `insert_deduplication_token = tank:480:<n>:<hash>:<table>`,
//! the same for every writer of that table and block (streamer, raw heal, trace-gap healer, gap deriver), on tables
//! with `non_replicated_deduplication_window` set (deploy pack schema step; checked at start, fail closed), so a
//! send repeated under its token is dropped if an earlier one landed (out/DEDUP-PROOF.md). Log absence alone never
//! proves noncommit. Before any retry or generation advance, every recorded send must be unable to arrive or commit
//! later: a synchronous acknowledgement, a proven connection failure before submission, a terminal query_log record,
//! or persisted client-ended evidence followed by a later server restart (or host reboot) and flushed absence.
//! A live matching client is never ended by its age or by an empty log; legacy sends without a recorded client
//! identity cannot gain that proof from writer_pid. Missing records and query failures alone never prove completion.
//! Only after fencing can a global block-keyed count prove presence or absence; an unkeyed table requires commit
//! evidence unless every send was rejected before its pipeline started. A JSON-only Heal owner follows the same
//! protocol for core repairs and remains until all its recorded sends are fenced and its tables are done.
//! Truncated v3 histories cannot be fenced automatically.
//!
//! One writer process: `Journal::open` takes an exclusive flock on `/var/lib/tank-engine/writer.lock` (released by
//! the kernel when the process exits). Inside the process, `BlockLocks` gives one task at a time a block, from the
//! presence check to the end of its inserts (streamer, raw heal, trace-gap healer and gap deriver).

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::error;

pub const JOURNAL_DIR: &str = "/var/lib/tank-engine/ingest-pending";
pub const WRITER_LOCK: &str = "/var/lib/tank-engine/writer.lock";
/// Written by the deploy pack: the first block of the C1 v2 regime (see `Cutover`).
pub const CUTOVER_FILE: &str = "/var/lib/tank-engine/cutover.json";

pub const ENTRY_VERSION: u32 = 2;
const PAYLOAD_MAGIC: &[u8; 8] = b"TKC2BLK1";
const PAYLOAD_HEADER: usize = 48;

/// One row per finished block: the evidence that every table of the block is written (deploy verify, doctor).
pub const LEDGER_TABLE: &str = "tank_ingest_ledger";
/// The raw heal's queue takes gaps in rows of at most this many heights.
pub const MAX_GAP_CHUNK: u64 = 10_000;
/// Former v2 time threshold, retained only to test recovery across ages that v2 treated differently.
#[cfg(test)]
pub const RESEND_HORIZON_MS: u64 = 24 * 3_600_000;
/// non_replicated_deduplication_window every table the engine writes idempotently must have at least (schema check;
/// the pack sets 90,000-790,000 per table).
pub const MIN_DEDUP_WINDOW: u64 = 90_000;
/// Seconds a writer keeps an unfenced table owned and retried, sending nothing, before it parks the table
/// (`Journal::fence_wait_left`). TANK_FENCE_WAIT_SECS overrides it; 0 parks immediately. This wait is policy only,
/// never evidence that a request cannot still arrive.
pub const DEFAULT_FENCE_WAIT_SECS: u64 = 300;

/// Every table the engine writes a block's rows to with a deduplication token. `api_uniswap_mints` does not exist on
/// production (the matcher emits no rows for it); the schema check requires the window only on tables that exist,
/// except the ledger, which must exist.
pub const DEDUP_TABLES: [&str; 25] = [
    "world_transactions",
    "world_logs",
    "world_traces",
    "world_blocks",
    "world_token_transfers",
    "api_live_dex_swaps",
    "api_universal_defi_events",
    "api_dex_swaps",
    "api_gas_sponsorship",
    "api_humanity_feed",
    "api_nft_transfers",
    "api_oracle_prices",
    "world_erc1155_transfers",
    "api_internal_transfers",
    "api_uniswap_v3_prices",
    "api_whale_alerts",
    "world_contract_interactions",
    "api_gas_by_function",
    "api_lending_liquidations",
    "world_mev_hourly_stats",
    "api_token_balances",
    "latest_oracle_prices",
    "api_uniswap_mints",
    "api_morpho_activity",
    LEDGER_TABLE,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableClass {
    /// world_blocks/transactions/logs/traces: several writers; presence = `chain_id = 480 AND block_number = n`.
    Core,
    /// tank_ingest_ledger: one row per finished block.
    Ledger,
    /// Derived tables with a block_number column.
    Derived,
    /// Derived tables without block_number (summing, aggregating, latest value): a block's rows can't be counted.
    Unkeyed,
}

pub fn table_class(table: &str) -> TableClass {
    match table {
        "world_blocks" | "world_transactions" | "world_logs" | "world_traces" => TableClass::Core,
        LEDGER_TABLE => TableClass::Ledger,
        "world_contract_interactions"
        | "api_gas_by_function"
        | "world_mev_hourly_stats"
        | "api_token_balances"
        | "latest_oracle_prices" => TableClass::Unkeyed,
        _ => TableClass::Derived,
    }
}

/// The deduplication token of one table's rows of one block: the same for every writer, so ClickHouse keeps the
/// first insert and drops every later one within the table's window, whoever sends it.
pub fn dedup_token(table: &str, block_number: u64, block_hash: &str) -> String {
    format!(
        "tank:480:{block_number}:{}:{table}",
        block_hash.trim().to_ascii_lowercase()
    )
}

/// The token of a table's send in its generation. Generation 0 is the shared token; a later generation exists only
/// after the shared one was proved to be recorded without its part (a failure between ClickHouse's deduplication-log
/// write and its part commit; out/DEDUP-PROOF.md H1), or by the operator's `resolve … retry`.
pub fn generation_token(base: &str, generation: u32) -> String {
    if generation == 0 {
        base.to_string()
    } else {
        format!("{base}:g{generation}")
    }
}

/// One send's durable identity: a random process incarnation plus a process-local sequence prevent reuse across
/// restarts, even after clock rollback. Terminal evidence must match this exact ID, never merely its token.
pub fn query_id(block_number: u64, table: &str, at_ms: u64) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    static INCARNATION: LazyLock<u128> = LazyLock::new(rand::random::<u128>);
    let incarnation = *INCARNATION;
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("tank2-{block_number}-{table}-{at_ms}-{incarnation:032x}-{seq}")
}

/// When a send started, read back from its query ID (`query_id`; v3 IDs carry it in the same field). None for an ID
/// this engine did not format: such a send cannot be dated.
fn send_started_ms(query_id: &str) -> Option<u64> {
    query_id
        .strip_prefix("tank2-")?
        .split('-')
        .nth(2)?
        .parse()
        .ok()
}

/// Derived tables whose block_number is not a leading key column: their count is narrowed to one entry's rows by the
/// pinned ingested_at every row of the entry carries (seconds for the first four, milliseconds for the last two).
const OWN_ROWS_COUNTED: [&str; 6] = [
    "api_live_dex_swaps",
    "api_gas_sponsorship",
    "api_humanity_feed",
    "api_internal_transfers",
    "world_erc1155_transfers",
    "api_nft_transfers",
];

/// SQL counting the rows one block has in a table with a block_number column, or None (unkeyed tables). Core tables,
/// the ledger and most derived tables count every writer's rows (the raw heal's rule: a table holds all of a block's
/// rows or none); the OWN_ROWS_COUNTED tables count only this entry's rows (`count_proves_absence`).
pub fn count_rows_sql(table: &str, block: u64, block_ts: u64, now_ms: u64) -> Option<String> {
    let now_s = now_ms / 1000;
    let pred = match table_class(table) {
        TableClass::Unkeyed => return None,
        TableClass::Core => format!("chain_id = 480 AND block_number = {block}"),
        TableClass::Ledger => format!("block_number = {block}"),
        TableClass::Derived => match table {
            // block_number leads the sorting key (world_token_transfers: also partitioned by block time)
            "world_token_transfers" => {
                format!("block_number = {block} AND block_timestamp = toDateTime({block_ts})")
            }
            "api_dex_swaps" | "api_universal_defi_events" | "api_whale_alerts" => {
                format!("block_number = {block}")
            }
            // partitioned by block time, sorted by (x, block_timestamp)
            "api_oracle_prices" | "api_uniswap_v3_prices" => {
                format!("block_timestamp = toDateTime({block_ts}) AND block_number = {block}")
            }
            // OWN_ROWS_COUNTED: ingested_at in seconds, partitioned by it
            "api_live_dex_swaps"
            | "api_gas_sponsorship"
            | "api_humanity_feed"
            | "api_internal_transfers" => {
                format!("ingested_at = toDateTime({now_s}) AND block_number = {block}")
            }
            // OWN_ROWS_COUNTED: ingested_at in milliseconds
            "world_erc1155_transfers" | "api_nft_transfers" => {
                format!("ingested_at = fromUnixTimestamp64Milli(toInt64({now_ms})) AND block_number = {block}")
            }
            _ => format!("block_number = {block}"),
        },
    };
    Some(format!(
        "SELECT toUInt64(count()) AS n FROM worldchain.{table} WHERE {pred}"
    ))
}

/// True when a `count_rows_sql` count that finds no rows proves the block has none in the table. False for tables
/// without block_number and for OWN_ROWS_COUNTED, whose count sees only this entry's rows (its pinned ingested_at):
/// rows of the block written under another ingested_at stay unseen. C1 v3 keeps one entry per block, its payload
/// rebuilt into it when lost; the predicate stays conservative all the same. Rows found prove presence either way.
pub fn count_proves_absence(table: &str) -> bool {
    match table_class(table) {
        TableClass::Unkeyed => false,
        TableClass::Core | TableClass::Ledger => true,
        TableClass::Derived => !OWN_ROWS_COUNTED.contains(&table),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A block received from the ExEx stream: every table, core and derived.
    Stream,
    /// A block the raw heal filled from the archive node: its derived tables, rebuilt from its stored core rows.
    Derive,
    /// A JSON-only owner for RPC core repairs, without a payload or ledger row.
    Heal,
}

impl EntryKind {
    fn code(self) -> u64 {
        match self {
            EntryKind::Stream => 1,
            EntryKind::Derive => 2,
            EntryKind::Heal => 3,
        }
    }

    fn from_code(c: u64) -> Option<EntryKind> {
        match c {
            1 => Some(EntryKind::Stream),
            2 => Some(EntryKind::Derive),
            3 => Some(EntryKind::Heal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableStatus {
    Pending,
    Done,
    /// Kept for the operator, never sent again automatically.
    Parked,
}

/// The process that may still submit a recorded send. Client end alone is not server terminal evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientSend {
    pub pid: u32,
    pub boot_id: String,
    pub start_ticks: u64,
    pub started_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ms: Option<u64>,
    /// Rounded-up CLOCK_MONOTONIC_RAW observation, comparable to server uptime only on this send's host boot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_raw_ms: Option<u64>,
}

struct ClientIdentity {
    pid: u32,
    boot_id: String,
    start_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcessStat {
    state: u8,
    start_ticks: u64,
}

fn parse_process_stat(raw: &str, pid: u32) -> Result<ProcessStat, String> {
    let invalid = || format!("invalid /proc/{pid}/stat identity");
    // comm is parenthesized, but can itself contain spaces, '(' and ')'. Only the final boundary is structural.
    let (head, fields) = raw.rsplit_once(") ").ok_or_else(invalid)?;
    let (recorded_pid, _) = head.split_once(" (").ok_or_else(invalid)?;
    if pid == 0 || recorded_pid.parse::<u32>().map_err(|_| invalid())? != pid {
        return Err(invalid());
    }
    let mut fields = fields.split_ascii_whitespace();
    let state = fields.next().ok_or_else(invalid)?.as_bytes();
    let [state] = state else {
        return Err(invalid());
    };
    if !matches!(
        *state,
        b'R' | b'S' | b'D' | b'Z' | b'T' | b't' | b'X' | b'x' | b'K' | b'W' | b'P' | b'I'
    ) {
        return Err(invalid());
    }
    // State is field 3; after consuming it, field 22 (starttime) is the nineteenth remaining field.
    let start_ticks = fields
        .nth(18)
        .ok_or_else(invalid)?
        .parse::<u64>()
        .map_err(|_| invalid())?;
    if start_ticks == 0 {
        return Err(invalid());
    }
    Ok(ProcessStat {
        state: *state,
        start_ticks,
    })
}

fn read_process_stat(pid: u32) -> Result<Option<ProcessStat>, String> {
    let path = format!("/proc/{pid}/stat");
    match fs::read_to_string(&path) {
        Ok(raw) => parse_process_stat(&raw, pid).map(Some),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

fn current_client_identity() -> Result<&'static ClientIdentity, String> {
    static IDENTITY: LazyLock<Result<ClientIdentity, String>> = LazyLock::new(|| {
        let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map_err(|e| format!("/proc/sys/kernel/random/boot_id: {e}"))?;
        let boot_id = boot_id.trim();
        if boot_id.is_empty() {
            return Err("empty /proc/sys/kernel/random/boot_id".to_string());
        }
        let pid = std::process::id();
        let stat = read_process_stat(pid)?
            .ok_or_else(|| format!("current client /proc/{pid}/stat is missing"))?;
        Ok(ClientIdentity {
            pid,
            boot_id: boot_id.to_string(),
            start_ticks: stat.start_ticks,
        })
    });
    IDENTITY.as_ref().map_err(Clone::clone)
}

/// Local identity for the directly connected, same-host ClickHouse server. Never compare raw epochs across boots.
pub(crate) fn current_boot_id() -> Result<&'static str, String> {
    current_client_identity().map(|identity| identity.boot_id.as_str())
}

/// The pinned ClickHouse uptime clock on Linux. Floor before a server request; round client-end observations up.
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
pub(crate) fn monotonic_raw_ms() -> Result<u64, String> {
    use std::ffi::{c_int, c_long};

    #[repr(C)]
    struct Timespec {
        tv_sec: c_long,
        tv_nsec: c_long,
    }
    unsafe extern "C" {
        fn clock_gettime(clock_id: c_int, tp: *mut Timespec) -> c_int;
    }
    const CLOCK_MONOTONIC_RAW: c_int = 4;
    let mut timestamp = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: Linux 64-bit timespec has these two c_long fields; timestamp is writable for the entire call.
    if unsafe { clock_gettime(CLOCK_MONOTONIC_RAW, &mut timestamp) } != 0 {
        return Err(format!(
            "CLOCK_MONOTONIC_RAW: {}",
            io::Error::last_os_error()
        ));
    }
    if timestamp.tv_sec < 0 || !(0..1_000_000_000).contains(&timestamp.tv_nsec) {
        return Err("invalid CLOCK_MONOTONIC_RAW timestamp".to_string());
    }
    (timestamp.tv_sec as u64)
        .checked_mul(1000)
        .and_then(|ms| ms.checked_add(timestamp.tv_nsec as u64 / 1_000_000))
        .filter(|ms| *ms > 0)
        .ok_or_else(|| "CLOCK_MONOTONIC_RAW milliseconds out of range".to_string())
}

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
pub(crate) fn monotonic_raw_ms() -> Result<u64, String> {
    Err("CLOCK_MONOTONIC_RAW proof requires 64-bit Linux".to_string())
}

fn raw_end_ms(raw_ms: u64) -> Result<u64, String> {
    // A floored millisecond plus one is never earlier than the actual observation, even between clock ticks.
    raw_ms
        .checked_add(1)
        .filter(|_| raw_ms > 0)
        .ok_or_else(|| "CLOCK_MONOTONIC_RAW client end out of range".to_string())
}

fn client_process_ended(client: &ClientSend, boot_id: &str, stat: Option<ProcessStat>) -> bool {
    client.boot_id != boot_id
        || stat.is_none_or(|stat| {
            stat.start_ticks != client.start_ticks || matches!(stat.state, b'Z' | b'X' | b'x')
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableState {
    pub table: String,
    pub rows: u64,
    pub status: TableStatus,
    /// Sends started, recorded (fsynced) before each one starts.
    #[serde(default)]
    pub sends: u32,
    /// When the first send started (ms): the commit-evidence search starts the day before it. Its age decides nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_send_ms: Option<u64>,
    /// The token generation of the sends (generation_token).
    #[serde(default)]
    pub generation: u32,
    /// Every query_id sent in the current generation. Never truncate: an omitted send cannot be fenced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_ids: Vec<String>,
    /// Per-send process identity, durably recorded with the send, before the first request byte. Missing on legacy
    /// sends.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub clients: BTreeMap<String, ClientSend>,
    /// Known current-generation sends that cannot still commit later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal_query_ids: Vec<String>,
    /// Subset of terminal_query_ids proven not submitted or rejected before a pipeline started.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_query_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl TableState {
    pub fn sends_fenced(&self) -> bool {
        self.sends as usize == self.query_ids.len()
            && self
                .query_ids
                .iter()
                .all(|id| self.terminal_query_ids.contains(id))
    }

    pub fn all_rejected(&self) -> bool {
        self.sends > 0
            && self.sends_fenced()
            && self
                .query_ids
                .iter()
                .all(|id| self.rejected_query_ids.contains(id))
    }

    pub fn record_terminal(&mut self, id: &str, rejected: bool) {
        if !self.query_ids.iter().any(|known| known == id) {
            return;
        }
        if !self.terminal_query_ids.iter().any(|known| known == id) {
            self.terminal_query_ids.push(id.to_string());
        }
        if rejected && !self.rejected_query_ids.iter().any(|known| known == id) {
            self.rejected_query_ids.push(id.to_string());
        }
    }

    pub fn record_send(&mut self, id: &str, now_ms: u64) -> Result<(), String> {
        self.record_send_with_identity(id, now_ms, current_client_identity)
    }

    fn record_send_with_identity<'a>(
        &mut self,
        id: &str,
        now_ms: u64,
        identity: impl FnOnce() -> Result<&'a ClientIdentity, String>,
    ) -> Result<(), String> {
        if self.query_ids.iter().any(|known| known == id) {
            return Err(format!("table {}: duplicate query_id {id}", self.table));
        }
        let sends = self
            .sends
            .checked_add(1)
            .ok_or_else(|| format!("table {}: send count overflow", self.table))?;
        let identity = identity()?;
        let client = ClientSend {
            pid: identity.pid,
            boot_id: identity.boot_id.clone(),
            start_ticks: identity.start_ticks,
            started_ms: now_ms,
            ended_ms: None,
            ended_raw_ms: None,
        };
        self.sends = sends;
        self.first_send_ms.get_or_insert(now_ms);
        self.query_ids.push(id.to_string());
        self.clients.insert(id.to_string(), client);
        Ok(())
    }

    /// An insert call returned: this client can no longer submit that send, but the server may still commit it.
    pub fn record_client_end(&mut self, id: &str, now_ms: u64) -> Result<(), String> {
        self.record_client_end_with(id, now_ms, current_boot_id()?, monotonic_raw_ms)
    }

    fn record_client_end_with(
        &mut self,
        id: &str,
        now_ms: u64,
        boot_id: &str,
        read_raw_ms: impl FnOnce() -> Result<u64, String>,
    ) -> Result<(), String> {
        let Some(client) = self.clients.get(id) else {
            return Ok(()); // Legacy IDs without a client identity cannot acquire proof.
        };
        if client.ended_ms.is_some() && (client.ended_raw_ms.is_some() || client.boot_id != boot_id)
        {
            return Ok(());
        }
        let ended_raw_ms = raw_end_ms(read_raw_ms()?)?;
        let client = self.clients.get_mut(id).expect("client checked above");
        client.ended_ms.get_or_insert(client.started_ms.max(now_ms));
        if client.boot_id == boot_id {
            client.ended_raw_ms.get_or_insert(ended_raw_ms);
        }
        Ok(())
    }

    /// Observe process end, never infer it from age. A legacy wall-only end gets a fresh raw observation now.
    /// Persist changes before querying server-side fence evidence.
    pub fn observe_client_ends(&mut self, now_ms: u64) -> Result<bool, String> {
        if self.clients.is_empty() {
            return Ok(false);
        }
        self.observe_client_ends_with(
            now_ms,
            current_boot_id()?,
            read_process_stat,
            monotonic_raw_ms,
        )
    }

    fn observe_client_ends_with(
        &mut self,
        now_ms: u64,
        boot_id: &str,
        mut read_stat: impl FnMut(u32) -> Result<Option<ProcessStat>, String>,
        read_raw_ms: impl FnOnce() -> Result<u64, String>,
    ) -> Result<bool, String> {
        self.validate_clients()?;
        // Gather first: any failed identity or clock read must leave every client unchanged.
        let needs_observation = |client: &ClientSend| {
            client.ended_ms.is_none()
                || (client.boot_id == boot_id && client.ended_raw_ms.is_none())
        };
        let ended = self
            .clients
            .values()
            .filter(|client| needs_observation(client))
            .map(|client| {
                if client.ended_ms.is_some() || client.boot_id != boot_id {
                    return Ok(true); // A recorded return or a different boot needs no PID lookup.
                }
                read_stat(client.pid).map(|stat| client_process_ended(client, boot_id, stat))
            })
            .collect::<Result<Vec<_>, String>>()?;
        if !ended.iter().any(|ended| *ended) {
            return Ok(false);
        }
        let ended_raw_ms = raw_end_ms(read_raw_ms()?)?;
        for (client, ended) in self
            .clients
            .values_mut()
            .filter(|client| needs_observation(client))
            .zip(ended)
        {
            if ended {
                client.ended_ms.get_or_insert(client.started_ms.max(now_ms));
                if client.boot_id == boot_id {
                    client.ended_raw_ms.get_or_insert(ended_raw_ms);
                }
            }
        }
        Ok(true)
    }

    fn validate_clients(&self) -> Result<(), String> {
        for (id, client) in &self.clients {
            if !self.query_ids.contains(id)
                || client.pid == 0
                || client.start_ticks == 0
                || client.boot_id.trim().is_empty()
                || client.ended_raw_ms == Some(0)
                || (client.ended_raw_ms.is_some() && client.ended_ms.is_none())
                || client
                    .ended_ms
                    .is_some_and(|ended| ended < client.started_ms)
            {
                return Err(format!(
                    "table {}: invalid client proof for query_id {id}",
                    self.table
                ));
            }
        }
        Ok(())
    }

    pub fn next_generation(&mut self) -> Result<u32, String> {
        if !self.sends_fenced() {
            return Err(format!(
                "table {}: generation {} still has unfenced sends",
                self.table, self.generation
            ));
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| format!("table {}: generation overflow", self.table))?;
        self.generation = generation;
        self.sends = 0;
        self.first_send_ms = None;
        self.query_ids.clear();
        self.terminal_query_ids.clear();
        self.rejected_query_ids.clear();
        self.clients.clear();
        Ok(generation)
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub version: u32,
    pub kind: EntryKind,
    pub block_number: u64,
    /// Lowercase 0x-hex; part of every deduplication token of the block.
    pub block_hash: String,
    /// Block time, seconds.
    pub block_timestamp: u64,
    /// ingested_at of every row of this entry (ms); a resumed entry rebuilds identical rows from it.
    pub now_ms: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub writer_pid: u32,
    pub payload_bytes: u64,
    pub payload_fnv1a64: String,
    /// The entire intended RPC repair path succeeded, not merely the currently recorded table prefix.
    #[serde(default, skip_serializing_if = "is_false")]
    pub heal_complete: bool,
    /// In write order; tables the block has no rows for are not listed. The ledger row is last.
    pub tables: Vec<TableState>,
}

impl Entry {
    pub fn new(
        kind: EntryKind,
        block_number: u64,
        block_hash: &str,
        block_timestamp: u64,
        now_ms: u64,
        tables: &[(&str, u64)],
    ) -> Entry {
        Entry {
            version: ENTRY_VERSION,
            kind,
            block_number,
            block_hash: block_hash.trim().to_ascii_lowercase(),
            block_timestamp,
            now_ms,
            created_at_ms: wall_ms(),
            updated_at_ms: 0,
            writer_pid: std::process::id(),
            payload_bytes: 0,
            payload_fnv1a64: String::new(),
            heal_complete: false,
            tables: tables
                .iter()
                .map(|(table, rows)| TableState {
                    table: (*table).to_string(),
                    rows: *rows,
                    status: TableStatus::Pending,
                    sends: 0,
                    first_send_ms: None,
                    generation: 0,
                    query_ids: Vec::new(),
                    clients: BTreeMap::new(),
                    terminal_query_ids: Vec::new(),
                    rejected_query_ids: Vec::new(),
                    last_error: None,
                    note: None,
                })
                .collect(),
        }
    }

    pub fn is_done(&self) -> bool {
        self.tables.iter().all(|t| t.status == TableStatus::Done)
    }

    pub fn has_pending(&self) -> bool {
        self.tables.iter().any(|t| t.status == TableStatus::Pending)
    }

    pub fn parked(&self) -> Vec<&str> {
        self.tables
            .iter()
            .filter(|t| t.status == TableStatus::Parked)
            .map(|t| t.table.as_str())
            .collect()
    }

    /// Parks pending data tables, never the ledger: it must remain pending so operator reconciliation can finish it.
    pub fn park_pending(&mut self, why: &str) {
        for t in self
            .tables
            .iter_mut()
            .filter(|t| t.status == TableStatus::Pending && t.table != LEDGER_TABLE)
        {
            t.status = TableStatus::Parked;
            t.note = Some(why.to_string());
        }
    }

    fn validate_heal(&self) -> io::Result<()> {
        if self.kind == EntryKind::Heal
            && (self.payload_bytes != 0
                || !self.payload_fnv1a64.is_empty()
                || self
                    .tables
                    .iter()
                    .any(|t| table_class(&t.table) != TableClass::Core || t.rows == 0))
        {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "block {}: Heal owner must be JSON-only with nonempty core states",
                    self.block_number
                ),
            ));
        }
        Ok(())
    }

    fn validate_clients(&self) -> io::Result<()> {
        for table in &self.tables {
            table.validate_clients().map_err(|e| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!("block {}: {e}", self.block_number),
                )
            })?;
        }
        Ok(())
    }
}

/// A saved source block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    pub kind: EntryKind,
    pub now_ms: u64,
    pub bytes: Vec<u8>,
}

/// What a journal scan found for one block of the requested kind.
#[derive(Debug)]
pub enum Loaded {
    Entry(Entry),
    /// `<n>.block` without `<n>.json`: saved, but the entry was never committed, so no insert ran for it.
    Orphan {
        block_number: u64,
        payload: Payload,
    },
}

/// Heights the stream skipped, kept until the raw heal's queue holds every one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapClaim {
    pub version: u32,
    pub lo: u64,
    pub hi: u64,
    pub source: String,
    /// detected_at of every queue row of this gap (fixed, so a repeated enqueue never reopens a healed row:
    /// world_sequence_gaps keeps the row with the newest detected_at).
    pub detected_at_ms: u64,
    pub created_at_ms: u64,
    /// Chunks acknowledged by ClickHouse.
    #[serde(default)]
    pub enqueued: Vec<[u64; 2]>,
}

impl GapClaim {
    pub fn chunks(&self) -> Vec<(u64, u64)> {
        gap_chunks(self.lo, self.hi)
    }

    pub fn pending_chunks(&self) -> Vec<(u64, u64)> {
        self.chunks()
            .into_iter()
            .filter(|(lo, hi)| !self.enqueued.contains(&[*lo, *hi]))
            .collect()
    }

    fn file_name(&self) -> String {
        format!("gap-{}-{}.json", self.lo, self.hi)
    }
}

/// [lo, hi] in consecutive chunks of at most MAX_GAP_CHUNK heights, lowest first; nothing is left out.
pub fn gap_chunks(lo: u64, hi: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    if hi < lo {
        return out;
    }
    let mut a = lo;
    loop {
        let b = hi.min(a.saturating_add(MAX_GAP_CHUNK - 1));
        out.push((a, b));
        if b >= hi {
            return out;
        }
        a = b + 1;
    }
}

/// The C1 v2 regime starts at `floor`. Written once by the deploy pack (install) from the old engine's last fully
/// committed block (`complete_tip`, floor = complete_tip + 1), right after the schema step: `created_at_ms` is when the
/// deduplication windows were set, so no send older than it is held by them (a schema rollback deletes the
/// deduplication logs). The first stream after the install claims [floor, first streamed block - 1] as a gap, so the
/// raw heal completes those blocks and the gap deriver derives them; `claimed_at_ms` records that it happened. Below
/// `floor` nothing is derived automatically (the old engine and the repair packs own those blocks).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cutover {
    pub version: u32,
    pub floor: u64,
    pub complete_tip: u64,
    pub created_at_ms: u64,
    #[serde(default)]
    pub created_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_gap: Option<[u64; 2]>,
}

impl Cutover {
    pub fn load(path: &Path) -> io::Result<Cutover> {
        let raw = fs::read(path)?;
        let c: Cutover = serde_json::from_slice(&raw).map_err(|e| {
            io::Error::new(ErrorKind::InvalidData, format!("{}: {}", path.display(), e))
        })?;
        if c.version != 1 || c.floor == 0 || c.complete_tip + 1 != c.floor || c.created_at_ms == 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "{}: version {} floor {} complete_tip {} created_at_ms {}",
                    path.display(),
                    c.version,
                    c.floor,
                    c.complete_tip,
                    c.created_at_ms
                ),
            ));
        }
        Ok(c)
    }

    /// Atomic durable replace of the file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "cutover path has no dir"))?;
        let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            io::Error::new(ErrorKind::InvalidInput, "cutover path has no file name")
        })?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
        write_durable(dir, name, &bytes)
    }
}

pub struct Journal {
    dir: PathBuf,
    _lock: File,
    /// Gap claims are written by the streamer and the healer task: one at a time.
    claims_lock: Mutex<()>,
    /// How long an unfenced table may wait for terminal evidence before it is parked (`fence_wait_left`).
    fence_wait_ms: u64,
}

impl Journal {
    /// Opens the journal for its single writer process and clears what an interrupted process left: `*.next` files
    /// (never authoritative) are removed, a torn payload without an entry (no insert ran for it) is moved aside as
    /// `<n>.block.corrupt`. Fails when the directory is missing or another process holds the writer lock (a second
    /// engine, or `--ingest-journal resolve` while tank-live runs).
    pub fn open(dir: &Path, lock_path: &Path) -> io::Result<Journal> {
        if !fs::metadata(dir)?.is_dir() {
            return Err(io::Error::new(
                ErrorKind::NotFound,
                format!("{} is not a directory", dir.display()),
            ));
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o644)
            .open(lock_path)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    ErrorKind::WouldBlock,
                    format!(
                        "{} is held by another tank-engine process",
                        lock_path.display()
                    ),
                ))
            }
            Err(fs::TryLockError::Error(e)) => return Err(e),
        }
        let journal = Journal {
            dir: dir.to_path_buf(),
            _lock: lock,
            claims_lock: Mutex::new(()),
            fence_wait_ms: std::env::var("TANK_FENCE_WAIT_SECS")
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(DEFAULT_FENCE_WAIT_SECS)
                .saturating_mul(1000),
        };
        journal.clear_leftovers()?;
        Ok(journal)
    }

    pub fn fence_wait_ms(&self) -> u64 {
        self.fence_wait_ms
    }

    #[cfg(test)]
    pub fn with_fence_wait_ms(mut self, ms: u64) -> Journal {
        self.fence_wait_ms = ms;
        self
    }

    /// Milliseconds an unfenced table may still wait, measured from the start of its newest unresolved send.
    /// Waiting sends nothing, changes no generation and keeps the entry owned. None means park now: the wait is
    /// over, or the history cannot be dated or is incomplete (truncated legacy IDs can never be fenced).
    pub fn fence_wait_left(&self, t: &TableState, now_ms: u64) -> Option<u64> {
        if t.sends as usize != t.query_ids.len() {
            return None;
        }
        let mut newest: Option<u64> = None;
        for id in t
            .query_ids
            .iter()
            .filter(|id| !t.terminal_query_ids.contains(id))
        {
            let at = send_started_ms(id)?;
            newest = Some(newest.map_or(at, |n| n.max(at)));
        }
        let left = self
            .fence_wait_ms
            .saturating_sub(now_ms.saturating_sub(newest?));
        (left > 0).then_some(left)
    }

    fn clear_leftovers(&self) -> io::Result<()> {
        let mut changed = false;
        let mut payloads = Vec::new();
        for item in fs::read_dir(&self.dir)? {
            let item = item?;
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            if name.ends_with(".next") {
                remove_if_exists(&item.path())?;
                changed = true;
            } else if let Some(n) = parse_name(&name, ".block") {
                payloads.push(n);
            }
        }
        for n in payloads {
            if self.json_path(n).exists() {
                continue;
            }
            if let Err(e) = self.read_payload(n) {
                if e.kind() != ErrorKind::InvalidData {
                    return Err(e);
                }
                let aside = self.dir.join(format!("{n}.block.corrupt"));
                fs::rename(self.block_path(n), &aside)?;
                changed = true;
                error!(
                    "TANK-ENGINE INGEST_JOURNAL torn payload of uncommitted block {} moved to {} ({}); no insert ran for it",
                    n,
                    aside.display(),
                    e
                );
            }
        }
        if changed {
            sync_dir(&self.dir)?;
        }
        Ok(())
    }

    /// Saves the payload, then commits the entry. Inserts may start only after this returns Ok. A payload this call
    /// saved before failing to commit the entry (same kind, ingested_at and bytes) is reused, so a retry with the same
    /// received block completes it; any other existing file of the block is AlreadyExists.
    pub fn create(&self, entry: &mut Entry, payload: &[u8]) -> io::Result<()> {
        if entry.kind == EntryKind::Heal {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "Heal owners are JSON-only; use create_heal",
            ));
        }
        let n = entry.block_number;
        if self.json_path(n).exists() {
            return Err(io::Error::new(
                ErrorKind::AlreadyExists,
                format!("{} already exists", self.json_path(n).display()),
            ));
        }
        let sum = fnv1a64(payload);
        entry.payload_bytes = payload.len() as u64;
        entry.payload_fnv1a64 = format!("{sum:016x}");
        if self.block_path(n).exists() {
            match self.read_payload(n) {
                Ok(p) if p.kind == entry.kind && p.now_ms == entry.now_ms && p.bytes == payload => {
                    return self.save(entry);
                }
                _ => {
                    return Err(io::Error::new(
                        ErrorKind::AlreadyExists,
                        format!(
                            "{} already exists with another payload",
                            self.block_path(n).display()
                        ),
                    ))
                }
            }
        }
        write_durable(
            &self.dir,
            &format!("{n}.block"),
            &payload_file(entry.kind, n, entry.now_ms, payload, sum),
        )?;
        self.save(entry)
    }

    /// Claims an unowned block before any RPC repair insert, including its first generation-zero send.
    /// A stream/derive entry or orphan payload is never replaced. The caller holds the block lock.
    pub fn create_heal(
        &self,
        block_number: u64,
        block_hash: &str,
        block_timestamp: u64,
        now_ms: u64,
    ) -> io::Result<Entry> {
        if self.owns(block_number) {
            return Err(io::Error::new(
                ErrorKind::AlreadyExists,
                format!("block {block_number} already has a journal owner"),
            ));
        }
        let mut entry = Entry::new(
            EntryKind::Heal,
            block_number,
            block_hash,
            block_timestamp,
            now_ms,
            &[],
        );
        self.save(&mut entry)?;
        Ok(entry)
    }

    /// Persists a Heal owner's nonempty core manifest before its first send. Existing Stream/Derive
    /// manifests and all existing generations/history are immutable here.
    pub fn prepare_core(&self, entry: &mut Entry, table: &str, rows: u64) -> io::Result<usize> {
        if table_class(table) != TableClass::Core || rows == 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "block {}: invalid core manifest {table} rows={rows}",
                    entry.block_number
                ),
            ));
        }
        if !self.json_path(entry.block_number).exists() {
            return Err(io::Error::new(
                ErrorKind::NotFound,
                "core repair has no durable owner",
            ));
        }
        if let Some(i) = entry.tables.iter().position(|t| t.table == table) {
            if entry.tables[i].rows != rows {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "block {} table {table}: RPC rows={rows} differ from journal rows={}",
                        entry.block_number, entry.tables[i].rows
                    ),
                ));
            }
            return Ok(i);
        }
        if entry.kind != EntryKind::Heal {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "block {} table {table}: existing entry has no core state",
                    entry.block_number
                ),
            ));
        }
        let i = entry.tables.len();
        entry.tables.push(TableState {
            table: table.to_string(),
            rows,
            status: TableStatus::Pending,
            sends: 0,
            first_send_ms: None,
            generation: 0,
            query_ids: Vec::new(),
            clients: BTreeMap::new(),
            terminal_query_ids: Vec::new(),
            rejected_query_ids: Vec::new(),
            last_error: None,
            note: None,
        });
        self.save(entry)?;
        Ok(i)
    }

    /// Commits the entry of an orphan payload (`<n>.block` already on disk and verified).
    pub fn commit_orphan(&self, entry: &mut Entry, payload: &Payload) -> io::Result<()> {
        if entry.kind == EntryKind::Heal {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "Heal owners have no payload",
            ));
        }
        entry.payload_bytes = payload.bytes.len() as u64;
        entry.payload_fnv1a64 = format!("{:016x}", fnv1a64(&payload.bytes));
        self.save(entry)
    }

    /// Atomically replaces `<n>.json` (durable when this returns).
    pub fn save(&self, entry: &mut Entry) -> io::Result<()> {
        entry.validate_heal()?;
        entry.validate_clients()?;
        entry.updated_at_ms = wall_ms();
        let bytes = serde_json::to_vec_pretty(entry)
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
        write_durable(&self.dir, &format!("{}.json", entry.block_number), &bytes)
    }

    /// The saved payload; InvalidData when the file is torn or corrupt.
    pub fn read_payload(&self, block_number: u64) -> io::Result<Payload> {
        read_payload_file(&self.block_path(block_number), block_number)
    }

    /// The payload of `entry`, verified against it; None when it is missing or does not match.
    pub fn entry_payload(&self, entry: &Entry) -> io::Result<Option<Payload>> {
        if entry.kind == EntryKind::Heal {
            return Ok(None);
        }
        match self.read_payload(entry.block_number) {
            Ok(p)
                if p.kind == entry.kind
                    && p.now_ms == entry.now_ms
                    && format!("{:016x}", fnv1a64(&p.bytes)) == entry.payload_fnv1a64 =>
            {
                Ok(Some(p))
            }
            Ok(_) => Ok(None),
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::InvalidData) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Removes a finished entry: the payload first, so a stop in between leaves `<n>.json` with every table done
    /// (finished again on the next start), never a payload that looks unwritten.
    pub fn finish(&self, entry: &Entry) -> io::Result<()> {
        if !entry.is_done() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "block {} still has tables that are not done",
                    entry.block_number
                ),
            ));
        }
        if entry.tables.iter().any(|t| !t.sends_fenced()) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "block {} still has unfenced sends; owner retained",
                    entry.block_number
                ),
            ));
        }
        if entry.kind == EntryKind::Heal && !entry.heal_complete {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "block {}: Heal path completion is not durable; owner retained",
                    entry.block_number
                ),
            ));
        }
        self.remove_block_files(entry.block_number)
    }

    /// C1 v3: writes a payload rebuilt from the stored rows for a committed entry whose own is missing or does not
    /// match it, then records its length and checksum in the entry. Every table keeps its status, generation, send
    /// history and query_ids. A payload file still there is moved aside as `<n>.block.corrupt` (or
    /// `<n>.block.corrupt.<ms>` when that exists), never deleted.
    pub fn restore_payload(&self, entry: &mut Entry, payload: &[u8]) -> io::Result<()> {
        if entry.kind == EntryKind::Heal {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "Heal owners have no payload",
            ));
        }
        let n = entry.block_number;
        if !self.json_path(n).exists() {
            return Err(io::Error::new(
                ErrorKind::NotFound,
                format!(
                    "{} missing: no entry to restore",
                    self.json_path(n).display()
                ),
            ));
        }
        if self.block_path(n).exists() {
            let mut aside = self.dir.join(format!("{n}.block.corrupt"));
            if aside.exists() {
                aside = self.dir.join(format!("{n}.block.corrupt.{}", wall_ms()));
            }
            fs::rename(self.block_path(n), &aside)?;
            sync_dir(&self.dir)?;
        }
        let sum = fnv1a64(payload);
        write_durable(
            &self.dir,
            &format!("{n}.block"),
            &payload_file(entry.kind, n, entry.now_ms, payload, sum),
        )?;
        entry.payload_bytes = payload.len() as u64;
        entry.payload_fnv1a64 = format!("{sum:016x}");
        self.save(entry)
    }

    /// The committed entry of block `n` as it is on disk now; None when the block has none.
    pub fn entry(&self, n: u64) -> io::Result<Option<Entry>> {
        let entry = match read_entry(&self.json_path(n)) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if entry.block_number != n {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "{}: holds block {}",
                    self.json_path(n).display(),
                    entry.block_number
                ),
            ));
        }
        Ok(Some(entry))
    }

    /// True when block `n` has a committed entry whose payload is missing or does not match it. The entry keeps the
    /// block until its payload is rebuilt from the stored rows; meanwhile the raw heal may store the block's core rows
    /// under the shared tokens (raw_healer::heal_block).
    pub fn lost_payload(&self, n: u64) -> bool {
        match self.entry(n) {
            Ok(Some(entry)) if entry.kind != EntryKind::Heal => {
                matches!(self.entry_payload(&entry), Ok(None))
            }
            _ => false,
        }
    }

    /// The heights in [lo, hi] the journal holds a file of (an entry or a payload), lowest first.
    pub fn owned_between(&self, lo: u64, hi: u64) -> io::Result<Vec<u64>> {
        let mut out = Vec::new();
        for item in fs::read_dir(&self.dir)? {
            let item = item?;
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            if let Some(n) = parse_name(&name, ".json").or_else(|| parse_name(&name, ".block")) {
                if (lo..=hi).contains(&n) {
                    out.push(n);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    fn remove_block_files(&self, n: u64) -> io::Result<()> {
        remove_if_exists(&self.block_path(n))?;
        sync_dir(&self.dir)?;
        remove_if_exists(&self.json_path(n))?;
        sync_dir(&self.dir)
    }

    /// True when the journal holds any file of block `n` (an entry or a payload): the block is owned by its entry
    /// and no other writer may take it.
    pub fn owns(&self, n: u64) -> bool {
        self.json_path(n).exists() || self.block_path(n).exists()
    }

    /// Every unfinished block of `kind`, oldest first. Removes nothing. An unreadable `<n>.json` is an error: it is
    /// never removed or rewritten here (the operator's).
    pub fn load(&self, kind: EntryKind) -> io::Result<Vec<Loaded>> {
        let mut found: BTreeMap<u64, (bool, bool)> = BTreeMap::new();
        for item in fs::read_dir(&self.dir)? {
            let item = item?;
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            if let Some(n) = parse_name(&name, ".json") {
                found.entry(n).or_default().0 = true;
            } else if let Some(n) = parse_name(&name, ".block") {
                found.entry(n).or_default().1 = true;
            }
        }
        let mut out = Vec::new();
        for (n, (json, block)) in found {
            if json {
                let entry = read_entry(&self.json_path(n))?;
                if entry.block_number != n {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        format!(
                            "{}: holds block {}",
                            self.json_path(n).display(),
                            entry.block_number
                        ),
                    ));
                }
                if entry.kind == kind {
                    out.push(Loaded::Entry(entry));
                }
            } else if block {
                match self.read_payload(n) {
                    Ok(payload) if payload.kind == kind => out.push(Loaded::Orphan {
                        block_number: n,
                        payload,
                    }),
                    Ok(_) => {}
                    // being written right now by the other task, or torn (moved aside at the next start)
                    Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::InvalidData) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(out)
    }

    /// Saves a gap claim (durable when this returns). The same range claimed again keeps the first claim.
    pub fn claim_gap(
        &self,
        lo: u64,
        hi: u64,
        source: &str,
        detected_at_ms: u64,
    ) -> io::Result<GapClaim> {
        if hi < lo {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("gap [{lo}, {hi}] is empty"),
            ));
        }
        let claim = GapClaim {
            version: 1,
            lo,
            hi,
            source: source.to_string(),
            detected_at_ms,
            created_at_ms: wall_ms(),
            enqueued: Vec::new(),
        };
        let _claims = self.claims_lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.claim_path(&claim).exists() {
            return read_claim(&self.claim_path(&claim));
        }
        write_claim(&self.dir, &claim)?;
        Ok(claim)
    }

    pub fn save_claim(&self, claim: &GapClaim) -> io::Result<()> {
        let _claims = self.claims_lock.lock().unwrap_or_else(|e| e.into_inner());
        write_claim(&self.dir, claim)
    }

    /// Removes a claim once every chunk is acknowledged by the raw heal's queue.
    pub fn release_claim(&self, claim: &GapClaim) -> io::Result<()> {
        if !claim.pending_chunks().is_empty() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "gap [{}, {}] still has chunks not enqueued",
                    claim.lo, claim.hi
                ),
            ));
        }
        let _claims = self.claims_lock.lock().unwrap_or_else(|e| e.into_inner());
        remove_if_exists(&self.claim_path(claim))?;
        sync_dir(&self.dir)
    }

    /// Every saved gap claim, lowest first. An unreadable claim is an error (kept for the operator).
    pub fn claims(&self) -> io::Result<Vec<GapClaim>> {
        let _claims = self.claims_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for item in fs::read_dir(&self.dir)? {
            let item = item?;
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            if parse_claim_name(&name).is_some() {
                out.push(read_claim(&item.path())?);
            }
        }
        out.sort_by_key(|c| (c.lo, c.hi));
        Ok(out)
    }

    fn json_path(&self, n: u64) -> PathBuf {
        self.dir.join(format!("{n}.json"))
    }

    fn block_path(&self, n: u64) -> PathBuf {
        self.dir.join(format!("{n}.block"))
    }

    fn claim_path(&self, claim: &GapClaim) -> PathBuf {
        self.dir.join(claim.file_name())
    }
}

/// Read-only ownership observation used by journal tests, never an admission path for a writer.
#[cfg(test)]
pub fn journal_owns(dir: &Path, n: u64) -> bool {
    dir.join(format!("{n}.json")).exists() || dir.join(format!("{n}.block")).exists()
}

/// Per-block ownership inside the engine process: one task at a time holds a block, from its presence check to the
/// end of its inserts. Distinct from the process-wide writer flock. Released when the guard is dropped.
#[derive(Default)]
pub struct BlockLocks {
    held: Mutex<HashSet<u64>>,
    released: tokio::sync::Notify,
}

pub struct BlockGuard {
    locks: Arc<BlockLocks>,
    block: u64,
}

impl Drop for BlockGuard {
    fn drop(&mut self) {
        self.locks
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.block);
        self.locks.released.notify_waiters();
    }
}

impl BlockLocks {
    pub fn new() -> Arc<BlockLocks> {
        Arc::new(BlockLocks::default())
    }

    fn take(&self, block: u64) -> bool {
        self.held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(block)
    }

    /// The block, now or when its holder releases it.
    pub async fn lock(self: &Arc<Self>, block: u64) -> BlockGuard {
        loop {
            let released = self.released.notified();
            tokio::pin!(released);
            // registered before the check, so a release between the check and the await is not missed
            released.as_mut().enable();
            if self.take(block) {
                return BlockGuard {
                    locks: Arc::clone(self),
                    block,
                };
            }
            released.await;
        }
    }

    #[cfg(test)]
    fn try_lock(self: &Arc<Self>, block: u64) -> Option<BlockGuard> {
        self.take(block).then(|| BlockGuard {
            locks: Arc::clone(self),
            block,
        })
    }

    #[cfg(test)]
    fn is_held(&self, block: u64) -> bool {
        self.held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&block)
    }
}

/// What the engine's tasks share: the journal, the block ownership and the C1 v2 floor.
pub struct Ingest {
    pub journal: Journal,
    pub locks: Arc<BlockLocks>,
    pub cutover_path: PathBuf,
    /// First block of the C1 v2 regime: the gap deriver derives blocks at or above it only.
    pub floor: u64,
    /// When the deduplication windows were set (Cutover::created_at_ms): no send before it is held by them.
    pub windows_since_ms: u64,
    /// The next height this process's stream will write (0 until that stream computes its
    /// start). Every height below it is finished, owned by a journal entry, or claimed as a gap.
    /// Set when the stream subscribes, and advanced as blocks are handled, so a quiet tip still
    /// exposes heights the stream will not revisit.
    pub stream_expected: AtomicU64,
}

impl Ingest {
    pub fn new(journal: Journal, cutover_path: PathBuf, cutover: &Cutover) -> Ingest {
        Ingest {
            journal,
            locks: BlockLocks::new(),
            cutover_path,
            floor: cutover.floor,
            windows_since_ms: cutover.created_at_ms,
            stream_expected: AtomicU64::new(0),
        }
    }
}

/// Operator decision on a table that is not done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// The rows landed (or must not be written): never send them again.
    Done,
    /// The rows did not land: the engine sends them on its next start, under the next token generation (the old
    /// token may be recorded without its part, out/DEDUP-PROOF.md H1).
    Retry,
}

/// `tank-engine --ingest-journal resolve`: needs the writer lock, i.e. tank-live stopped ("under no writers"). `all`
/// means every table of the block's data: the ledger row stays pending, so the engine writes it once every table is
/// done (it is the evidence verify and the doctor read).
#[cfg(test)]
pub fn resolve(
    dir: &Path,
    lock_path: &Path,
    block: u64,
    table: &str,
    how: Resolution,
) -> io::Result<String> {
    let journal = Journal::open(dir, lock_path)?;
    resolve_locked(&journal, block, table, how)
}

/// Applies an operator decision while the caller retains the writer flock. The async CLI may refresh
/// and save terminal evidence under this same lock before calling it; assertions never fence a request.
pub fn resolve_locked(
    journal: &Journal,
    block: u64,
    table: &str,
    how: Resolution,
) -> io::Result<String> {
    let path = journal.json_path(block);
    let mut entry = read_entry(&path)?;
    let stamp = format!(
        "operator {} at {}",
        if how == Resolution::Done {
            "done"
        } else {
            "retry"
        },
        wall_ms()
    );
    let mut changed = Vec::new();
    for t in entry.tables.iter_mut() {
        if if table == "all" {
            t.table == LEDGER_TABLE
        } else {
            t.table != table
        } {
            continue;
        }
        if !t.sends_fenced() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "block {block} table {}: unresolved sends; terminal evidence required before {:?}",
                    t.table, how
                ),
            ));
        }
        match (t.status, how) {
            (TableStatus::Done, _) => {
                if table != "all" {
                    return Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        format!(
                            "block {block} table {} is already done; nothing to resolve",
                            t.table
                        ),
                    ));
                }
            }
            (_, Resolution::Done) => {
                t.status = TableStatus::Done;
                t.note = Some(stamp.clone());
                changed.push(t.table.clone());
            }
            (_, Resolution::Retry) => {
                // An operator's absence assertion does not stop an older request on the server.
                // Only persisted terminal evidence permits discarding this generation's send history.
                t.next_generation()
                    .map_err(|e| io::Error::new(ErrorKind::InvalidInput, e))?;
                t.status = TableStatus::Pending;
                t.note = Some(stamp.clone());
                changed.push(t.table.clone());
            }
        }
    }
    if changed.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("block {block} has no table '{table}' that is not done"),
        ));
    }
    journal.save(&mut entry)?;
    Ok(format!(
        "block {block}: {} -> {:?}\n",
        changed.join(","),
        how
    ))
}

/// `tank-engine --ingest-journal status`: read-only, safe while tank-live runs.
pub fn status_report(dir: &Path, cutover: &Path) -> io::Result<String> {
    let now = wall_ms();
    let mut names: Vec<String> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    let mut out = format!(
        "ingest journal {} ({} file(s))\n",
        dir.display(),
        names.len()
    );
    match Cutover::load(cutover) {
        Ok(c) => out.push_str(&format!(
            "  cutover floor={} complete_tip={} claimed={}\n",
            c.floor,
            c.complete_tip,
            c.claimed_gap
                .map_or("no".to_string(), |g| format!("[{}, {}]", g[0], g[1]))
        )),
        Err(e) => out.push_str(&format!("  cutover {}: {}\n", cutover.display(), e)),
    }
    for name in &names {
        let path = dir.join(name);
        let age_s = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| now.saturating_sub(d.as_millis() as u64) / 1000);
        out.push_str(&format!(
            "  {name}  age={}s\n",
            age_s.map_or("?".to_string(), |a| a.to_string())
        ));
        if parse_claim_name(name).is_some() {
            match read_claim(&path) {
                Ok(c) => out.push_str(&format!(
                    "    gap [{}, {}] source={} chunks={} enqueued={}\n",
                    c.lo,
                    c.hi,
                    c.source,
                    c.chunks().len(),
                    c.enqueued.len()
                )),
                Err(e) => out.push_str(&format!("    UNREADABLE: {e}\n")),
            }
            continue;
        }
        if parse_name(name, ".json").is_none() {
            continue;
        }
        match read_entry(&path) {
            Ok(entry) => {
                out.push_str(&format!(
                    "    {:?} block={} hash={} writer_pid={} created_ms={} updated_ms={}\n",
                    entry.kind,
                    entry.block_number,
                    entry.block_hash,
                    entry.writer_pid,
                    entry.created_at_ms,
                    entry.updated_at_ms
                ));
                for t in &entry.tables {
                    out.push_str(&format!(
                        "    {:<28} rows={:<6} {:?}",
                        t.table, t.rows, t.status
                    ));
                    if t.sends > 0 {
                        out.push_str(&format!(" sends={}", t.sends));
                    }
                    if t.generation > 0 {
                        out.push_str(&format!(" generation={}", t.generation));
                    }
                    if let Some(first) = t.first_send_ms {
                        out.push_str(&format!(
                            " first_send_age={}s",
                            now.saturating_sub(first) / 1000
                        ));
                    }
                    if let Some(err) = &t.last_error {
                        out.push_str(&format!(" last_error={err}"));
                    }
                    if let Some(note) = &t.note {
                        out.push_str(&format!(" note={note}"));
                    }
                    out.push('\n');
                }
            }
            Err(e) => out.push_str(&format!("    UNREADABLE: {e}\n")),
        }
    }
    Ok(out)
}

pub fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// FNV-1a 64: torn-write check of a payload file, not a security hash.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn read_entry(path: &Path) -> io::Result<Entry> {
    let raw = fs::read(path)?;
    let entry: Entry = serde_json::from_slice(&raw).map_err(|e| {
        io::Error::new(ErrorKind::InvalidData, format!("{}: {}", path.display(), e))
    })?;
    if entry.version != ENTRY_VERSION {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "{}: entry version {} (this engine reads {})",
                path.display(),
                entry.version,
                ENTRY_VERSION
            ),
        ));
    }
    entry.validate_heal()?;
    entry.validate_clients()?;
    Ok(entry)
}

fn write_claim(dir: &Path, claim: &GapClaim) -> io::Result<()> {
    let bytes =
        serde_json::to_vec_pretty(claim).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
    write_durable(dir, &claim.file_name(), &bytes)
}

fn read_claim(path: &Path) -> io::Result<GapClaim> {
    let raw = fs::read(path)?;
    let claim: GapClaim = serde_json::from_slice(&raw).map_err(|e| {
        io::Error::new(ErrorKind::InvalidData, format!("{}: {}", path.display(), e))
    })?;
    if claim.version != 1 || claim.hi < claim.lo {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("{}: bad gap claim", path.display()),
        ));
    }
    Ok(claim)
}

/// A payload file: the self-checking header (magic, kind, block, ingested_at, length, FNV-1a 64), then the bytes.
fn payload_file(kind: EntryKind, n: u64, now_ms: u64, payload: &[u8], sum: u64) -> Vec<u8> {
    let mut file = Vec::with_capacity(PAYLOAD_HEADER + payload.len());
    file.extend_from_slice(PAYLOAD_MAGIC);
    file.extend_from_slice(&kind.code().to_le_bytes());
    file.extend_from_slice(&n.to_le_bytes());
    file.extend_from_slice(&now_ms.to_le_bytes());
    file.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    file.extend_from_slice(&sum.to_le_bytes());
    file.extend_from_slice(payload);
    file
}

fn read_payload_file(path: &Path, block_number: u64) -> io::Result<Payload> {
    let mut raw = Vec::new();
    File::open(path)?.read_to_end(&mut raw)?;
    let bad =
        |why: &str| io::Error::new(ErrorKind::InvalidData, format!("{}: {why}", path.display()));
    if raw.len() < PAYLOAD_HEADER || &raw[..8] != PAYLOAD_MAGIC {
        return Err(bad("no payload header"));
    }
    let word = |i: usize| u64::from_le_bytes(raw[i..i + 8].try_into().expect("8 bytes"));
    let kind = EntryKind::from_code(word(8)).ok_or_else(|| bad("unknown payload kind"))?;
    if word(16) != block_number {
        return Err(bad("payload of another block"));
    }
    let now_ms = word(24);
    let len = word(32);
    let sum = word(40);
    let bytes = raw.split_off(PAYLOAD_HEADER);
    if bytes.len() as u64 != len {
        return Err(bad("payload length differs from its header (torn write)"));
    }
    if fnv1a64(&bytes) != sum {
        return Err(bad("payload checksum mismatch"));
    }
    Ok(Payload {
        kind,
        now_ms,
        bytes,
    })
}

fn parse_name(name: &str, suffix: &str) -> Option<u64> {
    let stem = name.strip_suffix(suffix)?;
    if stem.is_empty() || !stem.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

fn parse_claim_name(name: &str) -> Option<(u64, u64)> {
    let stem = name.strip_prefix("gap-")?.strip_suffix(".json")?;
    let (lo, hi) = stem.split_once('-')?;
    if lo.is_empty() || hi.is_empty() || !lo.bytes().chain(hi.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((lo.parse().ok()?, hi.parse().ok()?))
}

fn write_durable(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let next = dir.join(format!("{name}.next"));
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o644)
            .open(&next)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&next, dir.join(name))?;
    sync_dir(dir)
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// A fresh journal directory under the test's temp dir; removed when dropped.
    struct TempJournal {
        root: PathBuf,
    }

    impl TempJournal {
        fn new() -> TempJournal {
            let root = std::env::temp_dir().join(format!(
                "tank-c1v2-journal-{}-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::SeqCst),
                wall_ms()
            ));
            fs::create_dir_all(root.join("ingest-pending")).unwrap();
            TempJournal { root }
        }
        fn dir(&self) -> PathBuf {
            self.root.join("ingest-pending")
        }
        fn lock(&self) -> PathBuf {
            self.root.join("writer.lock")
        }
        fn open(&self) -> Journal {
            Journal::open(&self.dir(), &self.lock()).unwrap()
        }
        fn files(&self) -> Vec<String> {
            let mut v: Vec<String> = fs::read_dir(self.dir())
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            v.sort();
            v
        }
    }

    impl Drop for TempJournal {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn sample(kind: EntryKind, n: u64) -> Entry {
        Entry::new(
            kind,
            n,
            "0xABC",
            1_760_000_000,
            1_760_000_000_123,
            &[
                ("world_transactions", 3),
                ("world_blocks", 1),
                ("world_contract_interactions", 2),
                (LEDGER_TABLE, 1),
            ],
        )
    }

    fn entries(loaded: Vec<Loaded>) -> Vec<Entry> {
        loaded
            .into_iter()
            .map(|l| match l {
                Loaded::Entry(e) => e,
                other => panic!("expected an entry, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn fnv1a64_matches_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn token_is_shared_by_every_writer_of_a_table_and_block() {
        let t = dedup_token("world_traces", 36_000_001, " 0xAbC ");
        assert_eq!(t, "tank:480:36000001:0xabc:world_traces");
        assert_eq!(
            t,
            dedup_token("world_traces", 36_000_001, "0xabc"),
            "case and spaces of the hash do not matter"
        );
        assert_ne!(t, dedup_token("world_logs", 36_000_001, "0xabc"));
        assert_ne!(t, dedup_token("world_traces", 36_000_002, "0xabc"));
        assert_ne!(
            t,
            dedup_token("world_traces", 36_000_001, "0xabd"),
            "a reorged block is another block"
        );
        assert_eq!(
            generation_token(&t, 0),
            t,
            "generation 0 is the token every writer shares"
        );
        assert_eq!(
            generation_token(&t, 2),
            "tank:480:36000001:0xabc:world_traces:g2"
        );
        assert_ne!(generation_token(&t, 1), generation_token(&t, 2));
    }

    #[test]
    fn created_entry_survives_a_restart_with_its_payload_and_per_table_progress() {
        let t = TempJournal::new();
        {
            let j = t.open();
            let mut e = sample(EntryKind::Stream, 36_000_001);
            j.create(&mut e, b"payload-bytes").unwrap();
            e.tables[0].status = TableStatus::Done;
            e.tables[1].sends = 2;
            e.tables[1].first_send_ms = Some(5);
            j.save(&mut e).unwrap();
        } // the writer exits: lock released, nothing removed
        let j = t.open();
        let got = entries(j.load(EntryKind::Stream).unwrap());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].block_hash, "0xabc");
        assert_eq!(got[0].tables[0].status, TableStatus::Done);
        assert_eq!(
            (got[0].tables[1].sends, got[0].tables[1].first_send_ms),
            (2, Some(5))
        );
        assert_eq!(got[0].now_ms, 1_760_000_000_123);
        let p = j.entry_payload(&got[0]).unwrap().unwrap();
        assert_eq!(
            (p.kind, p.now_ms, p.bytes.as_slice()),
            (EntryKind::Stream, 1_760_000_000_123, &b"payload-bytes"[..])
        );
        assert!(
            entries(j.load(EntryKind::Derive).unwrap()).is_empty(),
            "kinds are kept apart"
        );
        assert_eq!(t.files(), vec!["36000001.block", "36000001.json"]);
        assert!(j.owns(36_000_001) && !j.owns(36_000_002));
        assert!(journal_owns(&t.dir(), 36_000_001));
    }

    #[test]
    fn second_writer_is_refused_until_the_first_exits() {
        let t = TempJournal::new();
        let first = t.open();
        let err = Journal::open(&t.dir(), &t.lock())
            .err()
            .expect("second writer must be refused");
        assert_eq!(err.kind(), ErrorKind::WouldBlock);
        assert!(
            resolve(&t.dir(), &t.lock(), 1, "all", Resolution::Done).is_err(),
            "resolve needs no writers"
        );
        drop(first);
        assert!(Journal::open(&t.dir(), &t.lock()).is_ok());
    }

    #[test]
    fn missing_directory_is_refused() {
        let t = TempJournal::new();
        let err = Journal::open(&t.root.join("absent"), &t.lock())
            .err()
            .expect("missing dir");
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn creating_an_existing_block_is_refused() {
        let t = TempJournal::new();
        let j = t.open();
        j.create(&mut sample(EntryKind::Stream, 7), b"x").unwrap();
        let err = j
            .create(&mut sample(EntryKind::Derive, 7), b"y")
            .err()
            .expect("duplicate block");
        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        assert_eq!(
            j.read_payload(7).unwrap().bytes,
            b"x".to_vec(),
            "the first payload is kept"
        );
    }

    /// Review finding 5: a save interrupted after the payload (entry commit failed) is completed by a retry with the
    /// same received block; a different payload for the block is refused.
    #[test]
    fn create_retry_with_the_same_block_completes_an_interrupted_save() {
        let t = TempJournal::new();
        let j = t.open();
        j.create(&mut sample(EntryKind::Stream, 8), b"block-8")
            .unwrap();
        fs::remove_file(t.dir().join("8.json")).unwrap(); // the entry commit failed after the payload
        let mut again = sample(EntryKind::Stream, 8);
        j.create(&mut again, b"block-8").unwrap();
        assert_eq!(t.files(), vec!["8.block", "8.json"]);
        assert_eq!(entries(j.load(EntryKind::Stream).unwrap()).len(), 1);
        fs::remove_file(t.dir().join("8.json")).unwrap();
        let mut other = sample(EntryKind::Stream, 8);
        other.now_ms += 1;
        assert_eq!(
            j.create(&mut other, b"block-8").err().unwrap().kind(),
            ErrorKind::AlreadyExists
        );
        assert_eq!(
            j.create(&mut sample(EntryKind::Stream, 8), b"other")
                .err()
                .unwrap()
                .kind(),
            ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn payload_without_committed_entry_is_returned_as_an_orphan_of_its_kind() {
        let t = TempJournal::new();
        let j = t.open();
        j.create(&mut sample(EntryKind::Derive, 42), b"orphan-payload")
            .unwrap();
        fs::remove_file(t.dir().join("42.json")).unwrap(); // stop between the payload and the entry commit
        assert!(j.load(EntryKind::Stream).unwrap().is_empty());
        match j.load(EntryKind::Derive).unwrap().as_slice() {
            [Loaded::Orphan {
                block_number,
                payload,
            }] => {
                assert_eq!(
                    (*block_number, payload.now_ms, payload.kind),
                    (42, 1_760_000_000_123, EntryKind::Derive)
                );
                assert_eq!(payload.bytes.as_slice(), b"orphan-payload");
            }
            other => panic!("expected one orphan, got {other:?}"),
        }
    }

    #[test]
    fn torn_orphan_payload_is_moved_aside_at_open_not_deleted() {
        let t = TempJournal::new();
        {
            let j = t.open();
            j.create(&mut sample(EntryKind::Stream, 43), b"0123456789")
                .unwrap();
        }
        fs::remove_file(t.dir().join("43.json")).unwrap();
        let path = t.dir().join("43.block");
        let raw = fs::read(&path).unwrap();
        fs::write(&path, &raw[..raw.len() - 3]).unwrap(); // torn write
        let j = t.open();
        assert!(j.load(EntryKind::Stream).unwrap().is_empty());
        assert_eq!(t.files(), vec!["43.block.corrupt"]);
    }

    #[test]
    fn corrupt_payload_of_a_committed_entry_is_reported_and_kept() {
        let t = TempJournal::new();
        let j = t.open();
        j.create(&mut sample(EntryKind::Stream, 44), b"0123456789")
            .unwrap();
        let path = t.dir().join("44.block");
        let mut raw = fs::read(&path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        fs::write(&path, &raw).unwrap();
        let got = entries(j.load(EntryKind::Stream).unwrap());
        assert_eq!(got.len(), 1);
        assert_eq!(
            j.read_payload(44).err().unwrap().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(j.entry_payload(&got[0]).unwrap(), None);
        assert_eq!(t.files(), vec!["44.block", "44.json"]);
    }

    #[test]
    fn leftover_temp_files_are_removed_at_open_and_old_claims_are_kept() {
        let t = TempJournal::new();
        {
            let j = t.open();
            j.create(&mut sample(EntryKind::Stream, 5), b"p").unwrap();
            j.claim_gap(1, 3, "streamer-midstream", 9).unwrap();
        }
        fs::write(t.dir().join("5.json.next"), b"{half").unwrap();
        fs::write(t.dir().join("6.block.next"), b"half").unwrap();
        fs::write(t.dir().join("gap-1-3.json.next"), b"half").unwrap();
        let week_ago = SystemTime::now() - Duration::from_secs(7 * 86_400);
        for name in ["5.json", "5.block", "gap-1-3.json"] {
            File::options()
                .write(true)
                .open(t.dir().join(name))
                .unwrap()
                .set_modified(week_ago)
                .unwrap();
        }
        let j = t.open();
        assert_eq!(
            entries(j.load(EntryKind::Stream).unwrap()).len(),
            1,
            "age never removes an entry"
        );
        assert_eq!(j.claims().unwrap().len(), 1, "age never removes a claim");
        assert_eq!(t.files(), vec!["5.block", "5.json", "gap-1-3.json"]);
        // a scan while the process runs removes nothing (the other task may be writing its .next file)
        fs::write(t.dir().join("8.json.next"), b"{half").unwrap();
        j.load(EntryKind::Stream).unwrap();
        j.claims().unwrap();
        assert!(t.dir().join("8.json.next").exists());
    }

    #[test]
    fn unreadable_entry_or_older_version_is_an_error_and_stays_on_disk() {
        let t = TempJournal::new();
        let j = t.open();
        fs::write(t.dir().join("9.json"), b"not json").unwrap();
        assert_eq!(
            j.load(EntryKind::Stream).err().unwrap().kind(),
            ErrorKind::InvalidData
        );
        let mut v1 = serde_json::to_value(sample(EntryKind::Stream, 9)).unwrap();
        v1["version"] = 1.into();
        fs::write(t.dir().join("9.json"), serde_json::to_vec(&v1).unwrap()).unwrap();
        assert_eq!(
            j.load(EntryKind::Derive).err().unwrap().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(t.files(), vec!["9.json"]);
    }

    #[test]
    fn finish_needs_every_table_done_and_removes_payload_first() {
        let t = TempJournal::new();
        let j = t.open();
        let mut e = sample(EntryKind::Stream, 11);
        j.create(&mut e, b"p").unwrap();
        assert_eq!(j.finish(&e).err().unwrap().kind(), ErrorKind::InvalidInput);
        assert_eq!(t.files(), vec!["11.block", "11.json"]);
        for tb in e.tables.iter_mut() {
            tb.status = TableStatus::Done;
        }
        j.save(&mut e).unwrap();
        // stop after the payload was removed: the entry (every table done) is still loaded, its payload is gone
        fs::remove_file(t.dir().join("11.block")).unwrap();
        let again = entries(j.load(EntryKind::Stream).unwrap());
        assert!(again[0].is_done());
        assert_eq!(
            j.read_payload(11).err().unwrap().kind(),
            ErrorKind::NotFound
        );
        j.finish(&again[0]).unwrap();
        assert!(t.files().is_empty());
    }

    #[test]
    fn rebuilt_payload_is_restored_into_its_entry_and_a_corrupt_one_kept_aside() {
        let t = TempJournal::new();
        let j = t.open();
        let mut e = sample(EntryKind::Stream, 20);
        j.create(&mut e, b"original").unwrap();
        e.tables[0].status = TableStatus::Done;
        e.tables[0].generation = 1;
        e.tables[2].status = TableStatus::Parked;
        j.save(&mut e).unwrap();
        assert!(!j.lost_payload(20));
        fs::write(t.dir().join("20.block"), b"garbage").unwrap();
        assert!(j.lost_payload(20));
        let mut got = j.entry(20).unwrap().unwrap();
        j.restore_payload(&mut got, b"rebuilt").unwrap();
        assert!(!j.lost_payload(20));
        assert_eq!(j.entry_payload(&got).unwrap().unwrap().bytes, b"rebuilt");
        assert_eq!(
            j.entry(20).unwrap().unwrap().tables,
            e.tables,
            "every table keeps its state"
        );
        assert_eq!(
            fs::read(t.dir().join("20.block.corrupt")).unwrap(),
            b"garbage"
        );
        // missing this time: restored without an aside file; a second corrupt one gets its own name
        fs::remove_file(t.dir().join("20.block")).unwrap();
        assert!(j.lost_payload(20));
        let mut got = j.entry(20).unwrap().unwrap();
        j.restore_payload(&mut got, b"rebuilt").unwrap();
        fs::write(t.dir().join("20.block"), b"garbage-2").unwrap();
        let mut got = j.entry(20).unwrap().unwrap();
        j.restore_payload(&mut got, b"rebuilt").unwrap();
        let files = t.files();
        assert_eq!(files.len(), 4, "{files:?}");
        assert!(
            files.iter().any(|f| f.starts_with("20.block.corrupt.")),
            "{files:?}"
        );
        assert_eq!(j.entry(21).unwrap(), None);
        assert_eq!(j.owned_between(0, 100).unwrap(), vec![20]);
        assert!(j.owned_between(21, 100).unwrap().is_empty());
    }

    #[test]
    fn gap_chunks_cover_every_height_in_bounded_chunks() {
        assert_eq!(gap_chunks(5, 4), vec![]);
        assert_eq!(gap_chunks(7, 7), vec![(7, 7)]);
        assert_eq!(gap_chunks(1, 10_000), vec![(1, 10_000)]);
        // the review's long outage: 12,600 heights, none left out (the old cap dropped the oldest 2,600)
        let c = gap_chunks(36_000_101, 36_012_700);
        assert_eq!(c, vec![(36_000_101, 36_010_100), (36_010_101, 36_012_700)]);
        let total: u64 = c.iter().map(|(a, b)| b - a + 1).sum();
        assert_eq!(total, 12_600);
        let big = gap_chunks(1, 25_001);
        assert_eq!(big.len(), 3);
        assert!(big.iter().all(|(a, b)| b - a + 1 <= MAX_GAP_CHUNK));
        assert!(big.windows(2).all(|w| w[0].1 + 1 == w[1].0));
        assert_eq!(
            gap_chunks(u64::MAX - 1, u64::MAX),
            vec![(u64::MAX - 1, u64::MAX)]
        );
    }

    #[test]
    fn gap_claim_is_durable_until_every_chunk_is_acknowledged() {
        let t = TempJournal::new();
        let j = t.open();
        let mut c = j
            .claim_gap(100, 20_150, "streamer-reconnect", 1_700)
            .unwrap();
        assert_eq!(
            c.pending_chunks(),
            vec![(100, 10_099), (10_100, 20_099), (20_100, 20_150)]
        );
        assert_eq!(
            j.release_claim(&c).err().unwrap().kind(),
            ErrorKind::InvalidInput
        );
        c.enqueued.push([100, 10_099]);
        j.save_claim(&c).unwrap();
        drop(j);
        let j = t.open();
        let mut again = j.claims().unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(
            again[0].pending_chunks(),
            vec![(10_100, 20_099), (20_100, 20_150)]
        );
        assert_eq!(
            j.claim_gap(100, 20_150, "other", 9).unwrap().detected_at_ms,
            1_700,
            "the first claim is kept"
        );
        again[0].enqueued.push([10_100, 20_099]);
        again[0].enqueued.push([20_100, 20_150]);
        j.release_claim(&again[0]).unwrap();
        assert!(j.claims().unwrap().is_empty());
        assert!(j.claim_gap(5, 4, "x", 1).is_err());
    }

    #[test]
    fn operator_resolution_changes_only_tables_that_are_not_done() {
        let t = TempJournal::new();
        {
            let j = t.open();
            let mut e = sample(EntryKind::Stream, 13);
            j.create(&mut e, b"p").unwrap();
            e.tables[0].status = TableStatus::Done;
            e.tables[2].status = TableStatus::Parked;
            e.tables[2].sends = 3;
            e.tables[2].first_send_ms = Some(1);
            e.tables[2].query_ids = vec!["q1".into(), "q2".into(), "q3".into()];
            j.save(&mut e).unwrap();
        }
        let err = resolve(
            &t.dir(),
            &t.lock(),
            13,
            "world_transactions",
            Resolution::Retry,
        )
        .err()
        .unwrap();
        assert_eq!(
            err.kind(),
            ErrorKind::InvalidInput,
            "a done table is never reopened"
        );
        for how in [Resolution::Done, Resolution::Retry] {
            assert!(
                resolve(&t.dir(), &t.lock(), 13, "world_contract_interactions", how,).is_err(),
                "operator assertions cannot fence unknown requests"
            );
        }
        {
            let j = t.open();
            let mut e = j.entry(13).unwrap().unwrap();
            assert_eq!(e.tables[2].status, TableStatus::Parked);
            assert_eq!(e.tables[2].sends, 3, "refused decisions preserve history");
            for id in ["q1", "q2", "q3"] {
                e.tables[2].record_terminal(id, false);
            }
            j.save(&mut e).unwrap();
        }
        resolve(
            &t.dir(),
            &t.lock(),
            13,
            "world_contract_interactions",
            Resolution::Retry,
        )
        .unwrap();
        let e = entries(t.open().load(EntryKind::Stream).unwrap()).remove(0);
        assert_eq!(e.tables[2].status, TableStatus::Pending);
        assert_eq!(
            (e.tables[2].sends, e.tables[2].first_send_ms),
            (0, None),
            "retry starts the horizon again"
        );
        assert_eq!(
            e.tables[2].generation, 1,
            "retry sends under the next token generation"
        );
        assert!(
            e.tables[2].query_ids.is_empty(),
            "the earlier generation's sends are not evidence for the new one"
        );
        assert!(e.tables[2].terminal_query_ids.is_empty());
        assert!(e.tables[2].rejected_query_ids.is_empty());
        assert!(e.tables[2].clients.is_empty());
        resolve(&t.dir(), &t.lock(), 13, "all", Resolution::Done).unwrap();
        let e = entries(t.open().load(EntryKind::Stream).unwrap()).remove(0);
        let left: Vec<&str> = e
            .tables
            .iter()
            .filter(|t| t.status != TableStatus::Done)
            .map(|t| t.table.as_str())
            .collect();
        assert_eq!(
            left,
            vec![LEDGER_TABLE],
            "`all` leaves the ledger row to the engine (written once every table is done)"
        );
        assert!(
            resolve(&t.dir(), &t.lock(), 13, "all", Resolution::Done).is_err(),
            "nothing left to resolve"
        );
        let report = status_report(&t.dir(), &t.root.join("cutover.json")).unwrap();
        assert!(
            report.contains("block=13") && report.contains("note=operator done"),
            "{report}"
        );
    }

    fn process_stat_fixture(pid: u32, comm: &str, state: u8, start_ticks: u64) -> String {
        format!(
            "{pid} ({comm}) {}{} {start_ticks}\n",
            state as char,
            " 0".repeat(18)
        )
    }

    #[test]
    fn process_stat_uses_the_final_comm_boundary_and_rejects_malformed_identity() {
        for comm in [
            "tank-engine",
            "a process with spaces",
            "a (nested) process)",
            "tricky ) R 0 (comm) suffix",
        ] {
            let raw = process_stat_fixture(17, comm, b'S', 77);
            assert_eq!(
                parse_process_stat(&raw, 17).unwrap(),
                ProcessStat {
                    state: b'S',
                    start_ticks: 77
                },
                "{comm}"
            );
            assert!(
                parse_process_stat(&raw, 18).is_err(),
                "PID must match the path"
            );
        }
        for raw in [
            "17 no-parentheses S 1".to_string(),
            "17 (comm) S 1".to_string(),
            process_stat_fixture(17, "comm", b'?', 77),
            process_stat_fixture(17, "comm", b'S', 0),
            process_stat_fixture(0, "comm", b'S', 77),
            format!("17 (comm) S{} not-a-starttime", " 0".repeat(18)),
        ] {
            assert!(parse_process_stat(&raw, 17).is_err(), "{raw}");
        }
    }

    #[test]
    fn send_persists_its_exact_client_identity_and_live_clients_never_end_by_age() {
        let tmp = TempJournal::new();
        let j = tmp.open();
        let mut entry = sample(EntryKind::Stream, 1);
        j.create(&mut entry, b"payload").unwrap();
        let id = query_id(1, &entry.tables[0].table, 10);
        entry.tables[0].record_send(&id, 10).unwrap();
        let identity = current_client_identity().unwrap();
        let expected = ClientSend {
            pid: std::process::id(),
            boot_id: identity.boot_id.clone(),
            start_ticks: identity.start_ticks,
            started_ms: 10,
            ended_ms: None,
            ended_raw_ms: None,
        };
        assert!(expected.pid > 0 && expected.start_ticks > 0 && !expected.boot_id.is_empty());
        assert_eq!(entry.tables[0].query_ids, [id.clone()]);
        assert_eq!(entry.tables[0].clients.get(&id), Some(&expected));
        j.save(&mut entry).unwrap();
        assert_eq!(
            j.entry(1).unwrap().unwrap(),
            entry,
            "client identity is durable with the send record"
        );
        let table = &mut entry.tables[0];
        let before = table.clone();
        assert!(!table.observe_client_ends(u64::MAX).unwrap());
        assert_eq!(
            *table, before,
            "a matching live process is not proof of end"
        );
        table.record_client_end("foreign", 50).unwrap();
        assert_eq!(*table, before);
        table.record_client_end(&id, 5).unwrap();
        assert_eq!(
            table.clients[&id].ended_ms,
            Some(10),
            "clock rollback cannot predate the send"
        );
        assert!(table.clients[&id].ended_raw_ms.is_some_and(|end| end > 0));
        let ended = table.clone();
        table.record_client_end(&id, 1_000).unwrap();
        assert!(!table.observe_client_ends(2_000).unwrap());
        assert_eq!(*table, ended, "the first end timestamp is stable");
        assert!(
            !table.sends_fenced(),
            "client end alone is never terminal evidence"
        );
        assert!(!table.all_rejected());
        j.save(&mut entry).unwrap();
        assert_eq!(j.entry(1).unwrap().unwrap(), entry);
    }

    #[test]
    fn identity_lookup_failure_does_not_mutate_send_history() {
        for already_sent in [false, true] {
            let mut table = sample(EntryKind::Stream, 1).tables.remove(0);
            if already_sent {
                table.record_send("first", 10).unwrap();
            }
            for why in [
                "boot ID unavailable",
                "process stat unreadable",
                "process stat malformed",
            ] {
                let before = table.clone();
                assert_eq!(
                    table.record_send_with_identity("next", 20, || Err(why.to_string())),
                    Err(why.to_string())
                );
                assert_eq!(table, before, "{why}");
            }
        }
    }

    #[test]
    fn dead_reused_and_rebooted_clients_end_at_observation_time_once() {
        for (boot_id, stat) in [
            ("boot-b", None),
            ("boot-a", None),
            (
                "boot-a",
                Some(ProcessStat {
                    state: b'S',
                    start_ticks: 78,
                }),
            ),
            (
                "boot-a",
                Some(ProcessStat {
                    state: b'Z',
                    start_ticks: 77,
                }),
            ),
            (
                "boot-a",
                Some(ProcessStat {
                    state: b'X',
                    start_ticks: 77,
                }),
            ),
            (
                "boot-a",
                Some(ProcessStat {
                    state: b'x',
                    start_ticks: 77,
                }),
            ),
        ] {
            let mut table = sample(EntryKind::Stream, 1).tables.remove(0);
            table.record_send("client", 10).unwrap();
            let client = table.clients.get_mut("client").unwrap();
            client.pid = 17;
            client.boot_id = "boot-a".to_string();
            client.start_ticks = 77;
            assert!(table
                .observe_client_ends_with(
                    1_000,
                    boot_id,
                    |pid| {
                        assert_eq!(pid, 17);
                        assert_eq!(boot_id, "boot-a", "a reboot needs no PID lookup");
                        Ok(stat)
                    },
                    || Ok(5_000),
                )
                .unwrap());
            assert_eq!(table.clients["client"].ended_ms, Some(1_000));
            assert_eq!(
                table.clients["client"].ended_raw_ms,
                (boot_id == "boot-a").then_some(5_001),
                "a new boot must not manufacture an old-boot raw timestamp"
            );
            let ended = table.clone();
            assert!(!table
                .observe_client_ends_with(
                    2_000,
                    "boot-c",
                    |_| panic!("already-ended clients must not be observed again"),
                    || panic!("a different boot needs no raw end comparison"),
                )
                .unwrap());
            assert_eq!(table, ended);
            assert!(!table.sends_fenced());
            assert!(!table.all_rejected());
        }
        for state in b"RSDTtKWPI" {
            let mut table = sample(EntryKind::Stream, 1).tables.remove(0);
            table.record_send("client", 10).unwrap();
            let client = table.clients["client"].clone();
            let before = table.clone();
            assert!(!table
                .observe_client_ends_with(
                    u64::MAX,
                    &client.boot_id,
                    |pid| {
                        assert_eq!(pid, client.pid);
                        Ok(Some(ProcessStat {
                            state: *state,
                            start_ticks: client.start_ticks,
                        }))
                    },
                    || panic!("a live identity cannot acquire a raw end"),
                )
                .unwrap());
            assert_eq!(table, before, "state {} is not process end", *state as char);
        }
    }

    #[test]
    fn observation_errors_never_infer_end_or_partially_mutate_clients() {
        let mut table = sample(EntryKind::Stream, 1).tables.remove(0);
        table.record_send("a-rebooted", 10).unwrap();
        table.record_send("b-unreadable", 20).unwrap();
        table.clients.get_mut("a-rebooted").unwrap().boot_id = "old-boot".to_string();
        let boot_id = table.clients["b-unreadable"].boot_id.clone();
        let before = table.clone();
        for why in ["permission denied", "I/O error", "invalid process stat"] {
            assert_eq!(
                table.observe_client_ends_with(
                    1_000,
                    &boot_id,
                    |_| Err(why.to_string()),
                    || panic!("identity error must precede raw observation"),
                ),
                Err(why.to_string())
            );
            assert_eq!(
                table, before,
                "an earlier observable end must not leak past {why}"
            );
        }
        assert!(table
            .observe_client_ends_with(
                1_000,
                &boot_id,
                |pid| parse_process_stat("malformed stat", pid).map(Some),
                || panic!("malformed identity must precede raw observation"),
            )
            .is_err());
        assert_eq!(table, before);
    }

    #[test]
    fn optional_client_fields_default_without_inventing_end_proof_and_old_caps_are_ignored() {
        let mut entry = sample(EntryKind::Stream, 1);
        entry.tables[0].record_send("client", 10).unwrap();
        let mut json = serde_json::to_value(&entry).unwrap();
        assert!(json["tables"][0]["clients"]["client"]
            .get("ended_ms")
            .is_none());
        assert!(json["tables"][0]["clients"]["client"]
            .get("ended_raw_ms")
            .is_none());
        json["tables"][0]["clients"]["client"]["receive_timeout_ms"] = serde_json::json!(0);
        json["tables"][0]["fence_until_ms"] = serde_json::json!(u64::MAX);
        let restored: Entry = serde_json::from_value(json).unwrap();
        let client = &restored.tables[0].clients["client"];
        assert_eq!(client.ended_ms, None);
        assert_eq!(client.ended_raw_ms, None);
        restored.validate_clients().unwrap();
        let json = serde_json::to_value(restored).unwrap();
        assert!(json["tables"][0]["clients"]["client"]
            .get("receive_timeout_ms")
            .is_none());
        assert!(json["tables"][0].get("fence_until_ms").is_none());
    }

    #[test]
    fn legacy_wall_end_is_reobserved_on_raw_clock_without_wall_time_translation() {
        for wall_ms in [0, u64::MAX] {
            let mut table = sample(EntryKind::Stream, 1).tables.remove(0);
            table.record_send("legacy", 10).unwrap();
            table.clients.get_mut("legacy").unwrap().ended_ms = Some(20);
            let boot_id = table.clients["legacy"].boot_id.clone();
            assert!(table
                .observe_client_ends_with(
                    wall_ms,
                    &boot_id,
                    |_| panic!("a recorded client return needs no new process lookup"),
                    || Ok(50_000),
                )
                .unwrap());
            assert_eq!(table.clients["legacy"].ended_ms, Some(20));
            assert_eq!(table.clients["legacy"].ended_raw_ms, Some(50_001));
            let observed = table.clone();
            assert!(!table
                .observe_client_ends_with(
                    wall_ms,
                    &boot_id,
                    |_| panic!("an observed end is stable"),
                    || panic!("an observed raw end is stable"),
                )
                .unwrap());
            assert_eq!(table, observed);
            assert!(!table.sends_fenced());
            assert!(!table.all_rejected());
        }
    }

    #[test]
    fn raw_clock_failures_leave_direct_and_multi_client_end_observations_atomic() {
        let mut table = sample(EntryKind::Stream, 1).tables.remove(0);
        table.record_send("a-rebooted", 10).unwrap();
        table.record_send("b-ended", 20).unwrap();
        table.record_send("c-legacy", 30).unwrap();
        table.clients.get_mut("a-rebooted").unwrap().boot_id = "old-boot".to_string();
        table.clients.get_mut("c-legacy").unwrap().ended_ms = Some(40);
        let boot_id = table.clients["b-ended"].boot_id.clone();
        let before = table.clone();
        for raw in [
            Err("CLOCK_MONOTONIC_RAW unavailable".to_string()),
            Ok(0),
            Ok(u64::MAX),
        ] {
            assert!(table
                .record_client_end_with("b-ended", 1_000, &boot_id, || raw.clone())
                .is_err());
            assert_eq!(
                table, before,
                "a clock failure must not record a wall-only end"
            );
            assert!(table
                .observe_client_ends_with(1_000, &boot_id, |_| Ok(None), || raw.clone())
                .is_err());
            assert_eq!(
                table, before,
                "no earlier client observation may leak past a clock failure"
            );
        }
        assert!(table
            .observe_client_ends_with(1_000, &boot_id, |_| Ok(None), || Ok(5_000))
            .unwrap());
        assert_eq!(table.clients["a-rebooted"].ended_ms, Some(1_000));
        assert_eq!(table.clients["a-rebooted"].ended_raw_ms, None);
        assert_eq!(table.clients["b-ended"].ended_raw_ms, Some(5_001));
        assert_eq!(table.clients["c-legacy"].ended_ms, Some(40));
        assert_eq!(table.clients["c-legacy"].ended_raw_ms, Some(5_001));
        assert!(!table.sends_fenced());
        assert!(!table.all_rejected());
    }

    #[test]
    fn malformed_client_proof_is_rejected_on_save_and_load_but_partial_legacy_maps_are_valid() {
        let tmp = TempJournal::new();
        let j = tmp.open();
        let mut entry = sample(EntryKind::Stream, 1);
        j.create(&mut entry, b"payload").unwrap();
        entry.tables[0].record_send("client", 10).unwrap();
        entry.tables[0].record_client_end("client", 20).unwrap();
        entry.tables[0].record_send("legacy", 30).unwrap();
        entry.tables[0].clients.remove("legacy");
        j.save(&mut entry).unwrap();
        assert_eq!(
            j.entry(1).unwrap().unwrap(),
            entry,
            "client maps may cover only a subset of sends"
        );
        let valid = serde_json::to_value(&entry).unwrap();
        let mut invalid_cases = Vec::new();
        for (field, value) in [
            ("pid", serde_json::json!(0)),
            ("start_ticks", serde_json::json!(0)),
            ("boot_id", serde_json::json!("")),
            ("boot_id", serde_json::json!(" \n ")),
            ("ended_raw_ms", serde_json::json!(0)),
            ("ended_ms", serde_json::Value::Null),
            ("ended_ms", serde_json::json!(9)),
        ] {
            let mut invalid = valid.clone();
            invalid["tables"][0]["clients"]["client"][field] = value;
            invalid_cases.push(invalid);
        }
        let mut orphan = valid.clone();
        let clients = orphan["tables"][0]["clients"].as_object_mut().unwrap();
        let client = clients.remove("client").unwrap();
        clients.insert("foreign".to_string(), client);
        invalid_cases.push(orphan);
        for invalid in invalid_cases {
            let mut decoded: Entry = serde_json::from_value(invalid.clone()).unwrap();
            let before = decoded.clone();
            assert_eq!(
                j.save(&mut decoded).unwrap_err().kind(),
                ErrorKind::InvalidData
            );
            assert_eq!(decoded, before);
            fs::write(
                tmp.dir().join("1.json"),
                serde_json::to_vec(&invalid).unwrap(),
            )
            .unwrap();
            assert_eq!(j.entry(1).unwrap_err().kind(), ErrorKind::InvalidData);
            assert!(
                tmp.dir().join("1.json").exists(),
                "invalid proof retains ownership"
            );
        }
        fs::write(
            tmp.dir().join("1.json"),
            serde_json::to_vec(&valid).unwrap(),
        )
        .unwrap();
        assert_eq!(j.entry(1).unwrap().unwrap(), entry);
    }

    #[test]
    fn every_current_send_must_be_fenced_and_history_is_never_truncated() {
        let mut t = sample(EntryKind::Stream, 1).tables.remove(0);
        assert!(t.sends_fenced());
        assert!(
            !t.all_rejected(),
            "an unsent table is not proof of rejection"
        );
        for n in 0..70 {
            t.record_send(&format!("q{n}"), 10 + n).unwrap();
        }
        assert_eq!(t.sends, 70);
        assert_eq!(t.query_ids.len(), 70);
        assert_eq!(t.first_send_ms, Some(10));
        assert_eq!(t.clients.len(), 70);
        t.record_terminal("foreign", true);
        assert!(
            t.terminal_query_ids.is_empty(),
            "unknown IDs are not evidence"
        );
        for n in 0..69 {
            let id = format!("q{n}");
            t.record_terminal(&id, true);
            t.record_terminal(&id, true);
        }
        assert!(!t.sends_fenced());
        let before = t.clone();
        assert!(t.next_generation().is_err());
        assert_eq!(t, before);
        t.record_terminal("q69", true);
        assert!(t.sends_fenced());
        assert!(t.all_rejected());
        assert_eq!(t.terminal_query_ids.len(), 70);
        assert_eq!(t.rejected_query_ids.len(), 70);
        t.sends += 1; // A v3 entry whose oldest query_id was truncated.
        assert!(!t.sends_fenced());
        assert!(!t.all_rejected());
        assert!(t.next_generation().is_err());
        t.sends -= 1;
        assert_eq!(t.next_generation().unwrap(), 1);
        assert_eq!(t.sends, 0);
        assert_eq!(t.first_send_ms, None);
        assert!(t.query_ids.is_empty());
        assert!(t.terminal_query_ids.is_empty());
        assert!(t.rejected_query_ids.is_empty());
        assert!(t.clients.is_empty());
    }

    #[test]
    fn send_and_generation_overflow_and_duplicate_ids_leave_state_unchanged() {
        let mut t = sample(EntryKind::Stream, 1).tables.remove(0);
        t.record_send("q1", 10).unwrap();
        let before = t.clone();
        assert!(t.record_send("q1", 20).is_err());
        assert_eq!(t, before);
        t.sends = u32::MAX;
        let before = t.clone();
        assert!(t.record_send("q2", 20).is_err());
        assert_eq!(t, before);
        t.sends = 1;
        t.record_terminal("q1", false);
        assert!(t.sends_fenced());
        assert!(
            !t.all_rejected(),
            "an ACK is terminal, not pre-pipeline rejection"
        );
        t.generation = u32::MAX;
        let before = t.clone();
        assert!(t.next_generation().is_err());
        assert_eq!(t, before);
    }

    /// The fence wait runs from the newest unresolved send, read from its query ID in the v4 and the v3 format.
    /// Terminal sends do not extend it; an undatable ID, a truncated legacy history or an expired wait parks at once.
    #[test]
    fn fence_wait_runs_from_the_newest_unresolved_send_and_never_for_an_undatable_history() {
        let tmp = TempJournal::new();
        let j = tmp.open().with_fence_wait_ms(300_000);
        let mut t = sample(EntryKind::Stream, 1).tables.remove(0);
        let v4_old = query_id(1, &t.table, 1_000_000);
        let v3_new = format!("tank2-1-{}-1100000-7", t.table);
        let v4_newest_terminal = query_id(1, &t.table, 1_200_000);
        t.record_send(&v4_old, 1_000_000).unwrap();
        t.record_send(&v3_new, 1_100_000).unwrap();
        t.record_send(&v4_newest_terminal, 1_200_000).unwrap();
        t.record_terminal(&v4_newest_terminal, false);
        assert!(!t.sends_fenced());
        assert_eq!(j.fence_wait_left(&t, 1_100_000), Some(300_000));
        assert_eq!(j.fence_wait_left(&t, 1_399_999), Some(1));
        assert_eq!(j.fence_wait_left(&t, 1_400_000), None, "the wait is over");
        assert_eq!(
            j.fence_wait_left(&t, 0),
            Some(300_000),
            "a clock behind a send never shortens the wait"
        );
        let mut undatable = t.clone();
        undatable.record_send("q-foreign", 1_300_000).unwrap();
        assert_eq!(j.fence_wait_left(&undatable, 1_300_000), None);
        let mut truncated = t.clone();
        truncated.sends += 1;
        assert_eq!(j.fence_wait_left(&truncated, 1_100_000), None);
        let j = j.with_fence_wait_ms(0);
        assert_eq!(
            j.fence_wait_left(&t, 1_100_000),
            None,
            "a zero wait parks at once"
        );
    }

    #[test]
    fn legacy_receive_deadlines_cannot_extend_wait_or_fence_a_send() {
        let tmp = TempJournal::new();
        let j = tmp.open().with_fence_wait_ms(300_000);
        let mut table = sample(EntryKind::Stream, 1).tables.remove(0);
        let id = query_id(1, &table.table, 1_000_000);
        table.record_send(&id, 1_000_000).unwrap();
        for stale in [0, u64::MAX] {
            let mut json = serde_json::to_value(&table).unwrap();
            json["fence_until_ms"] = serde_json::json!(stale);
            json["clients"][&id]["receive_timeout_ms"] = serde_json::json!(stale);
            let restored: TableState = serde_json::from_value(json).unwrap();
            assert_eq!(
                restored, table,
                "removed fields are ignored rather than migrated"
            );
            assert_eq!(j.fence_wait_left(&restored, 1_100_000), Some(200_000));
            assert_eq!(j.fence_wait_left(&restored, 1_299_999), Some(1));
            assert_eq!(j.fence_wait_left(&restored, 1_300_000), None);
            assert!(!restored.sends_fenced());
            assert!(!restored.all_rejected());
        }
        let j = j.with_fence_wait_ms(0);
        assert_eq!(
            j.fence_wait_left(&table, 1_000_000),
            None,
            "zero wait is parking policy only"
        );
        assert!(!table.sends_fenced());
    }

    #[test]
    fn legacy_entries_default_terminal_evidence_to_empty_not_safe_to_replay() {
        let mut entry = sample(EntryKind::Stream, 1);
        entry.tables[0].record_send("legacy", 10).unwrap();
        entry.writer_pid = u32::MAX; // An old writer PID, even absent now, is never per-send client identity.
        entry.tables[0].clients.clear();
        let json = serde_json::to_value(&entry).unwrap();
        assert!(json["tables"][0].get("terminal_query_ids").is_none());
        assert!(json["tables"][0].get("rejected_query_ids").is_none());
        assert!(json["tables"][0].get("clients").is_none());
        assert!(json["tables"][0].get("fence_until_ms").is_none());
        let mut restored: Entry = serde_json::from_value(json).unwrap();
        assert!(restored.tables[0].terminal_query_ids.is_empty());
        assert!(restored.tables[0].rejected_query_ids.is_empty());
        assert!(restored.tables[0].clients.is_empty());
        let before = restored.tables[0].clone();
        restored.tables[0].record_client_end("legacy", 50).unwrap();
        assert!(!restored.tables[0].observe_client_ends(u64::MAX).unwrap());
        assert_eq!(restored.tables[0], before);
        assert!(!restored.tables[0].sends_fenced());
    }

    #[test]
    fn heal_owner_and_core_manifest_are_json_only_and_survive_restart() {
        let t = TempJournal::new();
        let saved;
        {
            let j = t.open();
            let mut entry = j.create_heal(40, "0xABC", 123, 456).unwrap();
            assert_eq!(EntryKind::Heal.code(), 3);
            assert_eq!(EntryKind::from_code(3), Some(EntryKind::Heal));
            assert_eq!(serde_json::to_value(entry.kind).unwrap(), "heal");
            assert!(entry.tables.is_empty());
            assert_eq!(entry.payload_bytes, 0);
            assert!(entry.payload_fnv1a64.is_empty());
            assert_eq!(t.files(), ["40.json"]);
            assert!(
                !j.lost_payload(40),
                "missing payload is intentional for Heal"
            );
            assert!(j.entry_payload(&entry).unwrap().is_none());
            let i = j.prepare_core(&mut entry, "world_traces", 8).unwrap();
            assert_eq!(i, 0);
            assert_eq!(entry.tables[i].generation, 0);
            assert_eq!(entry.tables[i].status, TableStatus::Pending);
            assert_eq!(j.entry(40).unwrap().unwrap(), entry);
            let before = entry.clone();
            for (table, rows) in [
                (LEDGER_TABLE, 1),
                ("api_token_balances", 1),
                ("world_logs", 0),
            ] {
                assert!(j.prepare_core(&mut entry, table, rows).is_err());
                assert_eq!(entry, before);
            }
            assert!(j.prepare_core(&mut entry, "world_traces", 9).is_err());
            assert_eq!(entry, before);
            entry.tables[i].record_send("heal-first", 500).unwrap();
            j.save(&mut entry).unwrap();
            saved = entry;
        }
        let j = t.open();
        let entry = entries(j.load(EntryKind::Heal).unwrap()).remove(0);
        assert_eq!(entry, saved);
        assert_eq!(entry.tables[0].query_ids, ["heal-first"]);
        assert!(!entry.tables[0].sends_fenced());
        assert!(j.create_heal(40, "0xabc", 123, 456).is_err());
        assert!(j
            .create(&mut sample(EntryKind::Stream, 40), b"stream")
            .is_err());
        assert!(j
            .create(&mut sample(EntryKind::Derive, 40), b"derive")
            .is_err());
        assert_eq!(j.entry(40).unwrap().unwrap(), saved);
        assert_eq!(t.files(), ["40.json"]);
    }

    #[test]
    fn heal_never_replaces_an_existing_owner_or_changes_stream_derive_manifests() {
        let t = TempJournal::new();
        let j = t.open();
        for (n, kind) in [(1, EntryKind::Stream), (2, EntryKind::Derive)] {
            let mut entry = sample(kind, n);
            j.create(&mut entry, b"payload").unwrap();
            let before = entry.clone();
            assert!(j.create_heal(n, "0xabc", 123, 456).is_err());
            assert!(j.prepare_core(&mut entry, "world_traces", 8).is_err());
            assert_eq!(entry, before);
            assert_eq!(j.entry(n).unwrap().unwrap(), before);
        }
        fs::remove_file(t.dir().join("2.json")).unwrap(); // An orphan payload still owns its height.
        assert!(j.create_heal(2, "0xabc", 123, 456).is_err());
        assert!(t.dir().join("2.block").exists());
        let mut invalid = Entry::new(EntryKind::Heal, 3, "0xabc", 123, 456, &[]);
        assert!(j.create(&mut invalid, b"payload").is_err());
        assert!(!j.owns(3));
    }

    #[test]
    fn finish_and_operator_assertions_cannot_discard_unfenced_sends() {
        let t = TempJournal::new();
        let j = t.open();
        let mut entry = j.create_heal(4, "0xabc", 123, 456).unwrap();
        let i = j.prepare_core(&mut entry, "world_logs", 2).unwrap();
        entry.tables[i].record_send("still-running", 500).unwrap();
        entry.tables[i].status = TableStatus::Parked;
        j.save(&mut entry).unwrap();
        let before = entry.clone();
        for how in [Resolution::Done, Resolution::Retry] {
            assert!(resolve_locked(&j, 4, "world_logs", how).is_err());
            assert_eq!(j.entry(4).unwrap().unwrap(), before);
        }
        entry.tables[i].status = TableStatus::Done; // Legacy Done is not network terminal evidence.
        j.save(&mut entry).unwrap();
        assert!(j.finish(&entry).is_err());
        assert!(resolve_locked(&j, 4, "all", Resolution::Done).is_err());
        assert!(j.owns(4));
        entry.tables[i].record_terminal("still-running", false);
        j.save(&mut entry).unwrap();
        assert!(
            j.finish(&entry).is_err(),
            "an all-Done prefix is not the full heal path"
        );
        entry.heal_complete = true;
        j.save(&mut entry).unwrap();
        j.finish(&entry).unwrap();
        assert!(!j.owns(4));
    }

    #[test]
    fn query_ids_are_unique_with_a_random_incarnation_despite_identical_or_older_time() {
        let a = query_id(4, "world_logs", 500);
        let b = query_id(4, "world_logs", 500);
        let c = query_id(4, "world_logs", 1);
        assert_ne!(a, b);
        assert_ne!(a, c);
        let incarnation = |id: &str| id.rsplit('-').nth(1).unwrap().to_string();
        let random = incarnation(&a);
        assert_eq!(random.len(), 32);
        assert!(random.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(random, incarnation(&b));
        assert_eq!(random, incarnation(&c));
    }

    #[test]
    fn cutover_file_is_validated_and_records_its_claim() {
        let t = TempJournal::new();
        let path = t.root.join("cutover.json");
        assert_eq!(
            Cutover::load(&path).err().unwrap().kind(),
            ErrorKind::NotFound
        );
        fs::write(
            &path,
            br#"{"version":1,"floor":101,"complete_tip":99,"created_at_ms":5}"#,
        )
        .unwrap();
        assert_eq!(
            Cutover::load(&path).err().unwrap().kind(),
            ErrorKind::InvalidData,
            "floor = complete_tip + 1"
        );
        fs::write(
            &path,
            br#"{"version":1,"floor":100,"complete_tip":99,"created_by":"deploy_c1v2.sh"}"#,
        )
        .unwrap();
        assert_eq!(
            Cutover::load(&path).err().unwrap().kind(),
            ErrorKind::InvalidData,
            "when the windows were set is required"
        );
        fs::write(
            &path,
            br#"{"version":1,"floor":100,"complete_tip":99,"created_at_ms":0}"#,
        )
        .unwrap();
        assert_eq!(
            Cutover::load(&path).err().unwrap().kind(),
            ErrorKind::InvalidData
        );
        fs::write(&path, br#"{"version":1,"floor":100,"complete_tip":99,"created_at_ms":5,"created_by":"deploy_c1v2.sh"}"#).unwrap();
        let mut c = Cutover::load(&path).unwrap();
        assert_eq!((c.floor, c.created_at_ms, c.claimed_at_ms), (100, 5, None));
        c.claimed_at_ms = Some(7);
        c.claimed_gap = Some([100, 104]);
        c.save(&path).unwrap();
        assert_eq!(Cutover::load(&path).unwrap().claimed_gap, Some([100, 104]));
    }

    #[test]
    fn table_classes_and_count_predicates() {
        assert_eq!(table_class("world_blocks"), TableClass::Core);
        assert_eq!(table_class(LEDGER_TABLE), TableClass::Ledger);
        for t in [
            "world_contract_interactions",
            "api_gas_by_function",
            "world_mev_hourly_stats",
            "api_token_balances",
            "latest_oracle_prices",
        ] {
            assert_eq!(table_class(t), TableClass::Unkeyed, "{t}");
            assert_eq!(count_rows_sql(t, 1, 2, 3_000), None, "{t}");
        }
        let core =
            count_rows_sql("world_traces", 36_000_000, 1_760_000_000, 1_760_000_000_500).unwrap();
        assert!(
            core.contains("chain_id = 480 AND block_number = 36000000"),
            "{core}"
        );
        let own = count_rows_sql("api_internal_transfers", 5, 0, 1_760_086_400_999).unwrap();
        assert!(
            own.contains("ingested_at = toDateTime(1760086400) AND block_number = 5"),
            "{own}"
        );
        let ms = count_rows_sql("world_erc1155_transfers", 5, 0, 1_760_086_400_999).unwrap();
        assert!(
            ms.contains("fromUnixTimestamp64Milli(toInt64(1760086400999))"),
            "{ms}"
        );
        assert!(DEDUP_TABLES
            .iter()
            .all(|t| *t == LEDGER_TABLE || table_class(t) != TableClass::Ledger));
        assert_eq!(
            DEDUP_TABLES
                .iter()
                .filter(|t| table_class(t) == TableClass::Core)
                .count(),
            4
        );
        // no rows proves absence exactly where the count covers every writer's rows (not narrowed to one entry's
        // pinned ingested_at) of a table with block_number
        for t in DEDUP_TABLES {
            let narrowed_or_none = count_rows_sql(t, 5, 7, 1_760_086_400_999)
                .is_none_or(|sql| sql.contains("ingested_at"));
            assert_eq!(count_proves_absence(t), !narrowed_or_none, "{t}");
        }
        assert!(
            !count_proves_absence("api_internal_transfers")
                && count_proves_absence("world_token_transfers")
        );
    }

    #[test]
    fn block_lock_is_exclusive_and_wakes_its_waiter() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let locks = BlockLocks::new();
            let g = locks.lock(7).await;
            assert!(locks.try_lock(7).is_none(), "held");
            assert!(locks.try_lock(8).is_some(), "other blocks are free");
            let l2 = Arc::clone(&locks);
            let waiter = tokio::spawn(async move {
                let _g = l2.lock(7).await;
                7u64
            });
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(!waiter.is_finished(), "waits while the block is held");
            drop(g);
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), waiter)
                    .await
                    .unwrap()
                    .unwrap(),
                7
            );
            assert!(!locks.is_held(7));
        });
    }

    #[test]
    fn block_lock_never_lets_two_tasks_hold_a_block() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(8)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let locks = BlockLocks::new();
            let inside: Arc<[AtomicU32; 3]> = Arc::new(Default::default());
            let mut tasks = Vec::new();
            for i in 0..64u32 {
                let (locks, inside) = (Arc::clone(&locks), Arc::clone(&inside));
                tasks.push(tokio::spawn(async move {
                    let b = (i % 3) as usize; // three blocks, many contenders each
                    for _ in 0..50 {
                        let _g = locks.lock(b as u64).await;
                        assert_eq!(
                            inside[b].fetch_add(1, Ordering::SeqCst),
                            0,
                            "one holder per block"
                        );
                        tokio::task::yield_now().await;
                        inside[b].fetch_sub(1, Ordering::SeqCst);
                    }
                }));
            }
            for t in tasks {
                tokio::time::timeout(Duration::from_secs(30), t)
                    .await
                    .expect("no lost wake-up")
                    .unwrap();
            }
        });
    }
}
