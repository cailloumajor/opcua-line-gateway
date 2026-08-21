CREATE DATABASE IF NOT EXISTS traceability;

-- Traceability — general part sheet
CREATE TABLE IF NOT EXISTS traceability.general_part_sheet
(
    -- Produced by the gateway.
    saved_at   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    machine_id LowCardinality(String),
    -- Part identifier: a protocol variable, excluded from `data` to avoid duplication.
    part_id    String,

    -- General part sheet contract. Path names are OPC-UA BrowseNames.
    -- Declared paths are typed and always present; undeclared ones are still
    -- accepted and stored as Dynamic, so a new machine variable needs no ALTER.
    -- Adding a declared path triggers a mutation: schedule it.
    data JSON(
        RefePiecForg            LowCardinality(String),
        LotMati                 LowCardinality(String),
        RefePiecFini            LowCardinality(String),
        PiecAnnoChgtRefPiecFini Bool,
        PiecAnnoChgtLotMati     Bool,
        SuivTravPiecParPost     Array(Bool),
        SuivConfPiecParPost     Array(Bool),
        SuivPostAvecPassTrav    Array(Bool),
        SuivPrelPiecParPost     Array(Bool),
        SuivRejePiecParPost     Array(Bool),
        RefePiecFiniInco        Bool,
        PiecTravInco            Bool,
        PiecNonConfAmon         Bool,
        LotMatiInco             Bool,
        PiecRebu                Bool,
        DeclRebuMoti            LowCardinality(String),
        DeclRebuQui             LowCardinality(String)
    ),

    part_ref    FixedString(9) MATERIALIZED substring(part_id, 1, 9),
    line_id     FixedString(2) MATERIALIZED substring(part_id, 12, 2),
    produced_on Date           MATERIALIZED makeDate(2000 + toUInt8(substring(part_id, 14, 2)),
                                                    toUInt16(substring(part_id, 16, 3))),

    INDEX idx_ref   part_ref     TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_batch data.LotMati TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(saved_at)
ORDER BY (line_id, part_id, saved_at)
COMMENT 'General part sheet — one row per Save request, full history kept';

-- Traceability - operation part sheet
CREATE TABLE IF NOT EXISTS traceability.operation_part_sheet
(
    -- Produced by the gateway.
    saved_at   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    machine_id LowCardinality(String),
    part_id    String,

    -- Operation part sheet. Path names are OPC-UA BrowseNames.
    data JSON,

    part_ref    FixedString(9) MATERIALIZED substring(part_id, 1, 9),
    line_id     FixedString(2) MATERIALIZED substring(part_id, 12, 2),
    produced_on Date           MATERIALIZED makeDate(2000 + toUInt8(substring(part_id, 14, 2)),
                                                    toUInt16(substring(part_id, 16, 3))),

    INDEX idx_part part_id  TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_ref  part_ref TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(saved_at)
ORDER BY (machine_id, saved_at, part_id);
