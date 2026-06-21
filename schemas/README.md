# OPC-UA Schemas Management

This directory holds the OPC-UA schemas to be used by the line gateway.

It contains following subdirectories.

* `source`: schema description in source format (`xslx`);
* `modeldesign`: intermediate representation in [UA Model Design](https://github.com/OPCFoundation/UA-ModelCompiler/blob/master/Opc.Ua.ModelCompiler/UA%20Model%20Design.xsd) format;
* `nodeset`: generated node set in OPC-UA NodeSet2 format.

Only source schema description will be committed to version control, the next
steps being generated.

Schema generation obeys the schema below.

```mermaid
---
config:
    flowchart:
        subGraphTitleMargin:
            bottom: 30
---
flowchart TD
    A@{ shape: document, label: "example.xlsx" }
    B["`xls2modeldesign
    *Rust binary*`"]
    C@{ shape: document, label: "Example.Model.xml" }
    D["Opc.Ua.ModelCompiler"]
    E@{ shape: document, label: "Example.NodeSet2.xml" }
    F["`build.rs
    *async-opcua-codegen*`"]
    G(["Rust types"])

    A --> B --> C --> D --> E --> F --> G

    subgraph make ["Orchestrated by make from build.rs"]
        A
        B
        C
        D
        E
    end
````
