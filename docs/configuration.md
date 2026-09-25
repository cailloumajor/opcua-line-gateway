# LineGatewayConfig

OPC-UA line gateway configuration.

### Type: `object`

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| application_uri | `string` | ✅ | string | Globally unique identifier for the application instance, as of OPC-UA. |
| machines | `object` | ✅ | [MachineConfig](#machineconfig) | Connected machines configuration, mapped by machine identifier. |
| pki_dir | `string` | ✅ | string | Root directory of the OPC-UA PKI. |
| traceability | `object` | ✅ | [CommonTraceabilityConfig](#commontraceabilityconfig) | Traceability configuration for all machines. |


---

# Definitions

## CommonTraceabilityConfig

Traceability configuration for all machines.

#### Type: `object`

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| database | `object` | ✅ | [TraceabilityDatabaseConfig](#traceabilitydatabaseconfig) | ClickHouse database client configuration for archiving traceability data. |
| opc_ua | `object` | ✅ | [TraceabilityCommonOpcUaConfig](#traceabilitycommonopcuaconfig) | OPC-UA traceability information model, common to all machines. |
| queues_not_draining_threshold | `integer` | ✅ | `0 <= x ` | The upper limit of enqueued part sheets to consider draining as working. |
| redb_file | `string` | ✅ | string | Path to the redb file to use for traceability cache. It will be created<br />if it does not exist. |

## MachineConfig

Connected machine configuration.

#### Type: `object`

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| opc_ua_server | `object` | ✅ | [OpcUaServerConfig](#opcuaserverconfig) | OPC-UA server configuration for this machine. |
| traceability | `object` | ✅ | [MachineTraceabilityConfig](#machinetraceabilityconfig) | Traceability settings for this machine. |

## MachineTraceabilityConfig

Traceability related configuration for a machine.

#### Type: `object`

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| publish_interval | `string` | ✅ | string | Publish interval for OPC-UA subscription to request variable. |
| line_id | `string` or `null` |  | string | Two character production line identifier, only present if the machine makes<br />part identifier creation requests. |

## OpcUaServerConfig

Connected OPC-UA server configuration.

#### Type: `object`

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| security_mode | `string` | ✅ | `None` `Sign` `SignAndEncrypt` | OPC-UA security mode. |
| security_policy | `string` | ✅ | `None` `Aes128Sha256RsaOaep` `Basic256Sha256` `Aes256Sha256RsaPss` `Basic128Rsa15` `Basic256` | OPC-UA security policy. |
| url | `string` | ✅ | Format: [`uri`](https://json-schema.org/understanding-json-schema/reference/string#built-in-formats) | OPC-UA server URL. |
| password | `string` or `null` |  | string | Password to use if using username/password authentication. |
| user | `string` or `null` |  | string | Username if authenticating to the OPC-UA server with username/password.<br />If not provided, anonymous authentication will be used. |

## TraceabilityCommonOpcUaConfig

OPC-UA traceability information model, as implemented by all machines.

#### Type: `object`

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| general_part_sheet_nid | `integer` | ✅ | `0 <= x ` | OPC-UA node identifier of the general part sheet object. |
| heartbeat_nid | `integer` | ✅ | `0 <= x ` | OPC-UA node identifier of the heartbeat variable. |
| namespace_url | `string` | ✅ | Format: [`uri`](https://json-schema.org/understanding-json-schema/reference/string#built-in-formats) | OPC-UA namespace URL used for traceability. |
| part_id_nid | `integer` | ✅ | `0 <= x ` | OPC-UA node identifier of the `part ID` variable. |
| raw_batch_nid | `integer` | ✅ | `0 <= x ` | OPC-UA node identifier of the raw material batch variable. |
| raw_part_ref_nid | `integer` | ✅ | `0 <= x ` | OPC-UA node identifier of the raw part reference variable. |
| request_nid | `integer` | ✅ | `0 <= x ` | OPC-UA node identifier of the request variable. |
| response_nid | `integer` | ✅ | `0 <= x ` | OPC-UA node identifier of the response variable. |

## TraceabilityDatabaseConfig

ClickHouse database configuration for traceability.

#### Type: `object`

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| default_database | `string` | ✅ | string | Default database to use. |
| general_part_sheet_table | `string` | ✅ | string | Table to use for general part sheet. |
| operation_part_sheet_table | `string` | ✅ | string | Table to use for operation part sheet. |
| part_sheets_drain_period | `string` | ✅ | string | Part sheets draining task execution period. |
| password_file | `string` | ✅ | string | Path to a file containing the ClickHouse user's password. Whitespaces around<br />the password will be removed. |
| url | `string` | ✅ | Format: [`uri`](https://json-schema.org/understanding-json-schema/reference/string#built-in-formats) | URL of the ClickHouse HTTP(S) endpoint. |
| user | `string` | ✅ | string | ClickHouse user. |


---

Markdown generated with [jsonschema-markdown](https://github.com/elisiariocouto/jsonschema-markdown).
