pub mod analysis;
pub mod diff;
mod normalize;
pub mod report;

pub use analysis::{AnalysisResult, analyze_databases, get_postgres_version};
pub use diff::{DiffEntry, DiffStatus, SchemaDiff, compare_schemas};
pub use report::{generate_xml, xml_escape};
