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

## Traceability Protocol

```mermaid
sequenceDiagram
    participant PLC as PLC Program
    participant DB as Mailbox DB
    participant us as Traceability app

    PLC->>DB:Set request code
    critical ❗ PLC writing to mailbox DB forbidden ❗
        DB-->>us:Get request code notification
        alt Create request
            us->>+DB:Read general part sheet
            DB-->>-us:Response
            us->>us:Generate part ID
            us->>DB:Write general part sheet<br/>with part ID
        else Load request
            us->>us:Read parts sheets from cache
            us->>DB:Write part sheets from cache
        else Save request
            us->>+DB:Read part sheets to cache
            DB-->>-us:Response
            us->>us:Write part sheets to cache
        end
        us->>DB:Write response code
    end
    PLC->>DB:Reset request code
    DB-->>us:Get request code notification
    us->>DB:Reset response code
```
