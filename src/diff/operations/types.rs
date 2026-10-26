//! Type operations for schema migrations

use super::OperationKind;

#[derive(Debug, Clone)]
pub enum TypeOperation {
    Create {
        schema: String,
        name: String,
        kind: String,
        definition: String,
    },
    Drop {
        schema: String,
        name: String,
    },
    /// `ALTER TYPE ... ADD VALUE`: Postgres adds one label per statement.
    AddValue {
        schema: String,
        name: String,
        value: String,
        /// The label the new one follows; `None` appends it.
        after: Option<String>,
    },
}

impl TypeOperation {
    pub fn operation_kind(&self) -> OperationKind {
        match self {
            Self::Create { .. } => OperationKind::Create,
            Self::Drop { .. } => OperationKind::Drop,
            Self::AddValue { .. } => OperationKind::Alter,
        }
    }
}
