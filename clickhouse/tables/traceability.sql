-- ============================================================================
-- Traceability archive schema
--
-- These tables are currently managed BY HAND: there is no migration tool yet.
-- Apply this file with:
--
--     clickhouse client --queries-file clickhouse/tables/traceability.sql
--
-- CAUTION — `IF NOT EXISTS` means editing this file does NOT update an existing
-- table. Changing a definition requires dropping and recreating it, which is
-- why the breaking-change procedure below matters. Consider adopting `dbmate`
-- (plain SQL, ClickHouse driver, one line in `mise.toml`) once a second
-- environment exists; hand management stops being viable at that point.
--
-- Breaking changes — removing, renaming or retyping a declared path, or
-- altering a sorting key — require, in order:
--   1. stop the OPC-UA sessions
--   2. drain the outbox queue to empty
--   3. apply the schema change
--   4. restart the service
--
-- Adding a declared path is safe but inert until the next restart. A variable
-- that is NOT declared is still archived (stored as Dynamic), so a machine
-- gaining a variable never loses data and never fails an insert.
-- ============================================================================

CREATE DATABASE IF NOT EXISTS traceability;

-- ----------------------------------------------------------------------------
-- General part sheet: data common to every machine on the line.
-- One row per Save request, i.e. one row per part per operation. History is
-- kept: the sheet is enriched as the part travels, and each version is a row.
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS traceability.general_part_sheet
(
    -- Produced by the gateway, never read from a machine. Stamped when the row
    -- is queued in redb, never at insertion time: a retried insert must be
    -- byte-identical to the first attempt for deduplication to work.
    saved_at   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    machine_id LowCardinality(String),

    -- Part identifier. Kept as a real column rather than MATERIALIZED from
    -- `data.NumeUniq`, for three reasons:
    --   * it is the pivot of the traceability protocol — the gateway generates
    --     it, writes it back to the machine, and uses it as the redb cache and
    --     outbox deduplication key — so it is held in hand before the row is
    --     built and costs nothing to write;
    --   * a MATERIALIZED column is stored, so deriving it would keep the value
    --     twice (once in the JSON subcolumn, once in the column);
    --   * the sorting key would then depend on JSON parsing, and a missing
    --     `NumeUniq` silently yields an empty key.
    part_id    FixedString(23),

    -- General part sheet payload. Path names are OPC-UA BrowseNames, compared
    -- byte-for-byte against the browse result — no case folding, no
    -- normalisation.
    --
    -- `NumeUniq` is deliberately absent: it is carried by `part_id` above.
    --
    -- Why a JSON column rather than 17 typed columns: it gives the gateway one
    -- single code path for both part sheets, and it removes the whole "schema
    -- mismatch" failure class — an undeclared variable becomes a new path
    -- instead of a rejected insert, so nothing lands in the dead-letter queue
    -- and no ALTER is needed.
    --
    -- Why declare the types anyway: they show up in `DESCRIBE TABLE`, so this
    -- block is readable documentation; they are stored as real typed
    -- subcolumns rather than Dynamic; and `MATERIALIZED data.X` needs no cast.
    -- Note the enforcement is partial — an unparsable value is rejected, but a
    -- coercible one is silently converted (`42` into a String path becomes
    -- `"42"`). Monitor drift with:
    --
    --     SELECT DISTINCT arrayJoin(JSONDynamicPathsWithTypes(data))
    --     FROM traceability.general_part_sheet
    --     WHERE saved_at > now() - INTERVAL 1 DAY
    --
    -- Two consequences of declaring a path: it is ALWAYS materialised, so an
    -- absent variable is indistinguishable from `false` / `''` / `[]` (the
    -- startup conformance check is what guarantees presence); and adding one
    -- later triggers a MUTATION, unlike a plain `ADD COLUMN`. Schedule it.
    --
    -- `LowCardinality` on the strings only: references, batches, scrap reasons
    -- and operator names have few distinct values per monthly partition. NOT on
    -- `Bool` or `Array(Bool)`, where a dictionary costs more than it saves.
    --
    -- The order below groups fields by meaning for the reader; ClickHouse
    -- re-sorts declared paths alphabetically, as `SHOW CREATE TABLE` shows.
    data JSON(
        RefePiecForg            LowCardinality(String), -- forged part reference
        LotMati                 LowCardinality(String), -- material batch
        RefePiecFini            LowCardinality(String), -- finished part reference
        PiecAnnoChgtRefPiecFini Bool,                   -- announces a finished-ref change
        PiecAnnoChgtLotMati     Bool,                   -- announces a batch change
        SuivTravPiecParPost     Array(Bool),            -- worked, by station
        SuivConfPiecParPost     Array(Bool),            -- conforming, by station
        SuivPostAvecPassTrav    Array(Bool),            -- pass-through, by station
        SuivPrelPiecParPost     Array(Bool),            -- sampled, by station
        SuivRejePiecParPost     Array(Bool),            -- rejected, by station
        RefePiecFiniInco        Bool,                   -- inconsistent finished ref
        PiecTravInco            Bool,                   -- inconsistent worked part
        PiecNonConfAmon         Bool,                   -- non-conforming upstream
        LotMatiInco             Bool,                   -- inconsistent material batch
        PiecRebu                Bool,                   -- scrapped
        DeclRebuMoti            LowCardinality(String), -- scrap reason
        DeclRebuQui             LowCardinality(String)  -- scrap declared by
    ),

    -- Decomposition of `part_id`, as built by `create_part_identifier`:
    --   1- 9  normalised part reference, 9 digits
    --  10-11  raw material batch, 2 ASCII characters
    --  12-13  production line identifier, 2 digits
    --  14-15  year, 2 digits
    --  16-18  day of year, 3 digits
    --  19-23  per-day serial number, 5 digits
    --
    -- `part_ref` is the canonical grouping key: it is distinct from
    -- `data.RefePiecForg` / `data.RefePiecFini`, which are the raw references.
    --
    -- The conversions are STRICT on purpose — no `…OrZero`. A malformed part_id
    -- is an anomaly, not data to salvage, and must not be quietly turned into a
    -- sentinel date. Be aware of what strictness costs: `toUInt8` throws on a
    -- short or non-numeric value, and a throwing MATERIALIZED expression fails
    -- the insert of the WHOLE batch (verified on 26.8: `Code: 32` on a short
    -- id, `Code: 6` on a non-numeric one).
    --
    -- Two obligations follow:
    --   * the gateway MUST validate the part_id format before queueing, so the
    --     machine gets an error response code, holds the part and retries —
    --     nothing malformed ever reaches redb, and this check never fires;
    --   * the drain MUST bisect a batch on a 4xx to isolate the offending row,
    --     instead of dead-lettering thirty sound rows with it.
    part_ref    FixedString(9) MATERIALIZED substring(part_id, 1, 9),
    line_id     FixedString(2) MATERIALIZED substring(part_id, 12, 2),
    produced_on Date           MATERIALIZED makeDate(2000 + toUInt8(substring(part_id, 14, 2)),
                                                     toUInt16(substring(part_id, 16, 3))),  -- editorconfig-checker-disable-line

    -- No index on `part_id`: it is already in the sorting key.
    -- `data.LotMati` is indexable directly, so no promoted column is needed —
    -- and it should be effective because the sorting key starts with the part
    -- reference, whose batch characters sit at positions 10-11 of `part_id`,
    -- making the batch strongly correlated with the physical order.
    INDEX idx_ref   part_ref     TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_batch data.LotMati TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(saved_at)
-- Part-centric ordering: the critical query is "everything about this part"
-- (customer claim, recall, expertise), so `part_id` leads despite being high
-- cardinality — a point lookup on a non-leading key would scan. `line_id` is a
-- cheap low-cardinality discriminator ahead of it, useful only if several lines
-- ever write to this table; with a single line it is constant and free.
--
-- No TTL, deliberately: a TTL would silently delete regulatory records. At the
-- estimated rate (26.5 s takt, 14 machines ≈ 16 M rows/year) retention is a
-- storage question, not a performance one — revisit if that changes.
ORDER BY (line_id, part_id, saved_at)
COMMENT 'General part sheet — one row per Save request, full history kept';

-- ----------------------------------------------------------------------------
-- Operation part sheet: data specific to each operation on the line.
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS traceability.operation_part_sheet
(
    -- Same gateway-owned columns as above, same rationale.
    saved_at   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    machine_id LowCardinality(String),
    part_id    FixedString(23),

    -- Bare `JSON`, with no declared paths: unlike the general sheet, this
    -- payload is heterogeneous BY DESIGN — each machine exposes its own
    -- variables — so there is no contract to declare or to check. Declaring
    -- types here would mean either one table per machine, or a wide sparse
    -- union of every machine's variables.
    --
    -- If one machine's analysis becomes central, promote its hot paths with
    -- `ADD COLUMN … MATERIALIZED` (metadata only) rather than declaring types.
    data JSON,

    -- Same decomposition, same strictness, same obligations as above.
    part_ref    FixedString(9) MATERIALIZED substring(part_id, 1, 9),
    line_id     FixedString(2) MATERIALIZED substring(part_id, 12, 2),
    produced_on Date           MATERIALIZED makeDate(2000 + toUInt8(substring(part_id, 14, 2)),
                                                     toUInt16(substring(part_id, 16, 3))),  -- editorconfig-checker-disable-line

    -- `part_id` is only third in the sorting key, so genealogy queries need
    -- this bloom filter: high global cardinality, but a part passes a given
    -- machine once, so its values are confined to very few granules.
    INDEX idx_part part_id  TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_ref  part_ref TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(saved_at)
-- Process-centric ordering, deliberately the opposite of the general sheet.
-- This table answers "how did station PLC2 behave this week" (SPC, drift,
-- maintenance), which scans machine and time ranges; the general sheet answers
-- "everything about this part". Two access patterns, two tables, two keys — one
-- ORDER BY cannot serve both, hence the bloom filter above for the
-- part-centric case.
ORDER BY (machine_id, saved_at, part_id)
COMMENT 'Operation part sheet — one row per Save request, payload heterogeneous by machine';
