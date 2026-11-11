//! The browser leg of the relay conversation (batch 3).
//!
//! `Carrier` + `MailSession` implemented over the gRPC-Web envelope
//! (the protocol's ruled browser face, R32: unary + server-streaming
//! only) via `tonic-web-wasm-client`. The visit routine, the pumps
//! and the seal task stay the library's — this crate only bridges
//! the wire.
//!
//! The Send bridge: the traits promise `Send` futures; the js-bound
//! client's futures are `!Send`. The bridge = `send_wrapper::
//! SendWrapper` around (a) each call's future and (b) the plane
//! streams — sound under the wasm single-threaded executor, exactly
//! the SPI note's SendWrapper-class pattern.
//!
//! Native builds compile an inert stub (the crate never runs off
//! wasm; the workspace's native `cargo check` still passes).

#[cfg(target_arch = "wasm32")]
mod imp {
    use futures::Stream;
    use futures::StreamExt as _;
    use send_wrapper::SendWrapper;
    use zconned::transport::{Carrier, Error, MailSession, Result};
    use zconnor_mail_types::relay::mailbox_service_client::MailboxServiceClient;
    use zconnor_mail_types::{mail, relay};

    /// The connection factory: one gRPC-Web client per endpoint
    /// (the relay URL — https, grpc-web envelope).
    pub struct GrpcWebCarrier {
        endpoint: String,
    }

    impl GrpcWebCarrier {
        pub fn new(endpoint: impl Into<String>) -> Self {
            Self {
                endpoint: endpoint.into(),
            }
        }
    }

    impl Carrier for GrpcWebCarrier {
        type Session = GrpcWebSession;

        async fn open(&self) -> Result<Self::Session> {
            let endpoint = self.endpoint.clone();
            // the js-bound construction itself rides the bridge
            let made = SendWrapper::new(async move {
                let channel = tonic_web_wasm_client::Client::new(endpoint);
                MailboxServiceClient::new(channel)
            })
            .await;
            Ok(GrpcWebSession(SendWrapper::new(made)))
        }
    }

    /// The relay conversation: thin delegation over the generated
    /// `MailboxServiceClient` (the TonicSession is the reference
    /// shape; only the channel type differs). Every call's future
    /// is Send-wrapped: the wrapper's inner carries all the !Send
    /// js state, the outer promise stays honest.
    #[derive(Clone)]
    pub struct GrpcWebSession(SendWrapper<MailboxServiceClient<tonic_web_wasm_client::Client>>);

    /// The plane stream: the js-backed `tonic::Streaming` behind the
    /// Send bridge (Streaming is Unpin over its boxed inner), error-
    /// mapped to the transport's neutral Error.
    pub type GrpcWebPlanes = futures::stream::Map<
        SendStream<tonic::Streaming<relay::PlaneFrame>>,
        fn(core::result::Result<relay::PlaneFrame, tonic::Status>) -> Result<relay::PlaneFrame>,
    >;

    fn plane_item(
        item: core::result::Result<relay::PlaneFrame, tonic::Status>,
    ) -> Result<relay::PlaneFrame> {
        item.map_err(|status| Error::Call {
            message: status.to_string(),
        })
    }

    /// The Stream half of the bridge: send_wrapper only bridges
    /// Future; this newtype adds Stream (delegating poll_next to the
    /// Unpin inner through the wrapper).
    pub struct SendStream<S>(SendWrapper<S>);

    impl<S> Stream for SendStream<S>
    where
        S: Stream + Unpin,
    {
        type Item = S::Item;

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<<S as Stream>::Item>> {
            std::pin::Pin::new(&mut self.0).poll_next(cx)
        }
    }

    // SendWrapper is unconditionally Unpin (a boxed inner), so the
    // poll delegation above can pin the wrapper freely.
    impl<S> Unpin for SendStream<S> {}

    fn st(status: tonic::Status) -> Error {
        Error::Call {
            message: status.to_string(),
        }
    }

    impl MailSession for GrpcWebSession {
        type Planes = GrpcWebPlanes;

        async fn get_version(&mut self) -> Result<u32> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                Ok(c.get_version(relay::GetVersionRequest {})
                    .await
                    .map_err(st)?
                    .into_inner()
                    .version)
            })
            .await
        }

        async fn register_mailbox(&mut self, mailbox: mail::MailboxPublicSet) -> Result<()> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                c.register_mailbox(relay::RegisterMailboxRequest {
                    mailbox: Some(mailbox),
                })
                .await
                .map_err(st)?;
                Ok(())
            })
            .await
        }

        async fn get_challenge(&mut self) -> Result<relay::Challenge> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                Ok(c.get_challenge(relay::GetChallengeRequest {})
                    .await
                    .map_err(st)?
                    .into_inner())
            })
            .await
        }

        async fn claim_mailbox(&mut self, anchor: Vec<u8>, claim_sig: Vec<u8>) -> Result<()> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                c.claim_mailbox(relay::ClaimMailboxRequest { anchor, claim_sig })
                    .await
                    .map_err(st)?;
                Ok(())
            })
            .await
        }

        async fn set_spk(&mut self, spk: mail::Spk) -> Result<()> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                c.set_spk(relay::SetSpkRequest { spk: Some(spk) })
                    .await
                    .map_err(st)?;
                Ok(())
            })
            .await
        }

        async fn set_opks(&mut self, opks: Vec<mail::Opk>) -> Result<()> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                c.set_opks(relay::SetOpksRequest { opks })
                    .await
                    .map_err(st)?;
                Ok(())
            })
            .await
        }

        async fn get_bundle(&mut self, mailbox: Vec<u8>) -> Result<relay::Bundle> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                Ok(c.get_bundle(relay::GetBundleRequest { mailbox })
                    .await
                    .map_err(st)?
                    .into_inner())
            })
            .await
        }

        async fn send_transcript_mail(&mut self, mail: mail::Mail) -> Result<relay::MailReceipt> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                Ok(c.send_transcript_mail(mail).await.map_err(st)?.into_inner())
            })
            .await
        }

        async fn send_resources_mail(&mut self, mail: mail::Mail) -> Result<relay::MailReceipt> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                Ok(c.send_resources_mail(mail).await.map_err(st)?.into_inner())
            })
            .await
        }

        async fn report_mail_received(&mut self, from_mailbox: Vec<u8>, up_to: u64) -> Result<()> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                c.report_mail_received(relay::ReportMailReceivedRequest {
                    from_mailbox,
                    up_to,
                })
                .await
                .map_err(st)?;
                Ok(())
            })
            .await
        }

        async fn get_mail_cursors(&mut self) -> Result<Vec<relay::MailboxCursor>> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                Ok(c.get_mail_cursors(relay::GetMailCursorsRequest {})
                    .await
                    .map_err(st)?
                    .into_inner()
                    .outbox)
            })
            .await
        }

        async fn transcript_plane(&mut self) -> Result<Self::Planes> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                let stream = c
                    .transcript_plane(relay::TranscriptPlaneRequest {})
                    .await
                    .map_err(st)?
                    .into_inner();
                Ok(SendStream(SendWrapper::new(stream)).map(plane_item as _))
            })
            .await
        }

        async fn resources_plane(&mut self) -> Result<Self::Planes> {
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                let stream = c
                    .resources_plane(relay::ResourcesPlaneRequest {})
                    .await
                    .map_err(st)?
                    .into_inner();
                Ok(SendStream(SendWrapper::new(stream)).map(plane_item as _))
            })
            .await
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use imp::{GrpcWebCarrier, GrpcWebPlanes, GrpcWebSession};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    // The native shell: never runs (the core lives in a browser
    // worker, A17); the workspace's host-target `cargo check` still
    // needs the full trait surface to resolve.
    use zconned::transport::{Carrier, Error, MailSession, Result};
    use zconnor_mail_types::{mail, relay};

    pub struct GrpcWebCarrier;

    impl GrpcWebCarrier {
        pub fn new(_endpoint: impl Into<String>) -> Self {
            Self
        }
    }

    #[derive(Clone)]
    pub struct GrpcWebSession;

    fn never<T>() -> Result<T> {
        Err(Error::Call {
            message: "the native shell opens no session".into(),
        })
    }

    impl Carrier for GrpcWebCarrier {
        type Session = GrpcWebSession;

        async fn open(&self) -> Result<Self::Session> {
            never()
        }
    }

    impl MailSession for GrpcWebSession {
        type Planes = futures::stream::Empty<Result<relay::PlaneFrame>>;

        async fn get_version(&mut self) -> Result<u32> {
            never()
        }
        async fn register_mailbox(&mut self, _mailbox: mail::MailboxPublicSet) -> Result<()> {
            never()
        }
        async fn get_challenge(&mut self) -> Result<relay::Challenge> {
            never()
        }
        async fn claim_mailbox(&mut self, _anchor: Vec<u8>, _claim_sig: Vec<u8>) -> Result<()> {
            never()
        }
        async fn set_spk(&mut self, _spk: mail::Spk) -> Result<()> {
            never()
        }
        async fn set_opks(&mut self, _opks: Vec<mail::Opk>) -> Result<()> {
            never()
        }
        async fn get_bundle(&mut self, _mailbox: Vec<u8>) -> Result<relay::Bundle> {
            never()
        }
        async fn send_transcript_mail(&mut self, _mail: mail::Mail) -> Result<relay::MailReceipt> {
            never()
        }
        async fn send_resources_mail(&mut self, _mail: mail::Mail) -> Result<relay::MailReceipt> {
            never()
        }
        async fn report_mail_received(
            &mut self,
            _from_mailbox: Vec<u8>,
            _up_to: u64,
        ) -> Result<()> {
            never()
        }
        async fn get_mail_cursors(&mut self) -> Result<Vec<relay::MailboxCursor>> {
            never()
        }
        async fn transcript_plane(&mut self) -> Result<Self::Planes> {
            never()
        }
        async fn resources_plane(&mut self) -> Result<Self::Planes> {
            never()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use imp::GrpcWebCarrier;
#[cfg(not(target_arch = "wasm32"))]
pub use imp::GrpcWebSession;
