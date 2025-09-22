#![cfg(feature = "surreal")]

use crate::errors::{StorageError, StorageResult};
use crate::model::Sort;
use crate::spi::query::NamedArgs;
use serde_json::Value;

const DEFAULT_PREFIX: &str = "filter";

pub fn append_where(target: &mut String, clause: &str) {
    if clause.trim().is_empty() {
        return;
    }
    if !target.trim().is_empty() {
        target.push_str(" AND ");
    }
    target.push('(');
    target.push_str(clause);
    target.push(')');
}

pub fn build_filter_clause(
    filter: &Value,
    params: &mut NamedArgs,
    prefix: &str,
) -> StorageResult<String> {
    if filter.is_null() {
        return Ok(String::new());
    }
    let mut counter = 0usize;
    build_filter_recursive(filter, params, sanitize_prefix(prefix), &mut counter)
}

fn build_filter_recursive(
    filter: &Value,
    params: &mut NamedArgs,
    prefix: String,
    counter: &mut usize,
) -> StorageResult<String> {
    let obj = match filter.as_object() {
        Some(obj) if !obj.is_empty() => obj,
        Some(_) => return Ok(String::new()),
        None => {
            return Err(StorageError::schema(
                "repository filter must be a JSON object",
            ))
        }
    };

    let mut parts = Vec::new();
    for (key, value) in obj {
        if is_op(key, "and") {
            let clauses = expect_array("and", value)?
                .iter()
                .map(|v| build_filter_recursive(v, params, prefix.clone(), counter))
                .collect::<StorageResult<Vec<_>>>()?;
            let clauses: Vec<_> = clauses.into_iter().filter(|c| !c.is_empty()).collect();
            if !clauses.is_empty() {
                parts.push(format!("({})", clauses.join(" AND ")));
            }
        } else if is_op(key, "or") {
            let clauses = expect_array("or", value)?
                .iter()
                .map(|v| build_filter_recursive(v, params, prefix.clone(), counter))
                .collect::<StorageResult<Vec<_>>>()?;
            let clauses: Vec<_> = clauses.into_iter().filter(|c| !c.is_empty()).collect();
            if !clauses.is_empty() {
                parts.push(format!("({})", clauses.join(" OR ")));
            }
        } else if is_op(key, "not") {
            let clause = build_filter_recursive(value, params, prefix.clone(), counter)?;
            if !clause.is_empty() {
                parts.push(format!("NOT ({})", clause));
            }
        } else {
            parts.push(build_field_clause(
                key,
                value,
                params,
                prefix.clone(),
                counter,
            )?);
        }
    }

    Ok(parts.join(" AND "))
}

fn build_field_clause(
    field: &str,
    value: &Value,
    params: &mut NamedArgs,
    prefix: String,
    counter: &mut usize,
) -> StorageResult<String> {
    validate_identifier(field)?;
    if let Some(obj) = value.as_object() {
        if obj.is_empty() {
            return Ok(String::new());
        }
        let mut fragments = Vec::new();
        for (op, operand) in obj {
            let clause = if is_op(op, "eq") {
                build_simple_compare(field, "=", operand, params, &prefix, counter)?
            } else if is_op(op, "ne") {
                build_simple_compare(field, "!=", operand, params, &prefix, counter)?
            } else if is_op(op, "gt") {
                build_simple_compare(field, ">", operand, params, &prefix, counter)?
            } else if is_op(op, "gte") {
                build_simple_compare(field, ">=", operand, params, &prefix, counter)?
            } else if is_op(op, "lt") {
                build_simple_compare(field, "<", operand, params, &prefix, counter)?
            } else if is_op(op, "lte") {
                build_simple_compare(field, "<=", operand, params, &prefix, counter)?
            } else if is_op(op, "in") {
                build_in_clause(field, operand, params, &prefix, counter)?
            } else if is_op(op, "nin") {
                let inner = build_in_clause(field, operand, params, &prefix, counter)?;
                format!("NOT ({inner})")
            } else if is_op(op, "contains") {
                build_contains_clause(field, operand, params, &prefix, counter)?
            } else {
                return Err(StorageError::schema(format!(
                    "unsupported filter operator '{op}'"
                )));
            };
            if !clause.is_empty() {
                fragments.push(clause);
            }
        }
        Ok(fragments.join(" AND "))
    } else {
        build_simple_compare(field, "=", value, params, &prefix, counter)
    }
}

fn build_simple_compare(
    field: &str,
    op: &str,
    operand: &Value,
    params: &mut NamedArgs,
    prefix: &str,
    counter: &mut usize,
) -> StorageResult<String> {
    let param = next_param(prefix, counter);
    params.insert(param.clone(), operand.clone());
    let placeholder = format!("{}{}", '$', param);
    Ok(format!("{field} {op} {placeholder}"))
}

fn build_in_clause(
    field: &str,
    operand: &Value,
    params: &mut NamedArgs,
    prefix: &str,
    counter: &mut usize,
) -> StorageResult<String> {
    if !operand.is_array() {
        return Err(StorageError::schema(format!(
            "{}{} operand must be an array",
            '$', "in"
        )));
    }
    let param = next_param(prefix, counter);
    params.insert(param.clone(), operand.clone());
    Ok(format!("{field} IN {}{}", '$', param))
}

fn build_contains_clause(
    field: &str,
    operand: &Value,
    params: &mut NamedArgs,
    prefix: &str,
    counter: &mut usize,
) -> StorageResult<String> {
    let param = next_param(prefix, counter);
    params.insert(param.clone(), operand.clone());
    Ok(format!("{field} CONTAINS {}{}", '$', param))
}

pub fn build_sort_clause(sorts: &[Sort]) -> StorageResult<String> {
    if sorts.is_empty() {
        return Ok(String::new());
    }
    let mut clauses = Vec::new();
    for sort in sorts {
        let field = sort.field.trim();
        validate_identifier(field)?;
        let direction = if sort.asc { "ASC" } else { "DESC" };
        clauses.push(format!("{field} {direction}"));
    }
    Ok(format!(" ORDER BY {}", clauses.join(", ")))
}

fn sanitize_prefix(prefix: &str) -> String {
    let pref = if prefix.trim().is_empty() {
        DEFAULT_PREFIX
    } else {
        prefix
    };
    pref.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn next_param(prefix: &str, counter: &mut usize) -> String {
    let name = format!("{}_{}", prefix, *counter);
    *counter += 1;
    name
}

fn expect_array<'a>(name: &str, value: &'a Value) -> StorageResult<&'a Vec<Value>> {
    value.as_array().ok_or_else(|| {
        let mut label = String::with_capacity(name.len() + 1);
        label.push('$');
        label.push_str(name);
        StorageError::schema(format!("{label} operator expects an array"))
    })
}

fn validate_identifier(input: &str) -> StorageResult<()> {
    if input.is_empty()
        || !input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return Err(StorageError::schema(format!(
            "invalid field identifier '{input}'"
        )));
    }
    Ok(())
}

fn is_op(actual: &str, expected: &str) -> bool {
    actual.len() == expected.len() + 1
        && actual.as_bytes().first() == Some(&b'$')
        && &actual.as_bytes()[1..] == expected.as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    #[test]
    fn filter_eq_builds_clause() {
        let mut params = NamedArgs::default();
        let clause = build_filter_clause(
            &Value::Object({
                let mut map = Map::new();
                map.insert("name".into(), Value::String("alice".into()));
                map
            }),
            &mut params,
            "t",
        )
        .unwrap();
        assert_eq!(clause, format!("name = {}{}", '$', "t_0"));
        assert_eq!(params.get("t_0").unwrap(), &Value::String("alice".into()));
    }

    #[test]
    fn filter_with_and_or() {
        let mut params = NamedArgs::default();
        let filter = Value::Object({
            let mut root = Map::new();
            let mut clauses = Vec::new();

            let mut status = Map::new();
            status.insert(
                format!("{}{}", '$', "in"),
                Value::Array(vec![
                    Value::String("ready".into()),
                    Value::String("pending".into()),
                ]),
            );
            clauses.push(Value::Object({
                let mut entry = Map::new();
                entry.insert("status".into(), Value::Object(status));
                entry
            }));

            let mut or_list = Vec::new();
            let mut gte = Map::new();
            gte.insert(format!("{}{}", '$', "gte"), Value::from(2));
            or_list.push(Value::Object({
                let mut entry = Map::new();
                entry.insert("ver".into(), Value::Object(gte));
                entry
            }));
            let mut lte = Map::new();
            lte.insert(format!("{}{}", '$', "lte"), Value::from(0));
            or_list.push(Value::Object({
                let mut entry = Map::new();
                entry.insert("ver".into(), Value::Object(lte));
                entry
            }));
            clauses.push(Value::Object({
                let mut entry = Map::new();
                entry.insert(format!("{}{}", '$', "or"), Value::Array(or_list));
                entry
            }));

            root.insert(format!("{}{}", '$', "and"), Value::Array(clauses));
            root
        });

        let clause = build_filter_clause(&filter, &mut params, "s").unwrap();
        assert!(clause.contains("status IN"));
        assert!(clause.contains("ver >="));
    }

    #[test]
    fn sort_clause_valid() {
        let sorts = vec![Sort::ascending("created_at"), Sort::descending("ver")];
        let clause = build_sort_clause(&sorts).unwrap();
        assert_eq!(clause, " ORDER BY created_at ASC, ver DESC");
    }

    #[test]
    fn invalid_identifier_fails() {
        let mut params = NamedArgs::default();
        let mut filter = Map::new();
        filter.insert("na-me".into(), Value::String("oops".into()));
        let err = build_filter_clause(&Value::Object(filter), &mut params, "p").unwrap_err();
        assert!(err.to_string().contains("invalid field identifier"));
    }
}
