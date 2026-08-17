-- ============================================================================
-- Traceability — general part sheet
--
-- IMPORTANT: the gateway reads its contract at startup, from the columns
-- carrying an `opcua:<BrowseName>` comment.
--
-- Removing, renaming or retyping one of those columns requires:
--   1. stop the OPC-UA sessions     2. drain the outbox queue
--   3. apply the schema change      4. restart the service
--
-- Adding a marked column is safe, but has no effect until the next restart.
--
-- Contract derived from Bjc3.NodeSet2.xml, TraceabilityPartSheetGeneralType
-- (ns=1;i=1001), namespace `urn:ntn:Bjc3`.
-- ============================================================================

CREATE DATABASE IF NOT EXISTS traceability;

CREATE TABLE IF NOT EXISTS traceability.general_part_sheet
(
    -- ---- Columns produced by the gateway (no `opcua:` marker) --------------
    saved_at    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    machine_id  LowCardinality(String),

    -- ---- General part sheet contract ---------------------------------------
    part_id                       String                 COMMENT 'opcua:NumeUniq',
    forged_part_ref               LowCardinality(String) COMMENT 'opcua:RefePiecForg',
    material_batch                LowCardinality(String) COMMENT 'opcua:LotMati',
    finished_part_ref             LowCardinality(String) COMMENT 'opcua:RefePiecFini',
    announces_finished_ref_change Bool                   COMMENT 'opcua:PiecAnnoChgtRefPiecFini',
    announces_batch_change        Bool                   COMMENT 'opcua:PiecAnnoChgtLotMati',
    worked_by_station             Array(Bool)            COMMENT 'opcua:SuivTravPiecParPost',
    conforming_by_station         Array(Bool)            COMMENT 'opcua:SuivConfPiecParPost',
    pass_through_by_station       Array(Bool)            COMMENT 'opcua:SuivPostAvecPassTrav',
    sampled_by_station            Array(Bool)            COMMENT 'opcua:SuivPrelPiecParPost',
    rejected_by_station           Array(Bool)            COMMENT 'opcua:SuivRejePiecParPost',
    finished_ref_inconsistent     Bool                   COMMENT 'opcua:RefePiecFiniInco',
    worked_part_inconsistent      Bool                   COMMENT 'opcua:PiecTravInco',
    upstream_non_conforming       Bool                   COMMENT 'opcua:PiecNonConfAmon',
    batch_inconsistent            Bool                   COMMENT 'opcua:LotMatiInco',
    scrapped                      Bool                   COMMENT 'opcua:PiecRebu',
    scrap_reason                  LowCardinality(String) COMMENT 'opcua:DeclRebuMoti',
    scrap_declared_by             LowCardinality(String) COMMENT 'opcua:DeclRebuQui',

    -- ---- part_id decomposition (derived, outside the contract) -------------
    line_id     FixedString(2) MATERIALIZED substring(part_id, 12, 2),
    produced_on Date           MATERIALIZED makeDate(2000 + toUInt8(substring(part_id, 14, 2)),
                                                    toUInt16(substring(part_id, 16, 3))),

    INDEX idx_part_ref finished_part_ref TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_batch    material_batch    TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(saved_at)
ORDER BY (part_id, saved_at)
COMMENT 'General part sheet — one row per Save request, full history kept';

