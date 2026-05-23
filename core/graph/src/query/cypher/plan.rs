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
        super::parser::resolve_names(&mut hir);

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
        super::parser::parse_query(input).map(Plan::Internal)
    }

    /// Attempt to translate this plan into a ColdQuery for DuckDB cold storage.
    /// Returns `None` if the plan involves graph patterns (relationships) that
    /// cannot be represented as a simple tabular scan.
    pub fn to_cold_query(&self) -> Option<ColdQuery> {
        match self {
            Plan::External(ext) => ext.to_cold_query(),
            Plan::Internal(ir) => ir.to_cold_query(),
        }
    }
}

// ── Translation to ColdQuery ───────────────────────────────────────────────

impl ExternalPlan {
    /// Translate an external plan to a ColdQuery.
    /// Only supports simple Source → Filter → Project → Limit chains.
    fn to_cold_query(&self) -> Option<ColdQuery> {
        use cyrs_plan::ReadOp;

        let mut label: Option<String> = None;
        let mut filters: Vec<ColdFilter> = Vec::new();
        let mut projections: Vec<String> = Vec::new();
        let mut limit: Option<usize> = None;
        let mut distinct = false;

        for op in &self.ops {
            match op {
                ReadOp::Source { label: ls, .. } => {
                    // Only single-label sources are cold-eligible.
                    let labels = ls.as_ref()?;
                    let first = labels.0.first()?;
                    if !matches!(first.as_str(), "Fact" | "Intent" | "Hint") {
                        return None;
                    }
                    label = Some(first.to_string());
                }
                ReadOp::Filter { predicate, .. } => {
                    let cf = expr_to_cold_filter(predicate)?;
                    filters.push(cf);
                }
                ReadOp::Project { items, .. } => {
                    for item in items {
                        match &item.expr {
                            cyrs_plan::Expr::Prop { prop, .. } => {
                                // f.fact_id → "fact_id" (DuckDB view column name)
                                projections.push(prop.to_string());
                            }
                            // Var means "return whole node" → leave empty (SELECT *)
                            cyrs_plan::Expr::Var(_) => {}
                            _ => {}
                        }
                    }
                }
                ReadOp::Limit { count, .. } => {
                    // count is an Expr; extract integer literal if present.
                    if let cyrs_plan::Expr::Int(n) = count {
                        limit = Some(*n as usize);
                    }
                }
                ReadOp::Distinct { .. } => {
                    distinct = true;
                }
                // Any other op → not cold-eligible.
                _ => return None,
            }
        }

        let mut cq = ColdQuery::new(label?);
        cq.filters = filters;
        cq.projections = projections;
        cq.limit = limit;
        cq.distinct = distinct;
        Some(cq)
    }
}

impl PlanIR {
    /// Translate an internal PlanIR to a ColdQuery.
    /// Only supports a single Match (no relationship) + optional Where + optional Return.
    fn to_cold_query(&self) -> Option<ColdQuery> {
        let mut label: Option<String> = None;
        let mut filters: Vec<ColdFilter> = Vec::new();
        let mut projections: Vec<String> = Vec::new();
        for clause in &self.clauses {
            match clause {
                Clause::Match(m) => {
                    // Must be a single-label, relationship-free match.
                    if m.relationship.is_some() || m.target.is_some() {
                        return None;
                    }
                    let first = m.node.labels.first()?;
                    if !matches!(first.as_str(), "Fact" | "Intent" | "Hint") {
                        return None;
                    }
                    label = Some(first.clone());
                }
                Clause::Where(w) => {
                    // Translate field_eq comparisons.
                    for (left, right) in &w.field_eq {
                        let (field, value) = field_eq_to_filter(left, right)?;
                        filters.push(ColdFilter {
                            field,
                            op: "Eq".into(),
                            value,
                        });
                    }
                    // Translate comparisons.
                    for cmp in &w.comparisons {
                        let field = cmp.field.property.as_deref()?.to_string();
                        let op = match cmp.op {
                            CompareOp::Eq => "Eq",
                            CompareOp::Ne => "Ne",
                            CompareOp::Gt => "Gt",
                            CompareOp::Lt => "Lt",
                            CompareOp::Gte => "Gte",
                            CompareOp::Lte => "Lte",
                        };
                        let value = compare_value_to_json(&cmp.value)?;
                        filters.push(ColdFilter {
                            field,
                            op: op.into(),
                            value,
                        });
                    }
                }
                Clause::Return(r) => {
                    for item in &r.items {
                        if let Some(prop) = &item.property {
                            projections.push(prop.clone());
                        }
                    }
                }
                Clause::OptionalMatch(_) => return None,
                Clause::With(_) => return None,
                Clause::Create(_) => return None,
            }
        }

        let mut cq = ColdQuery::new(label?);
        cq.filters = filters;
        cq.projections = projections;
        Some(cq)
    }
}

/// Translate a field_eq pair (FieldRef, FieldRef) to (field_name, json_value).
/// The right side must be a literal value, not a field reference.
fn field_eq_to_filter(left: &FieldRef, right: &FieldRef) -> Option<(String, Value)> {
    // One side must have no variable (literal) or be a simple string literal.
    if right.property.is_none() && right.variable.is_empty() {
        // Right side is a literal.
        let field = format!("{}.{}", left.variable, left.property.as_ref()?);
        Some((field, Value::String(right.variable.clone())))
    } else if left.property.is_none() && left.variable.is_empty() {
        // Left side is a literal.
        let field = format!("{}.{}", right.variable, right.property.as_ref()?);
        Some((field, Value::String(left.variable.clone())))
    } else {
        // Both are field references — not a filterable pattern for cold storage.
        None
    }
}

/// Translate a CompareValue to a serde_json::Value.
fn compare_value_to_json(v: &CompareValue) -> Option<Value> {
    match v {
        CompareValue::Int(n) => Some(Value::Number((*n).into())),
        CompareValue::Float(f) => {
            let n = serde_json::Number::from_f64(*f)?;
            Some(Value::Number(n))
        }
        CompareValue::Str(s) => Some(Value::String(s.clone())),
        // Parser stores string literals as FieldRef { variable: "value", property: None }.
        CompareValue::Field(field) if field.property.is_none() => {
            Some(Value::String(field.variable.clone()))
        }
        // Actual field references (f.origin) cannot be literal values.
        CompareValue::Field(_) => None,
    }
}

/// Translate a cyrs_plan Expr (BinOp with Prop/Int/Str) to a ColdFilter.
fn expr_to_cold_filter(expr: &cyrs_plan::Expr) -> Option<ColdFilter> {
    use cyrs_plan::Expr;
    match expr {
        Expr::BinOp { op, lhs, rhs } => {
            let field = expr_to_field_name(lhs)?;
            let value = expr_to_json_value(rhs)?;
            let op_str = match op {
                cyrs_plan::BinOp::Eq => "Eq",
                cyrs_plan::BinOp::Neq => "Ne",
                cyrs_plan::BinOp::Gt => "Gt",
                cyrs_plan::BinOp::Lt => "Lt",
                cyrs_plan::BinOp::Ge => "Gte",
                cyrs_plan::BinOp::Le => "Lte",
                _ => return None,
            };
            Some(ColdFilter {
                field,
                op: op_str.into(),
                value,
            })
        }
        _ => None,
    }
}

/// Extract a column name from a cyrs_plan Expr representing a property access.
fn expr_to_field_name(expr: &cyrs_plan::Expr) -> Option<String> {
    use cyrs_plan::Expr;
    match expr {
        Expr::Prop { target, prop } => {
            let var_name = match target.as_ref() {
                Expr::Var(id) => id.0.to_string(),
                _ => return None,
            };
            Some(format!("{}.{}", var_name, prop.as_str()))
        }
        _ => None,
    }
}

/// Extract a JSON value from a cyrs_plan Expr literal.
fn expr_to_json_value(expr: &cyrs_plan::Expr) -> Option<Value> {
    use cyrs_plan::Expr;
    match expr {
        Expr::String(s) => Some(Value::String(s.to_string())),
        Expr::Int(n) => Some(Value::Number((*n).into())),
        Expr::Float(f) => {
            let n = serde_json::Number::from_f64(*f)?;
            Some(Value::Number(n))
        }
        Expr::Bool(b) => Some(Value::Bool(*b)),
        _ => None,
    }
}

// ── Lightweight PlanIR types (fallback) ────────────────────────────────────

use nexus_model::cold_query::{ColdFilter, ColdQuery};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
