# OPC-UA Line Gateway

Gateway for machines of an industrial production line, using OPC-UA to connect to
PLCs, with data caching and archiving.

## How it works

This project provides executable services, intended to run forever. The service
connects to the PLCs of the machines to allow them to request data save or retrieval.
The data is kept in a disk-backed memory cache to allow efficient storage and fetching.

### OPC-UA

The service acts as multiple OPC-UA clients, each one connected to an OPC-UA server
on a machine (PLC). Upon disconnection from the machine, the client tries to reconnect
forever.

### Configuration

This section summarizes the configuration contents.

#### Common

* Application URI
* OPC-UA PKI directory

#### For each OPC-UA server

* Target URL (e.g. `opc.tcp://ip-or-hostname:port`)
* Security Policy (e.g. `Basic256Sha256 - Sign & Encrypt`)
* Authentication mode (anonymous, user/pass, …)

#### For each traceability-enabled machine

* Namespace URL for Traceability NodeSet
* Request byte NodeId
* Response byte NodeId
* Part data sheet Objects NodeIds

## Traceability

This service handles traceability management, which involves moving data between,
on one side, OPC-UA servers it connects to, and on the other side, an in-memory
disk-persisted cache and a ClickHouse database.

The data consists of groups of OPC-UA variables, which can be scalars or arrays
of scalars. Grouping is achieved by organizing variables in a group as properties
of an OPC-UA object. In this project's terminology, some groups are called
"part sheets".

OPC-UA server data is organised in three groups:

* The "traceability protocol" group, which includes the request code, the response
code, and the OPC-UA client heartbeat;
* The "general part sheet" group, which includes data that is common to all OPC-UA
servers;
* The "operation part sheet" group, which includes data that is specific for each
operation on the production line.

### Protocol

```mermaid
sequenceDiagram
    box slategrey Machine controller
        participant Program
        participant Data as Traceability Data
    end
    box slategrey Traceability Application
        participant us as Runtime
        participant Cache@{ "type": "database" }
    end
    participant Database@{ "type": "database" }

    Program->>Data:Set request code
    critical❗ Machine program must not write traceability data ❗
        Data-->>us: Get request code notification
        alt Create request
            us->>+Data: Read required variables
            Data-->>-us: Response
            us->>us: Generate part ID
            us->>Data: Write part ID
        else Load request
            Cache->>us: Read general part sheet
            us->>+Data: Write general part sheet
            Data-->>-us: Response
        else Save request
            us->>+Data: Read part sheets
            Data-->>-us: Response
            us->>Cache: Write general part sheet
            us->>Database: Write general part sheet
            us->>Database: Write operation part sheet
        end
        us->>Data:Write response code
    end
    Program->>Data:Reset request code
    Data-->>us:Get request code notification
    us->>Data:Reset response code
```
