# Target OPC-UA information model

> [!NOTE]
> This model is not implemented yet, see the [TO-DO list](../TODO.md). The
> [README](../README.md) describes the model currently handled.

## Overview

Each machine exposes a heartbeat and one or more mailboxes. A mailbox holds the
traceability protocol variables (request and response codes) and a complete set
of part sheets: the general part sheet and the operation part sheet.

### Example for a production line

* Machine OP10
  * TraceabilityHeartbeat (boolean)
  * TraceabilityMailboxOP10 (object)
    * TraceabilityRequest (byte)
    * TraceabilityResponse (byte)
    * TraceabilityPartSheetGeneral (object)
      * ...general part sheet variables (mixed variable types)
    * TraceabilityPartSheetOP10
      * ...OP10 part sheet variables (mixed variable types)
* Machine OP20
  * TraceabilityHeartbeat (boolean)
  * TraceabilityMailboxOP20In (object)
    * TraceabilityRequest (byte)
    * TraceabilityResponse (byte)
    * TraceabilityPartSheetGeneral (object)
      * ...general part sheet variables (mixed variable types)
    * TraceabilityPartSheetOP20
      * ...OP20 part sheet variables (mixed variable types)
  * TraceabilityMailboxOP20Out (object)
    * TraceabilityRequest (byte)
    * TraceabilityResponse (byte)
    * TraceabilityPartSheetGeneral (object)
      * ...general part sheet variables (mixed variable types)
    * TraceabilityPartSheetOP20
      * ...OP20 part sheet variables (mixed variable types)
* Machine OP30
  * TraceabilityHeartbeat (boolean)
  * TraceabilityMailboxOP30In (object)
    * TraceabilityRequest (byte)
    * TraceabilityResponse (byte)
    * TraceabilityPartSheetGeneral (object)
      * ...general part sheet variables (mixed variable types)
    * TraceabilityPartSheetOP30
      * ...OP30 part sheet variables (mixed variable types)
  * TraceabilityMailboxOP30OkOut (object)
    * TraceabilityRequest (byte)
    * TraceabilityResponse (byte)
    * TraceabilityPartSheetGeneral (object)
      * ...general part sheet variables (mixed variable types)
    * TraceabilityPartSheetOP30
      * ...OP30 part sheet variables (mixed variable types)
  * TraceabilityMailboxOP30ScrapOut (object)
    * TraceabilityRequest (byte)
    * TraceabilityResponse (byte)
    * TraceabilityPartSheetGeneral (object)
      * ...general part sheet variables (mixed variable types)
    * TraceabilityPartSheetOP30
      * ...OP30 part sheet variables (mixed variable types)
* Machine OP40
  * TraceabilityHeartbeat (boolean)
  * TraceabilityMailboxOP40 (object)
    * TraceabilityRequest (byte)
    * TraceabilityResponse (byte)
    * TraceabilityPartSheetGeneral (object)
      * ...general part sheet variables (mixed variable types)
    * TraceabilityPartSheetOP40
      * ...OP40 part sheet variables (mixed variable types)

### Simplified view

Each machine of the line has one or more mailboxes to exchange part information
with the traceability system. Their number is chosen by the machine builder, and
is not necessarily related to the flow of parts: a machine can use a single
mailbox and perform data transfers internally at the right times, or use several
mailboxes, for instance one for each point where parts enter or leave it. Each
machine also has a heartbeat, showing that the traceability system is connected
and working:

```mermaid
flowchart TB
  subgraph OP10["🏭 Machine OP10"]
    direction LR
    OP10H["❤️ Heartbeat"]
    OP10M["📬 Mailbox OP10"]
  end
  subgraph OP20["🏭 Machine OP20"]
    direction LR
    OP20H["❤️ Heartbeat"]
    OP20I["📬 Mailbox OP20In"]
    OP20O["📬 Mailbox OP20Out"]
  end
  subgraph OP30["🏭 Machine OP30"]
    direction LR
    OP30H["❤️ Heartbeat"]
    OP30I["📬 Mailbox OP30In"]
    OP30O["📬 Mailbox OP30OkOut"]
    OP30S["📬 Mailbox OP30ScrapOut"]
  end
  subgraph OP40["🏭 Machine OP40"]
    direction LR
    OP40H["❤️ Heartbeat"]
    OP40M["📬 Mailbox OP40"]
  end

  OP10 == "⚙️ parts" ==> OP20 == "⚙️ parts" ==> OP30 == "⚙️ parts" ==> OP40
```

All mailboxes are organized the same way: a request and a response, to exchange
with the traceability system, and two forms describing the part, the part
sheets. The traceability system archives the part sheets, and can restore the
general part sheet on a machine further down the line. Next to the mailboxes,
the traceability system regularly changes the heartbeat, which the machine
monitors to know that the traceability system is connected and working:

```mermaid
flowchart TB
  Machine["🏭 Machine"]
  System["🖥️ Traceability system"]
  Heartbeat["`❤️ **Heartbeat**
  changes regularly`"]

  subgraph Mailbox["📬 Mailbox"]
    subgraph Exchange["Exchange"]
      Request["`🔔 **Request**
      what the machine asks for
      (e.g. save the part sheets)`"]
      Response["`✅ **Response**
      the traceability system answer`"]
    end
    subgraph Sheets["Part sheets"]
      General["`📄 **General part sheet**
      information common to all machines
      (e.g. part identifier)`"]
      Operation["`📄 **Operation part sheet**
      information specific to the operation
      (e.g. measurements made by the machine)`"]
    end
  end

  Machine <-- "`writes the request
  reads the response
  fills in the part sheets`" --> Mailbox
  Mailbox <-- "`reads the request
  writes the response
  archives / restores the part sheets`" --> System
  Machine <-- monitors --> Heartbeat
  Heartbeat <-- changes --> System
```

## Namespaces

The model is split in companion specifications, imported in the machines
controllers:

* A common namespace, imported on all machines, which only defines types (it has
no instances):
  * `TraceabilityPartSheetGeneralType`, with the general part sheet variables;
  * `TraceabilityPartSheetOperationType`, abstract and without variables, base of
  the operation part sheets types;
  * `TraceabilityMailboxType`, with:
    * `TraceabilityRequest` (`Byte`, read only);
    * `TraceabilityResponse` (`Byte`, read and write);
    * `TraceabilityPartSheetGeneral`, a mandatory component of
    `TraceabilityPartSheetGeneralType`;
  * `TraceabilityMachineType`, with:
    * `TraceabilityHeartbeat` (`Boolean`, read and write);
    * a `<Mailbox>` component of `TraceabilityMailboxType`, with the
    `MandatoryPlaceholder` modelling rule (i.e. one or more mailboxes, with any
    name).
* An operation namespace for each operation, imported on the machine performing
it, which defines (e.g. for OP30):
  * the operation part sheet type (`TraceabilityPartSheetOP30Type`), subtype of
  `TraceabilityPartSheetOperationType`, with the operation part sheet variables;
  * the mailbox type (`TraceabilityMailboxOP30Type`), subtype of
  `TraceabilityMailboxType`, adding the operation part sheet as a component
  (`TraceabilityPartSheetOP30`);
  * the machine instance (`OP30`), of `TraceabilityMachineType`, organized in the
  `Objects` folder, with its mailboxes as components of the operation mailbox type
  (e.g. `TraceabilityMailboxOP30In`).

The instances of the operation namespaces result in the tree shown in the
[example](#example-for-a-production-line).

### Diagrams

Types of the common namespace:

```mermaid
classDiagram
  class TraceabilityMachineType {
    +Boolean TraceabilityHeartbeat
  }
  class TraceabilityMailboxType {
    +Byte TraceabilityRequest
    +Byte TraceabilityResponse
  }
  class TraceabilityPartSheetGeneralType {
    ...general part sheet variables
  }
  class TraceabilityPartSheetOperationType {
    <<abstract>>
  }

  TraceabilityMachineType *-- "1..*" TraceabilityMailboxType : Mailbox (MandatoryPlaceholder)
  TraceabilityMailboxType *-- "1" TraceabilityPartSheetGeneralType : TraceabilityPartSheetGeneral
```

Example of the OP30 operation namespace (names of operation-specific types are
illustrative), with the common namespace types it relies on:

```mermaid
classDiagram
  direction BT

  namespace CommonNamespace {
    class TraceabilityMachineType
    class TraceabilityMailboxType
    class TraceabilityPartSheetOperationType {
      <<abstract>>
    }
  }

  namespace OP30Namespace {
    class OP30 {
      <<Object>>
      +Boolean TraceabilityHeartbeat
    }
    class TraceabilityMailboxOP30Type
    class TraceabilityPartSheetOP30Type {
      ...OP30 part sheet variables
    }
  }

  OP30 ..|> TraceabilityMachineType : HasTypeDefinition
  TraceabilityMailboxOP30Type --|> TraceabilityMailboxType : HasSubtype
  TraceabilityPartSheetOP30Type --|> TraceabilityPartSheetOperationType : HasSubtype
  OP30 *-- TraceabilityMailboxOP30Type : TraceabilityMailboxOP30In
  OP30 *-- TraceabilityMailboxOP30Type : TraceabilityMailboxOP30OkOut
  OP30 *-- TraceabilityMailboxOP30Type : TraceabilityMailboxOP30ScrapOut
  TraceabilityMailboxOP30Type *-- "1" TraceabilityPartSheetOP30Type : TraceabilityPartSheetOP30
```

Legend: links with a filled diamond are `HasComponent` references (labelled with
the BrowseName of the child, the diamond being on the parent side), links with a
hollow triangle are `HasSubtype` references (the triangle pointing to the
supertype), dashed links are `HasTypeDefinition` references.

## Design rationale

### Import in the machines controllers

* A controller (e.g. S7-1500 with TIA Portal) imports a companion specification
as a whole, and the variables of its instances are mapped to the controller
variables. As each machine only imports the common namespace and the namespace
of its operation, it has no instances of other machines to map.
* Controllers can only map the variables of existing instances, they do not
instantiate types. Instances are thus fully expanded in the operation namespaces
nodesets, with all their children.
* When several machines perform the same operation:
  * if they are identical (same object name, same mailboxes), the same operation
  namespace is imported on each of them, each OPC-UA server exposing its own
  instance;
  * otherwise, the operation namespace only defines the types, and each machine
  has an additional namespace, depending on it, which defines its instance. The
  types must not be duplicated in each machine namespace, as types from
  different namespaces are different types.

### BrowseNames

* The children of an instance take the BrowseNames of the type children they are
created from, including the namespace of the type defining them. The request, the
response, the general part sheet and its variables thus have BrowseNames in the
common namespace, identical on all machines, whatever the namespace of the
machine instance. This is what allows resolving nodes by BrowseName.
* The operation part sheet is found by its type rather than by its BrowseName,
which can then be specific to each operation (e.g. `TraceabilityPartSheetOP30`).

### Node identifiers stability

Node identifiers are assigned when generating the nodesets. The generator keeps
the identifiers of the previous generation, so that adding a variable does not
renumber the other nodes: the mappings of the controllers variables remain valid
when importing an updated nodeset. The gateway does not depend on these
identifiers anyway (see below).

## Nodes resolution

Node identifiers of the general part sheet differ between mailboxes and machines,
so they are not used as references. Instead:

* Mailboxes are discovered by browsing the machine object, and the operation part
sheet is found by its type, as their BrowseNames are not known beforehand;
* Nodes with BrowseNames defined by the common namespace types (the heartbeat in
the machine object, the request, the response and the general part sheet in each
mailbox) are resolved with the `TranslateBrowsePathsToNodeIds` service, in a
single request for all the mailboxes of a machine. A missing node gives a
specific error for its path (`BadNoMatch`), revealing an unexpected model;
* The variables of the part sheets are discovered by browsing the part sheets
objects, as all of them are needed;
* At session initialization, a `BrowseName → NodeId` table is built for each mailbox,
then OPC-UA reads and writes keep using numeric node identifiers;
* Cached general part sheet items are keyed by BrowseName (name part only), which
is identical on all machines as defined by the common namespace types.
