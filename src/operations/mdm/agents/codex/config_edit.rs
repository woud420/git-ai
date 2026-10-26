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

pub(super) fn update_config(source: &str, desired: &Value) -> Result<String, GitAiError> {
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
    let mut comments = String::new();
    sync_table(
        document.as_table_mut(),
        replacement.as_table(),
        before.as_table().unwrap(),
        desired.as_table().unwrap(),
        &mut comments,
    );
    let trailing = format!(
        "{}{}",
        document.trailing().as_str().unwrap_or_default(),
        comments
    );
    document.set_trailing(trailing);
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

fn keep_comments(decor: &toml_edit::Decor, comments: &mut String) {
    for text in [decor.prefix(), decor.suffix()]
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str())
    {
        if text.contains('#') {
            comments.push_str(text);
            if !text.ends_with('\n') {
                comments.push('\n');
            }
        }
    }
}

fn sync_table(
    table: &mut dyn TableLike,
    replacement: &dyn TableLike,
    before: &toml::Table,
    after: &toml::Table,
    comments: &mut String,
) {
    for key in before.keys().filter(|key| !after.contains_key(*key)) {
        keep_comments(table.key(key).unwrap().leaf_decor(), comments);
        if let Some(item) = table.remove(key)
            && let Some(value) = item.as_value()
        {
            keep_comments(value.decor(), comments);
        }
    }
    for (key, value) in after {
        if let Some(item) = table.get_mut(key) {
            sync_item(
                item,
                replacement.get(key).unwrap(),
                &before[key],
                value,
                comments,
            );
        } else {
            table.insert(key, replacement.get(key).unwrap().clone());
        }
    }
}

fn sync_item(
    item: &mut Item,
    replacement: &Item,
    before: &Value,
    after: &Value,
    comments: &mut String,
) {
    if equivalent(before, after) {
        return;
    }
    match (item, replacement) {
        (Item::Table(old), Item::Table(new)) => sync_table(
            old,
            new,
            before.as_table().unwrap(),
            after.as_table().unwrap(),
            comments,
        ),
        (Item::ArrayOfTables(old), Item::ArrayOfTables(new)) => {
            let mut original: Vec<_> = std::mem::take(old).into_iter().map(Some).collect();
            for (index, matched) in
                match_items(before.as_array().unwrap(), after.as_array().unwrap())
                    .into_iter()
                    .enumerate()
            {
                let mut table = matched
                    .and_then(|i| original[i].take())
                    .unwrap_or_else(|| new.get(index).unwrap().clone());
                if let Some(i) = matched {
                    sync_table(
                        &mut table,
                        new.get(index).unwrap(),
                        before[i].as_table().unwrap(),
                        after[index].as_table().unwrap(),
                        comments,
                    );
                }
                old.push(table);
            }
        }
        (Item::Value(old), new) => sync_value(
            old,
            &new.clone().into_value().unwrap(),
            before,
            after,
            comments,
        ),
        (old, new) => *old = new.clone(),
    }
}

fn sync_value(
    value: &mut toml_edit::Value,
    replacement: &toml_edit::Value,
    before: &Value,
    after: &Value,
    comments: &mut String,
) {
    if equivalent(before, after) {
        return;
    }
    match (&mut *value, replacement) {
        (toml_edit::Value::InlineTable(old), toml_edit::Value::InlineTable(new)) => {
            sync_table(
                old,
                new,
                before.as_table().unwrap(),
                after.as_table().unwrap(),
                comments,
            );
        }
        (toml_edit::Value::Array(old), toml_edit::Value::Array(new)) => {
            let matches = match_items(before.as_array().unwrap(), after.as_array().unwrap());
            for index in (0..old.len()).rev() {
                if !matches.contains(&Some(index)) {
                    let removed = old.remove(index);
                    let mut removed_comments = String::new();
                    keep_comments(removed.decor(), &mut removed_comments);
                    if !removed_comments.is_empty() {
                        if let Some(next) = old.get_mut(index) {
                            let prefix = next
                                .decor()
                                .prefix()
                                .and_then(|s| s.as_str())
                                .unwrap_or_default();
                            let prefix = format!("{removed_comments}{prefix}");
                            next.decor_mut().set_prefix(prefix);
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
                if let Some(i) = matched {
                    sync_value(
                        old.get_mut(next).unwrap(),
                        new.get(index).unwrap(),
                        &before[i],
                        &after[index],
                        comments,
                    );
                    next += 1;
                } else {
                    let mut value = new.get(index).unwrap().clone();
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

fn same_hook_group(before: &Value, after: &Value) -> bool {
    let (Some(before), Some(after)) = (before.as_table(), after.as_table()) else {
        return false;
    };
    let (Some(Value::Array(old)), Some(Value::Array(new))) =
        (before.get("hooks"), after.get("hooks"))
    else {
        return false;
    };
    let custom = |hook: &&Value| {
        !hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(CodexInstaller::is_git_ai_codex_command)
    };
    let old_custom: Vec<_> = old.iter().filter(custom).collect();
    let new_custom: Vec<_> = new.iter().filter(custom).collect();
    before.len() == after.len()
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
