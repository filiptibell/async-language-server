#![allow(clippy::missing_panics_doc)]

use std::num::NonZeroUsize;

use async_lsp::{
    client_monitor::ClientProcessMonitorLayer, concurrency::ConcurrencyLayer,
    panic::CatchUnwindLayer, router::Router, server::LifecycleLayer,
};
use tower::ServiceBuilder;

#[cfg(feature = "tracing")]
use async_lsp::tracing::TracingLayer;

use crate::{
    result::ServerResult, server_trait::Server, server_with_state::LanguageServerWithState,
    transport::Transport,
};

/**
    Serves a language server over the given transport.

    The server must be clonable, and shareable across threads.

    This will automatically attach middleware for:

    - Tracing metadata for each request
    - Maximum concurrency of 8 in-flight LSP requests at a time
    - Catching panics and safely returning internal server error statuses
    - Client process monitoring and automatic server shutdown when client exits

    # Errors

    - If the transport uses a socket and it could not connect
    - If the server encounters an I/O error while running
*/
pub async fn serve<S>(transport: Transport, server: S) -> ServerResult<()>
where
    S: Server + Clone,
    S: Send + Sync + 'static,
{
    let (reader, writer) = transport.into_read_write().await?;

    let (server, _) = async_lsp::MainLoop::new_server(|client| {
        let builder = ServiceBuilder::new().layer(LifecycleLayer::default());

        #[cfg(feature = "tracing")]
        let builder = builder.layer(TracingLayer::default());

        builder
            .layer(ConcurrencyLayer::new(NonZeroUsize::new(8).unwrap()))
            .layer(CatchUnwindLayer::default())
            .layer(ClientProcessMonitorLayer::new(client.clone()))
            .service(Router::from_language_server(LanguageServerWithState::new(
                client,
                server.clone(),
            )))
    });

    server
        .run_buffered(reader, writer)
        .await
        .map_err(Into::into)
}
