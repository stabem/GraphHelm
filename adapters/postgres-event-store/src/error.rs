use graphhelm_events::EventRepositoryError;

pub(crate) fn storage(_: sqlx::Error) -> EventRepositoryError {
    EventRepositoryError::Storage
}

pub(crate) fn decode(_: impl std::fmt::Debug) -> EventRepositoryError {
    EventRepositoryError::Integrity
}
