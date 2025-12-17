"""pgcmp - PostgreSQL Schema Comparison Tool."""

from .db import Database
from .comparison import compare_databases, ComparisonResult
from .report import generate_report
from .analysis import (
    analyze_databases,
    AnalysisResult,
    SchemaAnalysis,
    TableAnalysis,
    ColumnAnalysis,
    TriggerAnalysis,
    IndexAnalysis,
    ConstraintAnalysis,
    FunctionAnalysis,
    ViewAnalysis,
    MaterializedViewAnalysis,
    SummaryRow,
    Action,
)
from .xml_report import generate_xml_report

__all__ = [
    "Database",
    "compare_databases",
    "ComparisonResult",
    "generate_report",
    "analyze_databases",
    "AnalysisResult",
    "SchemaAnalysis",
    "TableAnalysis",
    "ColumnAnalysis",
    "TriggerAnalysis",
    "IndexAnalysis",
    "ConstraintAnalysis",
    "FunctionAnalysis",
    "ViewAnalysis",
    "MaterializedViewAnalysis",
    "SummaryRow",
    "Action",
    "generate_xml_report",
]
