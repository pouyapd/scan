use super::{Expression, Identifier, Type};
use serde::Deserialize;

#[derive(Deserialize)]
#[allow(dead_code)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct ConstantDeclaration {
    /// the constant's name, unique among all constants and variables
    pub(crate) name: Identifier,
    /// the constant's type; bounded types must not refer to this constant or
    /// constants declared after this one in the corresponding array
    pub(crate) r#type: Type,
    /// the constant's value, of type type; constant expression that must not refer to this
    /// constant or constants declared after this one in the corresponding array;
    /// if omitted, the constant is a model parameter
    #[serde(default)]
    pub(crate) value: Option<Expression>,
    /// an optional comment
    #[serde(skip)]
    pub(crate) _comment: String,
}
