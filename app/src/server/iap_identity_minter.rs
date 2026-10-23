use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt as _;
use torch_managed_secrets::client::IdentityTokenOptions;
use torch_server_client::iap::IapIdentityTokenMinter;
use torchui::r#async::BoxFuture;
use vec1::vec1;

use crate::server::server_api::managed_secrets::AppManagedSecretsClient;

/// Mints Torch-signed OIDC identity tokens for the runner-context IAP Workload
/// Identity Federation flow, backed by the managed-secrets client. Lives in the
/// app crate so `torch_server_client` need not depend on the managed-secrets
/// stack.
pub struct ManagedSecretsIapMinter {
    client: Arc<AppManagedSecretsClient>,
}

impl ManagedSecretsIapMinter {
    pub fn new(client: Arc<AppManagedSecretsClient>) -> Self {
        Self { client }
    }
}

impl IapIdentityTokenMinter for ManagedSecretsIapMinter {
    fn mint_identity_token(
        &self,
        audience: String,
        requested_duration: Duration,
    ) -> BoxFuture<'static, anyhow::Result<String>> {
        let client = self.client.clone();
        async move {
            let token = client
                .issue_task_identity_token(
                    None,
                    IdentityTokenOptions {
                        audience,
                        requested_duration,
                        subject_template: vec1!["principal".to_string()],
                    },
                )
                .await?;
            Ok(token.token)
        }
        .boxed()
    }
}
