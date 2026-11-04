//! The execution switches the host applies at process start. The storage is a
//! Settings source owned by the App Server; this port has no settings or file
//! dependency.
use rutis::host::BoxFuture;
use tokio_util::sync::CancellationToken;

pub const SOURCE: &str = "execution";

pub mod prompt;

/// The document form of the `execution` settings source, and the switches the
/// host reads at process start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSettings {
    pub sandbox: bool,
    pub network: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ExecutionSettingsError {
    #[error("execution settings: {0}")]
    Failed(String),
}

pub trait ExecutionSettingsSource: Send + Sync + 'static {
    fn read(
        &self,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<ExecutionSettings, ExecutionSettingsError>>;
}

pub struct ExecutionSettingsHost(pub std::sync::Arc<dyn ExecutionSettingsSource>);

impl ExecutionSettingsHost {
    /// Switches that never change, for an assembly that stores none, such as an
    /// evaluation or the architecture graph. The product's source is the App
    /// Server's `ExecutionSettings` part (ADR 011 D1).
    pub fn fixed(settings: ExecutionSettings) -> Self {
        struct Fixed(ExecutionSettings);
        impl ExecutionSettingsSource for Fixed {
            fn read(
                &self,
                _cancel: CancellationToken,
            ) -> BoxFuture<'_, Result<ExecutionSettings, ExecutionSettingsError>> {
                Box::pin(async move { Ok(self.0) })
            }
        }
        Self(std::sync::Arc::new(Fixed(settings)))
    }
}
