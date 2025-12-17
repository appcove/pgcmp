pub mod writer;

pub use writer::{generate_schema_file, group_by_schema, write_objects_by_schema};

/// Types of schema objects we track
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectType {
    Table,
    View,
    MaterializedView,
    Function,
    Index,
    Constraint,
    Trigger,
    Sequence,
}

/// A single schema object (table, view, function, etc.)
#[derive(Debug, Clone)]
pub struct SchemaObject {
    pub schema_name: String,
    pub object_name: String,
    pub object_type: ObjectType,
    pub ddl: String,
}
