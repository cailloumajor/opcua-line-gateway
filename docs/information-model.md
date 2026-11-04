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

All the variables of the model (heartbeat, request, response and part sheets
variables) are data variables, i.e. components (`HasComponent` references) of
`BaseDataVariableType`, rather than properties, as they hold the contents of their
parent object and not characteristics of it. The part sheets have no properties.

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

* All the nodes of a machine are discovered by browsing recursively the machine
object, following `HasComponent` references: the results hold, for each node
found, its BrowseName, its node class and its type definition;
* Nodes with BrowseNames defined by the common namespace types (the heartbeat in
the machine object, the request, the response and the general part sheet in each
mailbox) are identified by their BrowseName. A missing node gives a specific
error, naming the parent node and the expected BrowseName, revealing an
unexpected model;
* Mailboxes and operation part sheets are identified by their type, as their
BrowseNames are not known beforehand;
* The variables of the part sheets are all the variables found below the part
sheets objects, as all of them are needed;
* At session initialization, a `BrowseName → NodeId` table is built for each mailbox,
then OPC-UA reads and writes keep using numeric node identifiers;
* Cached general part sheet items are keyed by BrowseName (name part only), which
is identical on all machines as defined by the common namespace types.

Summary of how each kind of node is identified in the browse results:

| Nodes to find | BrowseName known beforehand | Identified by |
| --- | --- | --- |
| Heartbeat of a machine | Yes, in the common namespace | BrowseName |
| Mailboxes of a machine | No (`<Mailbox>` placeholder) | Type definition |
| Request, response and general part sheet of a mailbox | Yes, in the common namespace | BrowseName |
| Operation part sheet of a mailbox | No (specific to each operation) | Type definition |
| Variables of the part sheets | All of them are needed, not a fixed list | Node class (`Variable`) |

The `TranslateBrowsePathsToNodeIds` service is not used: it would only resolve
the nodes with BrowseNames known beforehand, in an additional request, and its
results only hold node identifiers, without BrowseNames nor types. Browsing
already returns all of them.

### Resolution sequence

The `Browse` service only returns the references of the nodes it is given, one
level at a time. The recursion is done by the client, with the `Browser` of
`async-opcua`: at each level, it browses all the nodes found at the previous
level in a single request, and handles the continuation points (`BrowseNext`).
Its policy follows `HasComponent` references (subtypes included), to nodes of
`Object` and `Variable` classes, and only descends into objects, so the
recursion stops at the part sheets variables.

The nodes of a machine are thus resolved with four requests, whatever the number
of mailboxes: three by the browser, and one to check the types found:

1. `Browse` of the machine object: heartbeat and mailboxes;
2. `Browse` of all the mailboxes: request, response, general part sheet and
operation part sheet of each mailbox;
3. `Browse` of all the part sheets: their variables;
4. `Browse` of the inverse `HasSubtype` references of the distinct type
definitions of the mailboxes and operation part sheets found, to get their
supertypes (see [Types check](#types-check)).

For example, with the three mailboxes of OP30:

| # | Nodes browsed | Nodes found |
| --- | --- | --- |
| 1 | `OP30` | `TraceabilityHeartbeat`, `TraceabilityMailboxOP30In`, `TraceabilityMailboxOP30OkOut`, `TraceabilityMailboxOP30ScrapOut` |
| 2 | The three mailboxes | For each mailbox: `TraceabilityRequest`, `TraceabilityResponse`, `TraceabilityPartSheetGeneral`, `TraceabilityPartSheetOP30` |
| 3 | The six part sheets | Their variables |
| 4 | `TraceabilityMailboxOP30Type`, `TraceabilityPartSheetOP30Type` | Their supertypes: `TraceabilityMailboxType`, `TraceabilityPartSheetOperationType` |

The browser collects the results in a single structure: the references between
the nodes found (parent, child and reference type), and the description of each
node (BrowseName, node class and type definition). Each mailbox is then
initialized in its own routine, from its part of these results, without sending
any request itself.

The results are walked down from the machine object, following the references
from each node to its children:

1. The children of the machine object whose type derives from
`TraceabilityMailboxType` are the mailboxes, each one given to its own
initialization routine;
2. The children of a mailbox are its request, response and general part sheet
(identified by BrowseName), and its operation part sheet (identified by its type,
deriving from `TraceabilityPartSheetOperationType`);
3. The children of a part sheet are its variables.

Walking down from each mailbox, the nodes found always belong to it, without
having to match them afterwards. No filtering on the reference type is needed:
the browser only followed `HasComponent` references, so the children recorded
are only components. As instances are fully expanded in the nodesets, each
mailbox has its own nodes, with distinct node identifiers, so a part sheet
variable has a single parent.

### Types check

The browser results do not hold the types hierarchy: the browser only followed
`HasComponent` references from the machine object, and never browsed the types.
The type definition of a node found is its concrete type, from the operation
namespace (e.g. `TraceabilityMailboxOP30Type`), whose node identifier is not known
beforehand, rather than the type of the common namespace it derives from.

The types are thus checked after the browser, with a single request:

1. Collect the distinct type definitions of the objects found whose BrowseName is
not known beforehand: the objects of the machine object, and the objects of the
mailboxes other than the general part sheet. There are usually two types per
machine (one mailbox type and one operation part sheet type), whatever the number
of mailboxes;
2. `Browse` the inverse `HasSubtype` references of these types, which return
their direct supertype, with its BrowseName;
3. Keep as mailboxes the children of the machine object whose type has
`<common>:TraceabilityMailboxType` as supertype, and as operation part sheet the
child of a mailbox whose type has `<common>:TraceabilityPartSheetOperationType`
as supertype, where `<common>` is the index of the common namespace, known when
connecting.

A single level of the types hierarchy is enough, as the operation types are
direct subtypes of the common types. The supertypes are compared by BrowseName,
as the node identifiers of the common namespace types are not used either.

An object of the machine object that is not a mailbox, or a mailbox without
exactly one operation part sheet, reveals an unexpected model.

### Points to consider

* As all the children are components, the browsing follows `HasComponent`
references, matching the model exactly: a node modelled differently (e.g. as a
property) is not found, revealing an unexpected model;
* Servers may limit the number of nodes per request (`OperationLimits` of the
server capabilities, e.g. `MaxNodesPerBrowse`). The limits of the controllers are
not known yet: they can be read when connecting and given to the browser, which
splits its requests accordingly (`max_nodes_per_request`).
