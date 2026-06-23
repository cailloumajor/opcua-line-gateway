use std::path::PathBuf;
use std::{env, fs};

use anyhow::Context as _;
use askama::Template;
use clap::Parser;
use serde::Deserialize;

/// The environment variable that will be read to get the `namespace ID` part of the
/// OPC-UA namespace URN.
const URN_NID_ENV_KEY: &str = "OPCUA_URN_NID";

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The path to the input directory (TOML descriptions of OPC-UA object types, one file for each).
    #[arg(short, long)]
    input: PathBuf,

    /// The path to the output Model Design file.
    #[arg(short, long)]
    output: PathBuf,
}

/// Represents an OPC-UA ObjectType.
#[derive(Deserialize)]
struct ObjectType {
    /// The name of the ObjectType (e.g. MotorType).
    name: String,
    /// The description of the ObjectType.
    description: String,
    /// The list of variables found in the ObjectDesign modelization.
    variable: Vec<Variable>,
}

/// Represents the modelization for a variable member of an ObjectType.
#[derive(Deserialize)]
struct Variable {
    /// The name of the variable.
    name: String,
    /// The description of the variable.
    description: String,
    /// The OPC-UA data type of the variable.
    data_type: String,
    /// A list of [array dimensions] for the variable.
    ///
    /// [array dimensions]: https://reference.opcfoundation.org/specs/OPC-10000-6/5.2.5
    array_dimensions: Option<Vec<i32>>,
}

/// Represents the template for generating ModelDesign file.
#[derive(Template)]
#[template(path = "modeldesign.xml")]
struct ModelDesign {
    /// UA target namespace.
    namespace: String,
    /// UA namespace prefix.
    ns_prefix: String,
    /// The list of object types.
    object_types: Vec<ObjectType>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let urn_nid = env::var(URN_NID_ENV_KEY).with_context(|| {
        format!("Failed to get the URN namespace from {URN_NID_ENV_KEY} environment variable")
    })?;

    let read_input_dir = fs::read_dir(&cli.input)
        .context("Failed to create an iterator over input directory files")?;

    // URN NSS (namespace-specific string).
    let urn_nss = cli
        .input
        .file_name()
        .and_then(|n| n.to_str())
        .context("Failed to get input directory name")?;
    // OPC-UA namespace URN.
    let namespace = format!("urn:{urn_nid}:{urn_nss}");

    let mut object_types = Vec::new();

    for entry in read_input_dir {
        let dir_entry = entry.context("Failed to get input dir file entry")?;
        let entry_path = dir_entry.path();

        if !entry_path.is_file() {
            continue;
        }

        let input_file_contents =
            fs::read_to_string(&entry_path).context("Failed to read input file")?;
        let object_type: ObjectType =
            toml::from_str(&input_file_contents).context("Failed to parse TOML input file")?;

        object_types.push(object_type);
    }

    let model_design = ModelDesign {
        namespace,
        ns_prefix: urn_nss.to_string(),
        object_types,
    };

    let mut out = fs::File::create(&cli.output).context("Failed to open the output file")?;

    model_design
        .write_into(&mut out)
        .context("Failed to write output file")?;

    Ok(())
}
