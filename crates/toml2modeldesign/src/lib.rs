use std::path::Path;
use std::{env, fs, io};

use askama::Template;
use serde::Deserialize;
use thiserror::Error;

/// Represents errors that can be encountered converting TOML to Model Design.
#[derive(Debug, Error)]
pub enum Toml2ModelDesignError {
    #[error("error creating iterator over input directory members: {0}")]
    InputDirIterator(io::Error),
    #[error("error getting input directory name")]
    InputDirName,
    #[error("error getting input directory entry: {0}")]
    InputDirEntry(io::Error),
    #[error("error reading input directory file: {0}")]
    InputFileRead(io::Error),
    #[error("input directory file deserialization error: {0}")]
    InputFileDeserialize(toml::de::Error),
    #[error("error rendering Model Design template: {0}")]
    TemplateRender(askama::Error),
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

/// Convert ObjectType descriptions from TOML files in input directory to UA Model Design,
/// provided the path to the input directory and the URN namespace to use in the generated
/// contents.
pub fn toml2modeldesign<P>(
    input_dir: &P,
    urn_namespace: &str,
) -> Result<String, Toml2ModelDesignError>
where
    P: AsRef<Path>,
{
    let read_input_dir =
        fs::read_dir(input_dir).map_err(Toml2ModelDesignError::InputDirIterator)?;

    // URN NSS (namespace-specific string).
    let urn_nss = input_dir
        .as_ref()
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(Toml2ModelDesignError::InputDirName)?;
    // OPC-UA namespace URN.
    let namespace = format!("urn:{urn_namespace}:{urn_nss}");

    let mut object_types = Vec::new();

    for entry in read_input_dir {
        let dir_entry = entry.map_err(Toml2ModelDesignError::InputDirEntry)?;
        let entry_path = dir_entry.path();

        if !entry_path.is_file() {
            continue;
        }

        let input_file_contents =
            fs::read_to_string(&entry_path).map_err(Toml2ModelDesignError::InputFileRead)?;
        let object_type: ObjectType = toml::from_str(&input_file_contents)
            .map_err(Toml2ModelDesignError::InputFileDeserialize)?;

        object_types.push(object_type);
    }

    let model_design = ModelDesign {
        namespace,
        ns_prefix: urn_nss.to_string(),
        object_types,
    };

    model_design
        .render()
        .map_err(Toml2ModelDesignError::TemplateRender)
}
