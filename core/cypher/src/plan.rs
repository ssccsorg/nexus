/// Minimal Plan IR covering the Cypher subset gap-detector needs.
/// Parsed from cyrs HIR; designed to be swappable if the cyrs dependency
/// is replaced.

use serde::{Deserialize, Serialize};

/// A parsed Cypher query represented as a Plan IR.
#[derive(Debug, Clone)]
pub struct PlanIR {
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone)]
pub enum Clause {
    Match(MatchClause),
    OptionalMatch(MatchClause),
    Where(WhereClause),
    /// WITH c, aggregate AS alias
    With(WithClause),
    Return(ReturnClause),
    Create(CreateClause),
}

/// MATCH / OPTIONAL MATCH
#[derive(Debug, Clone)]
pub struct MatchClause {
    pub node: NodePattern,
    pub relationship: Option<RelPattern>,
    /// Target node of the relationship (if any)
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

#[derive(Debug, Clone)]
pub enum Direction {
    Outgoing,  // -[]->
    Incoming,  // <-[]-
    Both,      // -[]-
}

/// WHERE clause with comparisons.
#[derive(Debug, Clone)]
pub struct WhereClause {
    /// field = field (for joining on shared properties)
    pub field_eq: Vec<(FieldRef, FieldRef)>,
    /// NOT EXISTS pattern  
    pub not_exists: Option<NotExistsPattern>,
    /// Simple label filter (WHERE n.field > value) etc.
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

#[derive(Debug, Clone)]
pub enum CompareOp {
    Eq, Ne, Gt, Lt, Gte, Lte,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompareValue {
    Int(i64),
    Float(f64),
    Str(String),
    /// Reference to another field: `r1 != r2` → CompareValue::Field("r2")
    Field(FieldRef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldRef {
    pub variable: String,
    pub property: Option<String>,
}

/// WITH clause (used for aggregation like count)
#[derive(Debug, Clone)]
pub struct WithClause {
    pub items: Vec<WithItem>,
    pub where_clause: Option<WhereClause>,
}

#[derive(Debug, Clone)]
pub enum WithItem {
    /// c as alias
    Var(String),
    /// count(r) AS rc
    Aggregate(AggregateFn, String),
}

#[derive(Debug, Clone)]
pub enum AggregateFn {
    Count(String),
}

/// RETURN clause
#[derive(Debug, Clone)]
pub struct ReturnClause {
    pub items: Vec<ReturnItem>,
}

#[derive(Debug, Clone)]
pub struct ReturnItem {
    pub property: Option<String>,
    pub alias: Option<String>,
}

/// CREATE clause
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
