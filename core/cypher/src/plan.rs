/// Plan IR — unified plan representation.
///
/// Two variants: external (cyrs_plan, production-grade) and internal
/// (our lightweight PlanIR, fallback). The executor handles both
/// through the same [`execute`] entry point.
///
/// Default is [`Plan::External`] when cyrs_plan lowering succeeds.
pub use cyrs_hir;

use cyrs_plan::{self, ReadOp, VarId, WriteOp};

/// Unified plan: one type for both execution paths.
#[derive(Debug, Clone)]
pub enum Plan {
    /// Production path: cyrs_plan logical operators.
    External(ExternalPlan),
    /// Fallback: our lightweight PlanIR.
    Internal(PlanIR),
}

/// cyrs_plan operator arena.
#[derive(Debug, Clone)]
pub struct ExternalPlan {
    pub ops: Vec<ReadOp>,
    pub write_ops: Vec<WriteOp>,
    pub var_map: Vec<(VarId, String)>,
}

impl Plan {
    /// Preferred factory: cyrs_plan via cyrs_hir pipeline.
    pub fn from_cyrs(input: &str) -> Result<Self, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err("empty query".to_string());
        }

        let result = cyrs_hir::parse_to_hir(trimmed);
        if !result.syntax_errors.is_empty() {
            return Err(format!("parse error: {}", result.syntax_errors[0]));
        }

        let mut hir = result.hir;
        crate::parser::resolve_names(&mut hir);

        let plan =
            cyrs_plan::lower::lower_statement(&hir).map_err(|e| format!("plan error: {e}"))?;

        let var_map: Vec<(VarId, String)> = plan
            .var_map
            .iter()
            .map(|(pid, hid)| (*pid, hid.0.to_string()))
            .collect();

        Ok(Plan::External(ExternalPlan {
            ops: plan.ops,
            write_ops: plan.write_ops,
            var_map,
        }))
    }

    /// Fallback: our lightweight PlanIR.
    pub fn from_internal(input: &str) -> Result<Self, String> {
        crate::parser::parse_query(input).map(Plan::Internal)
    }
}

// ── Lightweight PlanIR types (fallback) ────────────────────────────────────

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct PlanIR {
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone)]
pub enum Clause {
    Match(MatchClause),
    OptionalMatch(MatchClause),
    Where(WhereClause),
    With(WithClause),
    Return(ReturnClause),
    Create(CreateClause),
}

#[derive(Debug, Clone)]
pub struct MatchClause {
    pub node: NodePattern,
    pub relationship: Option<RelPattern>,
    pub target: Option<NodePattern>,
}

#[derive(Debug, Clone)]
pub struct NodePattern {
    pub variable: Option<String>,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RelPattern {
    pub variable: Option<String>,
    pub types: Vec<String>,
    pub direction: Direction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Outgoing,
    Incoming,
    Both,
}

#[derive(Debug, Clone)]
pub struct WhereClause {
    pub field_eq: Vec<(FieldRef, FieldRef)>,
    pub not_exists: Option<NotExistsPattern>,
    pub comparisons: Vec<Comparison>,
}

#[derive(Debug, Clone)]
pub struct NotExistsPattern {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct Comparison {
    pub field: FieldRef,
    pub op: CompareOp,
    pub value: CompareValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompareOp {
    Eq,
    Ne,
    Gt,
    Lt,
    Gte,
    Lte,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompareValue {
    Int(i64),
    Float(f64),
    Str(String),
    Field(FieldRef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldRef {
    pub variable: String,
    pub property: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WithClause {
    pub items: Vec<WithItem>,
    pub where_clause: Option<WhereClause>,
}

#[derive(Debug, Clone)]
pub enum WithItem {
    Var(String),
    Aggregate(AggregateFn, String),
}

#[derive(Debug, Clone)]
pub enum AggregateFn {
    Count(String),
}

#[derive(Debug, Clone)]
pub struct ReturnClause {
    pub items: Vec<ReturnItem>,
}

#[derive(Debug, Clone)]
pub struct ReturnItem {
    pub property: Option<String>,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateClause {
    pub nodes: Vec<(NodePattern, Vec<(String, PropertyValue)>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyValue {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}
