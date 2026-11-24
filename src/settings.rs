//! Extension settings, stored in the panel's settings table under `dev.caloptreyx.serverimporter`.
use serde::{Deserialize, Serialize};
use shared::{
    State,
    extensions::settings::{
        ExtensionSettings, SettingsDeserializeExt, SettingsDeserializer, SettingsSerializeExt,
        SettingsSerializer,
    },
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Maximum imports running at the same time across the whole panel.
    pub max_concurrent_imports: u32,
    /// Maximum total bytes a single import may transfer, in MiB (0 = unlimited).
    pub max_import_size_mb: u64,
    /// Maximum number of files a single import may transfer (0 = unlimited).
    pub max_import_files: u64,
    /// Timeout for establishing the SSH/SFTP connection, in seconds.
    pub connection_timeout_secs: u32,
    /// Allow connecting to loopback, link-local, or private network addresses.
    /// Off by default to prevent SSRF against the panel host and internal LAN.
    pub allow_private_networks: bool,
    /// Allow users to save encrypted SFTP logins for reuse across their servers.
    #[serde(default = "default_allow_saved_logins")]
    pub allow_saved_logins: bool,
}

fn default_allow_saved_logins() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_concurrent_imports: 3,
            max_import_size_mb: 25_600, // 25 GiB
            max_import_files: 50_000,
            connection_timeout_secs: 20,
            allow_private_networks: false,
            allow_saved_logins: true,
        }
    }
}

fn check<T: PartialOrd + std::fmt::Display>(
    errors: &mut Vec<String>,
    name: &str,
    value: T,
    min: T,
    max: T,
) {
    if value < min || value > max {
        errors.push(format!("{name} must be between {min} and {max}"));
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        check(
            &mut errors,
            "max_concurrent_imports",
            self.max_concurrent_imports,
            1,
            32,
        );
        check(
            &mut errors,
            "max_import_size_mb",
            self.max_import_size_mb,
            0,
            10_485_760, // 10 TiB
        );
        check(
            &mut errors,
            "max_import_files",
            self.max_import_files,
            0,
            5_000_000,
        );
        check(
            &mut errors,
            "connection_timeout_secs",
            self.connection_timeout_secs,
            5,
            120,
        );
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn max_import_bytes(&self) -> Option<u64> {
        (self.max_import_size_mb > 0).then(|| self.max_import_size_mb.saturating_mul(1024 * 1024))
    }

    pub fn max_import_files_limit(&self) -> Option<u64> {
        (self.max_import_files > 0).then_some(self.max_import_files)
    }
}

#[async_trait::async_trait]
impl SettingsSerializeExt for Settings {
    async fn serialize(
        &self,
        serializer: SettingsSerializer,
    ) -> Result<SettingsSerializer, anyhow::Error> {
        Ok(serializer
            .write_serde_setting("max_concurrent_imports", &self.max_concurrent_imports)?
            .write_serde_setting("max_import_size_mb", &self.max_import_size_mb)?
            .write_serde_setting("max_import_files", &self.max_import_files)?
            .write_serde_setting("connection_timeout_secs", &self.connection_timeout_secs)?
            .write_serde_setting("allow_private_networks", &self.allow_private_networks)?
            .write_serde_setting("allow_saved_logins", &self.allow_saved_logins)?)
    }
}

pub struct SettingsDataDeserializer;

#[async_trait::async_trait]
impl SettingsDeserializeExt for SettingsDataDeserializer {
    async fn deserialize_boxed(
        &self,
        deserializer: SettingsDeserializer<'_>,
    ) -> Result<ExtensionSettings, anyhow::Error> {
        let d = Settings::default();
        macro_rules! serde_or {
            ($key:literal, $default:expr) => {
                deserializer.read_serde_setting($key).unwrap_or($default)
            };
        }
        Ok(Box::new(Settings {
            max_concurrent_imports: serde_or!("max_concurrent_imports", d.max_concurrent_imports),
            max_import_size_mb: serde_or!("max_import_size_mb", d.max_import_size_mb),
            max_import_files: serde_or!("max_import_files", d.max_import_files),
            connection_timeout_secs: serde_or!(
                "connection_timeout_secs",
                d.connection_timeout_secs
            ),
            allow_private_networks: serde_or!("allow_private_networks", d.allow_private_networks),
            allow_saved_logins: serde_or!("allow_saved_logins", d.allow_saved_logins),
        }))
    }
}

pub async fn load(state: &State) -> Result<Settings, anyhow::Error> {
    let settings = state.settings.get().await?;
    Ok(settings
        .get_extension_settings::<Settings>(crate::PACKAGE)
        .cloned()
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_bounds_checked() {
        assert!(Settings::default().validate().is_ok());

        let invalid = Settings {
            max_concurrent_imports: 0,
            connection_timeout_secs: 2,
            ..Settings::default()
        };
        assert_eq!(invalid.validate().unwrap_err().len(), 2);
    }

    #[test]
    fn zero_limits_mean_unlimited() {
        let unlimited = Settings {
            max_import_size_mb: 0,
            max_import_files: 0,
            ..Settings::default()
        };
        assert!(unlimited.validate().is_ok());
        assert_eq!(unlimited.max_import_bytes(), None);
        assert_eq!(unlimited.max_import_files_limit(), None);
    }
}
