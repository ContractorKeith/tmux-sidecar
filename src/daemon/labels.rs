use crate::rpc;
pub fn validate(name: &str) -> Result<(), (i32, String)> {
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err((
            rpc::INVALID_PARAMS,
            "names must contain 1–64 characters and no control characters".into(),
        ));
    }
    Ok(())
}
#[derive(Clone)]
pub enum Group {
    Manual(String),
    Project(String),
}
impl Group {
    pub fn parse(value: Option<&str>) -> Result<Option<Self>, (i32, String)> {
        let Some(value) = value else {
            return Ok(None);
        };
        if let Some(root) = value
            .strip_prefix("project:")
            .or_else(|| value.starts_with('/').then_some(value))
        {
            if !std::path::Path::new(root).is_absolute() || root.chars().any(char::is_control) {
                return Err((
                    rpc::INVALID_PARAMS,
                    "project group must be an absolute path".into(),
                ));
            }
            return Ok(Some(Self::Project(root.into())));
        }
        let name = value.strip_prefix("manual:").unwrap_or(value);
        validate(name)?;
        Ok(Some(Self::Manual(name.into())))
    }
    pub fn manual(&self) -> Option<&str> {
        match self {
            Self::Manual(name) => Some(name),
            _ => None,
        }
    }
}
