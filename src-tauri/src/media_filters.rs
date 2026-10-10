#[derive(serde::Deserialize)]
pub(crate) struct DateRange { pub(crate) field: String, pub(crate) from: Option<i64>, pub(crate) to: Option<i64> }
impl DateRange {
    pub(crate) fn matches(&self, modified: Option<i64>, created: Option<i64>) -> Result<bool, String> {
        if self.from.zip(self.to).is_some_and(|(from, to)| from >= to) { return Err("Invalid date range; no files were deleted.".into()); }
        let value = match self.field.as_str() { "created" => created, "modified" => modified, _ => return Err("Unsupported date field; no files were deleted.".into()) };
        if self.from.is_none() && self.to.is_none() { return Ok(true); }
        Ok(value.is_some_and(|value| self.from.is_none_or(|from| value >= from) && self.to.is_none_or(|to| value < to)))
    }
}
impl Default for DateRange { fn default() -> Self { Self { field: "modified".into(), from: None, to: None } } }
