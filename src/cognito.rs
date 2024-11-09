use aws_sdk_cognitoidentityprovider::Client;
use tokio::sync::OnceCell;

static COGNITO_CLIENT: OnceCell<Client> = OnceCell::const_new();

pub(crate) async fn client() -> &'static Client {
    COGNITO_CLIENT
        .get_or_init(|| async {
            let config = if let Ok(cognito_aws_profile) = std::env::var("COGNITO_AWS_PROFILE") {
                aws_config::from_env()
                    .profile_name(cognito_aws_profile)
                    .load()
                    .await
            } else {
                aws_config::load_from_env().await
            };

            Client::new(&config)
        })
        .await
}
