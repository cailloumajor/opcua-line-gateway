use std::path::Path;
use std::{fs, io};

use askama::Template;
use serde::Deserialize;
use thiserror::Error;
use urn::Urn;

/// Represents errors that can be encountered converting TOML to Model Design.
#[derive(Debug, Error)]
pub enum Toml2ModelDesignError {
    #[error("bad provided namespace URN: {0}")]
    NamespaceUrn(urn::Error),
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
    /// The namespace URN.
    ns_urn: Urn,
    /// The list of object types.
    object_types: Vec<ObjectType>,
}

/// Convert ObjectType descriptions from TOML files in input directory to UA Model Design,
/// provided the path to the input directory and the namespace URN to use in the generated
/// contents.
pub fn toml2modeldesign<P>(
    input_dir: &P,
    namespace_urn: &str,
) -> Result<String, Toml2ModelDesignError>
where
    P: AsRef<Path>,
{
    let read_input_dir =
        fs::read_dir(input_dir).map_err(Toml2ModelDesignError::InputDirIterator)?;

    let ns_urn: Urn = namespace_urn
        .parse()
        .map_err(Toml2ModelDesignError::NamespaceUrn)?;

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
        ns_urn,
        object_types,
    };

    model_design
        .render()
        .map_err(Toml2ModelDesignError::TemplateRender)
}
