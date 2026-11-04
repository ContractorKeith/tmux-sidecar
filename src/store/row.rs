use super::*;
pub(super) fn run_id(id: &str) -> rusqlite::Result<i64> {
    let digits = id
        .strip_prefix('r')
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or(rusqlite::Error::InvalidParameterName(id.into()))?;
    digits
        .parse::<i64>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or(rusqlite::Error::InvalidParameterName(id.into()))
}
pub(super) fn number(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}
pub(super) fn unsigned(r: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = r.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
pub(super) fn enum_name(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .expect("fieldless enum serializes")
        .as_str()
        .expect("enum is a string")
        .into()
}
pub(super) fn enum_column<T: serde::de::DeserializeOwned>(
    r: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let value: String = r.get(index)?;
    serde_json::from_value(value.into()).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}
pub(super) fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRun> {
    Ok(AgentRun {
        id: format!("r{}", r.get::<_, i64>(0)?),
        server_start: unsigned(r, 1)?,
        pane_id: r.get(2)?,
        agent: enum_column(r, 3)?,
        agent_pid: r.get(4)?,
        agent_session_id: r.get(5)?,
        cwd: r.get(6)?,
        project_root: r.get(7)?,
        started_at: unsigned(r, 8)?,
        ended_at: r
            .get::<_, Option<i64>>(9)?
            .map(|v| u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(9, v)))
            .transpose()?,
        state: enum_column(r, 10)?,
        state_since: unsigned(r, 11)?,
        state_source: r.get(12)?,
        read_at: r
            .get::<_, Option<i64>>(13)?
            .map(|v| u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(13, v)))
            .transpose()?,
        last_assistant_text: r.get(14)?,
        transcript_path: r.get(15)?,
    })
}
