use super::CodexInstaller;
use crate::error::GitAiError;
use toml::Value;
use toml_edit::visit_mut::{self, VisitMut};
use toml_edit::{DocumentMut, Item, TableLike};

struct ExpandTables;

impl VisitMut for ExpandTables {
    fn visit_item_mut(&mut self, item: &mut Item) {
        *item = match std::mem::take(item).into_table() {
            Ok(table) => Item::Table(table),
            Err(item) => match item.into_array_of_tables() {
                Ok(array) => Item::ArrayOfTables(array),
                Err(item) => item,
            },
        };
        visit_mut::visit_item_mut(self, item);
    }
}

pub(crate) fn update_config(source: &str, desired: &Value) -> Result<String, GitAiError> {
    let before = CodexInstaller::parse_config_toml(source)?;
    if equivalent(&before, desired) {
        return Ok(source.to_owned());
    }
    if !desired.is_table() {
        return Err(invalid_edit("config root must be a table"));
    }
    let mut document: DocumentMut = source
        .parse()
        .map_err(|e: toml_edit::TomlError| invalid_edit(e.to_string()))?;
    let mut replacement =
        toml_edit::ser::to_document(desired).map_err(|e| invalid_edit(e.to_string()))?;
    ExpandTables.visit_document_mut(&mut replacement);
    sync_table(
        document.as_table_mut(),
        replacement.as_table(),
        as_table(&before)?,
        as_table(desired)?,
    )?;
    let mut result = document.to_string();
    if source.starts_with('\u{feff}') && !result.starts_with('\u{feff}') {
        result.insert(0, '\u{feff}');
    }
    if source.contains("\r\n") && !source.replace("\r\n", "").contains('\n') {
        result = result.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    validate_config(&result, desired)?;
    Ok(result)
}

fn validate_config(result: &str, desired: &Value) -> Result<(), GitAiError> {
    // The existing transformation owns the intended semantics; the editing
    // library owns syntax. Validate their agreement before the atomic write.
    if !equivalent(&CodexInstaller::parse_config_toml(result)?, desired) {
        return Err(invalid_edit(
            "edited config does not match the intended configuration",
        ));
    }
    Ok(())
}

fn invalid_edit(message: impl Into<String>) -> GitAiError {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("Cannot preserve Codex config.toml: {}", message.into()),
    )
    .into()
}

fn mismatch() -> GitAiError {
    invalid_edit("parsed and editable config structures disagree")
}

fn as_table(value: &Value) -> Result<&toml::Table, GitAiError> {
    value.as_table().ok_or_else(mismatch)
}

fn as_array(value: &Value) -> Result<&[Value], GitAiError> {
    value.as_array().map(Vec::as_slice).ok_or_else(mismatch)
}

fn prefix_comments(decor: &toml_edit::Decor) -> &str {
    decor
        .prefix()
        .and_then(|prefix| prefix.as_str())
        .filter(|prefix| prefix.contains('#'))
        .unwrap_or_default()
}

fn decor_comments(decor: &toml_edit::Decor) -> String {
    [decor.prefix(), decor.suffix()]
        .into_iter()
        .flatten()
        .filter_map(|text| text.as_str())
        .filter(|text| text.contains('#'))
        .map(|text| match text.ends_with('\n') {
            true => text.to_owned(),
            false => format!("{text}\n"),
        })
        .collect()
}

fn prepend_prefix(decor: &mut toml_edit::Decor, comments: &str) {
    let prefix = decor
        .prefix()
        .and_then(|prefix| prefix.as_str())
        .unwrap_or_default();
    decor.set_prefix(format!("{comments}{prefix}"));
}

/// Comment lines above a removed key stay where they were by moving onto the
/// next remaining key of the same table; the removed line's own trailing
/// comment annotated only that line and is dropped with it.
fn prepend_comments(table: &mut dyn TableLike, key: &str, comments: &str) {
    match table.get_mut(key) {
        Some(Item::Table(table)) => prepend_prefix(table.decor_mut(), comments),
        Some(Item::ArrayOfTables(array)) => {
            if let Some(table) = array.get_mut(0) {
                prepend_prefix(table.decor_mut(), comments);
            }
        }
        Some(Item::Value(_)) => {
            if let Some(mut key) = table.key_mut(key) {
                prepend_prefix(key.leaf_decor_mut(), comments);
            }
        }
        _ => {}
    }
}

fn sync_table(
    table: &mut dyn TableLike,
    replacement: &dyn TableLike,
    before: &toml::Table,
    after: &toml::Table,
) -> Result<(), GitAiError> {
    for (key, value) in after {
        let new = replacement.get(key).ok_or_else(mismatch)?;
        match (table.get_mut(key), before.get(key)) {
            (Some(item), Some(old)) => sync_item(item, new, old, value)?,
            _ => {
                table.insert(key, new.clone());
            }
        }
    }
    let keys: Vec<String> = table.iter().map(|(key, _)| key.to_owned()).collect();
    let mut carried = String::new();
    for key in keys {
        if after.contains_key(&key) {
            if !carried.is_empty() {
                prepend_comments(table, &key, &std::mem::take(&mut carried));
            }
        } else if let Some(removed) = table.key(&key) {
            carried.push_str(prefix_comments(removed.leaf_decor()));
            table.remove(&key);
        }
    }
    Ok(())
}

fn sync_item(
    item: &mut Item,
    replacement: &Item,
    before: &Value,
    after: &Value,
) -> Result<(), GitAiError> {
    if equivalent(before, after) {
        return Ok(());
    }
    match (item, replacement) {
        (Item::Table(old), Item::Table(new)) => {
            sync_table(old, new, as_table(before)?, as_table(after)?)
        }
        (Item::ArrayOfTables(old), Item::ArrayOfTables(new)) => {
            let (before, after) = (as_array(before)?, as_array(after)?);
            let mut original: Vec<_> = std::mem::take(old).into_iter().map(Some).collect();
            for (index, matched) in match_items(before, after).into_iter().enumerate() {
                let new = new.get(index).ok_or_else(mismatch)?;
                let mut table = matched
                    .and_then(|i| original[i].take())
                    .unwrap_or_else(|| new.clone());
                if let Some(i) = matched {
                    sync_table(
                        &mut table,
                        new,
                        as_table(&before[i])?,
                        as_table(&after[index])?,
                    )?;
                }
                old.push(table);
            }
            Ok(())
        }
        (Item::Value(old), new) => {
            let new = new.clone().into_value().map_err(|_| mismatch())?;
            sync_value(old, &new, before, after)
        }
        (old, new) => {
            *old = new.clone();
            Ok(())
        }
    }
}

fn sync_value(
    value: &mut toml_edit::Value,
    replacement: &toml_edit::Value,
    before: &Value,
    after: &Value,
) -> Result<(), GitAiError> {
    if equivalent(before, after) {
        return Ok(());
    }
    match (&mut *value, replacement) {
        (toml_edit::Value::InlineTable(old), toml_edit::Value::InlineTable(new)) => {
            sync_table(old, new, as_table(before)?, as_table(after)?)?;
        }
        (toml_edit::Value::Array(old), toml_edit::Value::Array(new)) => {
            let (before, after) = (as_array(before)?, as_array(after)?);
            let matches = match_items(before, after);
            for index in (0..old.len()).rev() {
                if !matches.contains(&Some(index)) {
                    let removed_comments = decor_comments(old.remove(index).decor());
                    if !removed_comments.is_empty() {
                        if let Some(next) = old.get_mut(index) {
                            prepend_prefix(next.decor_mut(), &removed_comments);
                        } else {
                            old.set_trailing(format!(
                                "{removed_comments}{}",
                                old.trailing().as_str().unwrap_or_default()
                            ));
                        }
                    }
                }
            }
            let retained = old.len();
            let mut next = 0;
            for (index, matched) in matches.into_iter().enumerate() {
                let new = new.get(index).ok_or_else(mismatch)?;
                if let Some(i) = matched {
                    let old = old.get_mut(next).ok_or_else(mismatch)?;
                    sync_value(old, new, &before[i], &after[index])?;
                    next += 1;
                } else {
                    let mut value = new.clone();
                    if old.len() == retained && retained > 0 {
                        let trailing = old.trailing().as_str().unwrap_or_default().to_owned();
                        if trailing.contains('#') {
                            value.decor_mut().set_prefix(trailing);
                            old.set_trailing("");
                        }
                    }
                    old.push_formatted(value);
                }
            }
        }
        (old, new) => {
            let decor = old.decor().clone();
            *old = new.clone();
            *old.decor_mut() = decor;
        }
    }
    Ok(())
}

fn match_items(before: &[Value], after: &[Value]) -> Vec<Option<usize>> {
    let mut next = 0;
    after
        .iter()
        .map(|desired| {
            let index = (next..before.len()).find(|index| {
                equivalent(&before[*index], desired) || same_hook_group(&before[*index], desired)
            });
            if let Some(index) = index {
                next = index + 1;
            }
            index
        })
        .collect()
}

/// The install transformation adds `hooks = []` to a matcher-only group, so a
/// missing `hooks` key is the same group as an empty one.
fn group_hooks(group: &toml::Table) -> Option<&[Value]> {
    match group.get("hooks") {
        None => Some(&[]),
        Some(Value::Array(hooks)) => Some(hooks),
        Some(_) => None,
    }
}

fn same_hook_group(before: &Value, after: &Value) -> bool {
    let (Some(before), Some(after)) = (before.as_table(), after.as_table()) else {
        return false;
    };
    let (Some(old), Some(new)) = (group_hooks(before), group_hooks(after)) else {
        return false;
    };
    let settings = |group: &toml::Table| group.keys().filter(|key| *key != "hooks").count();
    let custom = |hook: &&Value| {
        !hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(CodexInstaller::is_git_ai_codex_command)
    };
    let old_custom: Vec<_> = old.iter().filter(custom).collect();
    let new_custom: Vec<_> = new.iter().filter(custom).collect();
    settings(before) == settings(after)
        && before
            .iter()
            .filter(|(key, _)| *key != "hooks")
            .all(|(key, value)| after.get(key).is_some_and(|other| equivalent(value, other)))
        && old_custom.len() == new_custom.len()
        && old_custom
            .iter()
            .zip(new_custom)
            .all(|(a, b)| equivalent(a, b))
}

fn equivalent(before: &Value, after: &Value) -> bool {
    match (before, after) {
        // TOML accepts NaN; ordinary float equality would reject every edit to
        // a document containing an otherwise untouched NaN setting.
        (Value::Float(a), Value::Float(b)) => a == b || (a.is_nan() && b.is_nan()),
        (Value::Table(a), Value::Table(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| equivalent(value, other)))
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equivalent(a, b))
        }
        _ => before == after,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_semantically_mismatched_edits() {
        let desired = toml::from_str("model = 'intended'").unwrap();
        assert!(super::validate_config("model = 'unintended'", &desired).is_err());
    }
}
