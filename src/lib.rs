use indexmap::IndexMap;
use shared::{
    State,
    extensions::{
        Extension, ExtensionPermissionsBuilder, ExtensionRouteBuilder, ExtensionUpdateInfo,
        settings::ExtensionSettingsDeserializer,
    },
    permissions::PermissionGroup,
};
use std::sync::Arc;

mod db;
mod importer;
mod routes;
mod settings;
mod sftp;

pub const PACKAGE: &str = "dev.caloptreyx.serverimporter";

const LATEST_RELEASE: &str =
    "https://api.github.com/repos/Caloptreyx/Server-Importer/releases/latest";

#[derive(serde::Serialize, serde::Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: String,
}

fn version_from_tag(tag: &str) -> Result<semver::Version, semver::Error> {
    semver::Version::parse(tag.trim_start_matches('v'))
}

#[derive(Default)]
pub struct ExtensionStruct;

#[async_trait::async_trait]
impl Extension for ExtensionStruct {
    async fn initialize(&mut self, state: State) {
        if let Err(err) = db::fail_stale_jobs(&state.database).await {
            tracing::warn!(
                error = %err,
                "Server Importer: could not mark interrupted jobs on startup"
            );
        }
    }

    async fn initialize_router(
        &mut self,
        state: State,
        builder: ExtensionRouteBuilder,
    ) -> ExtensionRouteBuilder {
        builder
            .add_client_server_api_router(|router| {
                router.nest("/server-importer", routes::server::router(&state))
            })
            .add_admin_api_router(|router| {
                router.nest(
                    &format!("/extensions/{PACKAGE}"),
                    routes::admin::router(&state),
                )
            })
    }

    async fn initialize_permissions(
        &mut self,
        _state: State,
        builder: ExtensionPermissionsBuilder,
    ) -> ExtensionPermissionsBuilder {
        builder.add_server_permission_group(
            "server-importer",
            PermissionGroup {
                description: "Permissions for importing server files from another host over SFTP.",
                permissions: IndexMap::from([(
                    "import",
                    "Allows browsing a remote SFTP host and importing files into the server.",
                )]),
            },
        )
    }

    async fn settings_deserializer(&self, _state: State) -> ExtensionSettingsDeserializer {
        Arc::new(settings::SettingsDataDeserializer)
    }

    async fn check_for_updates(
        &self,
        state: State,
        current_version: &semver::Version,
    ) -> Result<Option<ExtensionUpdateInfo>, anyhow::Error> {
        let release: Release = state
            .cache
            .cached(
                "dev.caloptreyx.serverimporter::latest-release",
                60 * 60,
                || async {
                    let response = state.client.get(LATEST_RELEASE).send().await?;
                    Ok::<Release, anyhow::Error>(
                        response.error_for_status()?.json::<Release>().await?,
                    )
                },
            )
            .await?;

        let latest = version_from_tag(&release.tag_name)?;

        Ok((latest > *current_version).then(|| ExtensionUpdateInfo {
            version: latest,
            changes: release
                .body
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(compact_str::CompactString::from)
                .collect(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_from_tag_accepts_prefixed_and_bare_tags() {
        assert_eq!(version_from_tag("v0.1.0").unwrap().to_string(), "0.1.0");
        assert_eq!(version_from_tag("1.2.3").unwrap().to_string(), "1.2.3");
        assert!(version_from_tag("release-1").is_err());
    }
}
