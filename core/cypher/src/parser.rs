/// Cypher query parser using `cyrs-syntax`.
///
/// Parses a Cypher query string into [`PlanIR`] by walking the
/// lossless CST produced by [`cyrs_syntax::parse`].

use cyrs_syntax::{SyntaxKind, SyntaxNode, parse};
use crate::plan::*;

/// Parse a Cypher query string into [`PlanIR`].
pub fn parse_query(input: &str) -> Result<PlanIR, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty query".to_string());
    }

    let parsed = parse(trimmed);
    let root = parsed.syntax();
    let mut clauses = Vec::new();

    for statement in root.children() {
        if statement.kind() != SyntaxKind::STATEMENT {
            continue;
        }
        for child in statement.children() {
            match child.kind() {
                SyntaxKind::MATCH_CLAUSE => {
                    let (match_clauses, match_children) = parse_match(&child, false)?;
                    clauses.extend(match_clauses);
                    clauses.extend(match_children);
                }
                SyntaxKind::OPTIONAL_MATCH_CLAUSE => {
                    let (match_clauses, match_children) = parse_match(&child, true)?;
                    clauses.extend(match_clauses);
                    clauses.extend(match_children);
                }
                SyntaxKind::WITH_CLAUSE => {
                    clauses.push(Clause::With(parse_with(&child)?));
                }
                SyntaxKind::RETURN_CLAUSE => {
                    clauses.push(Clause::Return(parse_return(&child)?));
                }
                SyntaxKind::CREATE_CLAUSE => {
                    clauses.push(Clause::Create(parse_create(&child)?));
                }
                SyntaxKind::WHERE_CLAUSE => {
                    clauses.push(Clause::Where(parse_where(&child)?));
                }
                _ => {}
            }
        }
    }

    if clauses.is_empty() {
        if parsed.errors().is_empty() {
            return Err("no clauses parsed".to_string());
        }
        return Err(format!("parse error: {}", parsed.errors()[0]));
    }

    Ok(PlanIR { clauses })
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn token_texts(node: &SyntaxNode) -> Vec<String> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| !t.kind().is_trivia())
        .map(|t| t.text().to_string())
        .collect()
}

fn first_child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|c| c.kind() == kind)
}

fn collect_all_tokens(node: &SyntaxNode) -> Vec<String> {
    let mut result = Vec::new();
    for child_or_token in node.children_with_tokens() {
        match child_or_token {
            cyrs_syntax::SyntaxElement::Node(n) => {
                result.extend(collect_all_tokens(&n));
            }
            cyrs_syntax::SyntaxElement::Token(t) => {
                if !t.kind().is_trivia() {
                    result.push(t.text().to_string());
                }
            }
        }
    }
    result
}

fn node_variable(node: &SyntaxNode) -> Option<String> {
    let name = node.children()
        .find(|c| c.kind() == SyntaxKind::NAME)?;
    token_texts(&name).into_iter().next()
}

fn node_labels(node: &SyntaxNode) -> Vec<String> {
    node.children()
        .filter(|c| c.kind() == SyntaxKind::LABEL_EXPR)
        .filter_map(|c| {
            let texts = token_texts(&c);
            texts.get(1).cloned()
        })
        .collect()
}

// ── Clause parsers ─────────────────────────────────────────────────────────

fn parse_match(node: &SyntaxNode, is_optional: bool) -> Result<(Vec<Clause>, Vec<Clause>), String> {
    let pattern = first_child(node, SyntaxKind::PATTERN)
        .ok_or_else(|| "MATCH without pattern".to_string())?;

    let (node_pat, rel, target) = parse_pattern(&pattern)?;

    let match_clause = MatchClause {
        node: node_pat,
        relationship: rel,
        target,
    };

    let main_clause = if is_optional {
        Clause::OptionalMatch(match_clause)
    } else {
        Clause::Match(match_clause)
    };

    // Extra clauses found inside MATCH_CLAUSE (e.g. WHERE_CLAUSE)
    let mut extra = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::WHERE_CLAUSE => {
                extra.push(Clause::Where(parse_where(&child)?));
            }
            _ => {}
        }
    }

    Ok((vec![main_clause], extra))
}

fn parse_pattern(node: &SyntaxNode) -> Result<(NodePattern, Option<RelPattern>, Option<NodePattern>), String> {
    let part = first_child(node, SyntaxKind::PATTERN_PART)
        .unwrap_or(node.clone());

    let node_patterns: Vec<SyntaxNode> = part.children()
        .filter(|c| c.kind() == SyntaxKind::NODE_PATTERN)
        .collect();

    if node_patterns.is_empty() {
        return Err("expected node pattern".to_string());
    }

    let source = parse_node_pattern(&node_patterns[0])?;

    let rel = part.children()
        .find(|c| c.kind() == SyntaxKind::REL_PATTERN)
        .map(|c| parse_rel_pattern(&c))
        .transpose()?;

    let target = if rel.is_some() && node_patterns.len() > 1 {
        Some(parse_node_pattern(&node_patterns[1])?)
    } else {
        None
    };

    Ok((source, rel, target))
}

fn parse_node_pattern(node: &SyntaxNode) -> Result<NodePattern, String> {
    let variable = node_variable(node);
    let labels = node_labels(node);
    Ok(NodePattern { variable, labels })
}

fn parse_rel_pattern(node: &SyntaxNode) -> Result<RelPattern, String> {
    let detail = first_child(node, SyntaxKind::REL_DETAIL);
    let variable = detail.as_ref()
        .and_then(|d| first_child(d, SyntaxKind::NAME))
        .and_then(|n| token_texts(&n).into_iter().next());
    let types: Vec<String> = detail.iter()
        .flat_map(|d| d.children().filter(|c| c.kind() == SyntaxKind::REL_TYPE_EXPR))
        .filter_map(|c| token_texts(&c).into_iter().next())
        .collect();
    let texts = token_texts(node);
    let direction = if texts.contains(&"->".to_string()) {
        Direction::Outgoing
    } else {
        let all = texts.join(" ");
        if all.contains("<-") {
            Direction::Incoming
        } else {
            Direction::Both
        }
    };
    Ok(RelPattern { variable, types, direction })
}

fn parse_where(node: &SyntaxNode) -> Result<WhereClause, String> {
    let mut comparisons = Vec::new();

    for child in node.children() {
        if child.kind() == SyntaxKind::BINARY_EXPR {
            parse_comparison_from_expr(&child, &mut comparisons)?;
        } else if !child.kind().is_keyword() && !child.kind().is_trivia() {
            parse_simple_comparison(&child, &mut comparisons)?;
        }
    }

    Ok(WhereClause {
        field_eq: Vec::new(),
        not_exists: None,
        comparisons,
    })
}

fn parse_comparison_from_expr(node: &SyntaxNode, comparisons: &mut Vec<Comparison>) -> Result<(), String> {
    let all_tokens: Vec<String> = collect_all_tokens(node);
    let texts: Vec<&str> = all_tokens.iter().map(|s| s.as_str()).collect();

    if let Some(pos) = texts.iter().position(|t| {
        matches!(*t, "=" | "!=" | "<>" | "<" | ">" | "<=" | ">=")
    }) {
        let lhs: Vec<&str> = texts[..pos].to_vec();
        let op = texts[pos];
        let rhs: Vec<&str> = texts[pos + 1..].to_vec();

        if !lhs.is_empty() && !rhs.is_empty() {
            if let (Ok(field), Ok(value)) = (
                parse_field_from_slice(&lhs),
                parse_value_from_slice(&rhs),
            ) {
                let op_enum = match op {
                    "=" => CompareOp::Eq,
                    "!=" | "<>" => CompareOp::Ne,
                    "<" => CompareOp::Lt,
                    ">" => CompareOp::Gt,
                    "<=" => CompareOp::Lte,
                    ">=" => CompareOp::Gte,
                    _ => return Ok(()),
                };
                comparisons.push(Comparison { field, op: op_enum, value });
            }
        }
    }

    Ok(())
}

fn parse_simple_comparison(node: &SyntaxNode, comparisons: &mut Vec<Comparison>) -> Result<(), String> {
    parse_comparison_from_expr(node, comparisons)
}

fn parse_field_from_slice(tokens: &[&str]) -> Result<FieldRef, String> {
    if tokens.is_empty() {
        return Err("empty field".to_string());
    }
    let variable = tokens[0].to_string();
    let property = if tokens.len() >= 3 && tokens[1] == "." {
        Some(tokens[2].to_string())
    } else {
        None
    };
    Ok(FieldRef { variable, property })
}

fn parse_value_from_slice(tokens: &[&str]) -> Result<CompareValue, String> {
    if tokens.is_empty() {
        return Err("empty value".to_string());
    }

    let first = tokens[0];
    let joined = tokens.join(" ");

    if let Ok(n) = joined.parse::<i64>() {
        return Ok(CompareValue::Int(n));
    }
    if let Ok(f) = joined.parse::<f64>() {
        return Ok(CompareValue::Float(f));
    }

    if first.starts_with('"') || first.starts_with('\'') {
        let s = joined.trim_matches('"').trim_matches('\'').to_string();
        return Ok(CompareValue::Str(s));
    }

    if first.chars().all(|c| c.is_alphanumeric() || c == '_') && !first.is_empty() {
        let field = parse_field_from_slice(tokens)?;
        return Ok(CompareValue::Field(field));
    }

    Ok(CompareValue::Str(joined))
}

fn parse_with(node: &SyntaxNode) -> Result<WithClause, String> {
    let mut items = Vec::new();

    let body = first_child(node, SyntaxKind::RETURN_BODY);
    let items_node = body.as_ref()
        .and_then(|b| first_child(b, SyntaxKind::RETURN_ITEMS))
        .or_else(|| first_child(node, SyntaxKind::RETURN_ITEMS));

    if let Some(items_node) = items_node {
        for item in items_node.children() {
            if item.kind() == SyntaxKind::RETURN_ITEM {
                items.push(parse_with_item(&item)?);
            }
        }
    }

    let where_clause = body.as_ref()
        .and_then(|b| first_child(b, SyntaxKind::WHERE_CLAUSE))
        .map(|c| parse_where(&c))
        .transpose()?;

    Ok(WithClause { items, where_clause })
}

fn parse_with_item(node: &SyntaxNode) -> Result<WithItem, String> {
    let tokens = collect_all_tokens(node);
    let texts: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();

    let first_lower = texts.first().map(|s| s.to_lowercase());
    if matches!(first_lower.as_deref(), Some("count" | "sum" | "avg" | "min" | "max")) {
        let func_name = texts[0].to_lowercase();
        let var = texts.get(1)
            .map(|s| s.trim_matches('(').trim_matches(')').to_string())
            .unwrap_or_default();
        let alias = texts.iter()
            .position(|t| t.eq_ignore_ascii_case("as"))
            .and_then(|pos| texts.get(pos + 1))
            .map(|s| s.to_string())
            .unwrap_or_else(|| var.clone());
        match func_name.as_str() {
            "count" => return Ok(WithItem::Aggregate(AggregateFn::Count(var), alias)),
            _ => {}
        }
    }

    let var = texts.first().map(|s| s.to_string()).unwrap_or_default();
    let alias = texts.iter()
        .position(|t| t.eq_ignore_ascii_case("as"))
        .and_then(|pos| texts.get(pos + 1))
        .map(|s| s.to_string());
    Ok(WithItem::Var(alias.unwrap_or(var)))
}

fn parse_return(node: &SyntaxNode) -> Result<ReturnClause, String> {
    let mut items = Vec::new();

    let body = first_child(node, SyntaxKind::RETURN_BODY);
    let items_node = body.as_ref()
        .and_then(|b| first_child(b, SyntaxKind::RETURN_ITEMS))
        .or_else(|| first_child(node, SyntaxKind::RETURN_ITEMS));

    if let Some(items_node) = items_node {
        for item in items_node.children() {
            if item.kind() == SyntaxKind::RETURN_ITEM {
                items.push(parse_return_item(&item)?);
            }
        }
    }

    Ok(ReturnClause { items })
}

fn parse_return_item(node: &SyntaxNode) -> Result<ReturnItem, String> {
    let tokens = collect_all_tokens(node);
    let texts: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();

    let property = if texts.len() >= 3 && texts[1] == "." {
        Some(texts[2].to_string())
    } else {
        None
    };

    let alias = texts.iter()
        .position(|t| t.eq_ignore_ascii_case("as"))
        .and_then(|pos| texts.get(pos + 1))
        .map(|s| s.to_string());

    Ok(ReturnItem { property, alias })
}

fn parse_create(node: &SyntaxNode) -> Result<CreateClause, String> {
    let mut nodes = Vec::new();

    let pattern = first_child(node, SyntaxKind::PATTERN);
    let part = pattern.as_ref().and_then(|p| first_child(p, SyntaxKind::PATTERN_PART));

    let node_pats: Vec<SyntaxNode> = if let Some(part) = part {
        part.children().filter(|c| c.kind() == SyntaxKind::NODE_PATTERN).collect()
    } else if let Some(p) = pattern {
        p.children().filter(|c| c.kind() == SyntaxKind::NODE_PATTERN).collect()
    } else {
        node.children().filter(|c| c.kind() == SyntaxKind::NODE_PATTERN).collect()
    };

    for np in node_pats {
        let node_pat = parse_node_pattern(&np)?;
        let props = parse_node_properties(&np)?;
        nodes.push((node_pat, props));
    }

    Ok(CreateClause { nodes })
}

fn parse_node_properties(node: &SyntaxNode) -> Result<Vec<(String, PropertyValue)>, String> {
    let props_node = first_child(node, SyntaxKind::PROPERTY_MAP);
    let Some(props) = props_node else {
        return Ok(Vec::new());
    };

    let texts = collect_all_tokens(&props);
    let mut i = 1; // skip "{"
    let mut result = Vec::new();

    while i + 2 < texts.len() {
        let key = texts[i].clone();
        if texts.get(i + 1).map(|s| s.as_str()) != Some(":") {
            i += 1;
            continue;
        }
        let value_str = texts.get(i + 2).cloned().unwrap_or_default();
        let value = match value_str.as_str() {
            "true" => PropertyValue::Bool(true),
            "false" => PropertyValue::Bool(false),
            s if s.starts_with('"') || s.starts_with('\'') => {
                PropertyValue::Str(s.trim_matches('"').trim_matches('\'').to_string())
            }
            s if s.contains('.') => PropertyValue::Float(s.parse().unwrap_or(0.0)),
            s => PropertyValue::Int(s.parse().unwrap_or(0)),
        };
        result.push((key, value));
        i += 3;
        if texts.get(i).map(|s| s.as_str()) == Some(",") {
            i += 1;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_match() {
        let plan = parse_query("MATCH (c:Concept) RETURN c").unwrap();
        assert_eq!(plan.clauses.len(), 2, "clauses: {:?}", plan.clauses);
        match &plan.clauses[0] {
            Clause::Match(m) => {
                assert_eq!(m.node.variable, Some("c".into()));
                assert!(m.node.labels.contains(&"Concept".to_string()));
            }
            _ => panic!("expected Match"),
        }
    }

    #[test]
    fn test_parse_match_relationship() {
        let input = "MATCH (a)-[r]->(b) RETURN a, b";
        let plan = parse_query(input).unwrap();
        match &plan.clauses[0] {
            Clause::Match(m) => {
                assert!(m.relationship.is_some(), "expected relationship");
                assert!(m.target.is_some(), "expected target node");
            }
            _ => panic!("expected Match"),
        }
    }

    #[test]
    fn test_parse_optional_match() {
        let input = "OPTIONAL MATCH (c)-[r]-() WITH c, count(r) AS rc WHERE rc = 0";
        let plan = parse_query(input).unwrap();
        assert_eq!(plan.clauses.len(), 2);
        match &plan.clauses[0] {
            Clause::OptionalMatch(_) => {}
            _ => panic!("expected OptionalMatch"),
        }
        match &plan.clauses[1] {
            Clause::With(w) => {
                assert!(!w.items.is_empty());
                assert!(w.where_clause.is_some(), "WHERE in WITH not parsed");
            }
            _ => panic!("expected With"),
        }
    }

    #[test]
    fn test_parse_create() {
        let input = "CREATE (n:Person {name: \"Alice\", age: 30})";
        let plan = parse_query(input).unwrap();
        match &plan.clauses[0] {
            Clause::Create(c) => {
                assert_eq!(c.nodes.len(), 1);
                assert_eq!(c.nodes[0].0.variable, Some("n".into()),
                    "variable is {:?}", c.nodes[0].0.variable);
                assert!(c.nodes[0].0.labels.contains(&"Person".to_string()));
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn test_parse_empty() {
        let result = parse_query("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_where_condition() {
        let input = "MATCH (c:Concept) WHERE c.value > 5 RETURN c.name";
        let plan = parse_query(input).unwrap();
        assert_eq!(plan.clauses.len(), 3, "expected 3 clauses, got {:?}", plan.clauses);
        match &plan.clauses[1] {
            Clause::Where(wc) => {
                assert!(!wc.comparisons.is_empty(), "expected comparisons");
            }
            _ => panic!("expected Where as second clause"),
        }
    }
}
