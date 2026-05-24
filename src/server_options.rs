use async_lsp::lsp_types::{ConfigurationItem, LSPAny};

/**
    Options for the language server wrapper.
*/
#[derive(Debug, Default, Clone)]
pub struct ServerOptions {
    pub(crate) workspace_diagnostics: WorkspaceDiagnostics,
}

impl ServerOptions {
    /**
        Sets how workspace diagnostics should be exposed by the server.
    */
    #[must_use]
    pub fn with_workspace_diagnostics(
        mut self,
        workspace_diagnostics: impl Into<WorkspaceDiagnostics>,
    ) -> Self {
        self.workspace_diagnostics = workspace_diagnostics.into();
        self
    }
}

/**
    Controls how workspace diagnostics are made available.
*/
#[derive(Debug, Default, Clone)]
pub enum WorkspaceDiagnostics {
    /**
        Do not advertise or handle workspace diagnostics.
    */
    Disabled,
    /**
        Advertise and handle workspace diagnostics.
    */
    #[default]
    Enabled,
    /**
        Advertise workspace diagnostics and toggle them using a setting.
    */
    Configurable(WorkspaceDiagnosticsSetting),
}

impl WorkspaceDiagnostics {
    /**
        Do not advertise or handle workspace diagnostics.
    */
    #[must_use]
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    /**
        Advertise and handle workspace diagnostics.
    */
    #[must_use]
    pub const fn enabled() -> Self {
        Self::Enabled
    }

    /**
        Toggles workspace diagnostics using the given workspace setting.
    */
    #[must_use]
    pub fn setting(key: impl Into<ConfigurationKey>) -> WorkspaceDiagnosticsSetting {
        WorkspaceDiagnosticsSetting {
            key: key.into(),
            default_enabled: true,
        }
    }
}

/**
    Runtime setting for workspace diagnostics.
*/
#[derive(Debug, Clone)]
pub struct WorkspaceDiagnosticsSetting {
    pub(crate) key: ConfigurationKey,
    pub(crate) default_enabled: bool,
}

impl WorkspaceDiagnosticsSetting {
    /**
        Sets the initial value used before client configuration is available.
    */
    #[must_use]
    pub fn with_default_enabled(mut self, yes: bool) -> Self {
        self.default_enabled = yes;
        self
    }
}

impl From<WorkspaceDiagnosticsSetting> for WorkspaceDiagnostics {
    fn from(setting: WorkspaceDiagnosticsSetting) -> Self {
        Self::Configurable(setting)
    }
}

/**
    Key for a workspace configuration setting.
*/
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationKey {
    section: String,
    path: Vec<String>,
}

impl ConfigurationKey {
    /**
        Creates a configuration key from an LSP configuration section.
    */
    #[must_use]
    pub fn new(section: impl Into<String>) -> Self {
        Self {
            section: section.into(),
            path: Vec::new(),
        }
    }

    /**
        Looks up a nested boolean value inside the configuration section.
    */
    #[must_use]
    pub fn with_path(mut self, path: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.path = path.into_iter().map(Into::into).collect();
        self
    }

    pub(crate) fn item(&self) -> ConfigurationItem {
        ConfigurationItem {
            scope_uri: None,
            section: Some(self.section.clone()),
        }
    }

    pub(crate) fn value(&self, settings: &LSPAny) -> Option<bool> {
        if self.path.is_empty() {
            return settings
                .as_bool()
                .or_else(|| settings.get(&self.section).and_then(LSPAny::as_bool))
                .or_else(|| value_at(settings, self.section.split('.')));
        }

        value_at(settings, &self.path).or_else(|| {
            settings
                .get(&self.section)
                .and_then(|settings| value_at(settings, &self.path))
        })
    }

    pub(crate) fn section(&self) -> &str {
        &self.section
    }
}

impl From<String> for ConfigurationKey {
    fn from(section: String) -> Self {
        Self::new(section)
    }
}

impl From<&str> for ConfigurationKey {
    fn from(section: &str) -> Self {
        Self::new(section)
    }
}

fn value_at(value: &LSPAny, path: impl IntoIterator<Item = impl AsRef<str>>) -> Option<bool> {
    let mut value = value;
    for segment in path {
        value = value.get(segment.as_ref())?;
    }
    value.as_bool()
}

#[cfg(test)]
mod tests {
    use super::ConfigurationKey;

    #[test]
    fn configuration_key_reads_dotted_settings() {
        let key = ConfigurationKey::new("test.workspaceDiagnostics.enabled");

        assert_eq!(
            key.value(&serde_json::json!({
                "test": {
                    "workspaceDiagnostics": {
                        "enabled": true,
                    },
                },
            })),
            Some(true)
        );
        assert_eq!(
            key.value(&serde_json::json!({
                "test.workspaceDiagnostics.enabled": false,
            })),
            Some(false)
        );
        assert_eq!(key.value(&serde_json::json!(true)), Some(true));
    }

    #[test]
    fn configuration_key_reads_section_path_settings() {
        let key = ConfigurationKey::new("test").with_path(["workspaceDiagnostics", "enabled"]);

        assert_eq!(
            key.value(&serde_json::json!({
                "workspaceDiagnostics": {
                    "enabled": true,
                },
            })),
            Some(true)
        );
        assert_eq!(
            key.value(&serde_json::json!({
                "test": {
                    "workspaceDiagnostics": {
                        "enabled": false,
                    },
                },
            })),
            Some(false)
        );
    }
}
