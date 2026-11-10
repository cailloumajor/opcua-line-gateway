# LineGatewayConfig

OPC-UA line gateway configuration.

### Type: `object`

> ⚠️ Additional properties are not allowed.

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| application_uri | `string` | ✅ | string | Globally unique identifier for the application instance, as of OPC-UA. |
| machines | `object` | ✅ | [MachineConfig](#machineconfig) | Connected machines configuration, mapped by machine identifier. |
| pki_dir | `string` | ✅ | string | Root directory of the OPC-UA PKI. |
| traceability | `object` | ✅ | [TraceabilityCommonConfig](#traceabilitycommonconfig) | Traceability configuration for all machines. |


---

# Definitions

## MachineConfig

Connected machine configuration.

#### Type: `object`

> ⚠️ Additional properties are not allowed.

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| opc_ua_server | `object` | ✅ | [OpcUaServerConfig](#opcuaserverconfig) | OPC-UA server configuration for this machine. |
| traceability | `object` | ✅ | [TraceabilityMachineConfig](#traceabilitymachineconfig) | Traceability settings for this machine. |

## NodeName

`BrowseName` of an OPC-UA node, whose length in bytes is guaranteed to fit in
an [`u8`].

#### Type: `string`

## OpcUaServerConfig

Connected OPC-UA server configuration.

#### Type: `object`

> ⚠️ Additional properties are not allowed.

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| publish_interval | `string` | ✅ | string | Publish interval for OPC-UA subscription to request variable. |
| security_mode | `string` | ✅ | `None` `Sign` `SignAndEncrypt` | OPC-UA security mode. |
| security_policy | `string` | ✅ | `None` `Aes128Sha256RsaOaep` `Basic256Sha256` `Aes256Sha256RsaPss` `Basic128Rsa15` `Basic256` | OPC-UA security policy. |
| url | `string` | ✅ | Format: [`uri`](https://json-schema.org/understanding-json-schema/reference/string#built-in-formats) | OPC-UA server URL. |

## TraceabilityCommonConfig

Traceability configuration for all machines.

#### Type: `object`

> ⚠️ Additional properties are not allowed.

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| database | `object` | ✅ | [TraceabilityDatabaseConfig](#traceabilitydatabaseconfig) | ClickHouse database client configuration for archiving traceability data. |
| line_id | `string` | ✅ | [`^[0-9A-Z]{2}$`](https://regex101.com/?regex=%5E%5B0-9A-Z%5D%7B2%7D%24) | Two character production line identifier for traceability. |
| opc_ua | `object` | ✅ | [TraceabilityCommonOpcUaConfig](#traceabilitycommonopcuaconfig) | OPC-UA traceability information model, common to all machines. |
| queues_not_draining_threshold | `integer` | ✅ | `0 <= x ` | The upper limit of enqueued part sheets to consider draining as working. |
| redb_file | `string` | ✅ | string | Path to the redb file to use for traceability cache. It will be created<br />if it does not exist. |

## TraceabilityCommonOpcUaConfig

OPC-UA traceability information model, as implemented by all mailboxes of all
machines.

#### Type: `object`

> ⚠️ Additional properties are not allowed.

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| excluded_read_nodes | `array` | ✅ | [NodeName](#nodename) | OPC-UA `BrowseName` of the nodes to be excluded from general part sheet read request. |
| general_part_sheet_node | `string` | ✅ | string | OPC-UA `BrowseName` of the general part sheet object. |
| heartbeat_node | `string` | ✅ | string | OPC-UA `BrowseName` of the traceability heartbeat node. |
| mailbox_type | `string` | ✅ | string | OPC-UA `BrowseName` of the traceability mailbox common type. |
| namespace_url | `string` | ✅ | Format: [`uri`](https://json-schema.org/understanding-json-schema/reference/string#built-in-formats) | OPC-UA namespace URL used for common part of traceability. |
| operation_part_sheet_type | `string` | ✅ | string | OPC-UA `BrowseName` of the operation part sheet common type. |
| part_id_node | `string` | ✅ | [NodeName](#nodename) | OPC-UA `BrowseName` of the part identifier variable. |
| raw_batch_node | `string` | ✅ | [NodeName](#nodename) | OPC-UA `BrowseName` of the raw material batch variable. |
| raw_part_ref_node | `string` | ✅ | [NodeName](#nodename) | OPC-UA `BrowseName` of the raw part reference variable. |
| request_node | `string` | ✅ | string | OPC-UA `BrowseName` of the traceability request node. |
| response_node | `string` | ✅ | string | OPC-UA `BrowseName` of the traceability response node. |

## TraceabilityDatabaseConfig

ClickHouse database configuration for traceability.

#### Type: `object`

> ⚠️ Additional properties are not allowed.

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| default_database | `string` | ✅ | string | Default database to use. |
| general_part_sheet_table | `string` | ✅ | string | Table to use for general part sheet. |
| operation_part_sheet_table | `string` | ✅ | string | Table to use for operation part sheet. |
| part_sheets_drain_period | `string` | ✅ | string | Part sheets draining task execution period. |
| url | `string` | ✅ | Format: [`uri`](https://json-schema.org/understanding-json-schema/reference/string#built-in-formats) | URL of the ClickHouse HTTP(S) endpoint. |

## TraceabilityMachineConfig

Traceability related configuration for a machine.

#### Type: `object`

> ⚠️ Additional properties are not allowed.

| Property | Type | Required | Possible values | Description |
| -------- | ---- | -------- | --------------- | ----------- |
| machine_nid | `integer` | ✅ | `0 <= x ` | OPC-UA node identifier of the machine object (root traceability object). |
| namespace_url | `string` | ✅ | Format: [`uri`](https://json-schema.org/understanding-json-schema/reference/string#built-in-formats) | OPC-UA namespace URL used for operation part of traceability. |


---

Markdown generated with [jsonschema-markdown](https://github.com/elisiariocouto/jsonschema-markdown).
