#!/usr/bin/env bash
set -e

clickhouse client \
    --password "$CLICKHOUSE_PASSWORD" \
    --param_database="$CH_DATABASE" \
    --queries-file /tables/traceability.sql
