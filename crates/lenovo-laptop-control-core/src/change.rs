#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyMode {
    DryRun,
    Commit,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ChangeReport<T> {
    mode: ApplyMode,
    previous: T,
    requested: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual: Option<T>,
}

impl<T> ChangeReport<T> {
    pub fn dry_run(previous: T, requested: T) -> Self {
        Self {
            mode: ApplyMode::DryRun,
            previous,
            requested,
            actual: None,
        }
    }

    pub fn committed(previous: T, requested: T, actual: T) -> Self {
        Self {
            mode: ApplyMode::Commit,
            previous,
            requested,
            actual: Some(actual),
        }
    }

    #[must_use]
    pub const fn mode(&self) -> ApplyMode {
        self.mode
    }

    #[must_use]
    pub const fn previous(&self) -> &T {
        &self.previous
    }

    #[must_use]
    pub const fn requested(&self) -> &T {
        &self.requested
    }

    #[must_use]
    pub const fn actual(&self) -> Option<&T> {
        self.actual.as_ref()
    }
}
