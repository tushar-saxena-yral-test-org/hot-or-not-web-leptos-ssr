use component::auth_providers::google::GoogleAuthMessage;
use component::loading::Loading;
use leptos::prelude::*;
use leptos_router::hooks::use_query;
use leptos_router::params::Params;
use openidconnect::CsrfToken;
use serde::{Deserialize, Serialize};
use server_fn::codec::{GetUrl, Json};
use utils::route::go_to_root;
use yral_types::delegated_identity::DelegatedIdentityWire;
#[server]
async fn google_auth_redirector() -> Result<(), ServerFnError> {
    use auth::core_clients::CoreClients;
    use auth::server_impl::google::google_auth_url_impl;
    use http::header::HeaderMap;
    use leptos_axum::extract;

    let headers: HeaderMap = extract().await?;
    let host = headers.get("Host").unwrap().to_str().unwrap();

    let oauth_clients: CoreClients = expect_context();
    let oauth2 = oauth_clients.get_oauth_client(host);

    let url = google_auth_url_impl(oauth2, None).await?;
    leptos_axum::redirect(&url);
    Ok(())
}

#[cfg(feature = "ssr")]
fn is_valid_redirect_uri_inner(client_redirect_uri: &str) -> Option<()> {
    use utils::host::is_host_or_origin_from_preview_domain;

    let parsed_uri = http::Uri::try_from(client_redirect_uri).ok()?;

    if parsed_uri.scheme_str() == Some("yralmobile://") {
        return Some(());
    }

    let host = parsed_uri.host()?;
    if host == "yral.com" {
        return Some(());
    }

    is_host_or_origin_from_preview_domain(host).then_some(())
}

#[cfg(feature = "ssr")]
fn is_valid_redirect_uri(client_redirect_uri: &str) -> bool {
    is_valid_redirect_uri_inner(client_redirect_uri).is_some()
}

#[server(endpoint = "google_auth_url", input = GetUrl, output = Json)]
async fn google_auth_url(client_redirect_uri: String) -> Result<String, ServerFnError> {
    use auth::core_clients::CoreClients;
    use auth::server_impl::google::google_auth_url_impl;
    use http::header::HeaderMap;
    use leptos_axum::extract;

    let headers: HeaderMap = extract().await?;

    if !is_valid_redirect_uri(&client_redirect_uri) {
        return Err(ServerFnError::new("Invalid client redirect uri"));
    }

    let host = headers.get("Host").unwrap().to_str().unwrap();
    let oauth_clients: CoreClients = expect_context();
    let oauth2 = oauth_clients.get_oauth_client(host);
    let url = google_auth_url_impl(oauth2, Some(client_redirect_uri)).await?;

    Ok(url)
}

#[server(endpoint = "perform_google_auth", input = Json, output = Json)]
async fn perform_google_auth(oauth: OAuthQuery) -> Result<DelegatedIdentityWire, ServerFnError> {
    use auth::core_clients::CoreClients;
    use auth::server_impl::google::perform_google_auth_impl;
    use http::header::HeaderMap;
    use leptos_axum::extract;

    let headers: HeaderMap = extract().await?;
    let host = headers.get("Host").unwrap().to_str().unwrap();

    let oauth_clients: CoreClients = expect_context();
    let oauth2 = oauth_clients.get_oauth_client(host);

    perform_google_auth_impl(oauth.state, oauth.code, oauth2).await
}

#[derive(Params, Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct OAuthQuery {
    pub code: String,
    pub state: String,
}

#[component]
pub fn IdentitySender(identity_res: GoogleAuthMessage) -> impl IntoView {
    Effect::new(move |_| {
        let _id = &identity_res;
        #[cfg(feature = "hydrate")]
        {
            use web_sys::Window;

            let win = window();
            let origin = win.origin();
            let opener = win.opener().unwrap();
            if opener.is_null() {
                go_to_root();
            }
            let opener = Window::from(opener);
            let msg = serde_json::to_string(&_id).unwrap();
            _ = opener.post_message(&msg.into(), &origin);
        }
    });

    view! {
        <div class="h-dvh w-dvw bg-black flex flex-col justify-center items-center gap-10">
            <img class="h-56 w-56 object-contain animate-pulse" src="/img/yral/logo.webp"/>
            <span class="text-2xl text-white/60">Good things come to those who wait...</span>
        </div>
    }
}

async fn handle_oauth_query(oauth_query: OAuthQuery) -> GoogleAuthMessage {
    let delegated = perform_google_auth(oauth_query)
        .await
        .map_err(|e| e.to_string())?;
    Ok(delegated)
}

#[server]
async fn handle_oauth_query_for_external_client(
    client_redirect_uri: String,
    oauth_query: OAuthQuery,
) -> Result<(), ServerFnError> {
    leptos_axum::redirect(&format!(
        "{}?code={}&state={}",
        client_redirect_uri, oauth_query.code, oauth_query.state
    ));
    Ok(())
}

#[derive(Serialize, Deserialize, Clone)]
enum RedirectHandlerReturnType {
    Identity(GoogleAuthMessage),
    ExternalClient(Result<(), String>),
}

#[derive(Serialize, Deserialize)]
struct OAuthState {
    pub csrf_token: CsrfToken,
    pub client_redirect_uri: Option<String>,
}

#[component]
pub fn GoogleRedirectHandler() -> impl IntoView {
    let query = use_query::<OAuthQuery>();
    let identity_resource = Resource::new_blocking(query, |query_res| async move {
        let Ok(oauth_query) = query_res else {
            return RedirectHandlerReturnType::Identity(Err("Invalid query".to_string()));
        };

        let Ok(oauth_state) = serde_json::from_str::<OAuthState>(&oauth_query.state) else {
            return RedirectHandlerReturnType::Identity(Err("Invalid OAuth State".to_string()));
        };

        if oauth_state.client_redirect_uri.is_some() {
            let res = handle_oauth_query_for_external_client(
                oauth_state.client_redirect_uri.unwrap(),
                oauth_query,
            )
            .await
            .map_err(|e| e.to_string());
            RedirectHandlerReturnType::ExternalClient(res)
        } else {
            let res = handle_oauth_query(oauth_query).await;
            RedirectHandlerReturnType::Identity(res)
        }
    });

    view! {
        <Loading text="Logging out...".to_string()>
            <Suspense>
                {move || {
                    identity_resource.get()
                        .and_then(|identity_res: RedirectHandlerReturnType| match identity_res {
                            RedirectHandlerReturnType::Identity(identity_res) => {
                                Some(view! { <IdentitySender identity_res/> })
                            }
                            RedirectHandlerReturnType::ExternalClient(_) => None,
                        })
                }}

            </Suspense>
        </Loading>
    }
}

#[component]
pub fn GoogleRedirector() -> impl IntoView {
    let google_redirect = Resource::new_blocking(|| (), |_| google_auth_redirector());
    let do_close = RwSignal::new(false);
    Effect::new(move |_| {
        if !do_close() {
            return;
        }
        let window = window();
        _ = window.close();
    });

    view! {
        <Suspense>
            {move || {
                if let Some(Err(_)) = google_redirect.get() {
                    do_close.set(true)
                }
                None::<()>
            }}

        </Suspense>
    }
}
