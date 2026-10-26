use super::CodexInstaller;
use crate::error::GitAiError;
use std::ops::Range;
use toml::de::{DeTable, DeValue};
use toml::{Spanned, Table, Value};

type Node<'a> = Spanned<DeValue<'a>>;

struct Edit {
    range: Range<usize>,
    text: String,
}

struct Context {
    path: Vec<String>,
    offset: usize,
}

struct Editor<'a> {
    source: &'a str,
    newline: &'static str,
    edits: Vec<Edit>,
}

pub(super) fn update_config(source: &str, desired: &Value) -> Result<String, GitAiError> {
    let before = CodexInstaller::parse_config_toml(source)?;
    if equivalent(&before, desired) {
        return Ok(source.to_owned());
    }
    let doc = DeTable::parse(source).map_err(|e| invalid_edit(e.to_string()))?;
    let desired_table = desired
        .as_table()
        .ok_or_else(|| invalid_edit("config root must be a table"))?;
    let mut editor = Editor {
        source,
        newline: if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        },
        edits: Vec::new(),
    };
    editor.table(
        doc.get_ref(),
        before.as_table().unwrap(),
        desired_table,
        &[],
        &Context {
            path: Vec::new(),
            offset: source
                .strip_prefix('\u{feff}')
                .map_or(0, |_| '\u{feff}'.len_utf8()),
        },
    );
    let result = editor.finish()?;
    // Spans describe syntax, not trivia. Never write if an edit changed the
    // meaning or scope of a neighboring setting (notably array-of-table groups).
    if !equivalent(&CodexInstaller::parse_config_toml(&result)?, desired) {
        return Err(invalid_edit(
            "edited config does not match the intended configuration",
        ));
    }
    Ok(result)
}

fn invalid_edit(message: impl Into<String>) -> GitAiError {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("Cannot preserve Codex config.toml: {}", message.into()),
    )
    .into()
}

impl Editor<'_> {
    fn push(&mut self, range: Range<usize>, text: String) {
        self.edits.push(Edit { range, text });
    }

    fn line_end(&self, offset: usize) -> usize {
        self.source[offset..]
            .find('\n')
            .map_or(self.source.len(), |n| offset + n + 1)
    }

    fn insert_lines(&mut self, offset: usize, text: String) {
        let prefix = if offset > 0
            && self.source[..offset] != *"\u{feff}"
            && !self.source[..offset].ends_with('\n')
        {
            self.newline
        } else {
            ""
        };
        self.push(
            offset..offset,
            format!("{prefix}{}", text.replace('\n', self.newline)),
        );
    }

    fn is_header(&self, node: &Node<'_>) -> bool {
        match node.get_ref() {
            DeValue::Table(_) => self.source[node.span()].starts_with('['),
            DeValue::Array(array) => array.first().is_some_and(|first| {
                matches!(first.get_ref(), DeValue::Table(_))
                    && self.source[first.span()].starts_with("[[")
            }),
            _ => false,
        }
    }

    fn table(
        &mut self,
        nodes: &DeTable<'_>,
        before: &Table,
        after: &Table,
        path: &[String],
        context: &Context,
    ) {
        for (key, node) in nodes {
            let name = key.get_ref().as_ref();
            let mut child_path = path.to_vec();
            child_path.push(name.to_owned());
            if let Some(desired) = after.get(name) {
                self.value(node, &before[name], desired, &child_path, context);
            } else {
                self.remove(node);
            }
        }
        for (key, value) in after {
            if before.contains_key(key) {
                continue;
            }
            let mut child_path = path.to_vec();
            child_path.push(key.clone());
            let structured = value.is_table() || table_array(value);
            if structured && context.path == path {
                let offset = if path.is_empty() {
                    self.source.len()
                } else {
                    nodes
                        .values()
                        .map(|node| self.line_end(extent(node)))
                        .max()
                        .unwrap_or(context.offset)
                };
                self.insert_lines(offset, render(&child_path, value, false));
            } else {
                self.insert_lines(
                    context.offset,
                    format!(
                        "{} = {value}\n",
                        key_path(&child_path[context.path.len()..])
                    ),
                );
            }
        }
    }

    fn value(
        &mut self,
        node: &Node<'_>,
        before: &Value,
        after: &Value,
        path: &[String],
        context: &Context,
    ) {
        if equivalent(before, after) {
            return;
        }
        match (node.get_ref(), before, after) {
            (DeValue::Table(nodes), Value::Table(before), Value::Table(after))
                if self.is_header(node) =>
            {
                self.table(
                    nodes,
                    before,
                    after,
                    path,
                    &Context {
                        path: path.to_vec(),
                        offset: self.line_end(node.span().end),
                    },
                );
            }
            (DeValue::Table(nodes), Value::Table(before), Value::Table(after))
                if !self.source[node.span()].starts_with('{') =>
            {
                self.table(nodes, before, after, path, context);
            }
            (DeValue::Table(nodes), Value::Table(before), Value::Table(after)) => {
                self.inline_table(node.span(), nodes, before, after, path, context);
            }
            (DeValue::Array(nodes), Value::Array(before), Value::Array(after)) => {
                let matches = match_items(before, after);
                if self.is_header(node) {
                    for (index, child) in nodes.iter().enumerate() {
                        if let Some((desired_index, _)) = matches
                            .iter()
                            .enumerate()
                            .find(|(_, old)| **old == Some(index))
                        {
                            self.value(child, &before[index], &after[desired_index], path, context);
                        } else {
                            self.remove(child);
                        }
                    }
                    let offset = self.line_end(extent(node));
                    for (desired, matched) in after.iter().zip(matches) {
                        if matched.is_none() {
                            self.insert_lines(offset, render(path, desired, true));
                        }
                    }
                } else {
                    let spans: Vec<_> = nodes.iter().map(Spanned::span).collect();
                    self.delimited(
                        node.span(),
                        &spans,
                        &matches,
                        after.iter().map(ToString::to_string).collect(),
                    );
                    for (desired_index, old_index) in matches.iter().enumerate() {
                        if let Some(index) = old_index {
                            self.value(
                                &nodes[*index],
                                &before[*index],
                                &after[desired_index],
                                path,
                                context,
                            );
                        }
                    }
                }
            }
            _ if self.is_header(node) => {
                self.remove(node);
                self.insert_lines(
                    context.offset,
                    format!("{} = {after}\n", key_path(&path[context.path.len()..])),
                );
            }
            _ => self.push(node.span(), after.to_string()),
        }
    }

    fn inline_table(
        &mut self,
        span: Range<usize>,
        nodes: &DeTable<'_>,
        before: &Table,
        after: &Table,
        path: &[String],
        context: &Context,
    ) {
        let mut entries: Vec<_> = nodes.iter().collect();
        entries.sort_by_key(|(key, _)| key.span().start);
        let spans: Vec<_> = entries
            .iter()
            .map(|(key, value)| key.span().start..extent(value))
            .collect();
        let matches: Vec<_> = after
            .keys()
            .map(|name| {
                entries
                    .iter()
                    .position(|(key, _)| key.get_ref().as_ref() == name)
            })
            .collect();
        let additions = after
            .iter()
            .map(|(key, value)| format!("{} = {value}", key_path(std::slice::from_ref(key))))
            .collect();
        self.delimited(span, &spans, &matches, additions);
        for (key, node) in entries {
            let name = key.get_ref().as_ref();
            if let Some(desired) = after.get(name) {
                let mut child_path = path.to_vec();
                child_path.push(name.to_owned());
                self.value(node, &before[name], desired, &child_path, context);
            }
        }
    }

    fn delimited(
        &mut self,
        container: Range<usize>,
        spans: &[Range<usize>],
        matches: &[Option<usize>],
        rendered: Vec<String>,
    ) {
        let additions: Vec<_> = rendered
            .into_iter()
            .zip(matches)
            .filter_map(|(text, old)| old.is_none().then_some(text))
            .collect();
        let retained: Vec<_> = (0..spans.len())
            .filter(|i| matches.contains(&Some(*i)))
            .collect();
        for (index, span) in spans.iter().enumerate() {
            if retained.contains(&index) {
                continue;
            }
            self.push(span.clone(), String::new());
        }
        // Keep one original separator between surviving entries. All other
        // commas can go; whitespace and comments in the gaps stay verbatim.
        for (index, span) in spans.iter().enumerate() {
            let end = spans
                .get(index + 1)
                .map_or(container.end - 1, |next| next.start);
            let keep = retained.contains(&index)
                && (retained.iter().any(|next| *next > index)
                    || !additions.is_empty()
                    || self.source[container.clone()].starts_with('['));
            if !keep && let Some(comma) = comma_in(self.source, span.end..end) {
                self.push(comma..comma + 1, String::new());
            }
        }
        if !additions.is_empty() {
            if let Some(last) = retained.last() {
                let end = spans
                    .get(last + 1)
                    .map_or(container.end - 1, |next| next.start);
                if comma_in(self.source, spans[*last].end..end).is_none() {
                    self.push(spans[*last].end..spans[*last].end, ",".to_owned());
                }
            }
            // Append after trailing comments, so a custom hook's comment does
            // not become attached to the newly inserted git-ai hook.
            let offset = container.end - 1;
            self.push(offset..offset, format!(" {}", additions.join(", ")));
        }
    }

    fn remove(&mut self, node: &Node<'_>) {
        match node.get_ref() {
            DeValue::Table(children) if !self.source[node.span()].starts_with('{') => {
                if self.is_header(node) {
                    self.remove_line(node.span());
                }
                for child in children.values() {
                    self.remove(child);
                }
            }
            DeValue::Array(children) if self.is_header(node) => {
                for child in children {
                    self.remove(child);
                }
            }
            _ => self.remove_line(node.span()),
        }
    }

    fn remove_line(&mut self, span: Range<usize>) {
        let first_byte = self
            .source
            .strip_prefix('\u{feff}')
            .map_or(0, |_| '\u{feff}'.len_utf8());
        let start = self.source[..span.start]
            .rfind('\n')
            .map_or(first_byte, |n| n + 1);
        let end = self.line_end(span.end);
        let suffix = &self.source[span.end..end];
        if let Some(comment) = suffix.find('#') {
            self.push(start..span.end + comment, String::new());
        } else {
            self.push(start..end, String::new());
        }
    }

    fn finish(mut self) -> Result<String, GitAiError> {
        // Stable ordering also keeps multiple additions at the same offset in
        // the order in which they were generated.
        self.edits
            .sort_by_key(|edit| (edit.range.start, edit.range.end));
        let mut result = String::new();
        let mut cursor = 0;
        for edit in self.edits {
            if edit.range.start < cursor {
                return Err(invalid_edit("overlapping source edits"));
            }
            result.push_str(&self.source[cursor..edit.range.start]);
            result.push_str(&edit.text);
            cursor = edit.range.end;
        }
        result.push_str(&self.source[cursor..]);
        Ok(result)
    }
}

fn extent(node: &Node<'_>) -> usize {
    let children = match node.get_ref() {
        DeValue::Table(table) => table.values().map(extent).max(),
        DeValue::Array(array) => array.iter().map(extent).max(),
        _ => None,
    };
    children.unwrap_or(0).max(node.span().end)
}

fn comma_in(source: &str, range: Range<usize>) -> Option<usize> {
    let mut comment = false;
    for (offset, byte) in source.as_bytes()[range.clone()].iter().enumerate() {
        match byte {
            b'\n' => comment = false,
            b'#' => comment = true,
            b',' if !comment => return Some(range.start + offset),
            _ => {}
        }
    }
    None
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

fn table_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|array| !array.is_empty() && array.iter().all(Value::is_table))
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

fn key_path(path: &[String]) -> String {
    path.iter()
        .map(|key| {
            if !key.is_empty()
                && key
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            {
                key.clone()
            } else {
                Value::String(key.clone()).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn render(path: &[String], value: &Value, array_entry: bool) -> String {
    if let Value::Array(array) = value
        && table_array(value)
    {
        return array
            .iter()
            .map(|value| render(path, value, true))
            .collect();
    }
    if let Value::Table(table) = value {
        let mut text = if array_entry {
            format!("[[{}]]\n", key_path(path))
        } else {
            format!("[{}]\n", key_path(path))
        };
        for (key, value) in table {
            if !value.is_table() && !table_array(value) {
                text.push_str(&format!(
                    "{} = {value}\n",
                    key_path(std::slice::from_ref(key))
                ));
            }
        }
        for (key, value) in table {
            if value.is_table() || table_array(value) {
                let mut child_path = path.to_vec();
                child_path.push(key.clone());
                text.push_str(&render(&child_path, value, false));
            }
        }
        text
    } else {
        format!("{} = {value}\n", key_path(path))
    }
}
